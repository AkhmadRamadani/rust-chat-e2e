/// Testing utilities for `rust_e2e_chat_sdk`.
///
/// Import this library in your tests:
/// ```dart
/// import 'package:rust_e2e_chat_sdk/testing.dart';
/// ```
library rust_e2e_chat_sdk_testing;

import 'dart:async';
import 'package:uuid/uuid.dart';

import 'rust_e2e_chat_sdk.dart';
import 'src/transport/connection_manager.dart' as cm;
import 'src/transport/rt_event.dart';

export 'src/session/session_store.dart' show InMemorySessionStore;

const _uuid = Uuid();

/// A mock [ChatConversation] for testing. Allows injecting incoming messages.
class MockChatConversation implements ChatConversation {
  @override
  final String conversationId;
  @override
  final ConversationType type;
  @override
  List<ConversationMember> currentMembers;

  final _messagesController =
      StreamController<List<ChatMessage>>.broadcast();
  final _membersController =
      StreamController<List<ConversationMember>>.broadcast();
  final _unreadController = StreamController<int>.broadcast();
  final _uploadProgressController =
      StreamController<AttachmentUploadProgress?>.broadcast();

  final List<ChatMessage> _messages = [];
  final List<ChatMessage> sentMessages = [];
  int _unread = 0;

  MockChatConversation({
    required this.conversationId,
    this.type = ConversationType.oneToOne,
    this.currentMembers = const [],
  });

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
    final msg = ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: 'local-user',
      type: ChatMessageType.text,
      text: text,
      timestamp: DateTime.now(),
      seq: _messages.length,
      isMine: true,
    );
    sentMessages.add(msg);
    _messages.add(msg);
    _messagesController.add(List.unmodifiable(_messages));
    return msg;
  }

  @override
  Future<ChatMessage> sendAttachment(
      _, String filename, String contentType) async {
    final msg = ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: 'local-user',
      type: ChatMessageType.attachment,
      attachmentName: filename,
      timestamp: DateTime.now(),
      seq: _messages.length,
      isMine: true,
    );
    sentMessages.add(msg);
    _messages.add(msg);
    _messagesController.add(List.unmodifiable(_messages));
    return msg;
  }

  @override
  Future<List<ChatMessage>> fetchHistory(
      {int limit = 50, int? beforeSeq}) async => [];

  @override
  Future<void> markAsRead() async {
    _unread = 0;
    _unreadController.add(0);
  }

  @override
  Future<void> onIncomingMessage(MessageRtEvent event) async {}

  @override
  Future<void> addMember(String userId) async {}

  @override
  Future<void> removeMember(String userId) async {}

  /// Injects a simulated incoming [text] message from [senderUserId].
  void simulateIncomingMessage(String senderUserId, String text) {
    final msg = ChatMessage(
      id: _uuid.v4(),
      conversationId: conversationId,
      senderUserId: senderUserId,
      type: ChatMessageType.text,
      text: text,
      timestamp: DateTime.now(),
      seq: _messages.length,
      isMine: false,
    );
    _messages.add(msg);
    _unread++;
    _messagesController.add(List.unmodifiable(_messages));
    _unreadController.add(_unread);
  }

  void dispose() {
    _messagesController.close();
    _membersController.close();
    _unreadController.close();
    _uploadProgressController.close();
  }
}

/// A mock [ChatClient] for unit testing application code.
///
/// Allows injecting fake conversations, simulating messages, and
/// asserting on sent content without a real server.
///
/// Example:
/// ```dart
/// final mockClient = MockChatClient(userId: 'test-user');
/// final convo = mockClient.stubConversation('conv-1');
/// convo.simulateIncomingMessage('alice', 'Hello!');
/// expect(convo.sentMessages, isEmpty);
/// ```
class MockChatClient {
  final String userId;
  final String deviceId;

  final Map<String, MockChatConversation> _conversations = {};
  final StreamController<List<ChatConversation>> _conversationsController =
      StreamController<List<ChatConversation>>.broadcast();
  final StreamController<cm.ConnectionState> _connectionStateController =
      StreamController<cm.ConnectionState>.broadcast();
  final StreamController<SdkError> _connectionErrorsController =
      StreamController<SdkError>.broadcast();
  final StreamController<SdkError> _storageErrorsController =
      StreamController<SdkError>.broadcast();
  final StreamController<String> _warningsController =
      StreamController<String>.broadcast();


  MockChatClient({
    this.userId = 'test-user',
    this.deviceId = 'test-device',
  });

  // Mirror the ChatClient public API

  Stream<List<ChatConversation>> get conversations =>
      _conversationsController.stream;
  Stream<cm.ConnectionState> get connectionState =>
      _connectionStateController.stream;
  Stream<SdkError> get connectionErrors => _connectionErrorsController.stream;
  Stream<SdkError> get storageErrors => _storageErrorsController.stream;
  Stream<String> get warnings => _warningsController.stream;

  Future<void> connect() async =>
      simulateConnectionState(cm.ConnectionState.connected);
  Future<void> disconnect() async =>
      simulateConnectionState(cm.ConnectionState.disconnected);
  Future<void> updateToken(String _) async {}

  Future<ChatConversation> openConversation(String recipientUserId) async {
    return _conversations.values
            .where((c) =>
                c.type == ConversationType.oneToOne &&
                c.currentMembers.any((m) => m.userId == recipientUserId))
            .firstOrNull ??
        stubConversation('conv-${_uuid.v4()}',
            recipientUserId: recipientUserId);
  }

  Future<ChatConversation> createGroup(List<String> memberUserIds) async {
    return stubConversation(
      'group-${_uuid.v4()}',
      type: ConversationType.group,
      memberUserIds: memberUserIds,
    );
  }

  Future<ChatConversation?> findConversation(String conversationId) async {
    return _conversations[conversationId];
  }

  Future<void> dispose() async {
    for (final c in _conversations.values) {
      c.dispose();
    }
    await _conversationsController.close();
    await _connectionStateController.close();
    await _connectionErrorsController.close();
    await _storageErrorsController.close();
    await _warningsController.close();
  }

  // ---------------------------------------------------------------------------
  // Test helpers
  // ---------------------------------------------------------------------------

  /// Creates and registers a stub conversation.
  MockChatConversation stubConversation(
    String conversationId, {
    String? recipientUserId,
    ConversationType type = ConversationType.oneToOne,
    List<String>? memberUserIds,
  }) {
    final members = [
      ConversationMember(userId: userId),
      if (recipientUserId != null)
        ConversationMember(userId: recipientUserId),
      ...?memberUserIds?.map((uid) => ConversationMember(userId: uid)),
    ];

    final convo = MockChatConversation(
      conversationId: conversationId,
      type: type,
      currentMembers: members,
    );
    _conversations[conversationId] = convo;
    _conversationsController.add(List.unmodifiable(_conversations.values));
    return convo;
  }

  /// Simulates an incoming message in a specific conversation.
  void simulateIncomingMessage(
      String conversationId, String senderUserId, String text) {
    final convo = _conversations[conversationId];
    if (convo == null) {
      throw StateError(
          'No stubbed conversation with id $conversationId. '
          'Call stubConversation() first.');
    }
    convo.simulateIncomingMessage(senderUserId, text);
  }

  /// Simulates a connection state change.
  void simulateConnectionState(cm.ConnectionState state) {
    _connectionStateController.add(state);
  }

  /// Simulates a connection error.
  void simulateConnectionError(SdkError error) {
    _connectionErrorsController.add(error);
  }
}
