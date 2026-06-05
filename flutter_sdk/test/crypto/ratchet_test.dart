import 'dart:typed_data';
import 'dart:convert';
import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/key_generator.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/ratchet_engine.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/crypto_types.dart';
import 'package:rust_e2e_chat_sdk/src/session/session_data.dart';

Future<Curve25519KeyPair> genPair() async {
  final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 0);
  return bundle.signedPreKeyPair;
}

void main() {
  final sharedSecret = Uint8List.fromList(List.generate(32, (i) => i + 1));

  group('RatchetEngine', () {
    test('basic send/receive round-trip', () async {
      final bobPair = await genPair();

      var sender = await RatchetEngine.initSender(
          sharedSecret, bobPair.publicKey, 'conv-1');
      var receiver =
          RatchetEngine.initReceiver(sharedSecret, bobPair, 'conv-1');

      final (s2, ct) = await RatchetEngine.encrypt(
          sender, Uint8List.fromList(utf8.encode('Hello!')));
      final (_, plaintext) = await RatchetEngine.decrypt(receiver, ct);

      expect(utf8.decode(plaintext), equals('Hello!'));
    });

    test('multiple sequential messages', () async {
      final bobPair = await genPair();

      var sender = await RatchetEngine.initSender(
          sharedSecret, bobPair.publicKey, 'conv-2');
      var receiver =
          RatchetEngine.initReceiver(sharedSecret, bobPair, 'conv-2');

      final msgs = ['First', 'Second', 'Third'];
      final cts = [];

      for (final msg in msgs) {
        final (s2, ct) = await RatchetEngine.encrypt(
            sender, Uint8List.fromList(utf8.encode(msg)));
        sender = s2;
        cts.add(ct);
      }

      for (var i = 0; i < msgs.length; i++) {
        final (r2, pt) = await RatchetEngine.decrypt(receiver, cts[i]);
        receiver = r2;
        expect(utf8.decode(pt), equals(msgs[i]));
      }
    });

    test('ratchet state serializes and restores correctly', () async {
      final bobPair = await genPair();
      var state = await RatchetEngine.initSender(
          sharedSecret, bobPair.publicKey, 'conv-serial');

      final (newState, _) = await RatchetEngine.encrypt(
          state, Uint8List.fromList(utf8.encode('test')));

      final restored = RatchetState.fromJsonString(newState.toJsonString());

      expect(restored.conversationId, equals(newState.conversationId));
      expect(restored.nSend, equals(newState.nSend));
      expect(restored.rootKey, equals(newState.rootKey));
      expect(restored.dhSendPub, equals(newState.dhSendPub));
    });

    test('same plaintext produces different ciphertexts (nonce randomness)', () async {
      final bobPair = await genPair();
      var state = await RatchetEngine.initSender(
          sharedSecret, bobPair.publicKey, 'conv-diff');

      final (s2, ct1) = await RatchetEngine.encrypt(
          state, Uint8List.fromList(utf8.encode('same')));
      final (_, ct2) = await RatchetEngine.encrypt(
          s2, Uint8List.fromList(utf8.encode('same')));

      // Different message keys each time → different ciphertexts
      expect(ct1.ciphertext, isNot(equals(ct2.ciphertext)));
    });
  });
}
