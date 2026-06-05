/// The `rust_e2e_chat_sdk` package.
///
/// Provides a complete, type-safe Flutter/Dart client for the
/// `rust-e2e-chat-api` platform with:
/// - X3DH + Double Ratchet 1:1 end-to-end encryption
/// - Sender Key group messaging
/// - WebSocket real-time delivery with auto-reconnect
/// - Reactive (Stream-based) public API
///
/// ## Quick Start
/// ```dart
/// import 'package:rust_e2e_chat_sdk/rust_e2e_chat_sdk.dart';
///
/// final client = await ChatClient.initialize(ChatClientConfig(
///   baseUrl: 'https://api.example.com/api',
///   accessToken: myOidcToken,
///   userId: myUserId,
/// ));
///
/// final convo = await client.openConversation('alice');
/// await convo.sendMessage('Hello!');
/// ```
library rust_e2e_chat_sdk;

// Public entry point
export 'src/client/chat_client.dart';
export 'src/client/chat_client_config.dart';

// Connection state
export 'src/transport/connection_manager.dart' show ConnectionState;

// Conversation types
export 'src/conversations/chat_conversation.dart' show ChatConversation;
export 'src/conversations/chat_message.dart'
    show
        ChatMessage,
        ChatMessageType,
        ConversationType,
        ConversationMember,
        AttachmentUploadProgress;

// Session store interface + implementations
export 'src/session/session_store.dart' show SessionStore, InMemorySessionStore;
export 'src/session/flutter_secure_session_store.dart'
    show FlutterSecureSessionStore;

// Errors
export 'src/errors/sdk_error.dart'
    show
        SdkError,
        NetworkError,
        AuthError,
        DecryptionError,
        KeyExchangeError,
        StorageError,
        SessionNotFoundError,
        FileTooLargeError,
        UnknownError;

// Utilities (public — useful for custom SessionStore implementations)
export 'src/utils/logger.dart' show LogLevel;

// Key types (public — needed for custom implementations)
export 'src/crypto/crypto_types.dart'
    show KeyBundle, OneTimePreKey, SignedPreKeyUpdate;
