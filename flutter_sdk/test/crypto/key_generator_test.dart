import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/key_generator.dart';

void main() {
  group('KeyGenerator', () {
    test('generateKeyBundle produces correct key counts', () async {
      final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 10);
      expect(bundle.identityKeyPair.publicKey.length, equals(32));
      expect(bundle.identityKeyPair.privateKey.length, equals(32));
      expect(bundle.signedPreKeyPair.publicKey.length, equals(32));
      expect(bundle.signedPreKeySignature, isNotEmpty);
      expect(bundle.oneTimePreKeys.length, equals(10));
    });

    test('all OTPKs have unique IDs', () async {
      final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 20);
      final pubKeys =
          bundle.oneTimePreKeys.map((k) => k.keyPair.publicKey.join(',')).toSet();
      expect(pubKeys.length, equals(20));
    });

    test('SPK signature verifies correctly', () async {
      final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 0);
      final valid = await KeyGenerator.verifySignature(
        data: bundle.signedPreKeyPair.publicKey,
        signature: bundle.signedPreKeySignature,
        publicKey: bundle.identitySigningKeyPair.publicKey,
      );
      expect(valid, isTrue);
    });

    test('tampered SPK signature fails verification', () async {
      final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 0);
      final tampered = List<int>.from(bundle.signedPreKeySignature);
      tampered[0] ^= 0xFF; // flip bits
      final valid = await KeyGenerator.verifySignature(
        data: bundle.signedPreKeyPair.publicKey,
        signature: tampered as dynamic,
        publicKey: bundle.identitySigningKeyPair.publicKey,
      );
      expect(valid, isFalse);
    });

    test('generateSenderKey produces non-empty chain key', () async {
      final pair = await KeyGenerator.generateSenderKey();
      expect(pair.chainKey.length, equals(32));
      expect(pair.signingKeyPair.publicKey.length, equals(32));
      expect(pair.signingKeyPair.privateKey.length, equals(32));
    });

    test('two generateKeyBundle calls produce different keys', () async {
      final b1 = await KeyGenerator.generateKeyBundle(otpkCount: 0);
      final b2 = await KeyGenerator.generateKeyBundle(otpkCount: 0);
      expect(
        b1.identityKeyPair.publicKey,
        isNot(equals(b2.identityKeyPair.publicKey)),
      );
    });
  });
}
