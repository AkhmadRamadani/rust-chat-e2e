import 'dart:convert';
import 'dart:typed_data';
import '../crypto/crypto_types.dart';

/// Stored private key material for a registered device.
class DeviceRecord {
  final String userId;
  final String deviceId;
  final Uint8List identityKeyPrivate;
  final Uint8List identityKeyPublic;
  final Uint8List identitySigningKeyPrivate;
  final Uint8List identitySigningKeyPublic;
  final Uint8List signedPrekeyPrivate;
  final Uint8List signedPrekeyPublic;
  final int signedPrekeyId;
  final List<OtpkRecord> oneTimePrekeys;
  final DateTime signedPrekeyCreatedAt;

  const DeviceRecord({
    required this.userId,
    required this.deviceId,
    required this.identityKeyPrivate,
    required this.identityKeyPublic,
    required this.identitySigningKeyPrivate,
    required this.identitySigningKeyPublic,
    required this.signedPrekeyPrivate,
    required this.signedPrekeyPublic,
    required this.signedPrekeyId,
    required this.oneTimePrekeys,
    required this.signedPrekeyCreatedAt,
  });

  String toJsonString() => jsonEncode({
        'userId': userId,
        'deviceId': deviceId,
        'identityKeyPrivate': base64.encode(identityKeyPrivate),
        'identityKeyPublic': base64.encode(identityKeyPublic),
        'identitySigningKeyPrivate': base64.encode(identitySigningKeyPrivate),
        'identitySigningKeyPublic': base64.encode(identitySigningKeyPublic),
        'signedPrekeyPrivate': base64.encode(signedPrekeyPrivate),
        'signedPrekeyPublic': base64.encode(signedPrekeyPublic),
        'signedPrekeyId': signedPrekeyId,
        'oneTimePrekeys': oneTimePrekeys.map((k) => {
              'id': k.id,
              'privateKey': base64.encode(k.privateKey),
              'publicKey': base64.encode(k.publicKey),
            }).toList(),
        'signedPrekeyCreatedAt': signedPrekeyCreatedAt.toIso8601String(),
      });

  factory DeviceRecord.fromJsonString(String json) {
    final m = jsonDecode(json) as Map<String, dynamic>;
    return DeviceRecord(
      userId: m['userId'] as String,
      deviceId: m['deviceId'] as String,
      identityKeyPrivate: base64.decode(m['identityKeyPrivate'] as String),
      identityKeyPublic: base64.decode(m['identityKeyPublic'] as String),
      identitySigningKeyPrivate:
          base64.decode(m['identitySigningKeyPrivate'] as String),
      identitySigningKeyPublic:
          base64.decode(m['identitySigningKeyPublic'] as String),
      signedPrekeyPrivate: base64.decode(m['signedPrekeyPrivate'] as String),
      signedPrekeyPublic: base64.decode(m['signedPrekeyPublic'] as String),
      signedPrekeyId: m['signedPrekeyId'] as int,
      oneTimePrekeys: (m['oneTimePrekeys'] as List<dynamic>)
          .map((e) {
            final entry = e as Map<String, dynamic>;
            return OtpkRecord(
              id: entry['id'] as int,
              privateKey: base64.decode(entry['privateKey'] as String),
              publicKey: base64.decode(entry['publicKey'] as String),
            );
          })
          .toList(),
      signedPrekeyCreatedAt:
          DateTime.parse(m['signedPrekeyCreatedAt'] as String),
    );
  }

  DeviceRecord copyWith({List<OtpkRecord>? oneTimePrekeys}) => DeviceRecord(
        userId: userId,
        deviceId: deviceId,
        identityKeyPrivate: identityKeyPrivate,
        identityKeyPublic: identityKeyPublic,
        identitySigningKeyPrivate: identitySigningKeyPrivate,
        identitySigningKeyPublic: identitySigningKeyPublic,
        signedPrekeyPrivate: signedPrekeyPrivate,
        signedPrekeyPublic: signedPrekeyPublic,
        signedPrekeyId: signedPrekeyId,
        oneTimePrekeys: oneTimePrekeys ?? this.oneTimePrekeys,
        signedPrekeyCreatedAt: signedPrekeyCreatedAt,
      );
}

/// Serializable state of a Double Ratchet session.
class RatchetState {
  final String conversationId;
  final Uint8List rootKey;
  final Uint8List chainKeySend;
  final Uint8List chainKeyRecv;
  final Uint8List dhSendPub;
  final Uint8List dhSendPriv;
  final Uint8List dhRecvPub;
  final int nSend;
  final int nRecv;
  final int pn;
  final Map<int, Uint8List> skippedMessageKeys;

  const RatchetState({
    required this.conversationId,
    required this.rootKey,
    required this.chainKeySend,
    required this.chainKeyRecv,
    required this.dhSendPub,
    required this.dhSendPriv,
    required this.dhRecvPub,
    required this.nSend,
    required this.nRecv,
    required this.pn,
    required this.skippedMessageKeys,
  });

  RatchetState copyWith({
    Uint8List? rootKey,
    Uint8List? chainKeySend,
    Uint8List? chainKeyRecv,
    Uint8List? dhSendPub,
    Uint8List? dhSendPriv,
    Uint8List? dhRecvPub,
    int? nSend,
    int? nRecv,
    int? pn,
    Map<int, Uint8List>? skippedMessageKeys,
  }) =>
      RatchetState(
        conversationId: conversationId,
        rootKey: rootKey ?? this.rootKey,
        chainKeySend: chainKeySend ?? this.chainKeySend,
        chainKeyRecv: chainKeyRecv ?? this.chainKeyRecv,
        dhSendPub: dhSendPub ?? this.dhSendPub,
        dhSendPriv: dhSendPriv ?? this.dhSendPriv,
        dhRecvPub: dhRecvPub ?? this.dhRecvPub,
        nSend: nSend ?? this.nSend,
        nRecv: nRecv ?? this.nRecv,
        pn: pn ?? this.pn,
        skippedMessageKeys: skippedMessageKeys ?? this.skippedMessageKeys,
      );

  String toJsonString() => jsonEncode({
        'conversationId': conversationId,
        'rootKey': base64.encode(rootKey),
        'chainKeySend': base64.encode(chainKeySend),
        'chainKeyRecv': base64.encode(chainKeyRecv),
        'dhSendPub': base64.encode(dhSendPub),
        'dhSendPriv': base64.encode(dhSendPriv),
        'dhRecvPub': base64.encode(dhRecvPub),
        'nSend': nSend,
        'nRecv': nRecv,
        'pn': pn,
        'skippedMessageKeys': skippedMessageKeys
            .map((k, v) => MapEntry(k.toString(), base64.encode(v))),
      });

  factory RatchetState.fromJsonString(String json) {
    final m = jsonDecode(json) as Map<String, dynamic>;
    return RatchetState(
      conversationId: m['conversationId'] as String,
      rootKey: base64.decode(m['rootKey'] as String),
      chainKeySend: base64.decode(m['chainKeySend'] as String),
      chainKeyRecv: base64.decode(m['chainKeyRecv'] as String),
      dhSendPub: base64.decode(m['dhSendPub'] as String),
      dhSendPriv: base64.decode(m['dhSendPriv'] as String),
      dhRecvPub: base64.decode(m['dhRecvPub'] as String),
      nSend: m['nSend'] as int,
      nRecv: m['nRecv'] as int,
      pn: m['pn'] as int,
      skippedMessageKeys:
          (m['skippedMessageKeys'] as Map<String, dynamic>).map(
        (k, v) => MapEntry(int.parse(k), base64.decode(v as String)),
      ),
    );
  }
}

/// Stored Sender Key record for a group conversation participant.
class SenderKeyRecord {
  final String conversationId;
  final String userId;
  final Uint8List chainKey;
  final int iteration;
  final Uint8List signingKeyPublic;
  final Uint8List? signingKeyPrivate;

  const SenderKeyRecord({
    required this.conversationId,
    required this.userId,
    required this.chainKey,
    required this.iteration,
    required this.signingKeyPublic,
    this.signingKeyPrivate,
  });

  SenderKeyRecord copyWith({Uint8List? chainKey, int? iteration}) =>
      SenderKeyRecord(
        conversationId: conversationId,
        userId: userId,
        chainKey: chainKey ?? this.chainKey,
        iteration: iteration ?? this.iteration,
        signingKeyPublic: signingKeyPublic,
        signingKeyPrivate: signingKeyPrivate,
      );

  String toJsonString() => jsonEncode({
        'conversationId': conversationId,
        'userId': userId,
        'chainKey': base64.encode(chainKey),
        'iteration': iteration,
        'signingKeyPublic': base64.encode(signingKeyPublic),
        if (signingKeyPrivate != null)
          'signingKeyPrivate': base64.encode(signingKeyPrivate!),
      });

  factory SenderKeyRecord.fromJsonString(String json) {
    final m = jsonDecode(json) as Map<String, dynamic>;
    return SenderKeyRecord(
      conversationId: m['conversationId'] as String,
      userId: m['userId'] as String,
      chainKey: base64.decode(m['chainKey'] as String),
      iteration: m['iteration'] as int,
      signingKeyPublic: base64.decode(m['signingKeyPublic'] as String),
      signingKeyPrivate: m['signingKeyPrivate'] != null
          ? base64.decode(m['signingKeyPrivate'] as String)
          : null,
    );
  }
}

/// Metadata about a conversation stored locally.
class ConversationMeta {
  final String conversationId;
  final String type; // 'oneToOne' | 'group'
  final List<String> memberUserIds;
  final DateTime createdAt;

  const ConversationMeta({
    required this.conversationId,
    required this.type,
    required this.memberUserIds,
    required this.createdAt,
  });

  String toJsonString() => jsonEncode({
        'conversationId': conversationId,
        'type': type,
        'memberUserIds': memberUserIds,
        'createdAt': createdAt.toIso8601String(),
      });

  factory ConversationMeta.fromJsonString(String json) {
    final m = jsonDecode(json) as Map<String, dynamic>;
    return ConversationMeta(
      conversationId: m['conversationId'] as String,
      type: m['type'] as String,
      memberUserIds: List<String>.from(m['memberUserIds'] as List),
      createdAt: DateTime.parse(m['createdAt'] as String),
    );
  }
}
