import 'dart:async';
import '../transport/rt_event.dart';
import '../transport/ws_channel.dart';
import '../conversations/group_conversation.dart';
import '../conversations/chat_conversation.dart';
import 'conversation_manager.dart';
import '../utils/logger.dart';

/// Subscribes to [WsChannel.events] and routes each [RtEvent] to the
/// appropriate handler.
///
/// - [MessageRtEvent] → target [ChatConversation]
/// - [SenderKeyDistributionRtEvent] → target [GroupConversation]
/// - [MemberAddedRtEvent] / [MemberRemovedRtEvent] → target [GroupConversation]
/// - [LowOtpkRtEvent] → [_onLowOtpk] callback
class RealTimeEventRouter {
  final WsChannel _wsChannel;
  final ConversationManager _conversationManager;
  final void Function(LowOtpkRtEvent event) _onLowOtpk;
  final SdkLogger _logger;

  StreamSubscription<RtEvent>? _subscription;

  RealTimeEventRouter({
    required WsChannel wsChannel,
    required ConversationManager conversationManager,
    required void Function(LowOtpkRtEvent event) onLowOtpk,
    SdkLogger? logger,
  })  : _wsChannel = wsChannel,
        _conversationManager = conversationManager,
        _onLowOtpk = onLowOtpk,
        _logger = logger ?? SdkLogger(tag: 'RtEventRouter');

  /// Starts listening to [WsChannel.events].
  void start() {
    _subscription = _wsChannel.events.listen(
      _dispatch,
      onError: (e) => _logger.error('WS event stream error: $e'),
    );
  }

  /// Stops routing events.
  Future<void> stop() async {
    await _subscription?.cancel();
    _subscription = null;
  }

  // ---------------------------------------------------------------------------
  // Private
  // ---------------------------------------------------------------------------

  Future<void> _dispatch(RtEvent event) async {
    try {
      switch (event) {
        case MessageRtEvent():
          await _handleMessage(event);
        case SenderKeyDistributionRtEvent():
          await _handleSkdm(event);
        case MemberAddedRtEvent():
          await _handleMemberAdded(event);
        case MemberRemovedRtEvent():
          await _handleMemberRemoved(event);
        case LowOtpkRtEvent():
          _onLowOtpk(event);
        case PingRtEvent():
          break; // handled by WsChannel itself
      }
    } catch (e) {
      _logger.error('Error dispatching event ${event.runtimeType}: $e');
    }
  }

  Future<void> _handleMessage(MessageRtEvent event) async {
    final convo = _conversationManager.find(event.conversationId);
    if (convo == null) {
      _logger.warning(
          'Received message for unknown conversation ${event.conversationId}');
      return;
    }
    await convo.onIncomingMessage(event);
    // Send ack after successful delivery
    await _wsChannel.sendAck(event.conversationId, event.seq);
  }

  Future<void> _handleSkdm(SenderKeyDistributionRtEvent event) async {
    final convo = _conversationManager.find(event.conversationId);
    if (convo is GroupConversation) {
      await convo.onSkdm(event);
    } else {
      _logger.warning(
          'SKDM for unknown/non-group conversation ${event.conversationId}');
    }
  }

  Future<void> _handleMemberAdded(MemberAddedRtEvent event) async {
    final convo = _conversationManager.find(event.conversationId);
    if (convo is GroupConversation) {
      await convo.onMemberAdded(event);
    }
  }

  Future<void> _handleMemberRemoved(MemberRemovedRtEvent event) async {
    final convo = _conversationManager.find(event.conversationId);
    if (convo is GroupConversation) {
      await convo.onMemberRemoved(event);
    }
  }
}
