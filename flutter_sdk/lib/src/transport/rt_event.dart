import 'dart:convert';
import 'dart:typed_data';

/// Sealed class representing all real-time WebSocket event types.
sealed class RtEvent {
  const RtEvent();

  /// Deserializes a JSON frame from the WebSocket into an [RtEvent].
  /// Returns null for unknown frame types (forward compatibility).
  static RtEvent? fromJson(Map<String, dynamic> json) {
    final type = json['type'] as String?;
    switch (type) {
      case 'message':
        return MessageRtEvent.fromJson(json);
      case 'low_otpk':
        return LowOtpkRtEvent.fromJson(json);
      case 'member_added':
        return MemberAddedRtEvent.fromJson(json);
      case 'member_removed':
        return MemberRemovedRtEvent.fromJson(json);
      case 'sender_key_distribution':
        return SenderKeyDistributionRtEvent.fromJson(json);
      case 'ping':
        return const PingRtEvent();
      default:
        return null; // Unknown type — ignore for forward compatibility
    }
  }
}

/// A new encrypted message in a conversation.
class MessageRtEvent extends RtEvent {
  final String conversationId;
  final int seq;
  final String senderUserId;
  final String senderDeviceId;
  final Uint8List ciphertext;
  final Map<String, dynamic> protocolHeader;
  final int serverTs;
  final String? attachmentId;

  const MessageRtEvent({
    required this.conversationId,
    required this.seq,
    required this.senderUserId,
    required this.senderDeviceId,
    required this.ciphertext,
    required this.protocolHeader,
    required this.serverTs,
    this.attachmentId,
  });

  factory MessageRtEvent.fromJson(Map<String, dynamic> json) =>
      MessageRtEvent(
        conversationId: json['conversation_id'] as String,
        seq: json['seq'] as int,
        senderUserId: json['sender_user_id'] as String,
        senderDeviceId: json['sender_device_id'] as String,
        ciphertext: Uint8List.fromList(
            base64.decode(json['ciphertext'] as String)),
        protocolHeader:
            json['protocol_header'] as Map<String, dynamic>? ?? {},
        serverTs: json['server_ts'] as int,
        attachmentId: json['attachment_id'] as String?,
      );
}

/// The server is running low on one-time pre-keys for this device.
class LowOtpkRtEvent extends RtEvent {
  final String deviceId;
  final int count;

  const LowOtpkRtEvent({required this.deviceId, required this.count});

  factory LowOtpkRtEvent.fromJson(Map<String, dynamic> json) =>
      LowOtpkRtEvent(
        deviceId: json['device_id'] as String,
        count: json['count'] as int,
      );
}

/// A new member has been added to a group conversation.
class MemberAddedRtEvent extends RtEvent {
  final String conversationId;
  final String userId;
  final List<String> devices;

  const MemberAddedRtEvent({
    required this.conversationId,
    required this.userId,
    required this.devices,
  });

  factory MemberAddedRtEvent.fromJson(Map<String, dynamic> json) =>
      MemberAddedRtEvent(
        conversationId: json['conversation_id'] as String,
        userId: json['user_id'] as String,
        devices: List<String>.from(json['devices'] as List? ?? []),
      );
}

/// A member has been removed from a group conversation.
class MemberRemovedRtEvent extends RtEvent {
  final String conversationId;
  final String userId;

  const MemberRemovedRtEvent({
    required this.conversationId,
    required this.userId,
  });

  factory MemberRemovedRtEvent.fromJson(Map<String, dynamic> json) =>
      MemberRemovedRtEvent(
        conversationId: json['conversation_id'] as String,
        userId: json['user_id'] as String,
      );
}

/// A sender key distribution message for a group conversation.
class SenderKeyDistributionRtEvent extends RtEvent {
  final String conversationId;
  final String senderUserId;
  final Uint8List encryptedSkdm;

  const SenderKeyDistributionRtEvent({
    required this.conversationId,
    required this.senderUserId,
    required this.encryptedSkdm,
  });

  factory SenderKeyDistributionRtEvent.fromJson(Map<String, dynamic> json) =>
      SenderKeyDistributionRtEvent(
        conversationId: json['conversation_id'] as String,
        senderUserId: json['sender_user_id'] as String,
        encryptedSkdm: Uint8List.fromList(
            base64.decode(json['encrypted_skdm'] as String)),
      );
}

/// Server ping — the SDK responds with pong automatically.
class PingRtEvent extends RtEvent {
  const PingRtEvent();
}
