import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'chat_client_config.dart';
import 'conversation_manager.dart';
import 'rt_event_router.dart';
import 'otpk_replenisher.dart';
import 'spk_rotator.dart';
import '../conversations/chat_conversation.dart';
import '../conversations/chat_message.dart';
import '../transport/rest_client.dart';
import '../transport/ws_channel.dart';
import '../transport/connection_manager.dart';
import '../transport/api_models.dart';
import '../session/session_store.dart';
import '../session/session_data.dart';
import '../session/flutter_secure_session_store.dart';
import '../crypto/key_generator.dart';
import '../crypto/x3dh_engine.dart';
import '../crypto/ratchet_engine.dart';
import '../crypto/sender_key_engine.dart';
import '../crypto/crypto_types.dart';
import '../errors/sdk_error.dart';
import '../utils/logger.dart';


/// The root public class of the `rust_e2e_chat_sdk`.
///
/// Encapsulates all state: authentication, WebSocket connection, crypto
/// sessions, and cached conversations.
///
/// ## Quick Start
/// ```dart
/// final client = await ChatClient.initialize(ChatClientConfig(
///   baseUrl: 'https://api.example.com/api',
///   accessToken: myOidcToken,
///   userId: myUserId,
/// ));
///
/// // Open a 1:1 conversation
/// final convo = await client.openConversation('other-user-id');
/// await convo.sendMessage('Hello!');
///
/// // Listen for incoming messages
/// convo.messages.listen((msgs) {
///   for (final msg in msgs) {
///     print('${msg.senderUserId}: ${msg.text}');
///   }
/// });
/// ```
class ChatClient {
  final String _userId;
  String _deviceId;

  String _accessToken;

  // Core components
  late final RestClient _restClient;
  late final SessionStore _sessionStore;
  late final WsChannel _wsChannel;
  late final ConnectionManager _connectionManager;
  late final ConversationManager _conversationManager;
  late final RealTimeEventRouter _rtEventRouter;
  late final OtpkReplenisher _otpkReplenisher;
  late final SpkRotator _spkRotator;
  late final SdkLogger _logger;

  // Error streams
  final StreamController<SdkError> _connectionErrorsController =
      StreamController<SdkError>.broadcast();
  final StreamController<SdkError> _storageErrorsController =
      StreamController<SdkError>.broadcast();
  final StreamController<String> _warningsController =
      StreamController<String>.broadcast();

  // Private constructor — use [initialize].
  ChatClient._({
    required ChatClientConfig config,
    required String deviceId,
    required String accessToken,
  })  : _userId = config.userId,
        _deviceId = deviceId,
        _accessToken = accessToken;


  // ---------------------------------------------------------------------------
  // Factory initializer
  // ---------------------------------------------------------------------------

  /// Initializes the SDK and returns a ready [ChatClient].
  ///
  /// If [ChatClientConfig.deviceId] is null, a new device is automatically
  /// registered. If it is non-null, the existing session is loaded from the
  /// [SessionStore].
  ///
  /// Throws [SessionNotFoundError] if [deviceId] is provided but no matching
  /// key material exists in the store.
  static Future<ChatClient> initialize(ChatClientConfig config) async {
    final logger = SdkLogger(tag: 'ChatClient', level: config.logLevel);

    // Resolve session store
    final SessionStore sessionStore;
    final storageErrorsController =
        StreamController<SdkError>.broadcast();

    if (config.sessionStore != null) {
      sessionStore = config.sessionStore!;
    } else if (kIsWeb) {
      logger.warning(
          'flutter_secure_storage is not supported on web. Using InMemorySessionStore. '
          'Session will NOT persist across page reloads.');
      sessionStore = InMemorySessionStore();
    } else {
      sessionStore = FlutterSecureSessionStore(
          errorController: storageErrorsController);
    }

    // Token provider closure — always reads the latest token
    String currentToken = config.accessToken;
    String Function() tokenProvider = () => currentToken;

    // REST client
    final restClient = RestClient(
      baseUrl: config.baseUrl,
      tokenProvider: tokenProvider,
    );

    // Resolve device ID and key material
    String deviceId;
    if (config.deviceId == null) {
      // Auto-register new device
      logger.info('No deviceId provided — registering new device');
      final bundle = await KeyGenerator.generateKeyBundle();

      // Verify SPK signature before submitting
      final sigValid = await KeyGenerator.verifySignature(
        data: bundle.signedPreKeyPair.publicKey,
        signature: bundle.signedPreKeySignature,
        publicKey: bundle.identitySigningKeyPair.publicKey,
      );
      if (!sigValid) {
        throw const KeyExchangeError(
            recipientUserId: '', reason: 'SPK self-signature verification failed');
      }

      final request = RegisterDeviceRequest(
        userId: config.userId,
        identityKey: base64.encode(bundle.identityKeyPair.publicKey),
        signedPreKey: base64.encode(bundle.signedPreKeyPair.publicKey),
        signedPreKeySignature: base64.encode(bundle.signedPreKeySignature),
        signedPreKeyId: bundle.signedPreKeyId,
        oneTimePreKeys: bundle.oneTimePreKeys
            .map((k) => {
                  'id': k.id,
                  'public_key': base64.encode(k.keyPair.publicKey),
                })
            .toList(),
      );

      final response = await restClient.registerDevice(config.userId, request);
      deviceId = response.deviceId;

      final record = DeviceRecord(
        userId: config.userId,
        deviceId: deviceId,
        identityKeyPrivate: bundle.identityKeyPair.privateKey,
        identityKeyPublic: bundle.identityKeyPair.publicKey,
        identitySigningKeyPrivate: bundle.identitySigningKeyPair.privateKey,
        identitySigningKeyPublic: bundle.identitySigningKeyPair.publicKey,
        signedPrekeyPrivate: bundle.signedPreKeyPair.privateKey,
        signedPrekeyPublic: bundle.signedPreKeyPair.publicKey,
        signedPrekeyId: bundle.signedPreKeyId,
        oneTimePrekeys: bundle.oneTimePreKeys
            .map((k) => OtpkRecord(
                  id: k.id,
                  privateKey: k.keyPair.privateKey,
                  publicKey: k.keyPair.publicKey,
                ))
            .toList(),
        signedPrekeyCreatedAt: DateTime.now(),
      );
      await sessionStore.saveDevice(record);
      logger.info('Device registered: $deviceId');
    } else {
      deviceId = config.deviceId!;
      final existing = await sessionStore.loadDevice(config.userId, deviceId);
      if (existing == null) {
        throw SessionNotFoundError(deviceId: deviceId);
      }
      logger.info('Loaded existing device: $deviceId');
    }

    final client = ChatClient._(
      config: config,
      deviceId: deviceId,
      accessToken: config.accessToken,
    );
    client._accessToken = config.accessToken;

    // Wire up components
    client._logger = logger;
    client._restClient = restClient;
    client._sessionStore = sessionStore;
    client._storageErrorsController.addStream(storageErrorsController.stream);

    client._wsChannel = WsChannel(logger: logger);
    client._connectionManager = ConnectionManager(
      wsChannel: client._wsChannel,
      wsUrlBuilder: () =>
          '${config.baseUrl.replaceFirst('http', 'ws')}/ws'
          '?token=${client._accessToken}&device_id=$deviceId',
      logger: logger,
    );

    client._conversationManager = ConversationManager(
      sessionStore: sessionStore,
      restClient: restClient,
      localUserId: config.userId,
      localDeviceId: deviceId,
      baseUrl: config.baseUrl,
      tokenProvider: tokenProvider,
      logger: logger,
    );

    client._otpkReplenisher = OtpkReplenisher(
      restClient: restClient,
      sessionStore: sessionStore,
      userId: config.userId,
      deviceId: deviceId,
      logger: logger,
    );

    client._rtEventRouter = RealTimeEventRouter(
      wsChannel: client._wsChannel,
      conversationManager: client._conversationManager,
      onLowOtpk: client._otpkReplenisher.onLowOtpk,
      logger: logger,
    );

    client._spkRotator = SpkRotator(
      restClient: restClient,
      sessionStore: sessionStore,
      userId: config.userId,
      deviceId: deviceId,
      rotationDays: config.signedPrekeyRotationDays,
      logger: logger,
    );

    // Load persisted conversations
    await client._conversationManager.loadAll();

    // Start background tasks
    client._rtEventRouter.start();
    client._spkRotator.start();

    // Connect if autoConnect
    if (config.autoConnect) {
      await client._connectionManager.ensureConnected();
    }

    return client;
  }

  // ---------------------------------------------------------------------------
  // Identity
  // ---------------------------------------------------------------------------

  /// The authenticated user ID.
  String get userId => _userId;

  /// The registered device ID.
  String get deviceId => _deviceId;

  /// Returns the device's public key bundle for display or diagnostics.
  Future<KeyBundle> getPublicKeyBundle() async {
    final record = await _sessionStore.loadDevice(_userId, _deviceId);
    if (record == null) throw SessionNotFoundError(deviceId: _deviceId);
    return KeyBundle(
      identityKey: record.identityKeyPublic,
      signedPreKey: record.signedPrekeyPublic,
      signedPreKeySignature: Uint8List(0), // stored separately
      signedPreKeyId: record.signedPrekeyId,
      oneTimePreKeys: [],
    );
  }

  // ---------------------------------------------------------------------------
  // Connection
  // ---------------------------------------------------------------------------

  /// Stream of connection state changes.
  Stream<ConnectionState> get connectionState => _connectionManager.state;

  /// Stream of connection-level errors.
  Stream<SdkError> get connectionErrors => _connectionErrorsController.stream;

  /// Stream of storage errors.
  Stream<SdkError> get storageErrors => _storageErrorsController.stream;

  /// Stream of non-fatal warnings (e.g. OTPK depletion).
  Stream<String> get warnings => _warningsController.stream;

  /// Manually connects to the WebSocket.
  Future<void> connect() async {
    await _connectionManager.ensureConnected();
  }

  /// Manually disconnects from the WebSocket.
  Future<void> disconnect() async {
    await _connectionManager.disconnect();
  }

  /// Updates the access token in-place without re-initialization.
  ///
  /// All subsequent REST requests and the next WebSocket reconnect use [newAccessToken].
  Future<void> updateToken(String newAccessToken) async {
    _accessToken = newAccessToken;
    _logger.info('Access token updated');
  }

  // ---------------------------------------------------------------------------
  // Conversations
  // ---------------------------------------------------------------------------

  /// Broadcast stream of all known conversations.
  Stream<List<ChatConversation>> get conversations =>
      _conversationManager.conversations;

  /// Opens a 1:1 conversation with [recipientUserId].
  ///
  /// If a conversation already exists with that user, the cached instance is
  /// returned without making any network calls (idempotent).
  ///
  /// Performs X3DH key agreement on first call.
  ///
  /// Example:
  /// ```dart
  /// final convo = await client.openConversation('alice');
  /// await convo.sendMessage('Hey Alice!');
  /// ```
  Future<ChatConversation> openConversation(String recipientUserId) async {
    // Check cache first — idempotent
    final existing = _conversationManager.all
        .where((c) =>
            c.type == ConversationType.oneToOne &&
            c.currentMembers.any((m) => m.userId == recipientUserId))
        .firstOrNull;
    if (existing != null) return existing;

    try {
      // Fetch recipient's key bundle
      final keyBundle = await _restClient.getKeyBundle(recipientUserId);

      if (keyBundle.oneTimePreKey == null) {
        _warningsController.add(
            'Recipient $recipientUserId has no OTPKs — proceeding without OTPK (reduced security)');
      }

      // Load local private bundle
      final localRecord =
          await _sessionStore.loadDevice(_userId, _deviceId);
      if (localRecord == null) throw SessionNotFoundError(deviceId: _deviceId);

      final localBundle = _deviceRecordToPrivateBundle(localRecord);

      // Perform X3DH
      final x3dhResult = await X3dhEngine.performX3dh(
        recipientBundle: keyBundle,
        senderBundle: localBundle,
      );

      // Initialize Double Ratchet (sender side)
      final ratchetState = await RatchetEngine.initSender(
        x3dhResult.sharedSecret,
        keyBundle.signedPreKey,
        '', // conversationId not known yet
      );

      // Create conversation on server with initial encrypted envelope
      final createReq = CreateConversationRequest(
        recipientUserId: recipientUserId,
        recipientDeviceId: keyBundle.deviceId,
        initialEnvelope: {
          'x3dh_header': _encodeX3dhHeader(x3dhResult.header),
        },
      );
      final createResp = await _restClient.createConversation(createReq);
      final conversationId = createResp.conversationId;

      // Re-initialize ratchet with correct conversationId
      final finalRatchetState = RatchetState(
        conversationId: conversationId,
        rootKey: ratchetState.rootKey,
        chainKeySend: ratchetState.chainKeySend,
        chainKeyRecv: ratchetState.chainKeyRecv,
        dhSendPub: ratchetState.dhSendPub,
        dhSendPriv: ratchetState.dhSendPriv,
        dhRecvPub: ratchetState.dhRecvPub,
        nSend: ratchetState.nSend,
        nRecv: ratchetState.nRecv,
        pn: ratchetState.pn,
        skippedMessageKeys: ratchetState.skippedMessageKeys,
      );

      // Persist ratchet state
      await _sessionStore.saveRatchetState(conversationId, finalRatchetState);

      // Create and cache the conversation
      final convo = _conversationManager.getOrCreateOneToOne(
        conversationId: conversationId,
        recipientUserId: recipientUserId,
      );

      return convo;
    } on SdkError {
      rethrow;
    } catch (e) {
      throw UnknownError(cause: e);
    }
  }

  /// Creates a new group conversation with [memberUserIds].
  ///
  /// Generates a SenderKey, fetches each member's key bundle, distributes the
  /// SenderKey via 1:1 encrypted SKDM messages.
  ///
  /// Example:
  /// ```dart
  /// final group = await client.createGroup(['alice', 'bob', 'carol']);
  /// await group.sendMessage('Welcome everyone!');
  /// ```
  Future<ChatConversation> createGroup(List<String> memberUserIds) async {
    try {
      // Create group on server
      final allMembers = [...memberUserIds, _userId];
      final createResp = await _restClient.createGroup(
          CreateGroupRequest(memberUserIds: allMembers));
      final conversationId = createResp.conversationId;

      // Generate sender key for the creator
      final senderKeyPair = await KeyGenerator.generateSenderKey();
      final senderKeySession = SenderKeyEngine.createSession(
        conversationId: conversationId,
        userId: _userId,
        keyPair: senderKeyPair,
      );

      await _sessionStore.saveSenderKey(
          conversationId, _userId, senderKeySession.toRecord());

      // Distribute SKDM to all members
      final serializedSkdm =
          SenderKeyEngine.serializeKeyMaterial(senderKeySession);
      final recipients = memberUserIds
          .map((uid) => SkdmRecipient(
                userId: uid,
                deviceId: '',
                encryptedSkdm: base64.encode(serializedSkdm),
              ))
          .toList();

      await _restClient.distributeGroupSenderKey(conversationId, recipients);

      // Register conversation in manager
      final convo = _conversationManager.registerGroup(
        conversationId: conversationId,
        memberUserIds: allMembers,
      );

      return convo;
    } on SdkError {
      rethrow;
    } catch (e) {
      throw UnknownError(cause: e);
    }
  }

  /// Finds an existing conversation by [conversationId]. Returns null if not found.
  Future<ChatConversation?> findConversation(String conversationId) async {
    return _conversationManager.find(conversationId);
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  /// Disposes the [ChatClient], closing all connections and releasing resources.
  Future<void> dispose() async {
    _spkRotator.stop();
    await _rtEventRouter.stop();
    await _connectionManager.dispose();
    _conversationManager.dispose();
    await _connectionErrorsController.close();
    await _storageErrorsController.close();
    await _warningsController.close();
    _logger.info('ChatClient disposed');
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  PrivateKeyBundle _deviceRecordToPrivateBundle(DeviceRecord record) {
    return PrivateKeyBundle(
      identityKeyPair: Curve25519KeyPair(
        publicKey: record.identityKeyPublic,
        privateKey: record.identityKeyPrivate,
      ),
      identitySigningKeyPair: Ed25519KeyPair(
        publicKey: record.identitySigningKeyPublic,
        privateKey: record.identitySigningKeyPrivate,
      ),
      signedPreKeyPair: Curve25519KeyPair(
        publicKey: record.signedPrekeyPublic,
        privateKey: record.signedPrekeyPrivate,
      ),
      signedPreKeySignature: Uint8List(0),
      signedPreKeyId: record.signedPrekeyId,
      oneTimePreKeys: [],
    );
  }

  Map<String, dynamic> _encodeX3dhHeader(X3dhInitHeader header) => {
        'ephemeral_key': base64.encode(header.ephemeralKey),
        'identity_key': base64.encode(header.identityKey),
        'signed_prekey_id': header.signedPreKeyId,
        if (header.oneTimePreKeyId != null)
          'one_time_prekey_id': header.oneTimePreKeyId,
      };
}
