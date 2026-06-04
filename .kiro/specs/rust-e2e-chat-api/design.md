# Design Document: rust-e2e-chat-api

## Overview

The `rust-e2e-chat-api` is a Rust server application providing encrypted messaging for 1:1 and group conversations, deployed as a **multi-tenant SaaS platform**. Multiple independent tenant applications share a single deployment, with complete data isolation enforced at the application layer. The system is built around four core concerns:

1. **Multi-Tenancy** — Shared PostgreSQL schema with a `tenant_id` column on every table. Tenants are resolved from the JWT `iss` claim at the auth layer. Each tenant brings their own OIDC identity provider; the platform validates tokens against the tenant's configured JWKS endpoint.
2. **Transport** — HTTP/1.1 over TCP for all REST endpoints, with WebSocket upgrade (`GET /ws`) for real-time delivery. No QUIC or HTTP/3 dependency.
3. **Cryptography** — Signal Protocol (X3DH + Double Ratchet) for 1:1 sessions and Sender Keys for groups, with all crypto operations delegated to clients; the server only stores and forwards opaque ciphertext.
4. **State** — PostgreSQL for durable storage (key bundles, messages, group membership), Redis for session state and in-flight delivery queues, and a Prometheus + `tracing` observability stack.

The server is deliberately **zero-knowledge**: it stores and routes encrypted blobs but never derives, holds, or sees plaintext message content or cryptographic session material (except public key bundles in the KDS, which are inherently public).

### Key Architectural Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Multi-tenancy | Shared schema with `tenant_id` column + application-layer enforcement | Single DB is simpler to operate and cost-efficient; tenant isolation is enforced in every repository query |
| Tenant identity | Per-tenant OIDC (BYO identity provider) resolved from JWT `iss` claim | Tenants own their user directory; platform never manages passwords |
| HTTP framework | Axum over TCP/HTTP 1.1 with built-in WebSocket upgrade | Universal browser/proxy support; no special client required |
| Transport | HTTP/1.1 + WebSocket (replaced QUIC/HTTP-3/WebTransport) | Works through all proxies, CDNs, and load balancers; native browser support |
| Real-time | `GET /ws?token=<jwt>` WebSocket endpoint via axum `WebSocketUpgrade` | Token passed as query param since browsers can't set headers on WebSocket connections |
| DB | PostgreSQL via `sqlx` | Compile-time checked queries, async, strong ecosystem |
| In-flight queues | PostgreSQL-backed delivery state + Redis connection cache | Durability first; in-memory channels for sub-500ms fan-out |
| Auth | `jsonwebtoken` + JWKS fetch for OIDC token validation | Stateless JWT validation with JWKS key rotation support; JWKS cache keyed by `(tenant_id, jwks_url)` |
| Metrics | `prometheus-client` crate | Official Prometheus Rust client; all per-tenant metrics carry a `tenant_id` label |
| Logging | `tracing` + `tracing-subscriber` with JSON formatter | Structured JSON per request; all spans carry `tenant_id` |

---

## Architecture

### High-Level System Diagram

```
Clients (Browser / Mobile)
    │
    │  HTTP/1.1 + WebSocket (TCP port 8080)
    ▼
┌─────────────────────────────────────────────────────┐
│  ChatAPI Server (Rust / Axum)                       │
│                                                     │
│  ┌─────────┐  ┌───────────┐  ┌──────────────────┐  │
│  │  Auth   │  │  REST     │  │  WebSocket       │  │
│  │  Layer  │→ │  Handlers │  │  Session Manager │  │
│  │ (OIDC)  │  │ (Axum)    │  │  (WsSessionMgr)  │  │
│  └─────────┘  └─────┬─────┘  └────────┬─────────┘  │
│                     │                 │             │
│  ┌──────────────────┼─────────────────┘             │
│  │                  ▼                               │
│  │  ┌───────┐  ┌─────────┐  ┌───────┐  ┌────────┐  │
│  │  │  KDS  │  │  Msg    │  │Groups │  │  Obs   │  │
│  │  │       │  │ Router  │  │       │  │(Prom+  │  │
│  │  └───────┘  └─────────┘  └───────┘  │tracing)│  │
│  └──────────────────────────────────── └────────┘  │
└─────────────────────────────────────────────────────┘
    │                               │
    ▼                               ▼
PostgreSQL                        Redis
(messages, keys,                 (connection cache)
 members, state)
```

### Request Flow (REST)

```
Client → TCP → Axum router → AuthMiddleware (JWT validate) → Handler → Repository → PostgreSQL
```

### Real-Time Delivery Flow

```
POST /conversations/{id}/messages
  → store_envelope() → PostgreSQL
  → WsSessionManager.deliver(device_id, RtEvent::Message) → WebSocket JSON frame → Client
  → if no session: enqueue_offline() → delivery_state table
```

### WebSocket Connection Flow

```
Client → GET /ws?token=<jwt> → AuthMiddleware validates JWT → WebSocketUpgrade
  → handle_ws_session(socket, manager, tenant_id, user_id, device_id)
  → drain offline queue → bidirectional loop:
      server → client: JSON RtEvent text frames
      client → server: {"type":"pong"} or AckDatagram JSON
```

---

## Components and Interfaces

### 0. Tenant Registry (`crates/tenant`)

An in-memory cache loaded from the `tenants` table at startup, refreshed on admin mutations.

```rust
pub struct TenantConfig {
    pub tenant_id:   TenantId,
    pub name:        String,
    pub oidc_issuer: String,  // used to look up JWKS endpoint
    pub active:      bool,
}
```

**Key responsibilities:**
- `resolve_by_issuer(iss: &str) -> Option<TenantConfig>` — O(1) lookup.
- `invalidate(oidc_issuer: &str)` — called after admin updates a tenant's OIDC issuer.
- `load_all(db: &PgPool)` — bulk load from `tenants` table at startup.
- `upsert(config: TenantConfig)` — called after admin creates/updates tenant.

### 1. Transport Layer (HTTP/1.1 + WebSocket)

The `crates/transport` crate is now a placeholder. All transport is handled by:

- **Axum's built-in TCP listener** — `axum::serve(TcpListener, router)` on a single configurable port (default `0.0.0.0:8080`).
- **WebSocket upgrade** — `axum::extract::ws::WebSocketUpgrade` extractor on the `GET /ws` route.
- No TLS at the application layer — TLS is terminated by a reverse proxy (nginx, Cloudflare, etc.).

### 2. Auth Middleware (`auth` module)

Tower middleware layer that:
1. Extracts the `Authorization: Bearer <token>` header.
2. Decodes the JWT payload (without verifying) to read the `iss` claim.
3. Looks up `TenantConfig` from `TenantRegistry`; returns HTTP 401 `unknown_tenant` if not found, HTTP 403 `tenant_inactive` if inactive.
4. Fetches and caches the tenant's JWKS (5-minute TTL, keyed by `(TenantId, jwks_url)`).
5. Validates the token signature and expiry.
6. Injects `AuthenticatedUser { tenant_id, user_id, device_id }` into request extensions.

For WebSocket connections, auth is performed using the `token` query parameter before the upgrade.

### 3. Key Distribution Server (`kds` module)

REST handlers and `PgKdsRepository` wrapping PostgreSQL:
- `POST /users/{userID}/devices` — register device, store KeyBundle, verify SPK signature.
- `GET /users/{userID}/key-bundle` — fetch a KeyBundle (atomically consuming an OTPK).
- `PUT /users/{userID}/devices/{deviceID}/one-time-prekeys` — replenish OTPKs.
- `PUT /users/{userID}/devices/{deviceID}/signed-prekey` — rotate SPK.

Emits `low_otpk` events through the `WsSessionManager` when OTPK count drops below 10.

### 4. Message Router (`messaging` module)

- `PgMessagingRepository` handles all message persistence.
- Atomic sequence number assignment via `UPDATE conversations SET last_seq = last_seq + 1 RETURNING last_seq`.
- Fan-out to active WebSocket sessions via `WsSessionManager::deliver`.
- Offline queue management (10,000 envelope cap per device per conversation) via `delivery_state` table.

### 5. Group Manager (`groups` module)

- `PgGroupRepository` for group membership CRUD.
- Group creation, add/remove members, SKDM storage.
- Broadcasts `member_added`/`member_removed` events to all active WebSocket sessions.

### 6. WebSocket Session Manager (`crates/realtime`)

Replaces the previous WebTransport session manager. Uses the same session registry pattern:

```rust
pub struct WsSessionManager {
    sessions: Arc<RwLock<HashMap<(TenantId, DeviceId), SessionHandle>>>,
}
```

**Session loop** (`handle_ws_session`):
- Registers the session and drains the offline queue.
- Spawns a send task: reads `RtEvent`s from an mpsc channel, serialises to JSON, sends as WebSocket text frames. Also sends `{"type":"ping"}` every 30 seconds.
- Receive loop: handles `{"type":"pong"}` keepalive responses and `AckDatagram` JSON from client.
- On close: calls `on_session_closed`, which re-enqueues unacked envelopes.

**WebSocket endpoint** (`GET /ws?token=<jwt>`):
- JWT validated before upgrade using `validate_bearer_token`.
- `device_id` falls back to the device registered for that user when not in the JWT.

### 7. REST API (`api` module)

All routes served on a single HTTP/1.1 TCP listener:

```
# Tenant admin (platform admin token required)
POST   /admin/tenants
DELETE /admin/tenants/{tenantID}
PUT    /admin/tenants/{tenantID}/oidc
GET    /admin/tenants/{tenantID}/usage

# Tenant user routes (tenant OIDC bearer token required)
POST   /auth/refresh
POST   /users/{userID}/devices
GET    /users/{userID}/key-bundle
PUT    /users/{userID}/devices/{deviceID}/one-time-prekeys
PUT    /users/{userID}/devices/{deviceID}/signed-prekey
POST   /conversations
POST   /conversations/{conversationID}/messages
GET    /conversations/{conversationID}/messages
POST   /groups
POST   /groups/{conversationID}/messages
POST   /groups/{conversationID}/members
DELETE /groups/{conversationID}/members/{userID}
POST   /groups/{conversationID}/sender-key-distribution

# Real-time (WebSocket — auth via ?token query param)
GET    /ws

# Platform-level (no auth)
GET    /health
GET    /metrics
```

### 8. Observability (`observability` module)

- `tracing` spans on every request with `tenant_id`, `user_id`, `device_id`, `path`, `status`, `latency_ms`.
- `tracing-subscriber` with JSON formatter.
- `prometheus-client` registry at `/metrics`; all per-tenant metrics carry a `tenant_id` label.
- `/health` handler probes PostgreSQL (`SELECT 1`) and Redis (`PING`).

---

## Data Models

### PostgreSQL Schema

(Unchanged from original — all tables include `tenant_id UUID NOT NULL REFERENCES tenants(tenant_id)`)

The `delivery_state` table tracks per-device offline queue state. `WsSessionManager` uses in-memory mpsc channels for active sessions, falling back to `delivery_state` + `message_envelopes` for offline devices.

### Core Rust Types (unchanged)

All domain types (`TenantId`, `UserId`, `DeviceId`, `ConversationId`, `KeyBundle`, `MessageEnvelope`, `RtEvent`, `AckDatagram`, etc.) are unchanged.

**`RtEvent`** is serialised as JSON and sent as WebSocket text frames:
```json
{ "event": "Message", "conversation_id": "...", "seq": 1, ... }
{ "event": "LowOtpk", "device_id": "...", "count": 7 }
{ "event": "MemberAdded", "conversation_id": "...", "user_id": "...", "devices": [...] }
```

**Keepalive frames** (not `RtEvent`):
```json
{ "type": "ping" }   ← server → client
{ "type": "pong" }   ← client → server
```

**Ack frame** (client → server):
```json
{ "conversation_id": "...", "seq": 42 }
```

### Redis Usage

Redis is used for connection metadata caching. The primary offline queue state is stored in PostgreSQL (`delivery_state` table). Redis keys follow `{tenant_id}:` prefix for tenant isolation.

---

## Deployment

### Self-Hosted (Docker Compose)

```
┌─────────────────────────────────────────────────────┐
│  nginx (port 3000)                                  │
│  - Serves static dev client HTML                    │
│  - Proxies /ws, /health, /metrics, /admin/, etc.    │
│    → api:8080                                       │
│  - Proxies /oidc/ → oidc:80 (mock OIDC for dev)     │
└─────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────┐    ┌──────────────┐    ┌──────────────┐
│ api:8080    │    │ postgres:5432│    │ redis:6379   │
│ (Rust app)  │───▶│ (PgPool)     │    │ (cache)      │
└─────────────┘    └──────────────┘    └──────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────┐
│  oidc (nginx proxy, port 80, internal)              │
│  - Rewrites /{id}/.well-known/jwks.json → /{id}/jwks│
│  - Forwards to oidc-server:8080                     │
└─────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────┐
│  oidc-server (mock-oauth2-server, port 8080)         │
│  - Issues JWTs for dev/testing                      │
│  - JWKS at /{issuer_id}/jwks                        │
└─────────────────────────────────────────────────────┘
```

### Environment Variables

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | required | PostgreSQL connection string |
| `REDIS_URL` | required | Redis connection string |
| `ADMIN_TOKEN` | required | Static bearer token for `/admin/*` routes |
| `BIND_ADDR` | `0.0.0.0:8080` | TCP listener address |
| `RUST_LOG` | `info` | Tracing filter |

---

## Correctness Properties

All 15 correctness properties from the original design remain in force. References to "HTTP/3", "QUIC", "WebTransport", and "h3 unidirectional stream" should be read as "HTTP/1.1", "TCP", "WebSocket", and "WebSocket JSON text frame" respectively.

Property 6 (Low-OTPK Threshold Notification) now uses WebSocket delivery instead of WebTransport streams.
Property 9 (MessageEnvelope Field Completeness) applies to both REST retrieval and WebSocket delivery.
Properties 10.2–10.6 (WebSocket real-time) replace the original WebTransport requirements.

---

## Error Handling

All errors return a JSON body:

```json
{
  "error_code": "invalid_signed_prekey_signature",
  "message": "The SignedPreKey signature could not be verified against the provided IdentityKey.",
  "request_id": "550e8400-e29b-41d4-a716-446655440000"
}
```

Error code mapping is unchanged from the original design.
