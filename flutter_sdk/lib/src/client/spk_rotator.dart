import 'dart:async';


import '../transport/rest_client.dart';
import '../session/session_store.dart';
import '../session/session_data.dart';
import '../crypto/key_generator.dart';
import '../crypto/crypto_types.dart';
import '../utils/logger.dart';

/// Periodically rotates the Signed Pre-Key based on [rotationDays].
///
/// On each check (run at startup and every 24 h), if the SPK is older than
/// [rotationDays] days a fresh SPK is generated, signed, uploaded to the
/// server, and stored in the [SessionStore].
class SpkRotator {
  final RestClient _restClient;
  final SessionStore _sessionStore;
  final String _userId;
  final String _deviceId;
  final int _rotationDays;
  final SdkLogger _logger;

  Timer? _timer;



  SpkRotator({
    required RestClient restClient,
    required SessionStore sessionStore,
    required String userId,
    required String deviceId,
    int rotationDays = 7,
    SdkLogger? logger,
  })  : _restClient = restClient,
        _sessionStore = sessionStore,
        _userId = userId,
        _deviceId = deviceId,
        _rotationDays = rotationDays,
        _logger = logger ?? SdkLogger(tag: 'SpkRotator');

  /// Starts the rotation scheduler. Checks immediately and then every 24 hours.
  void start() {
    _checkAndRotate();
    _timer = Timer.periodic(const Duration(hours: 24), (_) => _checkAndRotate());
  }

  /// Stops the scheduler.
  void stop() {
    _timer?.cancel();
    _timer = null;
  }

  // ---------------------------------------------------------------------------
  // Private
  // ---------------------------------------------------------------------------

  Future<void> _checkAndRotate() async {
    try {
      final record = await _sessionStore.loadDevice(_userId, _deviceId);
      if (record == null) return;

      final age = DateTime.now().difference(record.signedPrekeyCreatedAt);
      if (age.inDays < _rotationDays) return;

      _logger.info(
          'SPK is ${age.inDays} days old — rotating (threshold: $_rotationDays days)');

      // Generate new SPK
      final bundle = await KeyGenerator.generateKeyBundle(otpkCount: 0);
      final newSpk = bundle.signedPreKeyPair;
      final newSpkId = bundle.signedPreKeyId;
      final newSig = bundle.signedPreKeySignature;

      // Verify before uploading
      final valid = await KeyGenerator.verifySignature(
        data: newSpk.publicKey,
        signature: newSig,
        publicKey: record.identitySigningKeyPublic,
      );
      if (!valid) {
        _logger.error('SPK self-verification failed — aborting rotation');
        return;
      }

      final update = SignedPreKeyUpdate(
        id: newSpkId,
        publicKey: newSpk.publicKey,
        signature: newSig,
      );

      await _restClient.rotateSignedPreKey(_userId, _deviceId, update);

      // Persist new SPK
      final updatedRecord = DeviceRecord(
        userId: record.userId,
        deviceId: record.deviceId,
        identityKeyPrivate: record.identityKeyPrivate,
        identityKeyPublic: record.identityKeyPublic,
        identitySigningKeyPrivate: record.identitySigningKeyPrivate,
        identitySigningKeyPublic: record.identitySigningKeyPublic,
        signedPrekeyPrivate: newSpk.privateKey,
        signedPrekeyPublic: newSpk.publicKey,
        signedPrekeyId: newSpkId,
        oneTimePrekeys: record.oneTimePrekeys,
        signedPrekeyCreatedAt: DateTime.now(),
      );
      await _sessionStore.saveDevice(updatedRecord);

      _logger.info('SPK rotated successfully (new id: $newSpkId)');
    } catch (e) {
      _logger.error('SPK rotation failed: $e');
    }
  }
}
