import 'dart:typed_data';

/// A Curve25519 key pair used for Diffie-Hellman operations.
class Curve25519KeyPair {
  /// The public key bytes (32 bytes).
  final Uint8List publicKey;

  /// The private key bytes (32 bytes).
  final Uint8List privateKey;

  const Curve25519KeyPair({required this.publicKey, required this.privateKey});
}

/// An Ed25519 key pair used for signing operations.
class Ed25519KeyPair {
  /// The public key bytes (32 bytes).
  final Uint8List publicKey;

  /// The private key bytes (64 bytes).
  final Uint8List privateKey;

  const Ed25519KeyPair({required this.publicKey, required this.privateKey});
}

/// A one-time pre-key pair, associated with a server-assigned ID.
class OneTimeKeyPair {
  /// Server-assigned ID for this key.
  final int id;

  /// The Curve25519 key pair.
  final Curve25519KeyPair keyPair;

  const OneTimeKeyPair({required this.id, required this.keyPair});
}

/// A single stored one-time pre-key record (with private bytes).
class OtpkRecord {
  final int id;
  final Uint8List privateKey;
  final Uint8List publicKey;

  const OtpkRecord({
    required this.id,
    required this.privateKey,
    required this.publicKey,
  });

  Map<String, dynamic> toJson() => {
        'id': id,
        'privateKey': privateKey,
        'publicKey': publicKey,
      };

  factory OtpkRecord.fromJson(Map<String, dynamic> json) => OtpkRecord(
        id: json['id'] as int,
        privateKey: Uint8List.fromList(List<int>.from(json['privateKey'] as List)),
        publicKey: Uint8List.fromList(List<int>.from(json['publicKey'] as List)),
      );
}

/// The full private key bundle for a device.
class PrivateKeyBundle {
  final Curve25519KeyPair identityKeyPair;
  final Ed25519KeyPair identitySigningKeyPair;
  final Curve25519KeyPair signedPreKeyPair;
  final Uint8List signedPreKeySignature;
  final int signedPreKeyId;
  final List<OneTimeKeyPair> oneTimePreKeys;

  const PrivateKeyBundle({
    required this.identityKeyPair,
    required this.identitySigningKeyPair,
    required this.signedPreKeyPair,
    required this.signedPreKeySignature,
    required this.signedPreKeyId,
    required this.oneTimePreKeys,
  });
}

/// The public key bundle shared with other users.
class KeyBundle {
  final Uint8List identityKey;
  final Uint8List signedPreKey;
  final Uint8List signedPreKeySignature;
  final int signedPreKeyId;
  final List<OneTimePreKey> oneTimePreKeys;

  const KeyBundle({
    required this.identityKey,
    required this.signedPreKey,
    required this.signedPreKeySignature,
    required this.signedPreKeyId,
    required this.oneTimePreKeys,
  });
}

/// A public one-time pre-key with its ID.
class OneTimePreKey {
  final int id;
  final Uint8List publicKey;

  const OneTimePreKey({required this.id, required this.publicKey});

  Map<String, dynamic> toJson() => {
        'id': id,
        'public_key': publicKey,
      };
}

/// Update payload for rotating the Signed Pre-Key.
class SignedPreKeyUpdate {
  final int id;
  final Uint8List publicKey;
  final Uint8List signature;

  const SignedPreKeyUpdate({
    required this.id,
    required this.publicKey,
    required this.signature,
  });

  Map<String, dynamic> toJson() => {
        'id': id,
        'public_key': publicKey,
        'signature': signature,
      };
}

/// The header included in the first X3DH message.
class X3dhInitHeader {
  /// Sender's ephemeral public key.
  final Uint8List ephemeralKey;

  /// Sender's identity public key.
  final Uint8List identityKey;

  /// ID of the recipient's signed pre-key used.
  final int signedPreKeyId;

  /// ID of the recipient's one-time pre-key used (null if depleted).
  final int? oneTimePreKeyId;

  const X3dhInitHeader({
    required this.ephemeralKey,
    required this.identityKey,
    required this.signedPreKeyId,
    this.oneTimePreKeyId,
  });

  Map<String, dynamic> toJson() => {
        'ephemeral_key': ephemeralKey,
        'identity_key': identityKey,
        'signed_prekey_id': signedPreKeyId,
        if (oneTimePreKeyId != null) 'one_time_prekey_id': oneTimePreKeyId,
      };

  factory X3dhInitHeader.fromJson(Map<String, dynamic> json) => X3dhInitHeader(
        ephemeralKey: Uint8List.fromList(List<int>.from(json['ephemeral_key'] as List)),
        identityKey: Uint8List.fromList(List<int>.from(json['identity_key'] as List)),
        signedPreKeyId: json['signed_prekey_id'] as int,
        oneTimePreKeyId: json['one_time_prekey_id'] as int?,
      );
}

/// Result of an X3DH initiator operation.
class X3dhInitResult {
  final Uint8List sharedSecret;
  final X3dhInitHeader header;

  const X3dhInitResult({required this.sharedSecret, required this.header});
}

/// Encrypted ciphertext from the Double Ratchet, with its message header.
class RatchetCiphertext {
  final Uint8List ciphertext;
  final RatchetMessageHeader header;

  const RatchetCiphertext({required this.ciphertext, required this.header});

  Map<String, dynamic> toJson() => {
        'ciphertext': ciphertext,
        'header': header.toJson(),
      };

  factory RatchetCiphertext.fromJson(Map<String, dynamic> json) =>
      RatchetCiphertext(
        ciphertext: Uint8List.fromList(List<int>.from(json['ciphertext'] as List)),
        header: RatchetMessageHeader.fromJson(json['header'] as Map<String, dynamic>),
      );
}

/// The Double Ratchet message header.
class RatchetMessageHeader {
  /// Current DH ratchet public key.
  final Uint8List dhPublicKey;

  /// Message number in the current sending chain.
  final int messageNumber;

  /// Number of messages in the previous sending chain.
  final int previousChainLength;

  /// True if this is the first message (X3DH init envelope).
  final bool isInitialMessage;

  /// X3DH init header (only present when isInitialMessage == true).
  final X3dhInitHeader? x3dhHeader;

  const RatchetMessageHeader({
    required this.dhPublicKey,
    required this.messageNumber,
    required this.previousChainLength,
    this.isInitialMessage = false,
    this.x3dhHeader,
  });

  Map<String, dynamic> toJson() => {
        'dh': dhPublicKey,
        'n': messageNumber,
        'pn': previousChainLength,
        if (isInitialMessage) 'init': true,
        if (x3dhHeader != null) 'x3dh': x3dhHeader!.toJson(),
      };

  factory RatchetMessageHeader.fromJson(Map<String, dynamic> json) =>
      RatchetMessageHeader(
        dhPublicKey: Uint8List.fromList(List<int>.from(json['dh'] as List)),
        messageNumber: json['n'] as int,
        previousChainLength: json['pn'] as int,
        isInitialMessage: json['init'] as bool? ?? false,
        x3dhHeader: json['x3dh'] != null
            ? X3dhInitHeader.fromJson(json['x3dh'] as Map<String, dynamic>)
            : null,
      );
}

/// A Sender Key pair for group messaging.
class SenderKeyPair {
  final Uint8List chainKey;
  final Ed25519KeyPair signingKeyPair;

  const SenderKeyPair({required this.chainKey, required this.signingKeyPair});
}
