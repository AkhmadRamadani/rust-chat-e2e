import 'dart:typed_data';
import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/src/session/session_store.dart';
import 'package:rust_e2e_chat_sdk/src/session/session_data.dart';

DeviceRecord _makeDevice(String userId, String deviceId) => DeviceRecord(
      userId: userId,
      deviceId: deviceId,
      identityKeyPrivate: Uint8List.fromList(List.filled(32, 1)),
      identityKeyPublic: Uint8List.fromList(List.filled(32, 2)),
      identitySigningKeyPrivate: Uint8List.fromList(List.filled(32, 3)),
      identitySigningKeyPublic: Uint8List.fromList(List.filled(32, 4)),
      signedPrekeyPrivate: Uint8List.fromList(List.filled(32, 5)),
      signedPrekeyPublic: Uint8List.fromList(List.filled(32, 6)),
      signedPrekeyId: 42,
      oneTimePrekeys: [],
      signedPrekeyCreatedAt: DateTime(2024, 1, 1),
    );

RatchetState _makeRatchetState(String conversationId) => RatchetState(
      conversationId: conversationId,
      rootKey: Uint8List.fromList(List.filled(32, 10)),
      chainKeySend: Uint8List.fromList(List.filled(32, 11)),
      chainKeyRecv: Uint8List.fromList(List.filled(32, 12)),
      dhSendPub: Uint8List.fromList(List.filled(32, 13)),
      dhSendPriv: Uint8List.fromList(List.filled(32, 14)),
      dhRecvPub: Uint8List.fromList(List.filled(32, 15)),
      nSend: 5,
      nRecv: 3,
      pn: 2,
      skippedMessageKeys: {7: Uint8List.fromList(List.filled(32, 99))},
    );

SenderKeyRecord _makeSenderKey(String conversationId, String userId) =>
    SenderKeyRecord(
      conversationId: conversationId,
      userId: userId,
      chainKey: Uint8List.fromList(List.filled(32, 20)),
      iteration: 7,
      signingKeyPublic: Uint8List.fromList(List.filled(32, 21)),
      signingKeyPrivate: Uint8List.fromList(List.filled(32, 22)),
    );

void main() {
  group('InMemorySessionStore', () {
    late InMemorySessionStore store;

    setUp(() => store = InMemorySessionStore());

    test('saveDevice / loadDevice round-trip', () async {
      final record = _makeDevice('user-1', 'device-1');
      await store.saveDevice(record);
      final loaded = await store.loadDevice('user-1', 'device-1');
      expect(loaded, isNotNull);
      expect(loaded!.userId, equals('user-1'));
      expect(loaded.deviceId, equals('device-1'));
      expect(loaded.signedPrekeyId, equals(42));
    });

    test('loadDevice returns null for unknown device', () async {
      final result = await store.loadDevice('unknown', 'device');
      expect(result, isNull);
    });

    test('saveRatchetState / loadRatchetState round-trip', () async {
      final state = _makeRatchetState('conv-abc');
      await store.saveRatchetState('conv-abc', state);
      final loaded = await store.loadRatchetState('conv-abc');
      expect(loaded, isNotNull);
      expect(loaded!.nSend, equals(5));
      expect(loaded.nRecv, equals(3));
      expect(loaded.skippedMessageKeys[7], isNotNull);
    });

    test('saveSenderKey / loadSenderKey round-trip', () async {
      final record = _makeSenderKey('group-1', 'alice');
      await store.saveSenderKey('group-1', 'alice', record);
      final loaded = await store.loadSenderKey('group-1', 'alice');
      expect(loaded, isNotNull);
      expect(loaded!.iteration, equals(7));
    });

    test('saveConversationMeta / loadAllConversations', () async {
      await store.saveConversationMeta(ConversationMeta(
        conversationId: 'conv-1',
        type: 'oneToOne',
        memberUserIds: ['alice', 'bob'],
        createdAt: DateTime(2024),
      ));
      await store.saveConversationMeta(ConversationMeta(
        conversationId: 'conv-2',
        type: 'group',
        memberUserIds: ['alice', 'bob', 'carol'],
        createdAt: DateTime(2024),
      ));
      final all = await store.loadAllConversations();
      expect(all.length, equals(2));
      expect(all.map((c) => c.conversationId),
          containsAll(['conv-1', 'conv-2']));
    });

    test('clear removes all stored data', () async {
      await store.saveDevice(_makeDevice('u', 'd'));
      await store.saveRatchetState('conv', _makeRatchetState('conv'));
      await store.clear();
      expect(await store.loadDevice('u', 'd'), isNull);
      expect(await store.loadRatchetState('conv'), isNull);
      expect(await store.loadAllConversations(), isEmpty);
    });

    test('overwriting a device record replaces previous', () async {
      final original = _makeDevice('user-1', 'device-1');
      await store.saveDevice(original);

      final updated = DeviceRecord(
        userId: 'user-1',
        deviceId: 'device-1',
        identityKeyPrivate: Uint8List.fromList(List.filled(32, 99)),
        identityKeyPublic: Uint8List.fromList(List.filled(32, 99)),
        identitySigningKeyPrivate: Uint8List.fromList(List.filled(32, 99)),
        identitySigningKeyPublic: Uint8List.fromList(List.filled(32, 99)),
        signedPrekeyPrivate: Uint8List.fromList(List.filled(32, 99)),
        signedPrekeyPublic: Uint8List.fromList(List.filled(32, 99)),
        signedPrekeyId: 100,
        oneTimePrekeys: [],
        signedPrekeyCreatedAt: DateTime(2025),
      );
      await store.saveDevice(updated);

      final loaded = await store.loadDevice('user-1', 'device-1');
      expect(loaded!.signedPrekeyId, equals(100));
    });
  });

  group('DeviceRecord serialization', () {
    test('toJsonString / fromJsonString round-trip', () {
      final record = _makeDevice('u', 'd');
      final json = record.toJsonString();
      final restored = DeviceRecord.fromJsonString(json);
      expect(restored.userId, equals(record.userId));
      expect(restored.signedPrekeyId, equals(record.signedPrekeyId));
      expect(restored.identityKeyPrivate, equals(record.identityKeyPrivate));
    });
  });

  group('RatchetState serialization', () {
    test('toJsonString / fromJsonString with skipped keys', () {
      final state = _makeRatchetState('conv-s');
      final json = state.toJsonString();
      final restored = RatchetState.fromJsonString(json);
      expect(restored.nSend, equals(state.nSend));
      expect(restored.nRecv, equals(state.nRecv));
      expect(restored.skippedMessageKeys.keys, contains(7));
    });
  });

  group('SenderKeyRecord serialization', () {
    test('toJsonString / fromJsonString with private key', () {
      final record = _makeSenderKey('g', 'u');
      final json = record.toJsonString();
      final restored = SenderKeyRecord.fromJsonString(json);
      expect(restored.iteration, equals(record.iteration));
      expect(restored.signingKeyPrivate, isNotNull);
      expect(restored.signingKeyPrivate, equals(record.signingKeyPrivate));
    });
  });
}
