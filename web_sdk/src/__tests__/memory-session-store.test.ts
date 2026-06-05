// src/__tests__/memory-session-store.test.ts

import { describe, it, expect, beforeEach } from 'vitest';
import { MemorySessionStore } from '../session/memory-session-store.js';
import type { DeviceRecord, RatchetState, SenderKeyRecord, ConversationMeta } from '../session/session-store.js';

const mockDevice: DeviceRecord = {
  userId: 'alice',
  deviceId: 'device-1',
  identityKeyDhPriv: 'privDh',
  identityKeyDhPub: 'pubDh',
  identityKeyEdPriv: 'privEd',
  identityKeyEdPub: 'pubEd',
  signedPrekeyPriv: 'spkPriv',
  signedPrekeyPub: 'spkPub',
  signedPrekeyId: 1,
  signedPrekeyCreatedAt: 1000000,
  otpks: [{ id: 1, privateKey: 'otpkPriv', publicKey: 'otpkPub' }],
  nextOtpkId: 2,
};

const mockRatchet: RatchetState = {
  conversationId: 'conv-1',
  rootKey: 'rootKey',
  chainKeySend: 'chainSend',
  chainKeyRecv: 'chainRecv',
  dhSendPub: 'dhSendPub',
  dhSendPriv: 'dhSendPriv',
  dhRecvPub: 'dhRecvPub',
  nSend: 0,
  nRecv: 0,
  pn: 0,
  skippedMessageKeys: {},
};

const mockSenderKey: SenderKeyRecord = {
  conversationId: 'group-1',
  userId: 'alice',
  chainKey: 'chainKey',
  chainId: 42,
  iteration: 0,
  signingKeyPub: 'sigPub',
};

const mockConvMeta: ConversationMeta = {
  conversationId: 'conv-1',
  type: 'one_to_one',
  members: [{ userId: 'alice', deviceId: 'device-1' }],
  createdAt: 1000000,
  lastSeq: 0,
};

describe('MemorySessionStore', () => {
  let store: MemorySessionStore;

  beforeEach(() => {
    store = new MemorySessionStore();
  });

  it('saves and loads a DeviceRecord', async () => {
    await store.saveDevice(mockDevice);
    const loaded = await store.loadDevice('alice', 'device-1');
    expect(loaded).toEqual(mockDevice);
  });

  it('returns null for missing device', async () => {
    expect(await store.loadDevice('nobody', 'x')).toBeNull();
  });

  it('saves and loads RatchetState', async () => {
    await store.saveRatchetState('conv-1', mockRatchet);
    expect(await store.loadRatchetState('conv-1')).toEqual(mockRatchet);
    expect(await store.loadRatchetState('missing')).toBeNull();
  });

  it('saves and loads SenderKeyRecord', async () => {
    await store.saveSenderKey('group-1', 'alice', mockSenderKey);
    expect(await store.loadSenderKey('group-1', 'alice')).toEqual(mockSenderKey);
    expect(await store.loadSenderKey('group-1', 'bob')).toBeNull();
  });

  it('saves and loads ConversationMeta', async () => {
    await store.saveConversationMeta(mockConvMeta);
    const all = await store.loadAllConversations();
    expect(all).toHaveLength(1);
    expect(all[0]).toEqual(mockConvMeta);
  });

  it('clear() removes all records', async () => {
    await store.saveDevice(mockDevice);
    await store.saveRatchetState('conv-1', mockRatchet);
    await store.saveConversationMeta(mockConvMeta);
    await store.clear();
    expect(await store.loadDevice('alice', 'device-1')).toBeNull();
    expect(await store.loadAllConversations()).toEqual([]);
  });
});
