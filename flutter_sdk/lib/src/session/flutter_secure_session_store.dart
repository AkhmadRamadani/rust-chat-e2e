import 'dart:async';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import '../errors/sdk_error.dart';
import 'session_data.dart';
import 'session_store.dart';

/// Key prefixes for all stored values.
const _kDevice = 'rce_device_';
const _kRatchet = 'rce_ratchet_';
const _kSenderKey = 'rce_senderkey_';
const _kConv = 'rce_conv_';

/// [SessionStore] backed by [FlutterSecureStorage] (platform keychain/keystore).
///
/// Supported on iOS, Android, macOS, Windows, and Linux.
/// Not supported on web — use [InMemorySessionStore] there.
///
/// All values are JSON-serialized. The OS secure storage layer applies
/// platform-level AES encryption automatically.
class FlutterSecureSessionStore implements SessionStore {
  final FlutterSecureStorage _storage;

  /// Stream to emit storage errors without crashing the application.
  final StreamController<SdkError> _errorController;

  FlutterSecureSessionStore({
    FlutterSecureStorage? storage,
    required StreamController<SdkError> errorController,
  })  : _storage = storage ??
            const FlutterSecureStorage(
              aOptions: AndroidOptions(encryptedSharedPreferences: true),
              iOptions: IOSOptions(
                accessibility: KeychainAccessibility.first_unlock,
              ),
            ),
        _errorController = errorController;

  Future<T?> _safeRead<T>(Future<T> Function() op) async {
    try {
      return await op();
    } catch (e) {
      _errorController.add(StorageError(reason: e.toString()));
      return null;
    }
  }

  Future<void> _safeWrite(Future<void> Function() op) async {
    try {
      await op();
    } catch (e) {
      _errorController.add(StorageError(reason: e.toString()));
    }
  }

  @override
  Future<void> saveDevice(DeviceRecord record) => _safeWrite(() => _storage.write(
        key: '$_kDevice${record.userId}_${record.deviceId}',
        value: record.toJsonString(),
      ));

  @override
  Future<DeviceRecord?> loadDevice(String userId, String deviceId) async {
    final raw = await _safeRead(
        () => _storage.read(key: '$_kDevice${userId}_$deviceId'));
    if (raw == null) return null;
    try {
      return DeviceRecord.fromJsonString(raw);
    } catch (e) {
      _errorController.add(StorageError(reason: 'DeviceRecord parse error: $e'));
      return null;
    }
  }

  @override
  Future<void> saveRatchetState(
          String conversationId, RatchetState state) =>
      _safeWrite(() => _storage.write(
            key: '$_kRatchet$conversationId',
            value: state.toJsonString(),
          ));

  @override
  Future<RatchetState?> loadRatchetState(String conversationId) async {
    final raw = await _safeRead(
        () => _storage.read(key: '$_kRatchet$conversationId'));
    if (raw == null) return null;
    try {
      return RatchetState.fromJsonString(raw);
    } catch (e) {
      _errorController.add(StorageError(reason: 'RatchetState parse error: $e'));
      return null;
    }
  }

  @override
  Future<void> saveSenderKey(
          String conversationId, String userId, SenderKeyRecord record) =>
      _safeWrite(() => _storage.write(
            key: '$_kSenderKey${conversationId}_$userId',
            value: record.toJsonString(),
          ));

  @override
  Future<SenderKeyRecord?> loadSenderKey(
      String conversationId, String userId) async {
    final raw = await _safeRead(
        () => _storage.read(key: '$_kSenderKey${conversationId}_$userId'));
    if (raw == null) return null;
    try {
      return SenderKeyRecord.fromJsonString(raw);
    } catch (e) {
      _errorController.add(
          StorageError(reason: 'SenderKeyRecord parse error: $e'));
      return null;
    }
  }

  @override
  Future<void> saveConversationMeta(ConversationMeta meta) =>
      _safeWrite(() => _storage.write(
            key: '$_kConv${meta.conversationId}',
            value: meta.toJsonString(),
          ));

  @override
  Future<List<ConversationMeta>> loadAllConversations() async {
    try {
      final all = await _storage.readAll();
      final results = <ConversationMeta>[];
      for (final entry in all.entries) {
        if (entry.key.startsWith(_kConv)) {
          try {
            results.add(ConversationMeta.fromJsonString(entry.value));
          } catch (_) {
            // Skip malformed entries
          }
        }
      }
      return results;
    } catch (e) {
      _errorController.add(StorageError(reason: e.toString()));
      return [];
    }
  }

  @override
  Future<void> clear() => _safeWrite(_storage.deleteAll);
}
