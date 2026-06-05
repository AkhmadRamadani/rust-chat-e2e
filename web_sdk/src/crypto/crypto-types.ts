// src/crypto/crypto-types.ts

export interface Curve25519KeyPair {
  publicKey: CryptoKey;
  privateKey: CryptoKey;
}

export interface Ed25519KeyPair {
  publicKey: CryptoKey;
  privateKey: CryptoKey;
}

export interface PrivateKeyBundle {
  identityKeyDh: Curve25519KeyPair;   // X25519 for DH
  identityKeyEd: Ed25519KeyPair;       // Ed25519 for signing
  signedPrekey: SignedPreKeyPair;
  otpks: OtpkKeyPair[];
  nextOtpkId: number;
}

export interface SignedPreKeyPair {
  id: number;
  keyPair: Curve25519KeyPair;
  signature: Uint8Array;  // Ed25519 sig over the public key bytes
  createdAt: number;
}

export interface OtpkKeyPair {
  id: number;
  keyPair: Curve25519KeyPair;
}

export interface SenderKeyMaterial {
  chainKey: Uint8Array;       // 32 bytes
  chainId: number;
  signingKey: Ed25519KeyPair;
}

export interface SenderKeySession {
  chainKey: Uint8Array;
  chainId: number;
  iteration: number;
  signingKey: Ed25519KeyPair;
}

export interface X3dhResult {
  sharedSecret: Uint8Array;
  header: X3dhHeader;
}

export interface X3dhHeader {
  type: 'x3dh_init';
  ek: Uint8Array;       // ephemeral public key
  spkId: number;
  otpkId?: number;
}

export interface RatchetHeader {
  type: 'double_ratchet';
  dh: Uint8Array;   // sender's current DH ratchet public key
  n: number;        // message number in sending chain
  pn: number;       // message count in previous sending chain
}

export interface RatchetSession {
  rootKey: Uint8Array;
  chainKeySend: Uint8Array;
  chainKeyRecv: Uint8Array;
  dhSendPub: Uint8Array;
  dhSendPriv: Uint8Array;
  dhRecvPub: Uint8Array;
  nSend: number;
  nRecv: number;
  pn: number;
  skippedMessageKeys: Map<string, Uint8Array>;
}

export interface EncryptResult {
  ciphertext: Uint8Array;
  header: RatchetHeader;
  nextSession: RatchetSession;
}

export interface DecryptResult {
  plaintext: Uint8Array;
  nextSession: RatchetSession;
}

export interface KeyBundle {
  deviceId: string;
  identityKeyDhPub: Uint8Array;
  identityKeyEdPub: Uint8Array;
  signedPrekeyId: number;
  signedPrekeyPub: Uint8Array;
  signedPrekeySig: Uint8Array;
}
