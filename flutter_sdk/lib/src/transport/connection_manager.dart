import 'dart:async';
import '../utils/logger.dart';
import 'ws_channel.dart';

/// WebSocket connection state.
enum ConnectionState {
  connecting,
  connected,
  reconnecting,
  disconnected,
}

/// Manages the WebSocket connection lifecycle with exponential backoff.
///
/// Backoff schedule: 1s → 2s → 4s → 8s → 16s → 32s → 60s (cap).
class ConnectionManager {
  final WsChannel _wsChannel;
  final String Function() _wsUrlBuilder;
  final SdkLogger _logger;

  final StreamController<ConnectionState> _stateController =
      StreamController<ConnectionState>.broadcast();

  ConnectionState _state = ConnectionState.disconnected;
  bool _disposed = false;
  int _reconnectAttempts = 0;
  Timer? _reconnectTimer;
  StreamSubscription? _wsErrorSubscription;

  static const _backoffDelays = [1, 2, 4, 8, 16, 32, 60];

  ConnectionManager({
    required WsChannel wsChannel,
    required String Function() wsUrlBuilder,
    SdkLogger? logger,
  })  : _wsChannel = wsChannel,
        _wsUrlBuilder = wsUrlBuilder,
        _logger = logger ?? SdkLogger(tag: 'ConnectionManager');

  /// Stream of connection state changes.
  Stream<ConnectionState> get state => _stateController.stream;

  /// Current connection state.
  ConnectionState get currentState => _state;

  /// Ensures the WebSocket is connected. If already connected, no-op.
  Future<void> ensureConnected() async {
    if (_state == ConnectionState.connected) return;
    await _connect();
  }

  /// Manually disconnect. Disables auto-reconnect until [ensureConnected] is called again.
  Future<void> disconnect() async {
    _setState(ConnectionState.disconnected);
    _reconnectTimer?.cancel();
    _wsErrorSubscription?.cancel();
    await _wsChannel.disconnect();
  }

  /// Clean up all resources.
  Future<void> dispose() async {
    _disposed = true;
    await disconnect();
    await _stateController.close();
  }

  // ---------------------------------------------------------------------------
  // Private
  // ---------------------------------------------------------------------------

  Future<void> _connect() async {
    _setState(ConnectionState.connecting);
    _wsErrorSubscription?.cancel();

    try {
      final wsUrl = _wsUrlBuilder();
      await _wsChannel.connect(wsUrl);
      _setState(ConnectionState.connected);
      _reconnectAttempts = 0;

      // Listen for disconnection via stream completing/erroring
      _wsErrorSubscription = _wsChannel.events.listen(
        null,
        onError: (_) => _onDisconnect(),
        onDone: _onDisconnect,
      );
    } catch (e) {
      _logger.error('Connection failed: $e');
      _scheduleReconnect();
    }
  }

  void _onDisconnect() {
    if (_disposed || _state == ConnectionState.disconnected) return;
    _logger.info('WebSocket disconnected — scheduling reconnect');
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_disposed) return;
    _setState(ConnectionState.reconnecting);
    _reconnectTimer?.cancel();

    final delaySeconds = _backoffDelays[
        _reconnectAttempts.clamp(0, _backoffDelays.length - 1)];
    _reconnectAttempts++;

    _logger.info('Reconnecting in ${delaySeconds}s (attempt $_reconnectAttempts)');

    _reconnectTimer = Timer(Duration(seconds: delaySeconds), () {
      if (!_disposed) {
        _connect();
      }
    });
  }

  void _setState(ConnectionState newState) {
    if (_state == newState) return;
    _state = newState;
    _logger.debug('Connection state: $newState');
    if (!_stateController.isClosed) {
      _stateController.add(newState);
    }
  }
}
