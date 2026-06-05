// src/client/rt-event-router.ts

import type { WsManager } from '../transport/ws-manager.js';
import type { ConversationRegistry } from './conversation-registry.js';
import type { OtpkReplenisher } from './otpk-replenisher.js';
import type { RtEvent } from '../transport/rt-event.js';
import type { OneToOneConversation } from '../conversation/one-to-one-conversation.js';
import type { GroupConversation } from '../conversation/group-conversation.js';
import type { Logger } from '../utils/logger.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';

/**
 * Routes incoming WebSocket frames to the appropriate conversation handler.
 * All async handlers are wrapped in try/catch — errors are emitted, never unhandled.
 */
export class RtEventRouter {
  constructor(
    private readonly wsManager: WsManager,
    private readonly registry: ConversationRegistry,
    private readonly replenisher: OtpkReplenisher,
    private readonly logger: Logger,
    private readonly onError: (err: SdkError) => void,
    private readonly onAck: (conversationId: string, seq: number) => void,
  ) {
    wsManager.events.on('frame', (event: RtEvent) => {
      this.route(event).catch((err) => {
        this.logger.error('RtEventRouter unhandled error', err);
        this.onError(new SdkError(SdkErrorCode.UNKNOWN_ERROR, 'RtEvent routing error', { cause: err }));
      });
    });
  }

  private async route(event: RtEvent): Promise<void> {
    try {
      switch (event.type) {
        case 'message': {
          // Get or create conversation — default to one_to_one; server events carry type elsewhere
          const conv = this.registry.get(event.conversationId);
          if (!conv) {
            this.logger.warn(`Received message for unknown conversation ${event.conversationId}`);
            return;
          }
          if (conv.type === 'one_to_one') {
            await (conv as OneToOneConversation).onIncomingMessage(event);
          } else {
            await (conv as GroupConversation).onIncomingMessage(event);
          }
          this.onAck(event.conversationId, event.seq);
          break;
        }

        case 'low_otpk':
          await this.replenisher.handleLowOtpk(event.count);
          break;

        case 'member_added':
        case 'member_removed': {
          const conv = this.registry.get(event.conversationId);
          if (conv?.type === 'group') {
            await (conv as GroupConversation).onMemberChange(event);
          }
          break;
        }

        case 'sender_key_distribution': {
          const conv = this.registry.get(event.conversationId);
          if (conv?.type === 'group') {
            await (conv as GroupConversation).onSkdm(event);
          }
          break;
        }

        default:
          // Forward-compatible: unknown events silently ignored
          break;
      }
    } catch (err) {
      this.logger.error('Error routing RtEvent', err);
      this.onError(err instanceof SdkError ? err : new SdkError(SdkErrorCode.UNKNOWN_ERROR, 'Event handling error', { cause: err }));
    }
  }
}
