
import '../transport/rt_event.dart';
import '../transport/rest_client.dart';
import '../session/session_store.dart';
import '../session/session_data.dart';
import '../crypto/key_generator.dart';
import '../crypto/crypto_types.dart';
import '../utils/logger.dart';

/// Automatically replenishes one-time pre-keys when the server reports
/// a [LowOtpkRtEvent] with count < 10.
///
/// Generates 50 fresh OTPKs, uploads them, and updates the [DeviceRecord]
/// in the [SessionStore].
class OtpkReplenisher {
  final RestClient _restClient;
  final SessionStore _sessionStore;
  final String _userId;
  final String _deviceId;
  final SdkLogger _logger;

  static const _replenishCount = 50;
  static const _lowThreshold = 10;

  OtpkReplenisher({
    required RestClient restClient,
    required SessionStore sessionStore,
    required String userId,
    required String deviceId,
    SdkLogger? logger,
  })  : _restClient = restClient,
        _sessionStore = sessionStore,
        _userId = userId,
        _deviceId = deviceId,
        _logger = logger ?? SdkLogger(tag: 'OtpkReplenisher');

  /// Called by [RealTimeEventRouter] when a [LowOtpkRtEvent] is received.
  Future<void> onLowOtpk(LowOtpkRtEvent event) async {
    if (event.count >= _lowThreshold) return;
    _logger.info(
        'Low OTPK warning: ${event.count} remaining. Replenishing $_replenishCount keys...');

    try {
      final newKeys = await KeyGenerator.generateOneTimePreKeys(_replenishCount);

      final publicKeys = newKeys
          .map((k) => OneTimePreKey(
                id: k.id,
                publicKey: k.keyPair.publicKey,
              ))
          .toList();

      await _restClient.replenishOtpks(_userId, _deviceId, publicKeys);

      // Update stored DeviceRecord with new private key material
      final record = await _sessionStore.loadDevice(_userId, _deviceId);
      if (record != null) {
        final newOtpkRecords = newKeys
            .map((k) => OtpkRecord(
                  id: k.id,
                  privateKey: k.keyPair.privateKey,
                  publicKey: k.keyPair.publicKey,
                ))
            .toList();

        await _sessionStore.saveDevice(
          record.copyWith(
            oneTimePrekeys: [...record.oneTimePrekeys, ...newOtpkRecords],
          ),
        );
      }

      _logger.info('Successfully replenished $_replenishCount OTPKs');
    } catch (e) {
      _logger.error('Failed to replenish OTPKs: $e');
    }
  }
}
