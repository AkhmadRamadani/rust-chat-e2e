import 'dart:typed_data';
import 'package:cryptography/cryptography.dart';
import 'crypto_types.dart';
import '../session/session_data.dart';
import '../errors/sdk_error.dart';

/// Maximum number of skipped message keys stored per session.
const _maxSkippedKeys = 2000;

/// Maximum number of keys to skip in a single ratchet step.
const _maxSkip = 1000;

/// Implements the Double Ratchet Algorithm for 1:1 end-to-end encryption.
///
/// Provides forward secrecy and break-in recovery. Each message is encrypted
/// with a unique key derived from the ratchet chain.
class RatchetEngine {
  static final _x25519 = X25519();
  static final _aesGcm = AesGcm.with256bits();

  static const _rootKdfInfo = 'rust_e2e_root_ratchet';
  static const _chainKdfInfo = 'rust_e2e_chain_key';
  static const _messageKdfInfo = 'rust_e2e_message_key';

  /// Initialize the Double Ratchet session from the sender side.
  ///
  /// [sharedSecret] is the X3DH output.
  /// [recipientDhPublicKey] is the recipient's signed pre-key public bytes.
  static Future<RatchetState> initSender(
    Uint8List sharedSecret,
    Uint8List recipientDhPublicKey,
    String conversationId,
  ) async {
    // Generate the initial DH ratchet key pair for sending
    final dhPair = await _generateDhPair();

    // Derive root and chain keys
    final dhOut = await _dh(dhPair.privateKey, recipientDhPublicKey);
    final (rootKey, chainKeySend) =
        await _kdfRootKey(sharedSecret, dhOut);

    return RatchetState(
      conversationId: conversationId,
      rootKey: rootKey,
      chainKeySend: chainKeySend,
      chainKeyRecv: Uint8List(32), // empty until first message received
      dhSendPub: dhPair.publicKey,
      dhSendPriv: dhPair.privateKey,
      dhRecvPub: recipientDhPublicKey,
      nSend: 0,
      nRecv: 0,
      pn: 0,
      skippedMessageKeys: {},
    );
  }

  /// Initialize the Double Ratchet session from the receiver side.
  ///
  /// [sharedSecret] is the X3DH output.
  /// [localDhPair] is the receiver's signed pre-key pair.
  static RatchetState initReceiver(
    Uint8List sharedSecret,
    Curve25519KeyPair localDhPair,
    String conversationId,
  ) {
    return RatchetState(
      conversationId: conversationId,
      rootKey: sharedSecret,
      chainKeySend: Uint8List(32),
      chainKeyRecv: Uint8List(32),
      dhSendPub: localDhPair.publicKey,
      dhSendPriv: localDhPair.privateKey,
      dhRecvPub: Uint8List(32), // populated on first received message
      nSend: 0,
      nRecv: 0,
      pn: 0,
      skippedMessageKeys: {},
    );
  }

  /// Encrypts [plaintext] using the current ratchet [state].
  ///
  /// Returns the updated [RatchetState] and the [RatchetCiphertext].
  static Future<(RatchetState, RatchetCiphertext)> encrypt(
    RatchetState state,
    Uint8List plaintext, {
    bool isInitialMessage = false,
    X3dhInitHeader? x3dhHeader,
  }) async {
    final (newChainKey, messageKey) = await _kdfChainKey(state.chainKeySend);
    final ciphertext = await _encryptMessage(messageKey, plaintext);

    final header = RatchetMessageHeader(
      dhPublicKey: state.dhSendPub,
      messageNumber: state.nSend,
      previousChainLength: state.pn,
      isInitialMessage: isInitialMessage,
      x3dhHeader: x3dhHeader,
    );

    final newState = state.copyWith(
      chainKeySend: newChainKey,
      nSend: state.nSend + 1,
    );

    return (newState, RatchetCiphertext(ciphertext: ciphertext, header: header));
  }

  /// Decrypts [ciphertext] using the current ratchet [state].
  ///
  /// Returns the updated [RatchetState] and the decrypted plaintext bytes.
  /// Throws [DecryptionError] if decryption fails.
  static Future<(RatchetState, Uint8List)> decrypt(
    RatchetState state,
    RatchetCiphertext ciphertext,
  ) async {
    final header = ciphertext.header;

    // Check if this is a skipped message key
    final skipKey = _skippedKey(state, header);
    if (skipKey != null) {
      final plaintext = await _decryptMessage(skipKey, ciphertext.ciphertext)
          .then((p) => p)
          .catchError((_) => throw DecryptionError(
              conversationId: state.conversationId, seq: header.messageNumber));
      final newSkipped = Map<int, Uint8List>.from(state.skippedMessageKeys)
        ..remove(header.messageNumber);
      return (state.copyWith(skippedMessageKeys: newSkipped), plaintext);
    }

    RatchetState currentState = state;

    // If this message is from a new DH ratchet step
    final isNewDhKey = !_bytesEqual(header.dhPublicKey, state.dhRecvPub);

    if (isNewDhKey) {
      // Skip messages from the old chain
      currentState = await _skipMessageKeys(currentState, header.previousChainLength);
      // Perform DH ratchet step
      currentState = await _dhRatchetStep(currentState, header.dhPublicKey);
    }

    // Skip messages in the current receiving chain
    currentState =
        await _skipMessageKeys(currentState, header.messageNumber);

    // Derive message key
    final (newChainKeyRecv, messageKey) =
        await _kdfChainKey(currentState.chainKeyRecv);
    currentState = currentState.copyWith(
      chainKeyRecv: newChainKeyRecv,
      nRecv: currentState.nRecv + 1,
    );

    try {
      final plaintext = await _decryptMessage(messageKey, ciphertext.ciphertext);
      return (currentState, plaintext);
    } catch (_) {
      throw DecryptionError(
          conversationId: state.conversationId, seq: header.messageNumber);
    }
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  static Uint8List? _skippedKey(RatchetState state, RatchetMessageHeader header) {
    if (_bytesEqual(header.dhPublicKey, state.dhRecvPub)) {
      return state.skippedMessageKeys[header.messageNumber];
    }
    return null;
  }

  static Future<RatchetState> _skipMessageKeys(
      RatchetState state, int until) async {
    if (state.nRecv > until) return state;
    if (until - state.nRecv > _maxSkip) {
      throw const StorageError(reason: 'Too many skipped messages');
    }

    var currentState = state;
    final skipped = Map<int, Uint8List>.from(state.skippedMessageKeys);

    while (currentState.nRecv < until) {
      final (newChain, msgKey) = await _kdfChainKey(currentState.chainKeyRecv);
      skipped[currentState.nRecv] = msgKey;
      currentState = currentState.copyWith(
        chainKeyRecv: newChain,
        nRecv: currentState.nRecv + 1,
      );

      // Cap skipped message keys
      if (skipped.length > _maxSkippedKeys) {
        final oldest = skipped.keys.reduce((a, b) => a < b ? a : b);
        skipped.remove(oldest);
      }
    }

    return currentState.copyWith(skippedMessageKeys: skipped);
  }

  static Future<RatchetState> _dhRatchetStep(
      RatchetState state, Uint8List remotePublicKey) async {
    // Derive new recv chain key
    final dhRecv = await _dh(state.dhSendPriv, remotePublicKey);
    final (rootKey1, chainKeyRecv) = await _kdfRootKey(state.rootKey, dhRecv);

    // Generate new DH key pair for sending
    final newDhPair = await _generateDhPair();

    // Derive new send chain key
    final dhSend = await _dh(newDhPair.privateKey, remotePublicKey);
    final (rootKey2, chainKeySend) = await _kdfRootKey(rootKey1, dhSend);

    return state.copyWith(
      rootKey: rootKey2,
      chainKeySend: chainKeySend,
      chainKeyRecv: chainKeyRecv,
      dhSendPub: newDhPair.publicKey,
      dhSendPriv: newDhPair.privateKey,
      dhRecvPub: remotePublicKey,
      nSend: 0,
      nRecv: 0,
      pn: state.nSend,
    );
  }

  static Future<(Uint8List rootKey, Uint8List chainKey)> _kdfRootKey(
      Uint8List rootKey, Uint8List dhOutput) async {
    final hkdf = Hkdf(hmac: Hmac.sha256(), outputLength: 64);
    final output = await hkdf.deriveKey(
      secretKey: SecretKey(dhOutput),
      nonce: rootKey,
      info: _rootKdfInfo.codeUnits,
    );
    final bytes = await output.extractBytes();
    return (
      Uint8List.fromList(bytes.sublist(0, 32)),
      Uint8List.fromList(bytes.sublist(32, 64)),
    );
  }

  static Future<(Uint8List chainKey, Uint8List messageKey)> _kdfChainKey(
      Uint8List chainKey) async {
    final hkdf32 = Hkdf(hmac: Hmac.sha256(), outputLength: 32);
    final newChain = await hkdf32.deriveKey(
      secretKey: SecretKey(chainKey),
      nonce: Uint8List.fromList([0x01]),
      info: _chainKdfInfo.codeUnits,
    );
    final msgKey = await hkdf32.deriveKey(
      secretKey: SecretKey(chainKey),
      nonce: Uint8List.fromList([0x02]),
      info: _messageKdfInfo.codeUnits,
    );
    return (
      Uint8List.fromList(await newChain.extractBytes()),
      Uint8List.fromList(await msgKey.extractBytes()),
    );
  }

  static Future<Uint8List> _encryptMessage(
      Uint8List key, Uint8List plaintext) async {
    final secretKey = SecretKey(key);
    final nonce = _aesGcm.newNonce();
    final box = await _aesGcm.encrypt(plaintext,
        secretKey: secretKey, nonce: nonce);
    // Prepend nonce (12 bytes) to ciphertext
    final result = Uint8List(12 + box.cipherText.length + box.mac.bytes.length);
    result.setRange(0, 12, nonce);
    result.setRange(12, 12 + box.cipherText.length, box.cipherText);
    result.setRange(12 + box.cipherText.length, result.length, box.mac.bytes);
    return result;
  }

  static Future<Uint8List> _decryptMessage(
      Uint8List key, Uint8List ciphertextWithNonce) async {
    if (ciphertextWithNonce.length < 28) {
      throw const StorageError(reason: 'Ciphertext too short');
    }
    final nonce = ciphertextWithNonce.sublist(0, 12);
    final mac = ciphertextWithNonce.sublist(ciphertextWithNonce.length - 16);
    final ciphertext =
        ciphertextWithNonce.sublist(12, ciphertextWithNonce.length - 16);

    final secretKey = SecretKey(key);
    final box = SecretBox(ciphertext, nonce: nonce, mac: Mac(mac));
    final plaintext = await _aesGcm.decrypt(box, secretKey: secretKey);
    return Uint8List.fromList(plaintext);
  }

  static Future<Uint8List> _dh(
      Uint8List privateKeyBytes, Uint8List remotePublicKeyBytes) async {
    final privateKey = await _x25519.newKeyPairFromSeed(privateKeyBytes);
    final remotePublicKey =
        SimplePublicKey(remotePublicKeyBytes, type: KeyPairType.x25519);
    final sharedSecret = await _x25519.sharedSecretKey(
        keyPair: privateKey, remotePublicKey: remotePublicKey);
    return Uint8List.fromList(await sharedSecret.extractBytes());
  }

  static Future<Curve25519KeyPair> _generateDhPair() async {
    final keyPair = await _x25519.newKeyPair();
    final privateBytes = await keyPair.extractPrivateKeyBytes();
    final publicKey = await keyPair.extractPublicKey();
    return Curve25519KeyPair(
      publicKey: Uint8List.fromList(publicKey.bytes),
      privateKey: Uint8List.fromList(privateBytes),
    );
  }

  static bool _bytesEqual(Uint8List a, Uint8List b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }
}
