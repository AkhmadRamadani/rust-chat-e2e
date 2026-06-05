import 'dart:convert';
import 'dart:typed_data';
import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/key_generator.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/sender_key_engine.dart';

void main() {
  group('SenderKeyEngine', () {
    test('encrypt/decrypt round-trip', () async {
      final keyPair = await KeyGenerator.generateSenderKey();
      final session = SenderKeyEngine.createSession(
        conversationId: 'group-1',
        userId: 'alice',
        keyPair: keyPair,
      );

      // Create a receiver session with the same key material
      final receiverSession = SenderKeyEngine.deserializeKeyMaterial(
        bytes: SenderKeyEngine.serializeKeyMaterial(session),
        conversationId: 'group-1',
        senderUserId: 'alice',
      );
      // Restore sender state for the receiver from the same starting point
      final senderSession2 = SenderKeyEngine.createSession(
        conversationId: 'group-1',
        userId: 'alice',
        keyPair: keyPair,
      );

      const msg = 'Hello group!';
      final ciphertext = await SenderKeyEngine.encrypt(
        senderSession2,
        Uint8List.fromList(utf8.encode(msg)),
      );

      final plaintext = await SenderKeyEngine.decrypt(
        receiverSession,
        ciphertext,
        'group-1',
      );

      expect(utf8.decode(plaintext), equals(msg));
    });

    test('serializeKeyMaterial / deserializeKeyMaterial round-trip', () async {
      final keyPair = await KeyGenerator.generateSenderKey();
      final session = SenderKeyEngine.createSession(
        conversationId: 'group-2',
        userId: 'bob',
        keyPair: keyPair,
      );

      final bytes = SenderKeyEngine.serializeKeyMaterial(session);
      final restored = SenderKeyEngine.deserializeKeyMaterial(
        bytes: bytes,
        conversationId: 'group-2',
        senderUserId: 'bob',
      );

      expect(restored.chainKey, equals(session.chainKey));
      expect(restored.signingKeyPublic, equals(session.signingKeyPublic));
      expect(restored.iteration, equals(session.iteration));
      expect(restored.signingKeyPrivate, isNull); // receiver has no private key
    });

    test('multiple messages advance iteration correctly', () async {
      final keyPair = await KeyGenerator.generateSenderKey();
      final senderSession = SenderKeyEngine.createSession(
        conversationId: 'group-3',
        userId: 'alice',
        keyPair: keyPair,
      );
      final receiverSession = SenderKeyEngine.deserializeKeyMaterial(
        bytes: SenderKeyEngine.serializeKeyMaterial(senderSession),
        conversationId: 'group-3',
        senderUserId: 'alice',
      );
      // Rebuild sender from scratch
      final sender = SenderKeyEngine.createSession(
        conversationId: 'group-3',
        userId: 'alice',
        keyPair: keyPair,
      );

      final messages = ['msg1', 'msg2', 'msg3'];
      final ciphertexts = <Uint8List>[];
      for (final msg in messages) {
        ciphertexts.add(await SenderKeyEngine.encrypt(
            sender, Uint8List.fromList(utf8.encode(msg))));
      }

      for (var i = 0; i < messages.length; i++) {
        final pt = await SenderKeyEngine.decrypt(
            receiverSession, ciphertexts[i], 'group-3');
        expect(utf8.decode(pt), equals(messages[i]));
      }
    });

    test('wrong signing key causes DecryptionError', () async {
      final keyPair1 = await KeyGenerator.generateSenderKey();
      final keyPair2 = await KeyGenerator.generateSenderKey();

      final sender = SenderKeyEngine.createSession(
        conversationId: 'group-4',
        userId: 'alice',
        keyPair: keyPair1,
      );

      // Receiver has keyPair2's public key — mismatch
      final wrongReceiver = SenderKeyEngine.deserializeKeyMaterial(
        bytes: SenderKeyEngine.serializeKeyMaterial(
          SenderKeyEngine.createSession(
            conversationId: 'group-4',
            userId: 'alice',
            keyPair: keyPair2,
          ),
        ),
        conversationId: 'group-4',
        senderUserId: 'alice',
      );

      final ciphertext = await SenderKeyEngine.encrypt(
          sender, Uint8List.fromList(utf8.encode('secret')));

      expect(
        () => SenderKeyEngine.decrypt(wrongReceiver, ciphertext, 'group-4'),
        throwsA(isA<Exception>()),
      );
    });
  });
}
