import 'dart:convert';
import 'dart:typed_data';
import 'package:dio/dio.dart';
import '../errors/sdk_error.dart';
import '../crypto/crypto_types.dart';
import 'api_models.dart';

/// Typed REST client for all `rust-e2e-chat-api` endpoints.
///
/// All methods map 1:1 to a server endpoint. Authentication is injected via
/// the [tokenProvider] closure, which is called on every request so that
/// token hot-swaps (via [ChatClient.updateToken]) take effect immediately.
class RestClient {
  final Dio _dio;

  RestClient({
    required String baseUrl,
    required String Function() tokenProvider,
  })  : _dio = Dio(BaseOptions(
          baseUrl: baseUrl,
          connectTimeout: const Duration(seconds: 10),
          receiveTimeout: const Duration(seconds: 30),
          headers: {'Content-Type': 'application/json'},
        )) {
    _dio.interceptors.addAll([
      _AuthInterceptor(tokenProvider),
      _ErrorInterceptor(),
      _RetryInterceptor(dio: _dio),
    ]);
  }

  // ---------------------------------------------------------------------------
  // KDS endpoints
  // ---------------------------------------------------------------------------

  Future<RegisterDeviceResponse> registerDevice(
      String userId, RegisterDeviceRequest request) async {
    final response = await _dio.post(
      '/users/$userId/devices',
      data: jsonEncode(request.toJson()),
    );
    return RegisterDeviceResponse.fromJson(
        response.data as Map<String, dynamic>);
  }

  Future<KeyBundleResponse> getKeyBundle(String userId) async {
    final response = await _dio.get('/users/$userId/key-bundle');
    return KeyBundleResponse.fromJson(response.data as Map<String, dynamic>);
  }

  Future<ReplenishOtpksResponse> replenishOtpks(
      String userId,
      String deviceId,
      List<OneTimePreKey> keys) async {
    final request = ReplenishOtpksRequest(
      oneTimePreKeys: keys
          .map((k) => {
                'id': k.id,
                'public_key': base64.encode(k.publicKey),
              })
          .toList(),
    );
    final response = await _dio.put(
      '/users/$userId/devices/$deviceId/one-time-prekeys',
      data: jsonEncode(request.toJson()),
    );
    return ReplenishOtpksResponse.fromJson(
        response.data as Map<String, dynamic>);
  }

  Future<void> rotateSignedPreKey(
      String userId, String deviceId, SignedPreKeyUpdate update) async {
    await _dio.put(
      '/users/$userId/devices/$deviceId/signed-prekey',
      data: jsonEncode(update.toJson()),
    );
  }

  // ---------------------------------------------------------------------------
  // Conversation endpoints
  // ---------------------------------------------------------------------------

  Future<CreateConversationResponse> createConversation(
      CreateConversationRequest request) async {
    final response = await _dio.post(
      '/conversations',
      data: jsonEncode(request.toJson()),
    );
    return CreateConversationResponse.fromJson(
        response.data as Map<String, dynamic>);
  }

  Future<SendMessageResponse> sendMessage(
      String conversationId, SendMessageRequest request) async {
    final response = await _dio.post(
      '/conversations/$conversationId/messages',
      data: jsonEncode(request.toJson()),
    );
    return SendMessageResponse.fromJson(response.data as Map<String, dynamic>);
  }

  Future<GetMessagesResponse> getMessages(
    String conversationId, {
    int limit = 50,
    int? beforeSeq,
  }) async {
    final queryParams = {
      'limit': limit,
      if (beforeSeq != null) 'before_seq': beforeSeq,
    };
    final response = await _dio.get(
      '/conversations/$conversationId/messages',
      queryParameters: queryParams,
    );
    return GetMessagesResponse.fromJson(response.data as Map<String, dynamic>);
  }

  // ---------------------------------------------------------------------------
  // Group endpoints
  // ---------------------------------------------------------------------------

  Future<CreateGroupResponse> createGroup(
      CreateGroupRequest request) async {
    final response = await _dio.post(
      '/groups',
      data: jsonEncode(request.toJson()),
    );
    return CreateGroupResponse.fromJson(
        response.data as Map<String, dynamic>);
  }

  Future<SendMessageResponse> sendGroupMessage(
      String conversationId, SendMessageRequest request) async {
    final response = await _dio.post(
      '/groups/$conversationId/messages',
      data: jsonEncode(request.toJson()),
    );
    return SendMessageResponse.fromJson(response.data as Map<String, dynamic>);
  }

  Future<void> addGroupMember(
      String conversationId, String userId, String deviceId) async {
    await _dio.post(
      '/groups/$conversationId/members',
      data: jsonEncode({'user_id': userId, 'device_id': deviceId}),
    );
  }

  Future<void> removeGroupMember(
      String conversationId, String userId) async {
    await _dio.delete('/groups/$conversationId/members/$userId');
  }

  Future<void> distributeGroupSenderKey(
      String conversationId, List<SkdmRecipient> recipients) async {
    await _dio.post(
      '/groups/$conversationId/sender-key-distribution',
      data: jsonEncode({'recipients': recipients.map((r) => r.toJson()).toList()}),
    );
  }

  // ---------------------------------------------------------------------------
  // Attachment endpoints
  // ---------------------------------------------------------------------------

  Future<UploadResponse> uploadAttachment(
    Uint8List bytes,
    String filename,
    String contentType, {
    void Function(int sent, int total)? onProgress,
  }) async {
    if (bytes.length > 100 * 1024 * 1024) {
      throw FileTooLargeError(sizeBytes: bytes.length);
    }

    final formData = FormData.fromMap({
      'file': MultipartFile.fromBytes(
        bytes,
        filename: filename,
        contentType: DioMediaType.parse(contentType),
      ),
    });

    final response = await _dio.post(
      '/attachments',
      data: formData,
      onSendProgress: onProgress,
    );
    return UploadResponse.fromJson(response.data as Map<String, dynamic>);
  }
}

// ---------------------------------------------------------------------------
// Interceptors
// ---------------------------------------------------------------------------

class _AuthInterceptor extends Interceptor {
  final String Function() _tokenProvider;

  _AuthInterceptor(this._tokenProvider);

  @override
  void onRequest(RequestOptions options, RequestInterceptorHandler handler) {
    options.headers['Authorization'] = 'Bearer ${_tokenProvider()}';
    handler.next(options);
  }
}

class _ErrorInterceptor extends Interceptor {
  @override
  void onError(DioException err, ErrorInterceptorHandler handler) {
    final response = err.response;
    if (response != null) {
      String? errorCode;
      String message = 'Request failed';
      try {
        final data = response.data;
        if (data is Map<String, dynamic>) {
          errorCode = data['error_code'] as String?;
          message = data['message'] as String? ?? message;
        }
      } catch (_) {}

      final sdkError = sdkErrorFromApiResponse(
        statusCode: response.statusCode ?? 0,
        errorCode: errorCode,
        message: message,
      );
      handler.reject(
        DioException(
          requestOptions: err.requestOptions,
          error: sdkError,
          type: DioExceptionType.badResponse,
          response: response,
          message: sdkError.toString(),
        ),
      );
    } else {
      handler.reject(
        DioException(
          requestOptions: err.requestOptions,
          error: NetworkError(message: err.message ?? 'Connection failed'),
          type: err.type,
          message: err.message,
        ),
      );
    }
  }
}

class _RetryInterceptor extends Interceptor {
  final Dio dio;
  static const _maxRetries = 3;
  static const _retryDelay = Duration(milliseconds: 500);

  _RetryInterceptor({required this.dio});

  @override
  Future<void> onError(
      DioException err, ErrorInterceptorHandler handler) async {
    final response = err.response;
    final retryCount =
        (err.requestOptions.extra['retryCount'] as int?) ?? 0;

    final shouldRetry = retryCount < _maxRetries &&
        (response == null ||
            response.statusCode == 503 ||
            response.statusCode == 500 ||
            err.type == DioExceptionType.connectionTimeout ||
            err.type == DioExceptionType.receiveTimeout);

    if (shouldRetry) {
      await Future.delayed(_retryDelay * (retryCount + 1));
      final options = err.requestOptions;
      options.extra['retryCount'] = retryCount + 1;
      try {
        final retryResponse = await dio.fetch(options);
        handler.resolve(retryResponse);
        return;
      } catch (e) {
        handler.next(err);
        return;
      }
    }

    handler.next(err);
  }
}
