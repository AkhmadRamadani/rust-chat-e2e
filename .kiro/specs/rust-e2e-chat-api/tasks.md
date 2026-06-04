# Implementation Plan: rust-e2e-chat-api

## Overview

Incremental implementation of a Rust multi-tenant SaaS chat API with HTTP/1.1 + WebSocket transport, Signal Protocol E2EE, group Sender Keys, per-tenant OAuth 2.0 / OIDC auth, a Key Distribution Server, PostgreSQL + Redis storage (shared schema with `tenant_id` isolation), and Prometheus + tracing observability. The transport layer uses axum's built-in WebSocket support — no QUIC, HTTP/3, or WebTransport.

---

## Tasks

- [x] 0. Tenant management foundation
  - [x] 0.1 Add `TenantId` and `TenantConfig` types to `crates/common`; add new `crates/tenant` to workspace
  - [x] 0.2 Write and apply tenant table migration
  - [x] 0.3 Implement `TenantRepository` trait and `PgTenantRepository` in `crates/tenant`
  - [x] 0.4 Implement `TenantRegistry` in-memory cache in `crates/auth`
  - [x] 0.5 Update `JwksCache` and auth middleware for per-tenant JWKS
  - [x] 0.6 Implement Admin API handlers wired to `/admin/...` routes

- [x] 1. Project scaffold and shared domain types
  - [x] 1.1 Initialise Cargo workspace with all crate members
    - Workspace uses axum 0.7 with `ws` feature; no quinn/h3/h3-webtransport dependencies
    - _Requirements: 1.1_
  - [x] 1.2 Implement all shared domain types in `crates/common`
    - `RtEvent` serialises to JSON for WebSocket text frames
    - `AckDatagram` is sent by clients as a JSON text frame
    - _Requirements: 0.1, 3.1, 5.2, 6.1, 7.1_

- [x] 2. Database schema and repository infrastructure
  - [x] 2.1 Write and apply PostgreSQL migrations
  - [x] 2.2 Implement `KdsRepository` trait and `PgKdsRepository` in `crates/kds`
  - [x] 2.5 Implement `MessagingRepository` trait and `PgMessagingRepository` in `crates/messaging`
    - Also implements `realtime::OfflineQueueDrain` and `realtime::OfflineEnqueue` traits
    - _Requirements: 6.2, 6.4, 6.5, 9.1, 9.2_
  - [x] 2.9 Implement `GroupRepository` trait and `PgGroupRepository` in `crates/groups`
  - [x] 2.10 Implement `TokenStore` trait and `PgTokenStore` in `crates/auth`

- [x] 3. Checkpoint — data layer

- [x] 4. Auth middleware
  - [x] 4.1 Implement `validate_bearer_token` and `JwksCache` in `crates/auth`
    - JWKS URL constructed as `{iss}/.well-known/jwks.json`; 5-minute cache TTL
    - _Requirements: 2.1, 2.2, 2.3, 2.6, 2.9_
  - [x] 4.2 Implement `refresh_access_token` handler wired to `POST /auth/refresh`
  - [x] 4.3 Implement auth Tower middleware layer and `AuthenticatedUser` extractor

- [x] 5. Cryptographic key validation (KDS)
  - [x] 5.1 Implement `verify_signed_prekey` in `crates/kds` using `ed25519-dalek`

- [x] 6. KDS REST handlers
  - [x] 6.1 Implement `POST /users/{userID}/devices` handler
  - [x] 6.2 Implement `GET /users/{userID}/key-bundle` handler
    - Emits `low_otpk` event via `WsSessionManager::deliver` when count < 10
  - [x] 6.3 Implement `PUT /users/{userID}/devices/{deviceID}/one-time-prekeys` handler
  - [x] 6.4 Implement `PUT /users/{userID}/devices/{deviceID}/signed-prekey` handler

- [x] 7. 1:1 conversation and messaging REST handlers
  - [x] 7.1 Implement `POST /conversations` handler
    - `sender_device_id` is taken from `body.envelope.sender_device_id` (OIDC tokens don't carry device_id)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_
  - [x] 7.3 Implement `POST /conversations/{conversationID}/messages` handler
    - Fan out via `WsSessionManager::deliver`
  - [x] 7.4 Implement `GET /conversations/{conversationID}/messages` handler

- [x] 8. Group conversation and messaging REST handlers
  - [x] 8.1 Implement `POST /groups` handler
  - [x] 8.2 Implement `POST /groups/{conversationID}/messages` handler
  - [x] 8.4 Implement `POST /groups/{conversationID}/members` handler
  - [x] 8.5 Implement `DELETE /groups/{conversationID}/members/{userID}` handler
  - [x] 8.6 Implement `POST /groups/{conversationID}/sender-key-distribution` handler

- [x] 9. Checkpoint — REST handlers

- [x] 10. Transport layer (HTTP/1.1 + WebSocket)
  - [x] 10.1 `crates/transport` is a placeholder crate (QUIC/H3 removed)
    - All transport is handled by `axum::serve(TcpListener, router)` in `crates/api/src/main.rs`
    - _Requirements: 1.1_
  - [x] 10.2 Single TCP listener binds on `BIND_ADDR` (default `0.0.0.0:8080`)
    - Serves all REST routes, WebSocket upgrade, health, and metrics
    - _Requirements: 1.1, 1.2_

- [x] 11. WebSocket session manager (`crates/realtime`)
  - [x] 11.1 Implement `WsSessionManager` registry keyed by `(TenantId, DeviceId)`
    - `register_session` drains offline queue via `OfflineQueueDrain` trait
    - `deliver` sends to active session or returns `NoSession` for offline fallback
    - _Requirements: 10.1, 10.2_
  - [x] 11.2 Implement `handle_ws_session` WebSocket loop
    - Send task: reads `RtEvent` from mpsc channel, serialises as JSON, sends as WS text frame
    - Keepalive: `{"type":"ping"}` every 30s; closes on no pong within 10s
    - Recv loop: handles `{"type":"pong"}` and `AckDatagram` JSON from client
    - _Requirements: 6.3, 6.4, 8.3, 10.2, 10.6_
  - [x] 11.3 Implement `ack_envelope` and `on_session_closed`
    - `on_session_closed` re-enqueues unacked envelopes via `OfflineEnqueue`
    - _Requirements: 10.4, 10.5_
  - [x] 11.4 `OfflineQueueDrain` and `OfflineEnqueue` traits implemented by `PgMessagingRepository`
    - _Requirements: 6.4, 10.5_

- [x] 12. Observability (`crates/observability`)
  - [x] 12.1 Implement `MetricsRegistry` with Prometheus counters/gauges/histograms
  - [x] 12.2 Implement `GET /metrics` handler (Prometheus text format)
  - [x] 12.3 Implement `GET /health` handler (probes PostgreSQL + Redis)
  - [x] 12.4 Implement structured JSON request tracing middleware (`TracingLayer`)

- [x] 13. API router wiring (`crates/api`)
  - [x] 13.1 Assemble Axum router with all routes and middleware layers
    - `GET /ws` route with `WebSocketUpgrade` extractor + JWT auth via `?token` query param
    - Single HTTP/1.1 TCP listener via `axum::serve`
    - No QUIC endpoint; no TLS cert required at the app layer
    - `WsSessionManager` wired as real session manager (not `NoopWebTransportManager`)
    - `TenantRegistry::load_all` called at startup to populate registry from DB
    - _Requirements: 0.6, 1.1, 1.2, 2.1, 10.1, 11.1_
  - [x] 13.2 Implement consistent `ApiError` JSON error responses across all handlers
    - `sender_device_id` derived from request body when JWT doesn't contain device_id
    - _Requirements: 11.1, 11.5_

- [x] 14. Checkpoint — full integration
  - `cargo test --workspace` passes
  - `/health` returns HTTP 200 with all components ok
  - `/metrics` returns valid Prometheus text format
  - WebSocket connection at `ws://host/ws?token=<jwt>` delivers real-time events

---

## Notes

- **Transport**: QUIC, HTTP/3, and WebTransport are fully removed. The `crates/transport` crate is an empty placeholder kept for workspace compatibility.
- **WebSocket auth**: JWT is passed as `?token=<jwt>` query parameter because the browser `WebSocket` API does not support custom headers.
- **Device ID**: OIDC JWTs from standard providers (including mock-oauth2-server) do not include a `device_id` claim. The `sender_device_id` is taken from the request envelope body, and the WS session uses the registered device UUID when available.
- **Self-hosted stack**: nginx proxies all requests (including `/ws` with `Upgrade` headers) to `api:8080`. TLS is terminated at the reverse proxy layer.
- **Mock OIDC**: `mock-oauth2-server` serves JWKS at `/{issuer_id}/jwks`; an nginx rewrite proxy maps the standard `/.well-known/jwks.json` path to the actual endpoint.
- All repository methods accept `tenant_id: TenantId` as first parameter; all SQL queries use `tenant_id` as the leading filter.
- The server is zero-knowledge: ciphertext is never inspected, only stored and routed.

---

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["0.1"] },
    { "id": 1, "tasks": ["0.2", "1.1", "1.2"] },
    { "id": 2, "tasks": ["0.3", "2.1"] },
    { "id": 3, "tasks": ["0.4", "2.2", "2.9", "2.10"] },
    { "id": 4, "tasks": ["0.5", "0.6", "2.5"] },
    { "id": 5, "tasks": ["4.1", "5.1"] },
    { "id": 6, "tasks": ["4.2", "4.3"] },
    { "id": 7, "tasks": ["6.1", "6.2", "6.3", "6.4"] },
    { "id": 8, "tasks": ["7.1", "7.3", "7.4", "8.1"] },
    { "id": 9, "tasks": ["8.2", "8.4", "8.5", "8.6"] },
    { "id": 10, "tasks": ["10.1", "10.2"] },
    { "id": 11, "tasks": ["11.1", "11.2", "11.3", "11.4", "12.1", "12.4"] },
    { "id": 12, "tasks": ["12.2", "12.3"] },
    { "id": 13, "tasks": ["13.1"] },
    { "id": 14, "tasks": ["13.2"] }
  ]
}
```
