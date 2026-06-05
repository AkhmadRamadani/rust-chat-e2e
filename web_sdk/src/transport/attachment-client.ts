// src/transport/attachment-client.ts

import { RestClient } from './rest-client.js';
import { aesGcmEncrypt, aesGcmDecrypt } from '../crypto/crypto-utils.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';

const MAX_ATTACHMENT_BYTES = 100 * 1024 * 1024; // 100MB

export interface AttachmentProgress {
  sent: number;
  total: number;
}

/**
 * Handles encrypted attachment upload/download.
 */
export class AttachmentClient {
  constructor(private readonly rest: RestClient) {}

  /**
   * Encrypt and upload an attachment, streaming upload progress.
   */
  async upload(
    data: File | Blob | BufferSource,
    filename: string,
    contentType: string,
    onProgress?: (p: AttachmentProgress) => void,
  ): Promise<{ attachmentId: string; encryptionKey: Uint8Array }> {
    // Read raw bytes
    let rawBytes: Uint8Array;
    if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) {
      rawBytes = new Uint8Array(data instanceof ArrayBuffer ? data : (data as ArrayBufferView).buffer);
    } else {
      const buf = await (data as Blob).arrayBuffer();
      rawBytes = new Uint8Array(buf);
    }

    if (rawBytes.byteLength > MAX_ATTACHMENT_BYTES) {
      throw new SdkError(SdkErrorCode.FILE_TOO_LARGE, `Attachment exceeds ${MAX_ATTACHMENT_BYTES} bytes`);
    }

    // Generate encryption key and encrypt
    const encryptionKey = new Uint8Array(32);
    crypto.getRandomValues(encryptionKey);
    const encryptedBytes = await aesGcmEncrypt(encryptionKey, rawBytes);

    // Build multipart form
    const form = new FormData();
    const encryptedBuf = encryptedBytes.buffer.slice(encryptedBytes.byteOffset, encryptedBytes.byteOffset + encryptedBytes.byteLength) as ArrayBuffer;
    form.append('file', new Blob([encryptedBuf], { type: 'application/octet-stream' }), filename);
    form.append('content_type', contentType);

    const result = await this.rest.uploadAttachment(form, onProgress
      ? (sent, total) => onProgress({ sent, total })
      : undefined,
    );

    return { attachmentId: result.attachment_id, encryptionKey };
  }

  /**
   * Download and decrypt an attachment.
   */
  async download(url: string, encryptionKey: Uint8Array): Promise<Uint8Array> {
    let response: Response;
    try {
      response = await fetch(url);
    } catch (err) {
      throw SdkError.network('Failed to download attachment', err);
    }
    if (!response.ok) {
      throw SdkError.network(`Attachment download failed: ${response.status}`);
    }
    const encryptedBytes = new Uint8Array(await response.arrayBuffer());
    return aesGcmDecrypt(encryptionKey, encryptedBytes);
  }
}
