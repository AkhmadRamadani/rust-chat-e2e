// src/transport/rest-client.ts

import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';
import { withRetry } from '../utils/retry.js';
import type {
  KeyBundleRequest,
  RegisterDeviceResponse,
  KeyBundleResponse,
  OtpkEntry,
  ReplenishResponse,
  SignedPreKeyUpdate,
  CreateConversationRequest,
  CreateConversationResponse,
  MessageEnvelopeRequest,
  SendMessageResponse,
  GetMessagesResponse,
  CreateGroupRequest,
  CreateGroupResponse,
  SkdmRecipient,
  UploadResponse,
  ApiErrorBody,
} from './api-types.js';

/**
 * Typed fetch-based REST client.
 * Uses a `getToken()` closure to support hot-swap via `updateToken`.
 * All key bytes are base64url-encoded in JSON.
 */
export class RestClient {
  constructor(
    private readonly baseUrl: string,
    private readonly getToken: () => string,
  ) {}

  // --- Device ---

  /**
   * Register a new device and receive a DeviceId.
   */
  async registerDevice(userId: string, bundle: KeyBundleRequest): Promise<RegisterDeviceResponse> {
    return this.post<RegisterDeviceResponse>(`/users/${userId}/devices`, bundle);
  }

  /**
   * Fetch a user's public key bundle for X3DH.
   */
  async getKeyBundle(userId: string): Promise<KeyBundleResponse> {
    return this.get<KeyBundleResponse>(`/users/${userId}/key-bundle`);
  }

  /**
   * Upload new one-time prekeys for replenishment.
   */
  async replenishOtpks(
    userId: string,
    deviceId: string,
    keys: OtpkEntry[],
  ): Promise<ReplenishResponse> {
    return this.put<ReplenishResponse>(`/users/${userId}/devices/${deviceId}/one-time-prekeys`, { keys });
  }

  /**
   * Upload a new signed prekey after rotation.
   */
  async rotateSignedPreKey(
    userId: string,
    deviceId: string,
    update: SignedPreKeyUpdate,
  ): Promise<void> {
    await this.put<void>(`/users/${userId}/devices/${deviceId}/signed-prekey`, update);
  }

  // --- Conversations ---

  /**
   * Create a new 1:1 conversation with X3DH init envelope.
   */
  async createConversation(
    req: CreateConversationRequest,
  ): Promise<CreateConversationResponse> {
    return this.post<CreateConversationResponse>('/conversations', req);
  }

  /**
   * Send a Double Ratchet message envelope.
   */
  async sendMessage(
    conversationId: string,
    req: MessageEnvelopeRequest,
  ): Promise<SendMessageResponse> {
    return this.post<SendMessageResponse>(`/conversations/${conversationId}/messages`, req);
  }

  /**
   * Fetch message history for a conversation.
   */
  async getMessages(
    conversationId: string,
    params?: { limit?: number; beforeSeq?: number },
  ): Promise<GetMessagesResponse> {
    const qs = new URLSearchParams();
    if (params?.limit !== undefined) qs.set('limit', String(params.limit));
    if (params?.beforeSeq !== undefined) qs.set('before_seq', String(params.beforeSeq));
    const query = qs.toString() ? `?${qs.toString()}` : '';
    return this.get<GetMessagesResponse>(`/conversations/${conversationId}/messages${query}`);
  }

  // --- Groups ---

  /**
   * Create a new group conversation.
   */
  async createGroup(req: CreateGroupRequest): Promise<CreateGroupResponse> {
    return this.post<CreateGroupResponse>('/groups', req);
  }

  /**
   * Add a member to a group.
   */
  async addGroupMember(
    conversationId: string,
    userId: string,
    deviceId: string,
  ): Promise<void> {
    await this.post<void>(`/groups/${conversationId}/members`, { user_id: userId, device_id: deviceId });
  }

  /**
   * Remove a member from a group.
   */
  async removeGroupMember(conversationId: string, userId: string): Promise<void> {
    await this.delete(`/groups/${conversationId}/members/${userId}`);
  }

  /**
   * Distribute encrypted Sender Key Distribution Messages.
   */
  async distributeGroupSenderKey(
    conversationId: string,
    recipients: SkdmRecipient[],
  ): Promise<void> {
    await this.post<void>(`/groups/${conversationId}/sender-keys`, { recipients });
  }

  // --- Attachments ---

  /**
   * Upload an encrypted attachment.
   */
  async uploadAttachment(
    data: FormData,
    onProgress?: (sent: number, total: number) => void,
  ): Promise<UploadResponse> {
    // Use XHR for progress tracking in browser; fall back to fetch otherwise
    if (typeof XMLHttpRequest !== 'undefined' && onProgress) {
      return this.xhrUpload(data, onProgress);
    }
    return this.post<UploadResponse>('/attachments', data, { raw: true });
  }

  // --- HTTP helpers ---

  private async get<T>(path: string): Promise<T> {
    return this.request<T>('GET', path);
  }

  private async post<T>(path: string, body?: unknown, opts?: { raw?: boolean }): Promise<T> {
    return this.request<T>('POST', path, body, opts);
  }

  private async put<T>(path: string, body?: unknown): Promise<T> {
    return this.request<T>('PUT', path, body);
  }

  private async delete(path: string): Promise<void> {
    await this.request<void>('DELETE', path);
  }

  private async request<T>(
    method: string,
    path: string,
    body?: unknown,
    opts?: { raw?: boolean },
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;

    const execute = async (): Promise<T> => {
      const headers: Record<string, string> = {
        Authorization: `Bearer ${this.getToken()}`,
      };

      let bodyInit: BodyInit | undefined;
      if (body !== undefined) {
        if (opts?.raw && body instanceof FormData) {
          bodyInit = body;
        } else {
          headers['Content-Type'] = 'application/json';
          bodyInit = JSON.stringify(body);
        }
      }

      let response: Response;
      try {
        response = await fetch(url, { method, headers, ...(bodyInit !== undefined ? { body: bodyInit } : {}) });
      } catch (err) {
        throw SdkError.network(`Network request failed: ${method} ${path}`, err);
      }

      if (!response.ok) {
        let errorBody: ApiErrorBody = {};
        try {
          errorBody = (await response.json()) as ApiErrorBody;
        } catch {
          // ignore parse failure
        }
        throw SdkError.fromApiResponse(response.status, errorBody);
      }

      if (response.status === 204 || response.headers.get('content-length') === '0') {
        return undefined as T;
      }

      try {
        return (await response.json()) as T;
      } catch (err) {
        throw new SdkError(SdkErrorCode.NETWORK_ERROR, 'Failed to parse response JSON', { cause: err });
      }
    };

    // Retry on 503 and network errors, not on 4xx
    return withRetry(execute, { attempts: 3, backoffMs: 500 });
  }

  private xhrUpload(data: FormData, onProgress: (sent: number, total: number) => void): Promise<UploadResponse> {
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      xhr.open('POST', `${this.baseUrl}/attachments`);
      xhr.setRequestHeader('Authorization', `Bearer ${this.getToken()}`);

      xhr.upload.onprogress = (e) => {
        if (e.lengthComputable) onProgress(e.loaded, e.total);
      };

      xhr.onload = () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          try {
            resolve(JSON.parse(xhr.responseText) as UploadResponse);
          } catch (err) {
            reject(new SdkError(SdkErrorCode.NETWORK_ERROR, 'Failed to parse upload response', { cause: err }));
          }
        } else {
          let body: ApiErrorBody = {};
          try { body = JSON.parse(xhr.responseText) as ApiErrorBody; } catch { /* ignore */ }
          reject(SdkError.fromApiResponse(xhr.status, body));
        }
      };

      xhr.onerror = () => reject(SdkError.network('XHR upload failed'));
      xhr.send(data);
    });
  }
}
