import '../session/session_store.dart';
import '../utils/logger.dart';

/// Configuration for [ChatClient].
///
/// Example:
/// ```dart
/// final config = ChatClientConfig(
///   baseUrl: 'https://api.example.com/api',
///   accessToken: oidcToken,
///   userId: 'user-uuid',
/// );
/// final client = await ChatClient.initialize(config);
/// ```
class ChatClientConfig {
  /// Base URL of the `rust-e2e-chat-api` server (no trailing slash).
  /// e.g. `"https://api.example.com/api"`
  final String baseUrl;

  /// OIDC JWT access token for the current user.
  final String accessToken;

  /// The OIDC `sub` claim value identifying the user.
  final String userId;

  /// Previously registered device ID. Pass null to trigger auto-registration
  /// (generates a new key bundle and registers the device).
  final String? deviceId;

  /// Custom session store. Defaults to [FlutterSecureSessionStore] on mobile/
  /// desktop and [InMemorySessionStore] on web/server.
  final SessionStore? sessionStore;

  /// If true (default), the SDK connects to the WebSocket immediately after
  /// [ChatClient.initialize] returns.
  final bool autoConnect;

  /// How many days before the Signed Pre-Key is automatically rotated.
  /// Default: 7 days.
  final int signedPrekeyRotationDays;

  /// Controls the verbosity of internal SDK logging.
  /// Default: [LogLevel.warning].
  final LogLevel logLevel;

  const ChatClientConfig({
    required this.baseUrl,
    required this.accessToken,
    required this.userId,
    this.deviceId,
    this.sessionStore,
    this.autoConnect = true,
    this.signedPrekeyRotationDays = 7,
    this.logLevel = LogLevel.warning,
  });
}
