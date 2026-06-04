export function dummyKey32(): number[] {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return Array.from(b);
}

export function dummySig64(): number[] {
  const b = new Uint8Array(64);
  crypto.getRandomValues(b);
  return Array.from(b);
}

export function encodeMsg(t: string): number[] {
  return Array.from(new TextEncoder().encode(t));
}

export function decodeMsg(arr: number[]): string {
  try {
    return new TextDecoder().decode(new Uint8Array(arr));
  } catch {
    return '[unreadable]';
  }
}

export interface KeyBundle {
  identity_key: number[];
  signed_prekey_id: number;
  signed_prekey: number[];
  signed_prekey_sig: number[];
  one_time_prekeys: { key_id: number; public_key: number[] }[];
}

export async function buildBundle(otpkCount = 10): Promise<KeyBundle> {
  const identityKeyPair = await crypto.subtle.generateKey(
    { name: 'Ed25519' },
    true,
    ['sign', 'verify']
  );

  const identityKeyRaw = await crypto.subtle.exportKey('raw', identityKeyPair.publicKey);
  const identityKeyBytes = Array.from(new Uint8Array(identityKeyRaw));

  const signedPrekeyBytes = dummyKey32();

  const sigRaw = await crypto.subtle.sign(
    { name: 'Ed25519' },
    identityKeyPair.privateKey,
    new Uint8Array(signedPrekeyBytes)
  );
  const signedPrekeySig = Array.from(new Uint8Array(sigRaw));

  return {
    identity_key: identityKeyBytes,
    signed_prekey_id: 1,
    signed_prekey: signedPrekeyBytes,
    signed_prekey_sig: signedPrekeySig,
    one_time_prekeys: Array.from({ length: otpkCount }, (_, i) => ({
      key_id: i + 1,
      public_key: dummyKey32(),
    })),
  };
}
