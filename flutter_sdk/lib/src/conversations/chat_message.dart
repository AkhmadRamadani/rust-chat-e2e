/// The type of a chat message.
enum ChatMessageType {
  text,
  attachment,
  memberAdded,
  memberRemoved,
  system,
}

/// A decrypted, developer-facing chat message.
///
/// If [decryptionError] is true, [text] is null and the message could not be
/// decrypted. This is surfaced rather than crashing so the UI can show a
/// "message not available" placeholder.
class ChatMessage {
  /// Local unique ID (UUID v4).
  final String id;

  final String conversationId;
  final String senderUserId;
  final String? senderDeviceId;
  final ChatMessageType type;

  /// Decrypted message text. Null if attachment-only or [decryptionError].
  final String? text;

  /// Authenticated download URL for an attachment, if any.
  final String? attachmentUrl;

  /// Original filename of the attachment, if any.
  final String? attachmentName;

  final DateTime timestamp;

  /// Server-assigned sequence number.
  final int seq;

  /// True if this message was sent by the local user.
  final bool isMine;

  /// True if decryption of this message failed.
  final bool decryptionError;

  const ChatMessage({
    required this.id,
    required this.conversationId,
    required this.senderUserId,
    this.senderDeviceId,
    required this.type,
    this.text,
    this.attachmentUrl,
    this.attachmentName,
    required this.timestamp,
    required this.seq,
    required this.isMine,
    this.decryptionError = false,
  });

  @override
  String toString() =>
      'ChatMessage(id: $id, seq: $seq, type: $type, isMine: $isMine, '
      'decryptionError: $decryptionError, text: [REDACTED])';
}

/// A member of a conversation.
class ConversationMember {
  final String userId;
  final String? deviceId;
  final String? displayName;

  const ConversationMember({
    required this.userId,
    this.deviceId,
    this.displayName,
  });
}

/// The type of a conversation.
enum ConversationType { oneToOne, group }

/// Progress of an attachment upload.
class AttachmentUploadProgress {
  final String filename;
  final int bytesUploaded;
  final int totalBytes;

  const AttachmentUploadProgress({
    required this.filename,
    required this.bytesUploaded,
    required this.totalBytes,
  });

  double get fraction =>
      totalBytes > 0 ? bytesUploaded / totalBytes : 0.0;
}
