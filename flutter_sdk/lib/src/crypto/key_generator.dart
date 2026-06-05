import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';

import 'crypto_types.dart';

/// Generates all cryptographic key material needed by the SDK.
///
/// Uses the [cryptography] package for platform-appropriate secure randomness.
class KeyGenerator {
  static final _x25519 = X25519();
  static final _ed25519 = Ed25519();

  /// Generates a complete [PrivateKeyBundle] for device registration.
  ///
  /// Produces:
  /// - Curve25519 identity key pair (for DH)
  /// - Ed25519 signing key pair (for SPK signature)
  /// - Curve25519 signed pre-key pair
  /// - Ed25519 signature of the SPK public key
  /// - [otpkCount] one-time pre-key pairs (default 50)
  static Future<PrivateKeyBundle> generateKeyBundle(
      {int otpkCount = 50}) async {
    // Identity key (Curve25519 for DH)
    final identityDhPair = await _generateCurve25519();

    // Identity signing key (Ed25519)
    final identitySignPair = await _generateEd25519();

    // Signed pre-key
    final spkId = _generateKeyId();
    final spkPair = await _generateCurve25519();

    // Sign the SPK public key with the identity signing key
    final spkSignature = await _signBytes(
      spkPair.publicKey,
      identitySignPair.privateKey,
    );

    // One-time pre-keys
    final otpks = await generateOneTimePreKeys(otpkCount);

    return PrivateKeyBundle(
      identityKeyPair: identityDhPair,
      identitySigningKeyPair: identitySignPair,
      signedPreKeyPair: spkPair,
      signedPreKeySignature: spkSignature,
      signedPreKeyId: spkId,
      oneTimePreKeys: otpks,
    );
  }

  /// Generates a batch of [count] one-time pre-key pairs.
  static Future<List<OneTimeKeyPair>> generateOneTimePreKeys(int count) async {
    final keys = <OneTimeKeyPair>[];
    for (var i = 0; i < count; i++) {
      final pair = await _generateCurve25519();
      keys.add(OneTimeKeyPair(
        id: _generateKeyId(),
        keyPair: pair,
      ));
    }
    return keys;
  }

  /// Generates a new [SenderKeyPair] for group messaging.
  static Future<SenderKeyPair> generateSenderKey() async {
    // 32-byte random chain key
    final chainKey = await _randomBytes(32);

    // Ed25519 signing key for authenticating group messages
    final signingPair = await _generateEd25519();

    return SenderKeyPair(
      chainKey: chainKey,
      signingKeyPair: signingPair,
    );
  }

  /// Verifies that [signature] is a valid Ed25519 signature of [data]
  /// under the given [publicKey].
  static Future<bool> verifySignature({
    required Uint8List data,
    required Uint8List signature,
    required Uint8List publicKey,
  }) async {
    try {
      final pubKey = SimplePublicKey(publicKey, type: KeyPairType.ed25519);
      final sig = Signature(signature, publicKey: pubKey);
      return await _ed25519.verify(data, signature: sig);
    } catch (_) {
      return false;
    }
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  static Future<Curve25519KeyPair> _generateCurve25519() async {
    final keyPair = await _x25519.newKeyPair();
    final privateBytes = await keyPair.extractPrivateKeyBytes();
    final publicKey = await keyPair.extractPublicKey();
    return Curve25519KeyPair(
      publicKey: Uint8List.fromList(publicKey.bytes),
      privateKey: Uint8List.fromList(privateBytes),
    );
  }

  static Future<Ed25519KeyPair> _generateEd25519() async {
    final keyPair = await _ed25519.newKeyPair();
    final privateBytes = await keyPair.extractPrivateKeyBytes();
    final publicKey = await keyPair.extractPublicKey();
    return Ed25519KeyPair(
      publicKey: Uint8List.fromList(publicKey.bytes),
      privateKey: Uint8List.fromList(privateBytes),
    );
  }

  static Future<Uint8List> _signBytes(
      Uint8List data, Uint8List privateKeyBytes) async {
    final keyPair = await _ed25519.newKeyPairFromSeed(privateKeyBytes);
    final signature = await _ed25519.sign(data, keyPair: keyPair);
    return Uint8List.fromList(signature.bytes);
  }

  static Future<Uint8List> _randomBytes(int length) async {
    final algorithm = AesGcm.with256bits(); // Just to get the random source
    // Use SecretKeyData from cryptography for random bytes
    final key = await algorithm.newSecretKey();
    final bytes = await key.extractBytes();
    // Trim or pad to desired length
    return Uint8List.fromList(bytes.take(length).toList());
  }

  static int _generateKeyId() {
    // Generate a positive 32-bit key ID
    return DateTime.now().millisecondsSinceEpoch & 0x7FFFFFFF;
  }
}
