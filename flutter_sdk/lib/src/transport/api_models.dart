import 'dart:convert';
import 'dart:typed_data';

// ---------------------------------------------------------------------------
// KDS (Key Distribution Server) models
// ---------------------------------------------------------------------------

class RegisterDeviceRequest {
  final String userId;
  final String identityKey;
  final String signedPreKey;
  final String signedPreKeySignature;
  final int signedPreKeyId;
  final List<Map<String, dynamic>> oneTimePreKeys;

  const RegisterDeviceRequest({
    required this.userId,
    required this.identityKey,
    required this.signedPreKey,
    required this.signedPreKeySignature,
    required this.signedPreKeyId,
    required this.oneTimePreKeys,
  });

  Map<String, dynamic> toJson() => {
        'user_id': userId,
        'identity_key': identityKey,
        'signed_prekey': signedPreKey,
        'signed_prekey_signature': signedPreKeySignature,
        'signed_prekey_id': signedPreKeyId,
        'one_time_prekeys': oneTimePreKeys,
      };
}

class RegisterDeviceResponse {
  final String deviceId;

  const RegisterDeviceResponse({required this.deviceId});

  factory RegisterDeviceResponse.fromJson(Map<String, dynamic> json) =>
      RegisterDeviceResponse(deviceId: json['device_id'] as String);
}

/// Server-provided key bundle for a recipient device.
class KeyBundleResponse {
  final String userId;
  final String deviceId;
  final Uint8List identityKey;
  final Uint8List signedPreKey;
  final Uint8List signedPreKeySignature;
  final int signedPreKeyId;
  final OneTimePreKeyResponse? oneTimePreKey;

  const KeyBundleResponse({
    required this.userId,
    required this.deviceId,
    required this.identityKey,
    required this.signedPreKey,
    required this.signedPreKeySignature,
    required this.signedPreKeyId,
    this.oneTimePreKey,
  });

  factory KeyBundleResponse.fromJson(Map<String, dynamic> json) =>
      KeyBundleResponse(
        userId: json['user_id'] as String,
        deviceId: json['device_id'] as String,
        identityKey:
            Uint8List.fromList(base64.decode(json['identity_key'] as String)),
        signedPreKey:
            Uint8List.fromList(base64.decode(json['signed_prekey'] as String)),
        signedPreKeySignature: Uint8List.fromList(
            base64.decode(json['signed_prekey_signature'] as String)),
        signedPreKeyId: json['signed_prekey_id'] as int,
        oneTimePreKey: json['one_time_prekey'] != null
            ? OneTimePreKeyResponse.fromJson(
                json['one_time_prekey'] as Map<String, dynamic>)
            : null,
      );
}

class OneTimePreKeyResponse {
  final int id;
  final Uint8List publicKey;

  const OneTimePreKeyResponse({required this.id, required this.publicKey});

  factory OneTimePreKeyResponse.fromJson(Map<String, dynamic> json) =>
      OneTimePreKeyResponse(
        id: json['id'] as int,
        publicKey:
            Uint8List.fromList(base64.decode(json['public_key'] as String)),
      );
}

// ---------------------------------------------------------------------------
// Conversation models
// ---------------------------------------------------------------------------

class CreateConversationRequest {
  final String recipientUserId;
  final String recipientDeviceId;
  final Map<String, dynamic> initialEnvelope;

  const CreateConversationRequest({
    required this.recipientUserId,
    required this.recipientDeviceId,
    required this.initialEnvelope,
  });

  Map<String, dynamic> toJson() => {
        'recipient_user_id': recipientUserId,
        'recipient_device_id': recipientDeviceId,
        'initial_envelope': initialEnvelope,
      };
}

class CreateConversationResponse {
  final String conversationId;

  const CreateConversationResponse({required this.conversationId});

  factory CreateConversationResponse.fromJson(Map<String, dynamic> json) =>
      CreateConversationResponse(
          conversationId: json['conversation_id'] as String);
}

class SendMessageRequest {
  final String ciphertext;
  final Map<String, dynamic> protocolHeader;

  const SendMessageRequest({
    required this.ciphertext,
    required this.protocolHeader,
  });

  Map<String, dynamic> toJson() => {
        'ciphertext': ciphertext,
        'protocol_header': protocolHeader,
      };
}

class SendMessageResponse {
  final int seq;
  final int serverTs;

  const SendMessageResponse({required this.seq, required this.serverTs});

  factory SendMessageResponse.fromJson(Map<String, dynamic> json) =>
      SendMessageResponse(
        seq: json['seq'] as int,
        serverTs: json['server_ts'] as int,
      );
}

class ServerMessageEnvelope {
  final String id;
  final String senderUserId;
  final String senderDeviceId;
  final Uint8List ciphertext;
  final Map<String, dynamic> protocolHeader;
  final int seq;
  final DateTime serverTs;
  final String? attachmentId;

  const ServerMessageEnvelope({
    required this.id,
    required this.senderUserId,
    required this.senderDeviceId,
    required this.ciphertext,
    required this.protocolHeader,
    required this.seq,
    required this.serverTs,
    this.attachmentId,
  });

  factory ServerMessageEnvelope.fromJson(Map<String, dynamic> json) =>
      ServerMessageEnvelope(
        id: json['id'] as String,
        senderUserId: json['sender_user_id'] as String,
        senderDeviceId: json['sender_device_id'] as String,
        ciphertext: Uint8List.fromList(
            base64.decode(json['ciphertext'] as String)),
        protocolHeader:
            json['protocol_header'] as Map<String, dynamic>,
        seq: json['seq'] as int,
        serverTs: DateTime.fromMillisecondsSinceEpoch(json['server_ts'] as int),
        attachmentId: json['attachment_id'] as String?,
      );
}

class GetMessagesResponse {
  final List<ServerMessageEnvelope> messages;
  final bool hasMore;

  const GetMessagesResponse({required this.messages, required this.hasMore});

  factory GetMessagesResponse.fromJson(Map<String, dynamic> json) =>
      GetMessagesResponse(
        messages: (json['messages'] as List)
            .map((e) =>
                ServerMessageEnvelope.fromJson(e as Map<String, dynamic>))
            .toList(),
        hasMore: json['has_more'] as bool? ?? false,
      );
}

// ---------------------------------------------------------------------------
// Group models
// ---------------------------------------------------------------------------

class CreateGroupRequest {
  final List<String> memberUserIds;

  const CreateGroupRequest({required this.memberUserIds});

  Map<String, dynamic> toJson() => {'member_user_ids': memberUserIds};
}

class CreateGroupResponse {
  final String conversationId;

  const CreateGroupResponse({required this.conversationId});

  factory CreateGroupResponse.fromJson(Map<String, dynamic> json) =>
      CreateGroupResponse(conversationId: json['conversation_id'] as String);
}

class SkdmRecipient {
  final String userId;
  final String deviceId;
  final String encryptedSkdm;

  const SkdmRecipient({
    required this.userId,
    required this.deviceId,
    required this.encryptedSkdm,
  });

  Map<String, dynamic> toJson() => {
        'user_id': userId,
        'device_id': deviceId,
        'encrypted_skdm': encryptedSkdm,
      };
}

// ---------------------------------------------------------------------------
// Attachment models
// ---------------------------------------------------------------------------

class UploadResponse {
  final String attachmentId;
  final String downloadUrl;

  const UploadResponse({required this.attachmentId, required this.downloadUrl});

  factory UploadResponse.fromJson(Map<String, dynamic> json) => UploadResponse(
        attachmentId: json['attachment_id'] as String,
        downloadUrl: json['download_url'] as String,
      );
}

// ---------------------------------------------------------------------------
// One-time pre-key upload
// ---------------------------------------------------------------------------

class ReplenishOtpksRequest {
  final List<Map<String, dynamic>> oneTimePreKeys;

  const ReplenishOtpksRequest({required this.oneTimePreKeys});

  Map<String, dynamic> toJson() => {'one_time_prekeys': oneTimePreKeys};
}

class ReplenishOtpksResponse {
  final int uploadedCount;

  const ReplenishOtpksResponse({required this.uploadedCount});

  factory ReplenishOtpksResponse.fromJson(Map<String, dynamic> json) =>
      ReplenishOtpksResponse(uploadedCount: json['uploaded_count'] as int? ?? 0);
}
