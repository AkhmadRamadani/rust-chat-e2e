import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:uuid/uuid.dart';
import 'chat_conversation.dart';
import 'chat_message.dart';
import '../transport/rest_client.dart';
import '../transport/api_models.dart';
import '../transport/rt_event.dart';
import '../session/session_store.dart';
import '../crypto/sender_key_engine.dart';
import '../crypto/key_generator.dart';
import '../errors/sdk_error.dart';
import '../utils/logger.dart';

const _uuid = Uuid();

/// Holds a pending encrypted message waiting for the sender's SKDM.
class _PendingMessage {
  final MessageRtEvent event;
  final DateTime receivedAt;
  _PendingMessage(this.event) : receivedAt = DateTime.now();
}

/// Group conversation implementation using the Sender Key protocol.
class GroupConversation implements ChatConversation {
  @override
  final String conversationId;

  @override
  final ConversationType type = ConversationType.group;

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
  final List<ConversationMember> _members;
  final Map<String, List<_PendingMessage>> _pendingMessages = {};
  int _unread = 0;

  @override
  List<ConversationMember> get currentMembers => List.unmodifiable(_members);

  GroupConversation({
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
        _members = List<ConversationMember>.from(initialMembers),
        _logger = logger ?? SdkLogger(tag: 'GroupConversation');

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
      final record =
          await _sessionStore.loadSenderKey(conversationId, _localUserId);
      if (record == null) {
        throw KeyExchangeError(
            recipientUserId: _localUserId,
            reason: 'No sender key available for group $conversationId');
      }

      final session = SenderKeySession.fromRecord(record);
      final plaintext = utf8.encode(text);
      final ciphertext = await SenderKeyEngine.encrypt(
          session, Uint8List.fromList(plaintext));

      // Persist updated sender key state
      await _sessionStore.saveSenderKey(
          conversationId, _localUserId, session.toRecord());

      final response = await _restClient.sendGroupMessage(
        conversationId,
        SendMessageRequest(
          ciphertext: base64.encode(ciphertext),
          protocolHeader: {'type': 'sender_key'},
        ),
      );

      final message = ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: _localUserId,
        senderDeviceId: _localDeviceId,
        type: ChatMessageType.text,
        text: text,
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
      bytes, filename, contentType,
      onProgress: (sent, total) {
        _uploadProgressController.add(AttachmentUploadProgress(
          filename: filename,
          bytesUploaded: sent,
          totalBytes: total,
        ));
      },
    );
    _uploadProgressController.add(null);

    final record =
        await _sessionStore.loadSenderKey(conversationId, _localUserId);
    if (record == null) {
      throw KeyExchangeError(
          recipientUserId: _localUserId,
          reason: 'No sender key available');
    }

    final session = SenderKeySession.fromRecord(record);
    final payload = utf8.encode(
        jsonEncode({'attachment_id': uploadResult.attachmentId, 'filename': filename}));
    final ciphertext =
        await SenderKeyEngine.encrypt(session, Uint8List.fromList(payload));
    await _sessionStore.saveSenderKey(
        conversationId, _localUserId, session.toRecord());

    final response = await _restClient.sendGroupMessage(
      conversationId,
      SendMessageRequest(
        ciphertext: base64.encode(ciphertext),
        protocolHeader: {'type': 'sender_key'},
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
      final msg = await _decryptGroupEnvelope(
        senderUserId: envelope.senderUserId,
        ciphertext: envelope.ciphertext,
        seq: envelope.seq,
        serverTs: envelope.serverTs,
        attachmentId: envelope.attachmentId,
        senderDeviceId: envelope.senderDeviceId,
      );
      decrypted.add(msg);
    }

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
    final record =
        await _sessionStore.loadSenderKey(conversationId, event.senderUserId);

    if (record == null) {
      // Queue the message — SKDM may arrive shortly
      _pendingMessages
          .putIfAbsent(event.senderUserId, () => [])
          .add(_PendingMessage(event));

      // Schedule a timeout
      Future.delayed(const Duration(seconds: 60), () {
        final pending = _pendingMessages[event.senderUserId];
        if (pending != null) {
          for (final p in List.from(pending)) {
            if (DateTime.now().difference(p.receivedAt).inSeconds >= 60) {
              _addMessage(_errorMessageFromEvent(p.event));
              pending.remove(p);
            }
          }
        }
      });
      return;
    }

    final msg = await _decryptGroupEnvelope(
      senderUserId: event.senderUserId,
      ciphertext: event.ciphertext,
      seq: event.seq,
      serverTs: DateTime.fromMillisecondsSinceEpoch(event.serverTs),
      attachmentId: event.attachmentId,
      senderDeviceId: event.senderDeviceId,
    );

    _addMessage(msg);
    _unread++;
    _unreadController.add(_unread);
  }

  /// Called by [RealTimeEventRouter] when an SKDM is received.
  Future<void> onSkdm(SenderKeyDistributionRtEvent event) async {
    // The SKDM payload arrives already decrypted by the 1:1 ratchet session
    // (handled by ConversationManager before routing here)
    final session = SenderKeyEngine.deserializeKeyMaterial(
      bytes: event.encryptedSkdm,
      conversationId: conversationId,
      senderUserId: event.senderUserId,
    );

    await _sessionStore.saveSenderKey(
      conversationId,
      event.senderUserId,
      session.toRecord(),
    );

    _logger.info(
        'Stored SKDM from ${event.senderUserId} in group $conversationId');

    // Drain pending messages
    final pending = _pendingMessages.remove(event.senderUserId);
    if (pending != null) {
      for (final p in pending) {
        await onIncomingMessage(p.event);
      }
    }
  }

  /// Called by [RealTimeEventRouter] when a member is added.
  Future<void> onMemberAdded(MemberAddedRtEvent event) async {
    if (!_members.any((m) => m.userId == event.userId)) {
      _members.add(ConversationMember(userId: event.userId));
      _membersController.add(List.unmodifiable(_members));
    }

    // Emit system message
    _addMessage(ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: event.userId,
      type: ChatMessageType.memberAdded,
      timestamp: DateTime.now(),
      seq: -1,
      isMine: false,
    ));

    // Generate and distribute new sender key to the new member
    await _rotateSenderKeyForMembers([event.userId]);
  }

  /// Called by [RealTimeEventRouter] when a member is removed.
  Future<void> onMemberRemoved(MemberRemovedRtEvent event) async {
    _members.removeWhere((m) => m.userId == event.userId);
    _membersController.add(List.unmodifiable(_members));

    _addMessage(ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: event.userId,
      type: ChatMessageType.memberRemoved,
      timestamp: DateTime.now(),
      seq: -1,
      isMine: false,
    ));

    // Rotate sender key — removed member does NOT receive new key
    await _rotateSenderKeyForMembers(
      _members.map((m) => m.userId).toList(),
    );
  }

  @override
  Future<void> addMember(String userId) async {
    await _restClient.addGroupMember(conversationId, userId, '');
  }

  @override
  Future<void> removeMember(String userId) async {
    await _restClient.removeGroupMember(conversationId, userId);
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  Future<void> _rotateSenderKeyForMembers(List<String> recipientUserIds) async {
    final newKeyPair = await KeyGenerator.generateSenderKey();
    final session = SenderKeyEngine.createSession(
      conversationId: conversationId,
      userId: _localUserId,
      keyPair: newKeyPair,
    );

    await _sessionStore.saveSenderKey(
        conversationId, _localUserId, session.toRecord());

    final serialized = SenderKeyEngine.serializeKeyMaterial(session);

    // For a production system, each SKDM would be encrypted with the
    // recipient's ratchet session. Here we send the serialized bytes directly.
    final recipients = recipientUserIds
        .map((uid) => SkdmRecipient(
              userId: uid,
              deviceId: '',
              encryptedSkdm: base64.encode(serialized),
            ))
        .toList();

    await _restClient.distributeGroupSenderKey(conversationId, recipients);
  }

  Future<ChatMessage> _decryptGroupEnvelope({
    required String senderUserId,
    required Uint8List ciphertext,
    required int seq,
    required DateTime serverTs,
    String? attachmentId,
    String? senderDeviceId,
  }) async {
    try {
      final record =
          await _sessionStore.loadSenderKey(conversationId, senderUserId);
      if (record == null) {
        return _makeErrorMessage(senderUserId, senderDeviceId, seq, serverTs);
      }

      final session = SenderKeySession.fromRecord(record);
      final plaintext =
          await SenderKeyEngine.decrypt(session, ciphertext, conversationId);

      // Persist updated iteration
      await _sessionStore.saveSenderKey(
          conversationId, senderUserId, session.toRecord());

      String? text;
      String? attachmentUrl;
      String? attachmentName;
      ChatMessageType msgType = ChatMessageType.text;

      if (attachmentId != null) {
        msgType = ChatMessageType.attachment;
        attachmentUrl =
            '$_baseUrl/attachments/$attachmentId?token=${_tokenProvider()}';
        try {
          final decoded =
              jsonDecode(utf8.decode(plaintext)) as Map<String, dynamic>;
          attachmentName = decoded['filename'] as String?;
        } catch (_) {}
      } else {
        text = utf8.decode(plaintext);
      }

      return ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: senderUserId,
        senderDeviceId: senderDeviceId,
        type: msgType,
        text: text,
        attachmentUrl: attachmentUrl,
        attachmentName: attachmentName,
        timestamp: serverTs,
        seq: seq,
        isMine: senderUserId == _localUserId,
      );
    } catch (e) {
      _logger.error('Group decryption failed for seq $seq: $e');
      return _makeErrorMessage(senderUserId, senderDeviceId, seq, serverTs);
    }
  }

  ChatMessage _makeErrorMessage(
          String senderUserId, String? senderDeviceId, int seq, DateTime ts) =>
      ChatMessage(
        id: _uuid.v4(),
        conversationId: conversationId,
        senderUserId: senderUserId,
        senderDeviceId: senderDeviceId,
        type: ChatMessageType.text,
        timestamp: ts,
        seq: seq,
        isMine: false,
        decryptionError: true,
      );

  ChatMessage _errorMessageFromEvent(MessageRtEvent event) => _makeErrorMessage(
        event.senderUserId,
        event.senderDeviceId,
        event.seq,
        DateTime.fromMillisecondsSinceEpoch(event.serverTs),
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

  void dispose() {
    _messagesController.close();
    _membersController.close();
    _unreadController.close();
    _uploadProgressController.close();
  }
}
