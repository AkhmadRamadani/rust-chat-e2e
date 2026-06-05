import 'package:test/test.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/key_generator.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/x3dh_engine.dart';
import 'package:rust_e2e_chat_sdk/src/crypto/crypto_types.dart';
import 'package:rust_e2e_chat_sdk/src/transport/api_models.dart';

void main() {
  group('X3dhEngine', () {
    test('initiator and responder derive identical secrets (with OTPK)', () async {
      final aliceBundle = await KeyGenerator.generateKeyBundle(otpkCount: 5);
      final bobBundle   = await KeyGenerator.generateKeyBundle(otpkCount: 5);

      final bobOtpk = bobBundle.oneTimePreKeys.first;
      final bobPublicBundle = KeyBundleResponse(
        userId: 'bob',
        deviceId: 'bob-device',
        identityKey: bobBundle.identityKeyPair.publicKey,
        signedPreKey: bobBundle.signedPreKeyPair.publicKey,
        signedPreKeySignature: bobBundle.signedPreKeySignature,
        signedPreKeyId: bobBundle.signedPreKeyId,
        oneTimePreKey: OneTimePreKeyResponse(
          id: bobOtpk.id,
          publicKey: bobOtpk.keyPair.publicKey,
        ),
      );

      final aliceResult = await X3dhEngine.performX3dh(
        recipientBundle: bobPublicBundle,
        senderBundle: aliceBundle,
      );

      final bobOtpkRecord = OtpkRecord(
        id: bobOtpk.id,
        privateKey: bobOtpk.keyPair.privateKey,
        publicKey: bobOtpk.keyPair.publicKey,
      );

      final bobSecret = await X3dhEngine.deriveSharedSecret(
        header: aliceResult.header,
        recipientBundle: bobBundle,
        consumedOtpk: bobOtpkRecord,
      );

      expect(aliceResult.sharedSecret, equals(bobSecret));
      expect(aliceResult.sharedSecret.length, equals(32));
    });

    test('initiator and responder derive identical secrets (no OTPK)', () async {
      final aliceBundle = await KeyGenerator.generateKeyBundle(otpkCount: 1);
      final bobBundle   = await KeyGenerator.generateKeyBundle(otpkCount: 1);

      final bobPublicBundle = KeyBundleResponse(
        userId: 'bob',
        deviceId: 'bob-device',
        identityKey: bobBundle.identityKeyPair.publicKey,
        signedPreKey: bobBundle.signedPreKeyPair.publicKey,
        signedPreKeySignature: bobBundle.signedPreKeySignature,
        signedPreKeyId: bobBundle.signedPreKeyId,
        oneTimePreKey: null,
      );

      final aliceResult = await X3dhEngine.performX3dh(
        recipientBundle: bobPublicBundle,
        senderBundle: aliceBundle,
      );

      final bobSecret = await X3dhEngine.deriveSharedSecret(
        header: aliceResult.header,
        recipientBundle: bobBundle,
        consumedOtpk: null,
      );

      expect(aliceResult.sharedSecret, equals(bobSecret));
      expect(aliceResult.header.oneTimePreKeyId, isNull);
    });

    test('different sessions produce different secrets', () async {
      final aliceBundle = await KeyGenerator.generateKeyBundle(otpkCount: 1);
      final bobBundle   = await KeyGenerator.generateKeyBundle(otpkCount: 1);

      final bobPublicBundle = KeyBundleResponse(
        userId: 'bob',
        deviceId: 'bob-device',
        identityKey: bobBundle.identityKeyPair.publicKey,
        signedPreKey: bobBundle.signedPreKeyPair.publicKey,
        signedPreKeySignature: bobBundle.signedPreKeySignature,
        signedPreKeyId: bobBundle.signedPreKeyId,
        oneTimePreKey: null,
      );

      final r1 = await X3dhEngine.performX3dh(
          recipientBundle: bobPublicBundle, senderBundle: aliceBundle);
      final r2 = await X3dhEngine.performX3dh(
          recipientBundle: bobPublicBundle, senderBundle: aliceBundle);

      expect(r1.sharedSecret, isNot(equals(r2.sharedSecret)));
    });
  });
}
