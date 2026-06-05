/// Sealed error hierarchy for all SDK errors.
///
/// All public Future-returning methods throw subtypes of [SdkError].
/// No raw exceptions from dependencies are surfaced to callers.
///
/// Example:
/// ```dart
/// try {
///   final convo = await client.openConversation('user-123');
/// } on AuthError catch (e) {
///   print('Auth failed: ${e.message}');
/// } on NetworkError catch (e) {
///   print('Network error ${e.statusCode}: ${e.message}');
/// } on SdkError catch (e) {
///   print('SDK error: $e');
/// }
/// ```
sealed class SdkError implements Exception {
  const SdkError();
}

/// A network or HTTP error from the REST transport.
class NetworkError extends SdkError {
  /// HTTP status code, null for connection-level errors.
  final int? statusCode;

  /// Human-readable error message.
  final String message;

  const NetworkError({this.statusCode, required this.message});

  @override
  String toString() => 'NetworkError(statusCode: $statusCode, message: $message)';
}

/// Authentication or authorization failure.
class AuthError extends SdkError {
  final String message;

  const AuthError({required this.message});

  @override
  String toString() => 'AuthError(message: $message)';
}

/// A received message could not be decrypted.
class DecryptionError extends SdkError {
  /// The conversation containing the undecryptable message.
  final String conversationId;

  /// The sequence number of the undecryptable message.
  final int seq;

  const DecryptionError({required this.conversationId, required this.seq});

  @override
  String toString() =>
      'DecryptionError(conversationId: $conversationId, seq: $seq)';
}

/// X3DH key exchange with a recipient failed.
class KeyExchangeError extends SdkError {
  /// The user ID of the intended recipient.
  final String recipientUserId;

  /// Reason for the failure.
  final String reason;

  const KeyExchangeError({required this.recipientUserId, required this.reason});

  @override
  String toString() =>
      'KeyExchangeError(recipientUserId: $recipientUserId, reason: $reason)';
}

/// The session store failed to persist or load state.
class StorageError extends SdkError {
  final String reason;

  const StorageError({required this.reason});

  @override
  String toString() => 'StorageError(reason: $reason)';
}

/// No session was found for the given device ID.
///
/// Thrown when [ChatClientConfig.deviceId] is non-null but no matching
/// key material exists in the [SessionStore].
class SessionNotFoundError extends SdkError {
  final String deviceId;

  const SessionNotFoundError({required this.deviceId});

  @override
  String toString() => 'SessionNotFoundError(deviceId: $deviceId)';
}

/// The file to be uploaded exceeds the 100 MB limit.
class FileTooLargeError extends SdkError {
  final int sizeBytes;

  const FileTooLargeError({required this.sizeBytes});

  @override
  String toString() =>
      'FileTooLargeError(sizeBytes: $sizeBytes, limitBytes: ${100 * 1024 * 1024})';
}

/// An unexpected error not covered by other subtypes.
class UnknownError extends SdkError {
  final Object cause;

  const UnknownError({required this.cause});

  @override
  String toString() => 'UnknownError(cause: $cause)';
}

/// Maps a server [errorCode] and [statusCode] to the appropriate [SdkError].
SdkError sdkErrorFromApiResponse({
  required int statusCode,
  String? errorCode,
  required String message,
}) {
  switch (errorCode) {
    case 'unauthorized':
    case 'forbidden':
    case 'unknown_tenant':
    case 'tenant_inactive':
      return AuthError(message: message);
    case 'invalid_signed_prekey_signature':
      return KeyExchangeError(recipientUserId: '', reason: message);
    case 'storage_unavailable':
      return NetworkError(statusCode: 503, message: message);
    case 'not_found':
      return NetworkError(statusCode: 404, message: message);
    case 'device_limit_reached':
      return NetworkError(statusCode: 409, message: message);
    case 'bad_request':
      return NetworkError(statusCode: 400, message: message);
    case 'internal_error':
      return NetworkError(statusCode: 500, message: message);
    default:
      if (statusCode == 401 || statusCode == 403) {
        return AuthError(message: message);
      }
      return NetworkError(statusCode: statusCode, message: message);
  }
}
