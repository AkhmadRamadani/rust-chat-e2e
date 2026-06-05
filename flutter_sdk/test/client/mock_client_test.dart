import 'dart:async';
import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/testing.dart';
import 'package:rust_e2e_chat_sdk/rust_e2e_chat_sdk.dart';


void main() {
  group('MockChatClient', () {
    late MockChatClient client;

    setUp(() {
      client = MockChatClient(userId: 'test-user', deviceId: 'test-device');
    });

    tearDown(() async => client.dispose());

    test('openConversation returns a conversation', () async {
      final convo = await client.openConversation('alice');
      expect(convo, isNotNull);
      expect(convo.currentMembers.any((m) => m.userId == 'alice'), isTrue);
    });

    test('openConversation is idempotent', () async {
      final c1 = await client.openConversation('bob');
      final c2 = await client.openConversation('bob');
      expect(c1.conversationId, equals(c2.conversationId));
    });

    test('simulateIncomingMessage emits on messages stream', () async {
      final convo = client.stubConversation('conv-1', recipientUserId: 'alice');

      final msgs = <List<ChatMessage>>[];
      final sub = convo.messages.listen(msgs.add);
      addTearDown(sub.cancel);

      client.simulateIncomingMessage('conv-1', 'alice', 'Hey there!');

      await Future.microtask(() {});
      expect(msgs, isNotEmpty);
      expect(msgs.last.last.text, equals('Hey there!'));
      expect(msgs.last.last.senderUserId, equals('alice'));
      expect(msgs.last.last.isMine, isFalse);
    });

    test('sendMessage adds to sentMessages', () async {
      final convo = client.stubConversation('conv-2', recipientUserId: 'bob');

      await convo.sendMessage('Hello Bob!');
      expect(convo.sentMessages.length, equals(1));
      expect(convo.sentMessages.first.text, equals('Hello Bob!'));
      expect(convo.sentMessages.first.isMine, isTrue);
    });

    test('simulateConnectionState emits on connectionState stream', () async {
      final states = <ConnectionState>[];
      final sub = client.connectionState.listen(states.add);
      addTearDown(sub.cancel);

      client.simulateConnectionState(ConnectionState.disconnected);
      client.simulateConnectionState(ConnectionState.reconnecting);
      client.simulateConnectionState(ConnectionState.connected);

      await Future.microtask(() {});
      expect(states,
          containsAllInOrder([
            ConnectionState.disconnected,
            ConnectionState.reconnecting,
            ConnectionState.connected,
          ]));
    });

    test('unreadCount increments on incoming message', () async {
      final convo = client.stubConversation('conv-3', recipientUserId: 'carol');

      final counts = <int>[];
      final sub = convo.unreadCount.listen(counts.add);
      addTearDown(sub.cancel);

      client.simulateIncomingMessage('conv-3', 'carol', 'msg1');
      client.simulateIncomingMessage('conv-3', 'carol', 'msg2');

      await Future.microtask(() {});
      expect(counts.last, greaterThanOrEqualTo(2));
    });

    test('markAsRead resets unread count', () async {
      final convo = client.stubConversation('conv-4', recipientUserId: 'dave');

      client.simulateIncomingMessage('conv-4', 'dave', 'unread');
      await Future.microtask(() {});

      await convo.markAsRead();
      final counts = <int>[];
      final sub = convo.unreadCount.listen(counts.add);
      addTearDown(sub.cancel);

      await Future.microtask(() {});
      // After markAsRead the next emitted value should be 0
      expect(counts, anyElement(equals(0)));
    });

    test('createGroup returns a group conversation', () async {
      final group = await client.createGroup(['alice', 'bob']);
      expect(group.type, equals(ConversationType.group));
    });

    test('simulateConnectionError emits on connectionErrors stream', () async {
      final errors = <SdkError>[];
      final sub = client.connectionErrors.listen(errors.add);
      addTearDown(sub.cancel);

      client.simulateConnectionError(
          const NetworkError(statusCode: 503, message: 'service unavailable'));

      await Future.microtask(() {});
      expect(errors, hasLength(1));
      expect(errors.first, isA<NetworkError>());
    });
  });
}
