import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:uuid/uuid.dart';
import 'chat_message.dart';
import '../transport/rest_client.dart';
import '../transport/api_models.dart';
import '../transport/rt_event.dart';
import '../session/session_store.dart';
import '../crypto/ratchet_engine.dart';
import '../crypto/crypto_types.dart';
import '../errors/sdk_error.dart';
import '../utils/logger.dart';

const _uuid = Uuid();

/// Abstract base for 1:1 and group conversations.
///
/// Use [ChatClient.openConversation] or [ChatClient.createGroup] to obtain
/// instances. Never instantiate directly.
abstract class ChatConversation {
  String get conversationId;
  ConversationType get type;
  List<ConversationMember> get currentMembers;

  /// Broadcast stream of all messages, sorted ascending by [ChatMessage.seq].
  Stream<List<ChatMessage>> get messages;

  /// Broadcast stream of current member list.
  Stream<List<ConversationMember>> get members;

  /// Number of unread messages. Resets on [markAsRead].
  Stream<int> get unreadCount;

  /// Current upload progress, or null when no upload is in progress.
  Stream<AttachmentUploadProgress?> get uploadProgress;

  /// Sends a text message. Returns the optimistically-added [ChatMessage].
  Future<ChatMessage> sendMessage(String text);

  /// Uploads [bytes] as an attachment and sends the message.
  Future<ChatMessage> sendAttachment(
      Uint8List bytes, String filename, String contentType);

  /// Fetches historical messages, paging backward from [beforeSeq].
  Future<List<ChatMessage>> fetchHistory({int limit = 50, int? beforeSeq});

  /// Marks all messages in this conversation as read.
  Future<void> markAsRead();

  // Group-only — throws [UnsupportedError] on 1:1 conversations
  Future<void> addMember(String userId);
  Future<void> removeMember(String userId);

  /// Internal: called by [RealTimeEventRouter] on incoming message.
  Future<void> onIncomingMessage(MessageRtEvent event);
}

/// 1:1 conversation implementation using the Double Ratchet.
class OneToOneConversation implements ChatConversation {
  @override
  final String conversationId;

  @override
  final ConversationType type = ConversationType.oneToOne;

  final String _localUserId;
  final String _localDeviceId;
  final RestClient _restClient;
  final SessionStore _sessionStore;
  final SdkLogger _logger;
  final String _baseUrl;
  final String Function() _tokenProvider;

  final _messagesController =
      StreamController<List<ChatMessage>>.broadcast();
  final _membersController =
      StreamController<List<ConversationMember>>.broadcast();
  final _unreadController = StreamController<int>.broadcast();
  final _uploadProgressController =
      StreamController<AttachmentUploadProgress?>.broadcast();

  final List<ChatMessage> _messages = [];
  final List<ConversationMember> _members = [];
  int _unread = 0;

  @override
  List<ConversationMember> get currentMembers => List.unmodifiable(_members);

  OneToOneConversation({
    required this.conversationId,
    required String localUserId,
    required String localDeviceId,
    required RestClient restClient,
    required SessionStore sessionStore,
    required String baseUrl,
    required String Function() tokenProvider,
    List<ConversationMember> initialMembers = const [],
    SdkLogger? logger,
  })  : _localUserId = localUserId,
        _localDeviceId = localDeviceId,
        _restClient = restClient,
        _sessionStore = sessionStore,
        _baseUrl = baseUrl,
        _tokenProvider = tokenProvider,
        _logger = logger ?? SdkLogger(tag: 'OneToOneConversation') {
    _members.addAll(initialMembers);
  }

  @override
  Stream<List<ChatMessage>> get messages => _messagesController.stream;

  @override
  Stream<List<ConversationMember>> get members => _membersController.stream;

  @override
  Stream<int> get unreadCount => _unreadController.stream;

  @override
  Stream<AttachmentUploadProgress?> get uploadProgress =>
      _uploadProgressController.stream;

  @override
  Future<ChatMessage> sendMessage(String text) async {
    try {
      final state = await _sessionStore.loadRatchetState(conversationId);
      if (state == null) {
        throw DecryptionError(conversationId: conversationId, seq: -1);
      }

      final plaintext = utf8.encode(text);
      final (newState, ratchetCiphertext) =
          await RatchetEngine.encrypt(state, Uint8List.fromList(plaintext));

      // Persist state BEFORE sending (forward secrecy guarantee)
      await _sessionStore.saveRatchetState(conversationId, newState);

      final ciphertextB64 = base64.encode(ratchetCiphertext.ciphertext);
      final headerJson = ratchetCiphertext.header.toJson();
      // Convert Uint8List values in header to base64 for JSON transport
      final headerEncoded = _encodeHeaderForTransport(headerJson);

      final response = await _restClient.sendMessage(
        conversationId,
        SendMessageRequest(
          ciphertext: ciphertextB64,
          protocolHeader: headerEncoded,
        ),
      );

      final message = ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: _localUserId,
        senderDeviceId: _localDeviceId,
        type: ChatMessageType.text,
        text: text, // Only time plaintext exists in the SDK
        timestamp: DateTime.fromMillisecondsSinceEpoch(response.serverTs),
        seq: response.seq,
        isMine: true,
      );

      _addMessage(message);
      return message;
    } on SdkError {
      rethrow;
    } catch (e) {
      throw UnknownError(cause: e);
    }
  }

  @override
  Future<ChatMessage> sendAttachment(
      Uint8List bytes, String filename, String contentType) async {
    if (bytes.length > 100 * 1024 * 1024) {
      throw FileTooLargeError(sizeBytes: bytes.length);
    }

    final uploadResult = await _restClient.uploadAttachment(
      bytes,
      filename,
      contentType,
      onProgress: (sent, total) {
        _uploadProgressController.add(AttachmentUploadProgress(
          filename: filename,
          bytesUploaded: sent,
          totalBytes: total,
        ));
      },
    );
    _uploadProgressController.add(null);

    // Send a message envelope referencing the attachment
    final state = await _sessionStore.loadRatchetState(conversationId);
    if (state == null) {
      throw DecryptionError(conversationId: conversationId, seq: -1);
    }

    final payload = utf8.encode(
        jsonEncode({'attachment_id': uploadResult.attachmentId, 'filename': filename}));
    final (newState, ratchetCiphertext) =
        await RatchetEngine.encrypt(state, Uint8List.fromList(payload));
    await _sessionStore.saveRatchetState(conversationId, newState);

    final response = await _restClient.sendMessage(
      conversationId,
      SendMessageRequest(
        ciphertext: base64.encode(ratchetCiphertext.ciphertext),
        protocolHeader: _encodeHeaderForTransport(ratchetCiphertext.header.toJson()),
      ),
    );

    final attachmentUrl =
        '$_baseUrl/attachments/${uploadResult.attachmentId}?token=${_tokenProvider()}';

    final message = ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: _localUserId,
      senderDeviceId: _localDeviceId,
      type: ChatMessageType.attachment,
      attachmentUrl: attachmentUrl,
      attachmentName: filename,
      timestamp: DateTime.fromMillisecondsSinceEpoch(response.serverTs),
      seq: response.seq,
      isMine: true,
    );

    _addMessage(message);
    return message;
  }

  @override
  Future<List<ChatMessage>> fetchHistory(
      {int limit = 50, int? beforeSeq}) async {
    final response = await _restClient.getMessages(
      conversationId,
      limit: limit,
      beforeSeq: beforeSeq,
    );

    final decrypted = <ChatMessage>[];
    for (final envelope in response.messages) {
      final msg = await _decryptEnvelope(envelope);
      decrypted.add(msg);
    }

    // Merge into local cache (avoid duplicates by seq)
    for (final msg in decrypted) {
      if (!_messages.any((m) => m.seq == msg.seq)) {
        _messages.add(msg);
      }
    }
    _messages.sort((a, b) => a.seq.compareTo(b.seq));
    _messagesController.add(List.unmodifiable(_messages));

    return decrypted;
  }

  @override
  Future<void> markAsRead() async {
    _unread = 0;
    _unreadController.add(0);
  }

  @override
  Future<void> onIncomingMessage(MessageRtEvent event) async {
    final msg = await _decryptEnvelope(ServerMessageEnvelope(
      id: _uuid.v4(),
      senderUserId: event.senderUserId,
      senderDeviceId: event.senderDeviceId,
      ciphertext: event.ciphertext,
      protocolHeader: event.protocolHeader,
      seq: event.seq,
      serverTs: DateTime.fromMillisecondsSinceEpoch(event.serverTs),
      attachmentId: event.attachmentId,
    ));

    _addMessage(msg);
    _unread++;
    _unreadController.add(_unread);
  }

  @override
  Future<void> addMember(String userId) =>
      throw UnsupportedError('Cannot add members to a 1:1 conversation');

  @override
  Future<void> removeMember(String userId) =>
      throw UnsupportedError('Cannot remove members from a 1:1 conversation');

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  Future<ChatMessage> _decryptEnvelope(ServerMessageEnvelope envelope) async {
    try {
      var state = await _sessionStore.loadRatchetState(conversationId);
      if (state == null) {
        return _errorMessage(envelope);
      }

      // Rebuild RatchetCiphertext from envelope
      final headerJson =
          _decodeHeaderFromTransport(envelope.protocolHeader);
      final header = RatchetMessageHeader.fromJson(headerJson);
      final ratchetCiphertext = RatchetCiphertext(
        ciphertext: envelope.ciphertext,
        header: header,
      );

      final (newState, plaintext) =
          await RatchetEngine.decrypt(state, ratchetCiphertext);

      // Persist updated state
      await _sessionStore.saveRatchetState(conversationId, newState);

      String? text;
      String? attachmentUrl;
      String? attachmentName;
      ChatMessageType type = ChatMessageType.text;

      if (envelope.attachmentId != null) {
        type = ChatMessageType.attachment;
        attachmentUrl =
            '$_baseUrl/attachments/${envelope.attachmentId}?token=${_tokenProvider()}';
        try {
          final decoded = jsonDecode(utf8.decode(plaintext)) as Map<String, dynamic>;
          attachmentName = decoded['filename'] as String?;
        } catch (_) {}
      } else {
        text = utf8.decode(plaintext);
      }

      return ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: envelope.senderUserId,
        senderDeviceId: envelope.senderDeviceId,
        type: type,
        text: text,
        attachmentUrl: attachmentUrl,
        attachmentName: attachmentName,
        timestamp: envelope.serverTs,
        seq: envelope.seq,
        isMine: envelope.senderUserId == _localUserId,
      );
    } catch (e) {
      _logger.error('Decryption failed for seq ${envelope.seq}: $e');
      return _errorMessage(envelope);
    }
  }

  ChatMessage _errorMessage(ServerMessageEnvelope envelope) => ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: envelope.senderUserId,
        senderDeviceId: envelope.senderDeviceId,
        type: ChatMessageType.text,
        timestamp: envelope.serverTs,
        seq: envelope.seq,
        isMine: false,
        decryptionError: true,
      );

  void _addMessage(ChatMessage message) {
    final existingIdx = _messages.indexWhere((m) => m.seq == message.seq);
    if (existingIdx >= 0) {
      _messages[existingIdx] = message;
    } else {
      _messages.add(message);
      _messages.sort((a, b) => a.seq.compareTo(b.seq));
    }
    _messagesController.add(List.unmodifiable(_messages));
  }

  /// Encodes Uint8List values in the protocol header to base64 for JSON transport.
  Map<String, dynamic> _encodeHeaderForTransport(Map<String, dynamic> header) {
    return header.map((k, v) {
      if (v is Uint8List) return MapEntry(k, base64.encode(v));
      if (v is Map<String, dynamic>) return MapEntry(k, _encodeHeaderForTransport(v));
      return MapEntry(k, v);
    });
  }

  /// Decodes base64 strings back to Uint8List for the protocol header.
  Map<String, dynamic> _decodeHeaderFromTransport(Map<String, dynamic> header) {
    // Keys that are always raw bytes in the header
    const bytesKeys = {'dh', 'ephemeral_key', 'identity_key'};
    return header.map((k, v) {
      if (bytesKeys.contains(k) && v is String) {
        return MapEntry(k, Uint8List.fromList(base64.decode(v)));
      }
      if (v is Map<String, dynamic>) return MapEntry(k, _decodeHeaderFromTransport(v));
      return MapEntry(k, v);
    });
  }

  void dispose() {
    _messagesController.close();
    _membersController.close();
    _unreadController.close();
    _uploadProgressController.close();
  }
}
