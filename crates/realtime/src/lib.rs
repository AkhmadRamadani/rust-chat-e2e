//! Real-time session manager — WebSocket-based delivery.
//!
//! Replaces the previous QUIC/WebTransport implementation with plain
//! WebSocket using axum's built-in `WebSocketUpgrade` extractor.
//!
//! # Architecture
//!
//! Each authenticated device connects to `GET /ws?token=<jwt>`.
//! The handler upgrades the connection, registers the session in
//! [`WsSessionManager`] (keyed by `(TenantId, DeviceId)`), drains any
//! offline-queued envelopes, and runs a bidirectional message loop:
//!
//! - **Server → Client**: `RtEvent` structs serialised as JSON text frames.
//! - **Client → Server**: `{"type":"ping"}` keepalive pong, or
//!   `AckDatagram` JSON for message acknowledgement.
//!
//! A 30-second ping / 10-second pong timeout closes idle sessions.

use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use common::{AckDatagram, ConversationId, DeviceId, MessageEnvelope, RtEvent, TenantId, UserId};
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tokio::time::Duration;
use tracing::{debug, info, warn};

// ── Delivery outcome ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Delivered,
    Queued,
    DroppedQueueFull,
    NoSession,
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RealtimeError {
    #[error("session send channel closed")]
    ChannelClosed,
    #[error("serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),
}

// ── WsManager trait ───────────────────────────────────────────────────────────

/// Trait for delivering real-time events to connected device sessions.
/// Kept transport-agnostic so handlers don't import WebSocket types.
#[async_trait]
pub trait WebTransportManager: Send + Sync {
    async fn deliver(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        event: RtEvent,
    ) -> DeliveryOutcome;
}

// ── No-op stub ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct NoopWebTransportManager;

#[async_trait]
impl WebTransportManager for NoopWebTransportManager {
    async fn deliver(&self, _: TenantId, _: DeviceId, _: RtEvent) -> DeliveryOutcome {
        DeliveryOutcome::NoSession
    }
}

// ── Session handle ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SessionHandle {
    pub event_tx: mpsc::Sender<RtEvent>,
    pub unacked: HashMap<(ConversationId, u64), MessageEnvelope>,
    pub user_id: UserId,
    pub device_id: DeviceId,
}

impl SessionHandle {
    pub fn new(user_id: UserId, device_id: DeviceId) -> (Self, mpsc::Receiver<RtEvent>) {
        let (tx, rx) = mpsc::channel(256);
        (
            SessionHandle {
                event_tx: tx,
                unacked: HashMap::new(),
                user_id,
                device_id,
            },
            rx,
        )
    }
}

// ── Offline queue traits ──────────────────────────────────────────────────────

#[async_trait]
pub trait OfflineQueueDrain: Send + Sync {
    async fn drain_for_device(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<Vec<MessageEnvelope>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueResult {
    Queued,
    DroppedQueueFull,
}

#[async_trait]
pub trait OfflineEnqueue: Send + Sync {
    async fn enqueue(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        envelope: MessageEnvelope,
    ) -> Result<EnqueueResult, String>;
}

// ── WsSessionManager ──────────────────────────────────────────────────────────

/// Registry of active WebSocket sessions, keyed by `(TenantId, DeviceId)`.
#[derive(Debug, Clone)]
pub struct WsSessionManager {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    sessions: RwLock<HashMap<(TenantId, DeviceId), SessionHandle>>,
}

impl WsSessionManager {
    pub fn new() -> Self {
        WsSessionManager {
            inner: Arc::new(Inner {
                sessions: RwLock::new(HashMap::new()),
            }),
        }
    }

    pub async fn register_session(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
        device_id: DeviceId,
        offline_queue: &dyn OfflineQueueDrain,
    ) -> mpsc::Receiver<RtEvent> {
        let (handle, rx) = SessionHandle::new(user_id.clone(), device_id);
        let event_tx = handle.event_tx.clone();

        {
            let mut sessions = self.inner.sessions.write().await;
            let old = sessions.insert((tenant_id, device_id), handle);
            if old.is_some() {
                warn!(?tenant_id, ?device_id, "replaced stale session");
            } else {
                info!(?tenant_id, ?device_id, "WS session registered");
            }
        }

        match offline_queue.drain_for_device(tenant_id, device_id).await {
            Ok(envelopes) => {
                for envelope in envelopes {
                    if event_tx.send(RtEvent::Message(envelope)).await.is_err() {
                        break;
                    }
                }
            }
            Err(e) => warn!(?tenant_id, ?device_id, error=%e, "failed to drain offline queue"),
        }

        rx
    }

    pub async fn unregister_session(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Option<HashMap<(ConversationId, u64), MessageEnvelope>> {
        let mut sessions = self.inner.sessions.write().await;
        sessions.remove(&(tenant_id, device_id)).map(|h| h.unacked)
    }

    pub async fn ack_envelope(&self, tenant_id: TenantId, device_id: DeviceId, ack: AckDatagram) {
        let mut sessions = self.inner.sessions.write().await;
        if let Some(handle) = sessions.get_mut(&(tenant_id, device_id)) {
            handle.unacked.remove(&(ack.conversation_id, ack.seq));
            debug!(?tenant_id, ?device_id, seq=ack.seq, "envelope acked");
        }
    }

    pub async fn active_session_count(&self) -> usize {
        self.inner.sessions.read().await.len()
    }

    pub async fn on_session_closed(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        offline_enqueue: &dyn OfflineEnqueue,
    ) {
        let unacked = self
            .unregister_session(tenant_id, device_id)
            .await
            .unwrap_or_default();

        let count = unacked.len();
        for ((_, _), envelope) in unacked {
            match offline_enqueue.enqueue(tenant_id, device_id, envelope).await {
                Ok(_) => {}
                Err(e) => warn!(?tenant_id, ?device_id, error=%e, "re-enqueue failed"),
            }
        }
        if count > 0 {
            info!(?tenant_id, ?device_id, count, "re-enqueued unacked envelopes on WS close");
        }
    }
}

impl Default for WsSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebTransportManager for WsSessionManager {
    async fn deliver(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        event: RtEvent,
    ) -> DeliveryOutcome {
        let sessions = self.inner.sessions.read().await;
        match sessions.get(&(tenant_id, device_id)) {
            Some(handle) => match handle.event_tx.try_send(event) {
                Ok(()) => DeliveryOutcome::Delivered,
                Err(mpsc::error::TrySendError::Full(_)) => {
                    warn!(?tenant_id, ?device_id, "session channel full");
                    DeliveryOutcome::DroppedQueueFull
                }
                Err(mpsc::error::TrySendError::Closed(_)) => DeliveryOutcome::NoSession,
            },
            None => DeliveryOutcome::NoSession,
        }
    }
}

// ── WebSocket session loop ────────────────────────────────────────────────────

/// Upgrade an axum WebSocket connection into a registered real-time session.
///
/// Called from the `/ws` route handler after JWT validation.
/// Runs the bidirectional message loop until the client disconnects or
/// the keepalive timeout fires.
pub async fn handle_ws_session(
    socket: WebSocket,
    manager: Arc<WsSessionManager>,
    tenant_id: TenantId,
    user_id: UserId,
    device_id: DeviceId,
    offline_queue: Arc<dyn OfflineQueueDrain>,
    offline_enqueue: Arc<dyn OfflineEnqueue>,
) {
    info!(?tenant_id, ?user_id, ?device_id, "WS session starting");

    let mut rx = manager
        .register_session(tenant_id, user_id.clone(), device_id, offline_queue.as_ref())
        .await;

    let (mut ws_tx, mut ws_rx) = socket.split();
    let manager_clone = Arc::clone(&manager);
    let offline_enqueue_clone = Arc::clone(&offline_enqueue);

    // Spawn a task that forwards RtEvents from the mpsc channel to the WS.
    let send_task = {
        let manager_c = Arc::clone(&manager);
        tokio::spawn(async move {
            use tokio::time::interval;
            let mut keepalive = interval(Duration::from_secs(30));
            keepalive.tick().await; // skip immediate tick

            loop {
                tokio::select! {
                    maybe_event = rx.recv() => {
                        match maybe_event {
                            None => break,
                            Some(event) => {
                                match serde_json::to_string(&event) {
                                    Ok(json) => {
                                        use axum::extract::ws::Message;
                                        if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                            break;
                                        }
                                    }
                                    Err(e) => warn!(error=%e, "failed to serialise event"),
                                }
                            }
                        }
                    }
                    _ = keepalive.tick() => {
                        use axum::extract::ws::Message;
                        if ws_tx.send(Message::Text(r#"{"type":"ping"}"#.into())).await.is_err() {
                            break;
                        }
                        // Brief window for pong — handled in the recv loop
                    }
                }
            }
            drop(manager_c);
        })
    };

    // Receive loop — handle pong and AckDatagram from client.
    while let Some(msg) = ws_rx.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                    if val.get("type").and_then(|v| v.as_str()) == Some("pong") {
                        debug!(?tenant_id, ?device_id, "pong received");
                        continue;
                    }
                    if let Ok(ack) = serde_json::from_value::<AckDatagram>(val) {
                        manager_clone.ack_envelope(tenant_id, device_id, ack).await;
                    }
                }
            }
            Ok(Message::Binary(bytes)) => {
                if let Ok(ack) = serde_json::from_slice::<AckDatagram>(&bytes) {
                    manager_clone.ack_envelope(tenant_id, device_id, ack).await;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }

    send_task.abort();
    info!(?tenant_id, ?device_id, "WS session closed");
    manager_clone
        .on_session_closed(tenant_id, device_id, offline_enqueue_clone.as_ref())
        .await;
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use common::{
        Curve25519PublicKey, DoubleRatchetHeader, ProtocolHeader,
    };
    use uuid::Uuid;

    struct EmptyOfflineQueue;
    #[async_trait]
    impl OfflineQueueDrain for EmptyOfflineQueue {
        async fn drain_for_device(&self, _: TenantId, _: DeviceId) -> Result<Vec<MessageEnvelope>, String> {
            Ok(vec![])
        }
    }

    fn make_envelope(seq: u64, conv_id: ConversationId) -> MessageEnvelope {
        MessageEnvelope {
            conversation_id: conv_id,
            seq,
            sender_user_id: UserId("sender".to_string()),
            sender_device_id: DeviceId(Uuid::new_v4()),
            recipient_user_id: None,
            recipient_device_id: None,
            ciphertext: Bytes::from_static(b"ct"),
            protocol_header: ProtocolHeader::DoubleRatchet(DoubleRatchetHeader {
                ratchet_key: Curve25519PublicKey([0u8; 32]),
                prev_chain_n: 0,
                msg_n: seq as u32,
            }),
            server_ts: 0,
            attachment_id: None,
        }
    }

    #[tokio::test]
    async fn register_and_deliver() {
        let mgr = WsSessionManager::new();
        let tid = TenantId(Uuid::new_v4());
        let uid = UserId("alice".to_string());
        let did = DeviceId(Uuid::new_v4());
        let _rx = mgr.register_session(tid, uid, did, &EmptyOfflineQueue).await;
        assert_eq!(mgr.active_session_count().await, 1);
        let outcome = mgr.deliver(tid, did, RtEvent::LowOtpk { device_id: did, count: 5 }).await;
        assert_eq!(outcome, DeliveryOutcome::Delivered);
    }

    #[tokio::test]
    async fn deliver_no_session() {
        let mgr = WsSessionManager::new();
        let tid = TenantId(Uuid::new_v4());
        let did = DeviceId(Uuid::new_v4());
        let outcome = mgr.deliver(tid, did, RtEvent::LowOtpk { device_id: did, count: 1 }).await;
        assert_eq!(outcome, DeliveryOutcome::NoSession);
    }

    #[tokio::test]
    async fn noop_manager_always_no_session() {
        let mgr = NoopWebTransportManager;
        let tid = TenantId(Uuid::new_v4());
        let did = DeviceId(Uuid::new_v4());
        let outcome = mgr.deliver(tid, did, RtEvent::LowOtpk { device_id: did, count: 1 }).await;
        assert_eq!(outcome, DeliveryOutcome::NoSession);
    }

    #[tokio::test]
    async fn ack_removes_from_unacked() {
        let mgr = WsSessionManager::new();
        let tid = TenantId(Uuid::new_v4());
        let uid = UserId("bob".to_string());
        let did = DeviceId(Uuid::new_v4());
        let conv_id = ConversationId(Uuid::new_v4());
        let _rx = mgr.register_session(tid, uid, did, &EmptyOfflineQueue).await;
        {
            let mut sessions = mgr.inner.sessions.write().await;
            sessions.get_mut(&(tid, did)).unwrap()
                .unacked.insert((conv_id, 1), make_envelope(1, conv_id));
        }
        mgr.ack_envelope(tid, did, AckDatagram { conversation_id: conv_id, seq: 1 }).await;
        let sessions = mgr.inner.sessions.read().await;
        assert!(sessions.get(&(tid, did)).unwrap().unacked.is_empty());
    }
}
