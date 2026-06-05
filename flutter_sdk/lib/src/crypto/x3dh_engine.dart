import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';
import 'crypto_types.dart';
import '../transport/api_models.dart';

/// Implements the Extended Triple Diffie-Hellman (X3DH) key agreement protocol.
///
/// X3DH establishes a shared secret between two parties before any messages
/// are exchanged, providing forward secrecy and deniability.
class X3dhEngine {
  static final _x25519 = X25519();
  static final _hkdf = Hkdf(hmac: Hmac.sha256(), outputLength: 32);

  // X3DH info string per Signal spec
  static const _info = 'rust_e2e_chat_sdk_x3dh';

  /// Initiator side: performs X3DH using the [recipientBundle] and the
  /// sender's [senderBundle].
  ///
  /// Returns the 32-byte shared secret and the [X3dhInitHeader] that must be
  /// sent with the first message so the recipient can derive the same secret.
  static Future<X3dhInitResult> performX3dh({
    required KeyBundleResponse recipientBundle,
    required PrivateKeyBundle senderBundle,
  }) async {
    // Generate ephemeral key pair
    final ephemeralPair = await _generateEphemeral();

    // DH1: IK_sender ⊗ SPK_recipient
    final dh1 = await _dh(
      senderBundle.identityKeyPair.privateKey,
      recipientBundle.signedPreKey,
    );

    // DH2: EK_sender ⊗ IK_recipient
    final dh2 = await _dh(
      ephemeralPair.privateKey,
      recipientBundle.identityKey,
    );

    // DH3: EK_sender ⊗ SPK_recipient
    final dh3 = await _dh(
      ephemeralPair.privateKey,
      recipientBundle.signedPreKey,
    );

    // DH4 (optional): EK_sender ⊗ OTPK_recipient
    Uint8List? dh4;
    int? otpkId;
    if (recipientBundle.oneTimePreKey != null) {
      dh4 = await _dh(
        ephemeralPair.privateKey,
        recipientBundle.oneTimePreKey!.publicKey,
      );
      otpkId = recipientBundle.oneTimePreKey!.id;
    }

    // Concatenate all DH outputs for HKDF input
    final dhInput = _concat([dh1, dh2, dh3, if (dh4 != null) dh4]);

    // HKDF with 32 zero bytes as salt (Signal spec)
    final salt = Uint8List(32);
    final sharedSecret = await _hkdfDerive(dhInput, salt, _info);

    final header = X3dhInitHeader(
      ephemeralKey: ephemeralPair.publicKey,
      identityKey: senderBundle.identityKeyPair.publicKey,
      signedPreKeyId: recipientBundle.signedPreKeyId,
      oneTimePreKeyId: otpkId,
    );

    return X3dhInitResult(sharedSecret: sharedSecret, header: header);
  }

  /// Responder side: derives the same shared secret from the received [header]
  /// and the responder's [recipientBundle].
  static Future<Uint8List> deriveSharedSecret({
    required X3dhInitHeader header,
    required PrivateKeyBundle recipientBundle,
    OtpkRecord? consumedOtpk,
  }) async {
    // DH1: SPK_recipient ⊗ IK_sender
    final dh1 = await _dh(
      recipientBundle.signedPreKeyPair.privateKey,
      header.identityKey,
    );

    // DH2: IK_recipient ⊗ EK_sender
    final dh2 = await _dh(
      recipientBundle.identityKeyPair.privateKey,
      header.ephemeralKey,
    );

    // DH3: SPK_recipient ⊗ EK_sender
    final dh3 = await _dh(
      recipientBundle.signedPreKeyPair.privateKey,
      header.ephemeralKey,
    );

    // DH4 (optional): OTPK_recipient ⊗ EK_sender
    Uint8List? dh4;
    if (consumedOtpk != null) {
      dh4 = await _dh(consumedOtpk.privateKey, header.ephemeralKey);
    }

    final dhInput = _concat([dh1, dh2, dh3, if (dh4 != null) dh4]);
    final salt = Uint8List(32);
    return _hkdfDerive(dhInput, salt, _info);
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  static Future<_EphemeralPair> _generateEphemeral() async {
    final keyPair = await _x25519.newKeyPair();
    final privateBytes = await keyPair.extractPrivateKeyBytes();
    final publicKey = await keyPair.extractPublicKey();
    return _EphemeralPair(
      publicKey: Uint8List.fromList(publicKey.bytes),
      privateKey: Uint8List.fromList(privateBytes),
    );
  }

  static Future<Uint8List> _dh(
      Uint8List privateKeyBytes, Uint8List remotePublicKeyBytes) async {
    final privateKey = await _x25519.newKeyPairFromSeed(privateKeyBytes);
    final remotePublicKey =
        SimplePublicKey(remotePublicKeyBytes, type: KeyPairType.x25519);
    final sharedSecret =
        await _x25519.sharedSecretKey(keyPair: privateKey, remotePublicKey: remotePublicKey);
    return Uint8List.fromList(await sharedSecret.extractBytes());
  }

  static Future<Uint8List> _hkdfDerive(
      Uint8List ikm, Uint8List salt, String info) async {
    final ikmKey = SecretKey(ikm);
    final output = await _hkdf.deriveKey(
      secretKey: ikmKey,
      nonce: salt,
      info: _toBytes(info),
    );
    return Uint8List.fromList(await output.extractBytes());
  }

  static Uint8List _concat(List<Uint8List> parts) {
    final total = parts.fold(0, (sum, p) => sum + p.length);
    final result = Uint8List(total);
    var offset = 0;
    for (final part in parts) {
      result.setRange(offset, offset + part.length, part);
      offset += part.length;
    }
    return result;
  }

  static List<int> _toBytes(String s) => s.codeUnits;
}

class _EphemeralPair {
  final Uint8List publicKey;
  final Uint8List privateKey;
  const _EphemeralPair({required this.publicKey, required this.privateKey});
}
