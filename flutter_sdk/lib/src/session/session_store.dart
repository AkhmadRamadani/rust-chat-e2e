import 'session_data.dart';

/// Abstract interface for durable local storage of key material and session state.
///
/// Implement this interface to provide a custom storage backend.
/// The SDK provides [FlutterSecureSessionStore] and [InMemorySessionStore].
abstract class SessionStore {
  /// Persist a [DeviceRecord] (private key material) for a device.
  Future<void> saveDevice(DeviceRecord record);

  /// Load a [DeviceRecord] for the given [userId] and [deviceId].
  /// Returns null if not found.
  Future<DeviceRecord?> loadDevice(String userId, String deviceId);

  /// Persist the Double Ratchet state for a conversation.
  Future<void> saveRatchetState(String conversationId, RatchetState state);

  /// Load the Double Ratchet state for a conversation.
  /// Returns null if no session exists yet.
  Future<RatchetState?> loadRatchetState(String conversationId);

  /// Persist a Sender Key record for a user in a group conversation.
  Future<void> saveSenderKey(
      String conversationId, String userId, SenderKeyRecord record);

  /// Load a Sender Key record for a user in a group conversation.
  /// Returns null if not found.
  Future<SenderKeyRecord?> loadSenderKey(String conversationId, String userId);

  /// Persist conversation metadata.
  Future<void> saveConversationMeta(ConversationMeta meta);

  /// Load all known conversation metadata records.
  Future<List<ConversationMeta>> loadAllConversations();

  /// Clear all stored state. Used for logout and testing.
  Future<void> clear();
}

/// In-memory [SessionStore] implementation for testing and server environments.
///
/// All state is held in maps and is cleared on [dispose] or [clear].
class InMemorySessionStore implements SessionStore {
  final Map<String, DeviceRecord> _devices = {};
  final Map<String, RatchetState> _ratchetStates = {};
  final Map<String, SenderKeyRecord> _senderKeys = {};
  final Map<String, ConversationMeta> _conversations = {};

  static String _deviceKey(String userId, String deviceId) =>
      '$userId:$deviceId';

  static String _senderKeyKey(String conversationId, String userId) =>
      '$conversationId:$userId';

  @override
  Future<void> saveDevice(DeviceRecord record) async {
    _devices[_deviceKey(record.userId, record.deviceId)] = record;
  }

  @override
  Future<DeviceRecord?> loadDevice(String userId, String deviceId) async {
    return _devices[_deviceKey(userId, deviceId)];
  }

  @override
  Future<void> saveRatchetState(
      String conversationId, RatchetState state) async {
    _ratchetStates[conversationId] = state;
  }

  @override
  Future<RatchetState?> loadRatchetState(String conversationId) async {
    return _ratchetStates[conversationId];
  }

  @override
  Future<void> saveSenderKey(
      String conversationId, String userId, SenderKeyRecord record) async {
    _senderKeys[_senderKeyKey(conversationId, userId)] = record;
  }

  @override
  Future<SenderKeyRecord?> loadSenderKey(
      String conversationId, String userId) async {
    return _senderKeys[_senderKeyKey(conversationId, userId)];
  }

  @override
  Future<void> saveConversationMeta(ConversationMeta meta) async {
    _conversations[meta.conversationId] = meta;
  }

  @override
  Future<List<ConversationMeta>> loadAllConversations() async {
    return _conversations.values.toList();
  }

  @override
  Future<void> clear() async {
    _devices.clear();
    _ratchetStates.clear();
    _senderKeys.clear();
    _conversations.clear();
  }
}
