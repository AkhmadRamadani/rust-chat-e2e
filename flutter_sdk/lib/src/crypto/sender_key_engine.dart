import 'dart:convert';
import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';
import 'crypto_types.dart';
import '../session/session_data.dart';
import '../errors/sdk_error.dart';

/// Represents an active Sender Key session for a group conversation.
class SenderKeySession {
  final String conversationId;
  final String userId;
  Uint8List chainKey;
  int iteration;
  final Uint8List signingKeyPublic;
  final Uint8List? signingKeyPrivate; // Only for own key

  SenderKeySession({
    required this.conversationId,
    required this.userId,
    required this.chainKey,
    required this.iteration,
    required this.signingKeyPublic,
    this.signingKeyPrivate,
  });

  bool get isOwn => signingKeyPrivate != null;

  SenderKeyRecord toRecord() => SenderKeyRecord(
        conversationId: conversationId,
        userId: userId,
        chainKey: chainKey,
        iteration: iteration,
        signingKeyPublic: signingKeyPublic,
        signingKeyPrivate: signingKeyPrivate,
      );

  factory SenderKeySession.fromRecord(SenderKeyRecord record) =>
      SenderKeySession(
        conversationId: record.conversationId,
        userId: record.userId,
        chainKey: record.chainKey,
        iteration: record.iteration,
        signingKeyPublic: record.signingKeyPublic,
        signingKeyPrivate: record.signingKeyPrivate,
      );
}

/// Implements the Sender Key protocol for group messaging.
///
/// Each group member generates a unique [SenderKeyPair]. The chain key
/// advances on each encrypted message, providing forward secrecy within
/// the group. Messages are authenticated with Ed25519 signatures.
class SenderKeyEngine {
  static final _aesGcm = AesGcm.with256bits();
  static final _ed25519 = Ed25519();
  static final _hkdf = Hkdf(hmac: Hmac.sha256(), outputLength: 48); // 32+16
  static const _chainKdfInfo = 'rust_e2e_sender_key_chain';

  /// Creates a new [SenderKeySession] for the local user from a [SenderKeyPair].
  static SenderKeySession createSession({
    required String conversationId,
    required String userId,
    required SenderKeyPair keyPair,
  }) {
    return SenderKeySession(
      conversationId: conversationId,
      userId: userId,
      chainKey: keyPair.chainKey,
      iteration: 0,
      signingKeyPublic: keyPair.signingKeyPair.publicKey,
      signingKeyPrivate: keyPair.signingKeyPair.privateKey,
    );
  }

  /// Encrypts [plaintext] with the sender's chain key, advancing the ratchet.
  ///
  /// Returns the ciphertext bytes including the iteration counter, AES-GCM
  /// ciphertext, and Ed25519 signature.
  static Future<Uint8List> encrypt(
    SenderKeySession session,
    Uint8List plaintext,
  ) async {
    if (!session.isOwn) {
      throw const StorageError(reason: 'Cannot encrypt with a remote sender key');
    }

    // Advance chain key
    final (newChainKey, messageKey, iv) = await _advanceChain(session.chainKey);
    session.chainKey = newChainKey;
    final currentIteration = session.iteration;
    session.iteration++;

    // Encrypt
    final secretKey = SecretKey(messageKey);
    final box = await _aesGcm.encrypt(
      plaintext,
      secretKey: secretKey,
      nonce: iv,
    );
    final ciphertext = Uint8List.fromList([
      ...box.nonce,
      ...box.cipherText,
      ...box.mac.bytes,
    ]);

    // Build payload: iteration(4) + ciphertext_len(4) + ciphertext + signature(64)
    final iterBytes = _intToBytes(currentIteration);
    final lenBytes = _intToBytes(ciphertext.length);
    final dataToSign = Uint8List.fromList([
      ...iterBytes,
      ...lenBytes,
      ...ciphertext,
    ]);

    // Sign with Ed25519
    final keyPair =
        await _ed25519.newKeyPairFromSeed(session.signingKeyPrivate!);
    final signature = await _ed25519.sign(dataToSign, keyPair: keyPair);

    return Uint8List.fromList([
      ...dataToSign,
      ...signature.bytes,
    ]);
  }

  /// Decrypts a group message ciphertext.
  ///
  /// Verifies the Ed25519 signature and advances the chain to match the
  /// message's iteration counter.
  static Future<Uint8List> decrypt(
    SenderKeySession session,
    Uint8List ciphertextBytes,
    String conversationId,
  ) async {
    if (ciphertextBytes.length < 8 + 64) {
      throw DecryptionError(conversationId: conversationId, seq: -1);
    }

    // Parse iteration(4) + ciphertext_len(4) + ciphertext + signature(64)
    final msgIteration = _bytesToInt(ciphertextBytes.sublist(0, 4));
    final ciphertextLen = _bytesToInt(ciphertextBytes.sublist(4, 8));

    if (ciphertextBytes.length < 8 + ciphertextLen + 64) {
      throw DecryptionError(conversationId: conversationId, seq: msgIteration);
    }

    final ciphertext = ciphertextBytes.sublist(8, 8 + ciphertextLen);
    final signature = ciphertextBytes.sublist(8 + ciphertextLen);
    final dataToVerify = ciphertextBytes.sublist(0, 8 + ciphertextLen);

    // Verify signature
    final pubKey =
        SimplePublicKey(session.signingKeyPublic, type: KeyPairType.ed25519);
    final sig = Signature(signature, publicKey: pubKey);
    final valid = await _ed25519.verify(dataToVerify, signature: sig);
    if (!valid) {
      throw DecryptionError(
          conversationId: conversationId, seq: msgIteration);
    }

    // Advance chain to the right iteration
    var chainKey = session.chainKey;
    for (var i = session.iteration; i < msgIteration; i++) {
      final (newChain, _, __) = await _advanceChain(chainKey);
      chainKey = newChain;
    }

    final (newChainKey, messageKey, _) = await _advanceChain(chainKey);
    session.chainKey = newChainKey;
    session.iteration = msgIteration + 1;

    // Decrypt
    if (ciphertext.length < 28) {
      throw DecryptionError(
          conversationId: conversationId, seq: msgIteration);
    }
    final nonce = ciphertext.sublist(0, 12);
    final mac = ciphertext.sublist(ciphertext.length - 16);
    final encryptedData = ciphertext.sublist(12, ciphertext.length - 16);

    try {
      final secretKey = SecretKey(messageKey);
      final box = SecretBox(encryptedData, nonce: nonce, mac: Mac(mac));
      final plaintext = await _aesGcm.decrypt(box, secretKey: secretKey);
      return Uint8List.fromList(plaintext);
    } catch (_) {
      throw DecryptionError(
          conversationId: conversationId, seq: msgIteration);
    }
  }

  /// Serializes a [SenderKeySession] to bytes for SKDM distribution.
  ///
  /// The returned bytes are then encrypted via the Double Ratchet before
  /// being sent to the recipient.
  static Uint8List serializeKeyMaterial(SenderKeySession session) {
    final payload = {
      'chainKey': base64.encode(session.chainKey),
      'iteration': session.iteration,
      'signingKeyPublic': base64.encode(session.signingKeyPublic),
    };
    return Uint8List.fromList(jsonEncode(payload).codeUnits);
  }

  /// Deserializes a received SKDM payload into a [SenderKeySession].
  static SenderKeySession deserializeKeyMaterial({
    required Uint8List bytes,
    required String conversationId,
    required String senderUserId,
  }) {
    final payload =
        jsonDecode(String.fromCharCodes(bytes)) as Map<String, dynamic>;
    return SenderKeySession(
      conversationId: conversationId,
      userId: senderUserId,
      chainKey: base64.decode(payload['chainKey'] as String),
      iteration: payload['iteration'] as int,
      signingKeyPublic: base64.decode(payload['signingKeyPublic'] as String),
      // No private key — this is a remote sender's key
    );
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  static Future<(Uint8List chainKey, Uint8List messageKey, List<int> iv)>
      _advanceChain(Uint8List chainKey) async {
    final output = await _hkdf.deriveKey(
      secretKey: SecretKey(chainKey),
      nonce: Uint8List.fromList([0x01]),
      info: _chainKdfInfo.codeUnits,
    );
    final bytes = await output.extractBytes();
    // First 32 bytes = new chain key, next 16 bytes = IV (reused for AES-GCM nonce)
    final newChain = Uint8List.fromList(bytes.sublist(0, 32));

    // Derive message key separately
    final msgOutput = await _hkdf.deriveKey(
      secretKey: SecretKey(chainKey),
      nonce: Uint8List.fromList([0x02]),
      info: _chainKdfInfo.codeUnits,
    );
    final msgBytes = await msgOutput.extractBytes();
    final msgKey = Uint8List.fromList(msgBytes.sublist(0, 32));
    final iv = msgBytes.sublist(32, 44); // 12-byte AES-GCM nonce

    return (newChain, msgKey, iv);
  }

  static Uint8List _intToBytes(int value) {
    final bytes = Uint8List(4);
    bytes[0] = (value >> 24) & 0xFF;
    bytes[1] = (value >> 16) & 0xFF;
    bytes[2] = (value >> 8) & 0xFF;
    bytes[3] = value & 0xFF;
    return bytes;
  }

  static int _bytesToInt(Uint8List bytes) {
    return (bytes[0] << 24) | (bytes[1] << 16) | (bytes[2] << 8) | bytes[3];
  }
}
