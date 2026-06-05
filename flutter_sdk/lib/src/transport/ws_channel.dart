import 'dart:async';
import 'dart:convert';
import 'package:web_socket_channel/web_socket_channel.dart';
import 'rt_event.dart';
import '../utils/logger.dart';

/// WebSocket wrapper that handles frame parsing, ping/pong, and ack.
///
/// Exposes decoded [RtEvent] frames as a broadcast stream.
class WsChannel {
  final SdkLogger _logger;

  WebSocketChannel? _channel;
  StreamController<RtEvent>? _eventController;
  StreamSubscription? _subscription;

  WsChannel({SdkLogger? logger})
      : _logger = logger ?? SdkLogger(tag: 'WsChannel');

  /// Broadcast stream of decoded [RtEvent] frames from the server.
  Stream<RtEvent> get events => _eventController?.stream ?? const Stream.empty();

  bool get isConnected => _channel != null;

  /// Connects to the WebSocket at [wsUrl].
  Future<void> connect(String wsUrl) async {
    await disconnect();

    _eventController = StreamController<RtEvent>.broadcast();
    _channel = WebSocketChannel.connect(Uri.parse(wsUrl));

    await _channel!.ready.catchError((e) {
      _logger.error('WebSocket connect failed: $e');
      throw e;
    });

    _subscription = _channel!.stream.listen(
      _onFrame,
      onError: (e) {
        _logger.error('WebSocket error: $e');
        _eventController?.addError(e);
      },
      onDone: () {
        _logger.info('WebSocket closed');
        _eventController?.close();
      },
    );

    _logger.info('WebSocket connected to $wsUrl');
  }

  /// Disconnects the WebSocket cleanly.
  Future<void> disconnect() async {
    await _subscription?.cancel();
    _subscription = null;
    await _channel?.sink.close();
    _channel = null;
    await _eventController?.close();
    _eventController = null;
  }

  /// Sends an ack frame for a successfully decrypted message.
  Future<void> sendAck(String conversationId, int seq) async {
    _send(jsonEncode({'type': 'ack', 'conversation_id': conversationId, 'seq': seq}));
  }

  void _sendPong() {
    _send(jsonEncode({'type': 'pong'}));
  }

  void _send(String data) {
    if (_channel != null) {
      try {
        _channel!.sink.add(data);
      } catch (e) {
        _logger.error('WebSocket send failed: $e');
      }
    }
  }

  void _onFrame(dynamic raw) {
    if (raw is! String) return;
    try {
      final json = jsonDecode(raw) as Map<String, dynamic>;
      final event = RtEvent.fromJson(json);
      if (event == null) {
        _logger.debug('Unknown WS frame type: ${json['type']}');
        return;
      }
      if (event is PingRtEvent) {
        _sendPong();
        return; // Don't emit ping to consumers
      }
      _eventController?.add(event);
    } catch (e) {
      _logger.error('Failed to parse WS frame: $e — raw: $raw');
    }
  }
}
