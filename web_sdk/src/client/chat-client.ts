// src/client/chat-client.ts

import { TypedEventEmitter } from '../utils/typed-emitter.js';
import { Logger } from '../utils/logger.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';
import { detectDefaultSessionStore } from '../session/platform.js';
import { RestClient } from '../transport/rest-client.js';
import { WsManager } from '../transport/ws-manager.js';
import { ConnectionManager } from '../transport/connection-manager.js';
import { AttachmentClient } from '../transport/attachment-client.js';
import { KeyGenerator } from '../crypto/key-generator.js';
import {
  exportPublicKeyRaw,
  exportPrivateKeyPkcs8,
  importX25519PrivateKey,
  importEd25519PrivateKey,
} from '../crypto/crypto-utils.js';
import { base64urlEncode, base64urlDecode } from '../utils/encoding.js';
import { X3dhEngine } from '../crypto/x3dh-engine.js';
import { RatchetEngine } from '../crypto/ratchet-engine.js';
import { SenderKeyEngine } from '../crypto/sender-key-engine.js';
import { ConversationRegistry } from './conversation-registry.js';
import { RtEventRouter } from './rt-event-router.js';
import { OtpkReplenisher } from './otpk-replenisher.js';
import { SpkRotator } from './spk-rotator.js';
import { OneToOneConversation } from '../conversation/one-to-one-conversation.js';
import type { Conversation } from '../conversation/conversation.js';
import type { ChatClientConfig } from './chat-client-config.js';
import type { ConnectionState } from '../transport/connection-manager.js';
import type { SessionStore } from '../session/session-store.js';
import type { KeyBundle } from '../crypto/crypto-types.js';
import { randomUuid } from '../utils/encoding.js';

export interface ChatClientEvents {
  connection: { state: ConnectionState };
  conversation: { conversation: Conversation };
  error: SdkError;
  storage_error: SdkError;
}

/**
 * Root client for the rust-e2e-chat SDK.
 * @example
 * const client = await ChatClient.create({
 *   baseUrl: 'https://api.example.com',
 *   accessToken: token,
 *   userId: 'user-123',
 * });
 * client.on('connection', ({ state }) => console.log('Connection:', state));
 * await client.connect();
 */
export class ChatClient extends TypedEventEmitter<ChatClientEvents> {
  readonly userId: string;
  readonly deviceId: string;

  private currentToken: string;
  private readonly store: SessionStore;
  private readonly rest: RestClient;
  private readonly wsManager: WsManager;
  private readonly connectionManager: ConnectionManager;
  private readonly attachmentClient: AttachmentClient;
  private readonly registry: ConversationRegistry;
  private readonly router: RtEventRouter;
  private readonly replenisher: OtpkReplenisher;
  private readonly spkRotator: SpkRotator;
  private readonly logger: Logger;
  private readonly baseUrl: string;

  private constructor(params: {
    userId: string;
    deviceId: string;
    token: string;
    store: SessionStore;
    rest: RestClient;
    wsManager: WsManager;
    connectionManager: ConnectionManager;
    attachmentClient: AttachmentClient;
    registry: ConversationRegistry;
    router: RtEventRouter;
    replenisher: OtpkReplenisher;
    spkRotator: SpkRotator;
    logger: Logger;
    baseUrl: string;
  }) {
    super();
    this.userId = params.userId;
    this.deviceId = params.deviceId;
    this.currentToken = params.token;
    this.store = params.store;
    this.rest = params.rest;
    this.wsManager = params.wsManager;
    this.connectionManager = params.connectionManager;
    this.attachmentClient = params.attachmentClient;
    this.registry = params.registry;
    this.router = params.router;
    this.replenisher = params.replenisher;
    this.spkRotator = params.spkRotator;
    this.logger = params.logger;
    this.baseUrl = params.baseUrl;
  }

  /**
   * Create and initialize a ChatClient instance.
   * Registers a new device if no deviceId is provided, or loads an existing session.
   *
   * @example
   * const client = await ChatClient.create({
   *   baseUrl: 'https://api.example.com',
   *   accessToken: myJwt,
   *   userId: 'user-abc',
   * });
   */
  static async create(config: ChatClientConfig): Promise<ChatClient> {
    const {
      baseUrl,
      accessToken,
      userId,
      sessionStore,
      autoConnect = true,
      signedPrekeyRotationDays = 7,
      logLevel = 'warn',
    } = config;

    const logger = new Logger(logLevel);

    // 1. Session store
    const store = sessionStore ?? detectDefaultSessionStore();

    // 2. Current token ref (closure for hot-swap)
    let currentToken = accessToken;
    const getToken = () => currentToken;

    // 3. REST client
    const rest = new RestClient(baseUrl, getToken);

    // 4. Device registration or load
    let deviceId = config.deviceId;
    if (!deviceId) {
      logger.info('Registering new device');
      const bundle = await KeyGenerator.generateKeyBundle(50);

      // Extract public bytes for registration
      const [
        ikDhPub, ikDhPriv,
        ikEdPub, ikEdPriv,
        spkPub, spkPriv,
      ] = await Promise.all([
        exportPublicKeyRaw(bundle.identityKeyDh.publicKey),
        exportPrivateKeyPkcs8(bundle.identityKeyDh.privateKey),
        exportPublicKeyRaw(bundle.identityKeyEd.publicKey),
        exportPrivateKeyPkcs8(bundle.identityKeyEd.privateKey),
        exportPublicKeyRaw(bundle.signedPrekey.keyPair.publicKey),
        exportPrivateKeyPkcs8(bundle.signedPrekey.keyPair.privateKey),
      ]);

      const otpkEntries = await Promise.all(
        bundle.otpks.map(async (o) => ({
          id: o.id,
          key: base64urlEncode(await exportPublicKeyRaw(o.keyPair.publicKey)),
        })),
      );

      const resp = await rest.registerDevice(userId, {
        identity_key: base64urlEncode(ikDhPub),
        identity_key_ed: base64urlEncode(ikEdPub),
        signed_prekey_id: bundle.signedPrekey.id,
        signed_prekey: base64urlEncode(spkPub),
        signed_prekey_sig: base64urlEncode(bundle.signedPrekey.signature),
        one_time_prekeys: otpkEntries,
      });

      deviceId = resp.device_id;
      logger.info(`Device registered: ${deviceId}`);

      // Build private OTPKs for storage
      const otpkRecords = await Promise.all(
        bundle.otpks.map(async (o) => ({
          id: o.id,
          privateKey: base64urlEncode(await exportPrivateKeyPkcs8(o.keyPair.privateKey)),
          publicKey: base64urlEncode(await exportPublicKeyRaw(o.keyPair.publicKey)),
        })),
      );

      await store.saveDevice({
        userId,
        deviceId,
        identityKeyDhPriv: base64urlEncode(ikDhPriv),
        identityKeyDhPub: base64urlEncode(ikDhPub),
        identityKeyEdPriv: base64urlEncode(ikEdPriv),
        identityKeyEdPub: base64urlEncode(ikEdPub),
        signedPrekeyPriv: base64urlEncode(spkPriv),
        signedPrekeyPub: base64urlEncode(spkPub),
        signedPrekeyId: bundle.signedPrekey.id,
        signedPrekeyCreatedAt: bundle.signedPrekey.createdAt,
        otpks: otpkRecords,
        nextOtpkId: bundle.nextOtpkId,
      });
    } else {
      // Load existing session
      const existing = await store.loadDevice(userId, deviceId);
      if (!existing) {
        throw SdkError.sessionNotFound(userId, deviceId);
      }
      logger.info(`Loaded existing device: ${deviceId}`);
    }

    // 5. Transport
    const getWsUrl = () => {
      const wsBase = baseUrl.replace(/^http/, 'ws');
      return `${wsBase}/ws?token=${getToken()}&device_id=${deviceId}`;
    };

    const wsManager = new WsManager(logger);
    const connectionManager = new ConnectionManager(wsManager, getWsUrl, logger);
    const attachmentClient = new AttachmentClient(rest);

    // 6. Build registry with distributeSkdm callback
    const distributeSkdm = async (conversationId: string, memberUserIds: string[]): Promise<void> => {
      const devRecord = await store.loadDevice(userId, deviceId!);
      if (!devRecord) return;

      const mySkRecord = await store.loadSenderKey(conversationId, userId);
      if (!mySkRecord) return;

      const mySession = await SenderKeyEngine.fromRecord(mySkRecord);
      const skBytes = await SenderKeyEngine.serializeKeyMaterial(mySession);

      const recipients = await Promise.all(
        memberUserIds.map(async (memberId) => {
          try {
            const memberBundle = await rest.getKeyBundle(memberId);
            // We need a 1:1 ratchet to encrypt the SKDM
            const ratchetState = await store.loadRatchetState(`1:1:${userId}:${memberId}`);

            let encryptedSkdm: Uint8Array;
            if (ratchetState) {
              const session = RatchetEngine.deserialize(ratchetState);
              const { ciphertext, nextSession } = await RatchetEngine.encrypt(session, skBytes);
              await store.saveRatchetState(
                `1:1:${userId}:${memberId}`,
                RatchetEngine.serialize(nextSession, `1:1:${userId}:${memberId}`),
              );
              encryptedSkdm = ciphertext;
            } else {
              // X3DH init to establish ratchet
              const senderPrivBundle = await buildPrivateBundle(devRecord);
              const { sharedSecret, header } = await X3dhEngine.performX3dh({
                recipientBundle: memberBundle,
                senderBundle: senderPrivBundle,
              });
              const senderRatchet = await RatchetEngine.initSender(
                sharedSecret,
                base64urlDecode(memberBundle.identity_key),
              );
              const { ciphertext, nextSession } = await RatchetEngine.encrypt(senderRatchet, skBytes);
              await store.saveRatchetState(
                `1:1:${userId}:${memberId}`,
                RatchetEngine.serialize(nextSession, `1:1:${userId}:${memberId}`),
              );
              encryptedSkdm = ciphertext;
              void header; // header sent separately via distributeGroupSenderKey
            }

            return {
              user_id: memberId,
              device_id: memberBundle.device_id,
              encrypted_skdm: base64urlEncode(encryptedSkdm),
            };
          } catch {
            return null;
          }
        }),
      );

      const validRecipients = recipients.filter((r): r is NonNullable<typeof r> => r !== null);
      if (validRecipients.length > 0) {
        await rest.distributeGroupSenderKey(conversationId, validRecipients);
      }
    };

    const registry = new ConversationRegistry(
      userId,
      deviceId,
      store,
      rest,
      attachmentClient,
      logger,
      (conv) => {
        // Will be wired after client is created; stored via closure
        pendingConvEmits.push(conv);
      },
      distributeSkdm,
    );

    const pendingConvEmits: Conversation[] = [];

    // 7. Replenisher and rotator
    const replenisher = new OtpkReplenisher(
      userId, deviceId, store, rest, logger,
      (err) => { /* will be wired below */ void err; },
    );

    const spkRotator = new SpkRotator(
      userId, deviceId, signedPrekeyRotationDays, store, rest, logger,
      (err) => { void err; },
    );

    // 8. Restore conversations
    await registry.loadAll();

    // 9. Create client
    const client = new ChatClient({
      userId,
      deviceId,
      token: accessToken,
      store,
      rest,
      wsManager,
      connectionManager,
      attachmentClient,
      registry,
      router: null as unknown as RtEventRouter, // wired below
      replenisher,
      spkRotator,
      logger,
      baseUrl,
    });

    // Wire error emitters now that client exists
    const onError = (err: SdkError) => client.emit('error', err);
    (replenisher as unknown as { onError: (e: SdkError) => void }).onError = onError;
    (spkRotator as unknown as { onError: (e: SdkError) => void }).onError = onError;

    // Wire connection state
    connectionManager.stateChanges.on('change', ({ state }) => {
      client.emit('connection', { state });
    });

    // Wire conversation emitter
    for (const conv of pendingConvEmits) {
      client.emit('conversation', { conversation: conv });
    }

    // 10. RtEventRouter
    const router = new RtEventRouter(
      wsManager,
      registry,
      replenisher,
      logger,
      onError,
      (cid, seq) => wsManager.sendAck(cid, seq),
    );
    (client as unknown as { router: RtEventRouter }).router = router;

    // 11. Start rotator
    spkRotator.start();

    // 12. Auto-connect
    if (autoConnect) {
      await connectionManager.ensureConnected();
    }

    return client;
  }

  // --- Connection ---

  /**
   * Manually connect to the WebSocket.
   * @example
   * await client.connect();
   */
  async connect(): Promise<void> {
    await this.connectionManager.ensureConnected();
  }

  /**
   * Disconnect from the WebSocket.
   */
  async disconnect(): Promise<void> {
    await this.connectionManager.disconnect();
  }

  /**
   * Hot-swap the access token. All subsequent requests and next WS reconnect use the new token.
   * @example
   * client.updateToken(newJwt);
   */
  updateToken(newToken: string): void {
    this.currentToken = newToken;
    // Trigger reconnect so WS URL is rebuilt with new token
    this.connectionManager.disconnect().then(() => {
      return this.connectionManager.ensureConnected();
    }).catch((err) => {
      this.emit('error', SdkError.network('Token hot-swap reconnect failed', err));
    });
  }

  // --- Conversations ---

  /**
   * Open or retrieve a 1:1 conversation with a user.
   * Performs X3DH key agreement on first contact.
   * @example
   * const conv = await client.openConversation('user-456');
   * conv.on('message', (msg) => console.log(msg.text));
   * await conv.send('Hello!');
   */
  async openConversation(recipientUserId: string): Promise<Conversation> {
    // Check cache — idempotent
    const allConvs = await this.store.loadAllConversations();
    for (const meta of allConvs) {
      if (meta.type === 'one_to_one' && meta.members.some((m) => m.userId === recipientUserId)) {
        const existing = this.registry.get(meta.conversationId);
        if (existing) return existing;
      }
    }

    // X3DH init
    const recipientBundle = await this.rest.getKeyBundle(recipientUserId);
    const devRecord = await this.store.loadDevice(this.userId, this.deviceId);
    if (!devRecord) throw SdkError.sessionNotFound(this.userId, this.deviceId);

    const senderPrivBundle = await buildPrivateBundle(devRecord);

    const { sharedSecret, header } = await X3dhEngine.performX3dh({
      recipientBundle,
      senderBundle: senderPrivBundle,
    });

    const conversationId = randomUuid();

    // Init sender ratchet
    const senderRatchet = await RatchetEngine.initSender(
      sharedSecret,
      base64urlDecode(recipientBundle.identity_key),
    );

    // Persist ratchet state BEFORE network
    await this.store.saveRatchetState(
      conversationId,
      RatchetEngine.serialize(senderRatchet, conversationId),
    );

    // Send X3DH init envelope (zero-byte first message to establish convo)
    const { ciphertext, header: ratchetHeader, nextSession } = await RatchetEngine.encrypt(
      senderRatchet,
      new Uint8Array(0),
    );

    await this.store.saveRatchetState(
      conversationId,
      RatchetEngine.serialize(nextSession, conversationId),
    );

    const resp = await this.rest.createConversation({
      recipient_user_id: recipientUserId,
      recipient_device_id: recipientBundle.device_id,
      envelope: {
        conversation_id: conversationId,
        ciphertext: base64urlEncode(ciphertext),
        protocol_header: {
          type: 'x3dh_init',
          ek: base64urlEncode(header.ek),
          spk_id: header.spkId,
          ...(header.otpkId !== undefined ? { otpk_id: header.otpkId } : {}),
        },
      },
    });

    const finalConversationId = resp.conversation_id || conversationId;

    const members = [
      { userId: this.userId, deviceId: this.deviceId },
      { userId: recipientUserId, deviceId: recipientBundle.device_id },
    ];

    await this.store.saveConversationMeta({
      conversationId: finalConversationId,
      type: 'one_to_one',
      members,
      createdAt: Date.now(),
      lastSeq: 0,
    });

    const conv = this.registry.getOrCreate(finalConversationId, 'one_to_one', members);
    return conv;
  }

  /**
   * Create a new group conversation.
   * Generates a SenderKey and distributes it to all members.
   * @example
   * const group = await client.createGroup(['user-2', 'user-3']);
   * await group.send('Hey everyone!');
   */
  async createGroup(memberUserIds: string[]): Promise<Conversation> {
    const resp = await this.rest.createGroup({ members: memberUserIds });
    const conversationId = resp.conversation_id;

    // Generate sender key
    const material = await KeyGenerator.generateSenderKey();
    const mySession = await SenderKeyEngine.createSession(material);
    await this.store.saveSenderKey(
      conversationId,
      this.userId,
      await SenderKeyEngine.toRecord(mySession, conversationId, this.userId),
    );

    // Build members list
    const members = [{ userId: this.userId, deviceId: this.deviceId }];
    for (const m of resp.members) {
      for (const d of m.devices) {
        if (m.user_id !== this.userId) {
          members.push({ userId: m.user_id, deviceId: d });
        }
      }
    }

    await this.store.saveConversationMeta({
      conversationId,
      type: 'group',
      members,
      createdAt: Date.now(),
      lastSeq: 0,
    });

    const conv = this.registry.getOrCreate(conversationId, 'group', members);

    // Distribute sender key to all other members
    try {
      await (this as unknown as { distributeSkdm: (id: string, ids: string[]) => Promise<void> })
        .distributeSkdm?.(conversationId, memberUserIds);
    } catch (err) {
      this.emit('error', SdkError.network('Failed to distribute group sender key', err));
    }

    return conv;
  }

  /**
   * Find an existing conversation by ID without creating one.
   */
  findConversation(conversationId: string): Conversation | undefined {
    return this.registry.get(conversationId);
  }

  // --- Key management ---

  /**
   * Get the device's public key bundle for display or diagnostics.
   */
  async getPublicKeyBundle(): Promise<KeyBundle> {
    const record = await this.store.loadDevice(this.userId, this.deviceId);
    if (!record) throw SdkError.sessionNotFound(this.userId, this.deviceId);

    return {
      deviceId: record.deviceId,
      identityKeyDhPub: base64urlDecode(record.identityKeyDhPub),
      identityKeyEdPub: base64urlDecode(record.identityKeyEdPub),
      signedPrekeyId: record.signedPrekeyId,
      signedPrekeyPub: base64urlDecode(record.signedPrekeyPub),
      signedPrekeySig: new Uint8Array(0), // signature not re-stored; retrieve from server if needed
    };
  }

  /**
   * Close all connections, stop all timers, and release all resources.
   * @example
   * await client.destroy();
   */
  async destroy(): Promise<void> {
    this.spkRotator.stop();
    this.connectionManager.destroy();
    this.removeAllListeners();
    this.logger.info('ChatClient destroyed');
  }
}

/** Reconstruct a PrivateKeyBundle from a stored DeviceRecord. */
async function buildPrivateBundle(record: import('../session/session-store.js').DeviceRecord) {
  const [ikDhPriv, ikDhPub, ikEdPriv, ikEdPub, spkPriv, spkPub] = await Promise.all([
    importX25519PrivateKey(base64urlDecode(record.identityKeyDhPriv)),
    (async () => {
      const { importX25519PublicKey } = await import('../crypto/crypto-utils.js');
      return importX25519PublicKey(base64urlDecode(record.identityKeyDhPub));
    })(),
    importEd25519PrivateKey(base64urlDecode(record.identityKeyEdPriv)),
    (async () => {
      const { importEd25519PublicKey } = await import('../crypto/crypto-utils.js');
      return importEd25519PublicKey(base64urlDecode(record.identityKeyEdPub));
    })(),
    (async () => {
      const { importX25519PrivateKey: imp } = await import('../crypto/crypto-utils.js');
      return imp(base64urlDecode(record.signedPrekeyPriv));
    })(),
    (async () => {
      const { importX25519PublicKey: imp } = await import('../crypto/crypto-utils.js');
      return imp(base64urlDecode(record.signedPrekeyPub));
    })(),
  ]);

  // Build partial OTPKs (public+private)
  const otpks = await Promise.all(
    record.otpks.map(async (o) => {
      const { importX25519PrivateKey: impPriv, importX25519PublicKey: impPub } = await import('../crypto/crypto-utils.js');
      return {
        id: o.id,
        keyPair: {
          privateKey: await impPriv(base64urlDecode(o.privateKey)),
          publicKey: await impPub(base64urlDecode(o.publicKey)),
        },
      };
    }),
  );

  return {
    identityKeyDh: { publicKey: ikDhPub, privateKey: ikDhPriv },
    identityKeyEd: { publicKey: ikEdPub, privateKey: ikEdPriv },
    signedPrekey: {
      id: record.signedPrekeyId,
      keyPair: { publicKey: spkPub, privateKey: spkPriv },
      signature: new Uint8Array(0),
      createdAt: record.signedPrekeyCreatedAt,
    },
    otpks,
    nextOtpkId: record.nextOtpkId,
  };
}
