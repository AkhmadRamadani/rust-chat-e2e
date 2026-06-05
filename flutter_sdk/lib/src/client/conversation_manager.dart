import 'dart:async';
import '../conversations/chat_conversation.dart';
import '../conversations/chat_message.dart';
import '../conversations/group_conversation.dart';
import '../session/session_store.dart';
import '../session/session_data.dart';
import '../transport/rest_client.dart';
import '../utils/logger.dart';

/// Owns and caches all active [ChatConversation] instances.
///
/// Loaded from the [SessionStore] on startup; new conversations are registered
/// and persisted on first access.
class ConversationManager {
  final SessionStore _sessionStore;
  final RestClient _restClient;
  final String _localUserId;
  final String _localDeviceId;
  final String _baseUrl;
  final String Function() _tokenProvider;
  final SdkLogger _logger;

  final Map<String, ChatConversation> _cache = {};
  final StreamController<List<ChatConversation>> _conversationsController =
      StreamController<List<ChatConversation>>.broadcast();

  ConversationManager({
    required SessionStore sessionStore,
    required RestClient restClient,
    required String localUserId,
    required String localDeviceId,
    required String baseUrl,
    required String Function() tokenProvider,
    SdkLogger? logger,
  })  : _sessionStore = sessionStore,
        _restClient = restClient,
        _localUserId = localUserId,
        _localDeviceId = localDeviceId,
        _baseUrl = baseUrl,
        _tokenProvider = tokenProvider,
        _logger = logger ?? SdkLogger(tag: 'ConversationManager');

  /// Broadcast stream of all known conversations.
  Stream<List<ChatConversation>> get conversations =>
      _conversationsController.stream;

  /// All currently cached conversations.
  List<ChatConversation> get all => List.unmodifiable(_cache.values.toList());

  /// Loads all persisted conversations from the [SessionStore].
  Future<void> loadAll() async {
    final metas = await _sessionStore.loadAllConversations();
    for (final meta in metas) {
      if (!_cache.containsKey(meta.conversationId)) {
        final convo = _buildConversation(meta);
        _cache[meta.conversationId] = convo;
      }
    }
    _emit();
    _logger.info('Loaded ${_cache.length} conversations from store');
  }

  /// Returns an existing [ChatConversation] by ID, or null if not found.
  ChatConversation? find(String conversationId) => _cache[conversationId];

  /// Returns an existing conversation or creates a new 1:1 entry.
  ///
  /// Should only be called by [ChatClient.openConversation] after X3DH is done.
  OneToOneConversation getOrCreateOneToOne({
    required String conversationId,
    required String recipientUserId,
  }) {
    if (_cache.containsKey(conversationId)) {
      return _cache[conversationId]! as OneToOneConversation;
    }

    final convo = OneToOneConversation(
      conversationId: conversationId,
      localUserId: _localUserId,
      localDeviceId: _localDeviceId,
      restClient: _restClient,
      sessionStore: _sessionStore,
      baseUrl: _baseUrl,
      tokenProvider: _tokenProvider,
      initialMembers: [
        ConversationMember(userId: _localUserId),
        ConversationMember(userId: recipientUserId),
      ],
    );

    _cache[conversationId] = convo;
    _emit();

    // Persist metadata
    _sessionStore.saveConversationMeta(ConversationMeta(
      conversationId: conversationId,
      type: 'oneToOne',
      memberUserIds: [_localUserId, recipientUserId],
      createdAt: DateTime.now(),
    ));

    return convo;
  }

  /// Registers a new group conversation.
  GroupConversation registerGroup({
    required String conversationId,
    required List<String> memberUserIds,
  }) {
    if (_cache.containsKey(conversationId)) {
      return _cache[conversationId]! as GroupConversation;
    }

    final convo = GroupConversation(
      conversationId: conversationId,
      localUserId: _localUserId,
      localDeviceId: _localDeviceId,
      restClient: _restClient,
      sessionStore: _sessionStore,
      baseUrl: _baseUrl,
      tokenProvider: _tokenProvider,
      initialMembers:
          memberUserIds.map((uid) => ConversationMember(userId: uid)).toList(),
    );

    _cache[conversationId] = convo;
    _emit();

    _sessionStore.saveConversationMeta(ConversationMeta(
      conversationId: conversationId,
      type: 'group',
      memberUserIds: memberUserIds,
      createdAt: DateTime.now(),
    ));

    return convo;
  }

  // ---------------------------------------------------------------------------
  // Private
  // ---------------------------------------------------------------------------

  ChatConversation _buildConversation(ConversationMeta meta) {
    final members = meta.memberUserIds
        .map((uid) => ConversationMember(userId: uid))
        .toList();

    if (meta.type == 'group') {
      return GroupConversation(
        conversationId: meta.conversationId,
        localUserId: _localUserId,
        localDeviceId: _localDeviceId,
        restClient: _restClient,
        sessionStore: _sessionStore,
        baseUrl: _baseUrl,
        tokenProvider: _tokenProvider,
        initialMembers: members,
      );
    } else {
      return OneToOneConversation(
        conversationId: meta.conversationId,
        localUserId: _localUserId,
        localDeviceId: _localDeviceId,
        restClient: _restClient,
        sessionStore: _sessionStore,
        baseUrl: _baseUrl,
        tokenProvider: _tokenProvider,
        initialMembers: members,
      );
    }
  }

  void _emit() {
    if (!_conversationsController.isClosed) {
      _conversationsController
          .add(List.unmodifiable(_cache.values.toList()));
    }
  }

  void dispose() {
    _conversationsController.close();
  }
}
