// src/crypto/crypto-utils.ts

import { concatBytes } from '../utils/encoding.js';

// HKDF info strings as per spec
export const HKDF_INFO = {
  X3DH: new TextEncoder().encode('X3DH'),
  RATCHET_ROOT: new TextEncoder().encode('RatchetRoot'),
  RATCHET_CHAIN: new TextEncoder().encode('RatchetChain'),
  SENDER_KEY: new TextEncoder().encode('SenderKey'),
  MESSAGE_KEY: new TextEncoder().encode('MessageKey'),
} as const;

const ZERO_SALT = new Uint8Array(32); // 32 zero bytes

/** Helper to ensure a Uint8Array has a plain ArrayBuffer backing (not SharedArrayBuffer). */
function toArrayBuffer(u: Uint8Array): ArrayBuffer {
  return u.buffer.slice(u.byteOffset, u.byteOffset + u.byteLength) as ArrayBuffer;
}

/**
 * HKDF-SHA-256 extract+expand.
 */
export async function hkdf(
  inputKeyMaterial: Uint8Array,
  info: Uint8Array,
  outputLengthBytes: number,
  salt: Uint8Array = ZERO_SALT,
): Promise<Uint8Array> {
  const baseKey = await crypto.subtle.importKey(
    'raw',
    toArrayBuffer(inputKeyMaterial),
    { name: 'HKDF' },
    false,
    ['deriveBits'],
  );

  const bits = await crypto.subtle.deriveBits(
    {
      name: 'HKDF',
      hash: 'SHA-256',
      salt: toArrayBuffer(salt),
      info: toArrayBuffer(info),
    },
    baseKey,
    outputLengthBytes * 8,
  );

  return new Uint8Array(bits);
}

/**
 * AES-GCM 256-bit encrypt. Prepends 12-byte random IV to ciphertext.
 */
export async function aesGcmEncrypt(
  key: Uint8Array,
  plaintext: Uint8Array,
): Promise<Uint8Array> {
  const iv = new Uint8Array(12);
  crypto.getRandomValues(iv);

  const cryptoKey = await crypto.subtle.importKey('raw', toArrayBuffer(key), { name: 'AES-GCM' }, false, ['encrypt']);

  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    cryptoKey,
    toArrayBuffer(plaintext),
  );

  return concatBytes(iv, new Uint8Array(ciphertext));
}

/**
 * AES-GCM 256-bit decrypt. Expects 12-byte IV prepended to ciphertext.
 */
export async function aesGcmDecrypt(
  key: Uint8Array,
  ivAndCiphertext: Uint8Array,
): Promise<Uint8Array> {
  const iv = ivAndCiphertext.slice(0, 12);
  const ciphertext = ivAndCiphertext.slice(12);

  const cryptoKey = await crypto.subtle.importKey('raw', toArrayBuffer(key), { name: 'AES-GCM' }, false, ['decrypt']);

  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv },
    cryptoKey,
    toArrayBuffer(ciphertext),
  );

  return new Uint8Array(plaintext);
}

/**
 * ECDH using X25519. Returns raw shared secret bytes.
 */
export async function x25519Dh(
  privateKey: CryptoKey,
  publicKey: CryptoKey,
): Promise<Uint8Array> {
  const bits = await crypto.subtle.deriveBits(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'X25519', public: publicKey } as any,
    privateKey,
    256,
  );
  return new Uint8Array(bits);
}

/**
 * Generate a new X25519 key pair (extractable).
 */
export async function generateX25519KeyPair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'X25519' } as any,
    true,
    ['deriveBits'],
  );
}

/**
 * Generate a new Ed25519 key pair (extractable).
 */
export async function generateEd25519KeyPair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'Ed25519' } as any,
    true,
    ['sign', 'verify'],
  );
}

/**
 * Export an X25519 or Ed25519 public key to raw bytes.
 */
export async function exportPublicKeyRaw(key: CryptoKey): Promise<Uint8Array> {
  const raw = await crypto.subtle.exportKey('raw', key);
  return new Uint8Array(raw);
}

/**
 * Export a private key to PKCS8 bytes.
 */
export async function exportPrivateKeyPkcs8(key: CryptoKey): Promise<Uint8Array> {
  const pkcs8 = await crypto.subtle.exportKey('pkcs8', key);
  return new Uint8Array(pkcs8);
}

/**
 * Import an X25519 public key from raw bytes.
 */
export async function importX25519PublicKey(raw: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'raw',
    toArrayBuffer(raw),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'X25519' } as any,
    true,
    [],
  );
}

/**
 * Import an X25519 private key from PKCS8 bytes.
 */
export async function importX25519PrivateKey(pkcs8: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'pkcs8',
    toArrayBuffer(pkcs8),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'X25519' } as any,
    true,
    ['deriveBits'],
  );
}

/**
 * Import an Ed25519 public key from raw bytes.
 */
export async function importEd25519PublicKey(raw: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'raw',
    toArrayBuffer(raw),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'Ed25519' } as any,
    true,
    ['verify'],
  );
}

/**
 * Import an Ed25519 private key from PKCS8 bytes.
 */
export async function importEd25519PrivateKey(pkcs8: Uint8Array): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'pkcs8',
    toArrayBuffer(pkcs8),
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    { name: 'Ed25519' } as any,
    true,
    ['sign'],
  );
}

/**
 * Ed25519 sign.
 */
export async function ed25519Sign(privateKey: CryptoKey, data: Uint8Array): Promise<Uint8Array> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const sig = await crypto.subtle.sign({ name: 'Ed25519' } as any, privateKey, toArrayBuffer(data));
  return new Uint8Array(sig);
}

/**
 * Ed25519 verify.
 */
export async function ed25519Verify(
  publicKey: CryptoKey,
  signature: Uint8Array,
  data: Uint8Array,
): Promise<boolean> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  return crypto.subtle.verify({ name: 'Ed25519' } as any, publicKey, toArrayBuffer(signature), toArrayBuffer(data));
}

/**
 * Verify a SignedPreKey signature (SPK pub signed with Ed25519 identity key).
 */
export async function verifySPKSignature(
  spkPub: Uint8Array,
  sig: Uint8Array,
  identityEdPub: CryptoKey,
): Promise<boolean> {
  return ed25519Verify(identityEdPub, sig, spkPub);
}
