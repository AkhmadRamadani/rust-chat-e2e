# Developer Guide — rust-e2e-chat-api

This document covers everything you need to add features, fix bugs, and maintain this codebase at a high standard.

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [Workspace Structure](#2-workspace-structure)
3. [Development Setup](#3-development-setup)
4. [Architecture Deep Dive](#4-architecture-deep-dive)
5. [Adding a New Feature — Step-by-Step](#5-adding-a-new-feature--step-by-step)
6. [Adding a New REST Endpoint](#6-adding-a-new-rest-endpoint)
7. [Adding a Database Migration](#7-adding-a-database-migration)
8. [Adding a New Crate](#8-adding-a-new-crate)
9. [Error Handling Best Practices](#9-error-handling-best-practices)
10. [Testing Strategy](#10-testing-strategy)
11. [Real-Time Events](#11-real-time-events)
12. [Multi-Tenancy Rules](#12-multi-tenancy-rules)
13. [Authentication & Authorization](#13-authentication--authorization)
14. [Observability](#14-observability)
15. [Frontend Dev Client](#15-frontend-dev-client)
16. [Docker & Deployment](#16-docker--deployment)
17. [Code Style & Conventions](#17-code-style--conventions)
18. [Common Pitfalls](#18-common-pitfalls)

---

## 1. Project Overview

```
rust-chat/
├── crates/           — Rust workspace crates
├── migrations/       — PostgreSQL migration files (ordered by filename)
├── client/           — Dev HTML client + nginx configs
├── oidc-proxy/       — nginx config for mock OIDC JWKS path rewrite
├── docker-compose.yml         — Production stack
├── docker-compose.dev.yml     — Dev overlay (adds mock OIDC)
├── Dockerfile
├── README.md
└── docs/
    └── CONTRIBUTING.md   ← you are here
```

**What it does**: A multi-tenant SaaS chat backend. Multiple independent apps (tenants) share one deployment. Each tenant has its own OIDC identity provider. Users register devices, exchange public keys, and send encrypted messages. The server routes opaque ciphertext — it never sees plaintext.

**Key constraint**: Every database query **must** include `tenant_id` as a filter. This is the only thing preventing tenant A from reading tenant B's data. There is no database-level row security — it's enforced entirely in application code.

---

## 2. Workspace Structure

```
crates/
├── common/       — Shared domain types (TenantId, UserId, MessageEnvelope, RtEvent…)
├── tenant/       — TenantRepository trait + PgTenantRepository
├── auth/         — JWT validation, JWKS cache, TenantRegistry, auth Tower middleware
├── kds/          — Key Distribution Server repository + signature verification
├── messaging/    — MessagingRepository + offline queue logic
├── groups/       — GroupRepository for group membership + SKDM
├── realtime/     — WebSocket session manager (WsSessionManager + handle_ws_session)
├── observability/— Prometheus metrics, /health handler, TracingLayer
├── transport/    — Placeholder (was QUIC/HTTP-3, now empty)
└── api/          — Axum router + all HTTP handlers (the binary)
```

### Dependency flow

```
common  ←  tenant, kds, messaging, groups, realtime, auth, observability
auth    ←  api
kds     ←  api
messaging ← realtime (OfflineQueueDrain/OfflineEnqueue traits), api
groups  ←  api
realtime← api
observability ← api
api     (binary — depends on all crates above)
```

**Rule**: `common` has no internal dependencies. `api` depends on everything. No other crate should depend on `api`.

---

## 3. Development Setup

### Without Docker (fast iteration)

```sh
# Requires: Rust 1.88+, PostgreSQL 16, Redis 7
cargo build --workspace
cargo test --workspace

# Start supporting services only
docker compose up -d postgres redis

# Set env vars
export DATABASE_URL=postgresql://chatuser:chatpass@localhost:5432/chatdb
export REDIS_URL=redis://:redispass@localhost:6379
export ADMIN_TOKEN=dev-token
export RUST_LOG=debug

# Run migrations
for f in migrations/*.sql; do psql "$DATABASE_URL" -f "$f"; done

# Run the server
cargo run --bin api
```

### With Docker (full stack)

```sh
cp .env.example .env
# edit .env — set ADMIN_TOKEN at minimum

# Dev mode (includes mock OIDC)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# Watch logs
docker compose logs -f api
```

### Useful commands

```sh
# Check compilation without running
cargo check --workspace

# Run a single crate's tests
cargo test -p messaging

# Check for unused dependencies
cargo +nightly udeps

# Format
cargo fmt --all

# Lint
cargo clippy --workspace -- -D warnings
```

---

## 4. Architecture Deep Dive

### Request lifecycle

```
HTTP Request
    │
    ▼
Axum TCP listener (port 8080)
    │
    ▼
TracingLayer          — creates request span with request_id, tenant_id, path
    │
    ▼
CatchPanicLayer       — converts panics to HTTP 500
    │
    ├── /admin/*  ──→ AdminAuthLayer (checks ADMIN_TOKEN env var)
    │                   └─→ admin handlers
    │
    ├── /ws  ──────→ ws_handler (validates ?token JWT, upgrades to WebSocket)
    │
    └── all other → AuthLayer (validates Bearer JWT, injects AuthenticatedUser)
                        └─→ kds / conversations / groups / attachments handlers
```

### WebSocket lifecycle

```
Client connects: GET /ws?token=<jwt>
    │
    ▼ jwt validated → AuthenticatedUser { tenant_id, user_id, device_id }
    │
    ▼ handle_ws_session()
        │
        ├── WsSessionManager::register_session()
        │       └── drains offline queue (PgMessagingRepository::drain_for_device)
        │
        ├── send_task (spawned):
        │       loop {
        │           recv RtEvent from mpsc channel → serialize JSON → WS text frame
        │           every 30s: send {"type":"ping"}
        │       }
        │
        └── recv_loop:
                {"type":"pong"}    → keepalive acknowledged
                AckDatagram JSON  → WsSessionManager::ack_envelope
                Close/error       → break
                    └── on_session_closed() → re-enqueue unacked envelopes
```

### Message delivery flow

```
POST /conversations/{id}/messages
    │
    ▼ MessagingRepository::store_envelope()
        │  UPDATE conversations SET last_seq = last_seq + 1 RETURNING last_seq
        │  INSERT INTO message_envelopes (... , seq, server_ts)
    │
    ▼ WsSessionManager::deliver(tenant_id, device_id, RtEvent::Message(...))
        │
        ├── Active session → send via mpsc channel → JSON WS frame  [< 500ms]
        │
        └── No session → MessagingRepository::enqueue_offline()
                             → delivery_state table (max 10,000 per device per conv)
```

---

## 5. Adding a New Feature — Step-by-Step

Every feature follows this checklist:

**1. Define types in `crates/common`**
Add any new domain types, enums, or structs. Derive `Debug`, `Clone`, `Serialize`, `Deserialize`.

**2. Write the database migration**
Add a new file in `migrations/` with the next sequential timestamp prefix.

**3. Implement the repository trait**
Add methods to the relevant trait (`MessagingRepository`, `GroupRepository`, etc.) and implement them in the `Pg*` struct using `sqlx`.

**4. Implement the handler**
Add the Axum handler function in the relevant `crates/api/src/*.rs` file.

**5. Register the route**
Add the route in `build_router()` in `crates/api/src/main.rs`.

**6. Update real-time events** (if needed)
Add a new `RtEvent` variant in `common` and deliver it via `WsSessionManager::deliver`.

**7. Update the dev client** (optional)
Add UI to `client/index.html` to exercise the new feature.

**8. Write tests**
Unit tests in the crate, integration test if it crosses crate boundaries.

---

## 6. Adding a New REST Endpoint

### Example: `GET /users/{userId}/devices` — list registered devices

**Step 1 — Add to `crates/kds/src/lib.rs`**

```rust
// In the KdsRepository trait:
async fn list_devices(
    &self,
    tenant_id: TenantId,
    user_id: UserId,
) -> Result<Vec<DeviceInfo>, KdsError>;

// DeviceInfo can be a new type in crates/common or local to kds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub created_at: u64,  // Unix timestamp
}
```

**Step 2 — Implement in `PgKdsRepository`**

```rust
async fn list_devices(
    &self,
    tenant_id: TenantId,
    user_id: UserId,
) -> Result<Vec<DeviceInfo>, KdsError> {
    let rows = sqlx::query_as::<_, (Uuid, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT device_id, created_at
        FROM devices
        WHERE tenant_id = $1 AND user_id = $2
        ORDER BY created_at
        "#,
    )
    .bind(tenant_id.0)
    .bind(&user_id.0)
    .fetch_all(&self.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, ts)| DeviceInfo {
            device_id: DeviceId(id),
            created_at: ts.timestamp_millis() as u64,
        })
        .collect())
}
```

**Step 3 — Add the handler in `crates/api/src/kds.rs`**

```rust
#[derive(Debug, Serialize)]
pub struct ListDevicesResponse {
    pub devices: Vec<kds::DeviceInfo>,
}

pub async fn list_devices(
    State(state): State<KdsState>,
    auth_user: AuthenticatedUser,
    Path(user_id): Path<String>,
) -> Result<Json<ListDevicesResponse>, KdsHandlerError> {
    // Ownership check: only the user themselves can list their devices
    if auth_user.user_id.0 != user_id {
        return Err(KdsHandlerError::Forbidden);
    }

    let devices = state
        .repo
        .list_devices(auth_user.tenant_id, UserId(user_id))
        .await
        .map_err(KdsHandlerError::from)?;

    Ok(Json(ListDevicesResponse { devices }))
}
```

**Step 4 — Register in `build_router()` in `main.rs`**

```rust
let kds_routes = Router::new()
    // ... existing routes ...
    .route("/users/:user_id/devices", get(kds::list_devices))  // ← add this
    .route("/users/:user_id/devices", post(kds::register_device))
    // ...
```

> **Note**: `get` and `post` on the same path are chained: `.route("/path", get(handler_a).post(handler_b))`

---

## 7. Adding a Database Migration

Migration files are applied in **filename sort order**. Use the pattern:

```
migrations/YYYYMMDDHHMMSS_description.sql
```

Example — add a `last_seen_at` column to `users`:

```sh
# File: migrations/20240601000000_add_last_seen_to_users.sql
```

```sql
-- Migration: Add last_seen_at to users table
ALTER TABLE users
    ADD COLUMN last_seen_at TIMESTAMPTZ;

-- Backfill with creation time
UPDATE users SET last_seen_at = created_at;
```

### Rules for migrations

- **Never modify existing migrations** — they may already be applied in production.
- **Always make migrations additive** — add columns, add tables, add indexes.
- **Avoid dropping columns** in a single migration — make it a two-step process across two deploys to avoid downtime.
- **Every new table must have `tenant_id UUID NOT NULL REFERENCES tenants(tenant_id)`**.
- **Every index on a new table must use `tenant_id` as the leading column**.

```sql
-- ✅ Good
CREATE TABLE reactions (
    reaction_id  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL REFERENCES tenants(tenant_id),
    message_id   BIGINT NOT NULL,
    user_id      TEXT NOT NULL,
    emoji        TEXT NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_reactions_tenant_msg ON reactions(tenant_id, message_id);

-- ❌ Bad — missing tenant_id
CREATE TABLE reactions (
    reaction_id UUID PRIMARY KEY,
    message_id  BIGINT NOT NULL
);
```

---

## 8. Adding a New Crate

When a feature grows large enough to warrant its own crate:

```sh
# Create the crate
mkdir -p crates/notifications/src
```

**`crates/notifications/Cargo.toml`**:

```toml
[package]
name = "notifications"
version = "0.1.0"
edition = "2021"

[dependencies]
common      = { path = "../common" }
tokio       = { workspace = true }
serde       = { workspace = true }
async-trait = { workspace = true }
thiserror   = { workspace = true }
```

**Add to workspace root `Cargo.toml`**:

```toml
[workspace]
members = [
    # ... existing members ...
    "crates/notifications",
]
```

**Add to `crates/api/Cargo.toml`** (if the API needs it):

```toml
[dependencies]
notifications = { path = "../notifications" }
```

---

## 9. Error Handling Best Practices

### Handler errors

Every handler module defines its own error enum that maps to HTTP responses:

```rust
#[derive(Debug)]
pub enum MyHandlerError {
    BadRequest(String),
    NotFound,
    Forbidden,
    Storage(String),   // ← internal error, return 503
}

impl IntoResponse for MyHandlerError {
    fn into_response(self) -> Response {
        match self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, Json(ApiError {
                error_code: error_codes::BAD_REQUEST.to_string(),
                message: msg,
                request_id: Uuid::new_v4(),
            })).into_response(),

            Self::NotFound => (StatusCode::NOT_FOUND, Json(ApiError {
                error_code: error_codes::NOT_FOUND.to_string(),
                message: "Resource not found.".to_string(),
                request_id: Uuid::new_v4(),
            })).into_response(),

            Self::Forbidden => (StatusCode::FORBIDDEN, Json(ApiError {
                error_code: error_codes::FORBIDDEN.to_string(),
                message: "Access denied.".to_string(),
                request_id: Uuid::new_v4(),
            })).into_response(),

            Self::Storage(msg) => {
                // Log the internal error, never leak it to the client
                tracing::error!("Storage error: {msg}");
                (StatusCode::SERVICE_UNAVAILABLE, Json(ApiError {
                    error_code: error_codes::STORAGE_UNAVAILABLE.to_string(),
                    message: "A storage error occurred; please retry.".to_string(),
                    request_id: Uuid::new_v4(),
                })).into_response()
            }
        }
    }
}
```

### Converting repository errors

Implement `From<RepoError> for HandlerError`:

```rust
impl From<MessagingError> for MyHandlerError {
    fn from(err: MessagingError) -> Self {
        match err {
            MessagingError::ConversationNotFound => Self::NotFound,
            MessagingError::NotParticipant => Self::Forbidden,
            MessagingError::Database(e) => Self::Storage(e.to_string()),
            MessagingError::Serialization(e) => Self::Storage(e.to_string()),
        }
    }
}
```

### Rules

- **Never** return raw `sqlx::Error` or internal messages to the client.
- **Always** use `error_codes::*` constants for the `error_code` field — never hardcode strings.
- **Always** include a fresh `Uuid::new_v4()` as `request_id` — this is already threaded through the tracing span.
- Storage errors → HTTP 503 (don't say "database", say "storage error occurred").
- Auth errors → HTTP 401 with `WWW-Authenticate: Bearer` header.
- Tenant inactive → HTTP 403 (handled by the auth middleware).

---

## 10. Testing Strategy

### Unit tests

Place unit tests in the same file as the code (`#[cfg(test)]` at the bottom). Test pure logic — serialization, error conversion, business rules — without a real database.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_correct() {
        let err = MyHandlerError::NotFound;
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
```

### Integration tests with a real database

Use `sqlx::test` with `testcontainers-rs` for repository tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    // sqlx::test spins up a temporary PostgreSQL instance
    #[sqlx::test(migrations = "../migrations")]
    async fn create_and_fetch_tenant(pool: sqlx::PgPool) {
        let repo = PgTenantRepository::new(pool);
        let config = repo.create_tenant("Test", "https://example.com").await.unwrap();
        assert_eq!(config.name, "Test");
        assert!(config.active);
    }
}
```

### Handler tests

Test Axum handlers using `tower::ServiceExt::oneshot`:

```rust
#[tokio::test]
async fn returns_401_without_token() {
    let router = make_test_router();

    let req = Request::builder()
        .method("POST")
        .uri("/conversations")
        .header("Content-Type", "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

### Running tests

```sh
# All tests
cargo test --workspace

# Specific crate
cargo test -p messaging

# Specific test
cargo test -p api test_name

# With output
cargo test --workspace -- --nocapture
```

---

## 11. Real-Time Events

### Adding a new event type

**Step 1 — Add to `RtEvent` in `crates/common/src/lib.rs`**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum RtEvent {
    // ... existing variants ...

    /// A user has started typing in a conversation.
    Typing {
        conversation_id: ConversationId,
        user_id: UserId,
    },
}
```

**Step 2 — Deliver the event from a handler**

```rust
// In your handler, after the relevant operation:
let event = RtEvent::Typing {
    conversation_id: conv_id,
    user_id: sender_user_id.clone(),
};

// Deliver to all participants (fire-and-forget)
for member in conversation_members {
    let _ = state.wt_manager.deliver(tenant_id, member.device_id, event.clone()).await;
}
```

**Step 3 — Handle in the dev client**

```js
// In handleRtEvent() in client/index.html:
if (event.event === 'Typing') {
    const convId = event.conversation_id;
    // show typing indicator
}
```

### Delivery semantics

- `WsSessionManager::deliver` is **fire-and-forget** for events that don't need guaranteed delivery (typing indicators, presence updates).
- For messages, the offline queue provides guaranteed delivery — `MessagingRepository::enqueue_offline` is called when no session is active.
- Events are **not persisted** unless you explicitly enqueue them — only `RtEvent::Message` is backed by the database queue.

---

## 12. Multi-Tenancy Rules

These rules are **non-negotiable**. Violating them causes data leakage between tenants.

### Every repository method takes `tenant_id` as the first parameter

```rust
// ✅ Correct
async fn get_messages(
    &self,
    tenant_id: TenantId,   // ← always first
    params: GetMessagesParams,
) -> Result<Vec<MessageEnvelope>, MessagingError>;

// ❌ Wrong — missing tenant_id
async fn get_messages(
    &self,
    params: GetMessagesParams,
) -> Result<Vec<MessageEnvelope>, MessagingError>;
```

### Every SQL query filters by `tenant_id`

```rust
// ✅ Correct
sqlx::query("SELECT * FROM message_envelopes WHERE tenant_id = $1 AND conversation_id = $2")
    .bind(tenant_id.0)
    .bind(conv_id.0)

// ❌ Wrong — no tenant filter
sqlx::query("SELECT * FROM message_envelopes WHERE conversation_id = $1")
    .bind(conv_id.0)
```

### Tenant isolation in handlers

The `tenant_id` always comes from the validated JWT (via `AuthenticatedUser::tenant_id`), never from the request body or URL parameters.

```rust
pub async fn my_handler(
    State(state): State<MyState>,
    auth_user: AuthenticatedUser,
    // ...
) -> Result<...> {
    let tenant_id = auth_user.tenant_id;  // ← from JWT, trusted
    // NEVER: let tenant_id = body.tenant_id;  // ← from request, untrusted
```

### Cross-tenant operations are always HTTP 403

If a resource's `tenant_id` doesn't match the token's `tenant_id`, return 403 — never 404. Returning 404 leaks information about whether the resource exists.

---

## 13. Authentication & Authorization

### How auth works

1. Client sends `Authorization: Bearer <jwt>`
2. `AuthLayer` middleware decodes the JWT header without verifying to extract `iss`
3. `TenantRegistry::resolve_by_issuer(iss)` finds the tenant config
4. `JwksCache::get_or_fetch(tenant_id, jwks_url)` fetches/caches the JWKS (5-minute TTL)
5. Token signature and expiry are verified against the JWKS
6. `AuthenticatedUser { tenant_id, user_id, device_id }` is injected into request extensions

### Adding a protected route

Any route merged after `.layer(AuthLayer::new(...))` is automatically protected:

```rust
let my_routes = Router::new()
    .route("/my-resource", get(my_handler))
    .with_state(my_state)
    .layer(AuthLayer::new(
        Arc::clone(&tenant_registry),
        Arc::clone(&jwks_cache),
    ));
```

### Extracting the authenticated user

```rust
pub async fn my_handler(
    auth_user: AuthenticatedUser,  // ← auto-extracted by the AuthLayer
    // ...
) {
    let tenant_id = auth_user.tenant_id;  // TenantId(Uuid)
    let user_id = auth_user.user_id;      // UserId(String) — the JWT sub claim
    let device_id = auth_user.device_id; // Option<DeviceId> — may be None
}
```

### Ownership checks

Always verify the authenticated user owns the resource before mutating it:

```rust
// ✅ Correct — user can only modify their own device
if auth_user.user_id.0 != path_user_id {
    return Err(MyError::Forbidden);
}
```

---

## 14. Observability

### Adding a Prometheus metric

**Step 1 — Add to `MetricsRegistry` in `crates/observability/src/lib.rs`**

```rust
pub struct MetricsRegistry {
    inner: Arc<MetricsInner>,
}

struct MetricsInner {
    // ... existing fields ...
    pub attachments_uploaded: Family<TenantLabels, Counter>,
}
```

**Step 2 — Register in `MetricsRegistry::new()`**

```rust
let attachments_uploaded = Family::<TenantLabels, Counter>::default();
registry.register(
    "attachments_uploaded",
    "Total number of attachments uploaded",
    attachments_uploaded.clone(),
);
```

**Step 3 — Add a helper method**

```rust
impl MetricsRegistry {
    pub fn inc_attachments_uploaded(&self, tenant_id: &str) {
        self.inner
            .attachments_uploaded
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .inc();
    }
}
```

**Step 4 — Call it in the handler**

```rust
// Pass MetricsRegistry into your state, or use it from the global state
metrics.inc_attachments_uploaded(&tenant_id.0.to_string());
```

### Structured logging

Use `tracing` macros. The `TracingLayer` automatically adds `request_id`, `path`, `tenant_id`, `user_id` to every span.

```rust
// Info — normal operations
tracing::info!(tenant_id = %tenant_id.0, user_id = %user_id.0, "user registered device");

// Warn — recoverable issues
tracing::warn!(error = %e, "failed to drain offline queue");

// Error — unexpected failures (never include user data)
tracing::error!(error = %e, "storage operation failed");
```

**Never log sensitive data**: passwords, tokens, private keys, message content.

---

## 15. Frontend Dev Client

The dev client (`client/index.html`) is a single-file vanilla JS app. It's intentionally simple — no build step, no framework.

### Structure

```
client/
├── index.html          — single-file app (HTML + CSS + JS)
├── nginx.conf          — production nginx (no /oidc route)
└── nginx.dev.conf      — dev nginx (with /oidc proxy route)
```

### Adding a new tab / feature

1. Add the HTML tab panel in the `<!-- Tab nav -->` section
2. Add the `switchTab` name to the `names` array
3. Add the tab content panel `<div class="tab-panel" id="tab-myfeature">`
4. Add JS functions for the feature
5. Call initialization in the `DOMContentLoaded` handler

### Making API calls

Always use `apiFetch` (for tenant-scoped API) or `adminFetch` (for admin API) — they handle auth headers and logging automatically:

```js
// Tenant API (uses bearer token from #token input)
const res = await apiFetch('GET', `/my-resource/${id}`, null, chatToken(), log);
if (res.ok) { /* use res.data */ }

// Admin API (uses admin token from #adminToken input)
const res = await adminFetch('POST', '/admin/tenants', { name, oidc_issuer });
```

### Adding WebSocket event handling

Add a new branch to `handleRtEvent()`:

```js
function handleRtEvent(event) {
    if (event.event === 'MyNewEvent') {
        // handle it
        return;
    }
    // ... existing handlers
}
```

---

## 16. Docker & Deployment

### Adding a service to docker-compose

For a service needed in **both dev and production**, add it to `docker-compose.yml`.
For a service needed in **dev only**, add it to `docker-compose.dev.yml`.

### Environment variable conventions

| Variable | Description | Required |
|---|---|---|
| `DATABASE_URL` | Full PostgreSQL connection string | Yes |
| `REDIS_URL` | Full Redis connection string with password | Yes |
| `ADMIN_TOKEN` | Platform admin bearer token | Yes |
| `BIND_ADDR` | TCP bind address (default `0.0.0.0:8080`) | No |
| `ATTACHMENT_DIR` | File storage path (default `/app/attachments`) | No |
| `RUST_LOG` | Tracing filter (default `info`) | No |

New environment variables should:
1. Have a sensible default in the binary (`std::env::var("X").unwrap_or_else(|_| "default".to_string())`)
2. Be documented in `.env.example`
3. Be added to the `api` service in `docker-compose.yml`

### Dockerfile notes

The Dockerfile is a two-stage build:
- Stage 1: `rust:1.88-slim` — compiles the binary
- Stage 2: `debian:bookworm-slim` — minimal runtime image

The binary runs as non-root user `appuser`. If you need to write files (like attachments), the directory must be owned by `appuser`:

```dockerfile
RUN useradd -m -u 1000 appuser \
    && chown -R appuser:appuser /app
```

---

## 17. Code Style & Conventions

### Naming

| Thing | Convention | Example |
|---|---|---|
| Crate names | `snake_case` | `crates/messaging` |
| Struct names | `PascalCase` | `PgMessagingRepository` |
| Handler functions | `snake_case` | `send_message` |
| Error types | `PascalCase` + `Error` suffix | `MessagingError`, `KdsHandlerError` |
| Repository traits | `PascalCase` + `Repository` suffix | `MessagingRepository` |
| Pg implementations | `Pg` + trait name | `PgMessagingRepository` |
| State structs | `PascalCase` + `State` | `ConversationState`, `KdsState` |
| Test mocks | `Mock` + name | `MockMessagingRepo` |

### Module layout

Each handler file follows this structure:

```rust
// 1. Shared handler state struct
pub struct MyState { ... }

// 2. Request/response types
#[derive(Deserialize)] struct MyRequest { ... }
#[derive(Serialize)] struct MyResponse { ... }

// 3. Error type + IntoResponse impl
pub enum MyError { ... }
impl IntoResponse for MyError { ... }
impl From<RepoError> for MyError { ... }

// 4. Handler functions (one per route)
pub async fn my_handler(...) -> Result<..., MyError> { ... }

// 5. Unit tests
#[cfg(test)]
mod tests { ... }
```

### SQL style

- All SQL keywords in UPPERCASE
- Bind parameters as `$1`, `$2`, etc. (PostgreSQL style)
- One clause per line for queries > 2 lines
- `tenant_id` filter always comes first in `WHERE` clauses

```rust
sqlx::query(r#"
    SELECT conversation_id, seq, sender_user_id, ciphertext, server_ts
      FROM message_envelopes
     WHERE tenant_id       = $1   -- ← tenant_id first
       AND conversation_id = $2
       AND seq             > $3
     ORDER BY seq ASC
     LIMIT $4
"#)
```

### Async / concurrency

- Use `Arc<dyn Trait>` for shared state — all state structs are `Clone`
- Fire-and-forget real-time delivery: `let _ = manager.deliver(...).await;`
- Never hold a `RwLock` guard across an `.await`
- Prefer `try_send` over `send` for mpsc channels in the hot path

---

## 18. Common Pitfalls

### Missing `tenant_id` filter

**Symptom**: Users can see other tenants' data.
**Fix**: Add `WHERE tenant_id = $1` to every query. Review with `grep -r "FROM message_envelopes" crates/`.

### Foreign key violations on `conversation_members`

**Symptom**: `503 storage_unavailable` when creating conversations.
**Cause**: `sender_device_id` or `recipient_device_id` in the envelope doesn't exist in the `devices` table.
**Fix**: The `sender_device_id` must come from `body.envelope.sender_device_id` (the registered device UUID), not from the JWT (which doesn't carry `device_id` in standard OIDC tokens).

### `unknown_tenant` after restart

**Symptom**: All requests return 401 `unknown_tenant` after restarting the API.
**Cause**: `TenantRegistry` is in-memory and empty on startup.
**Fix**: `tenant_registry.load_all(&pool).await` is called in `main()` at startup. If you're seeing this, confirm it's being called.

### nginx returning 301 redirect

**Symptom**: `POST /conversations` → 301 → `GET /conversations/`
**Cause**: nginx `location /conversations/` (trailing slash) auto-redirects requests without trailing slash.
**Fix**: Use regex locations: `location ~ ^/(conversations|...)(/.*)?$` instead of prefix locations with trailing slashes.

### WebSocket token passing

**Symptom**: WS connection fails with 401.
**Cause**: Browser `WebSocket` API doesn't support custom headers.
**Fix**: Pass the JWT as a query parameter: `new WebSocket('ws://host/ws?token=' + encodeURIComponent(jwt))`. The `ws_handler` reads it from `?token=`.

### `attachment_id` missing in message constructions

**Symptom**: Compile error `missing structure fields: attachment_id`.
**Fix**: When constructing `MessageEnvelope` or `NewMessageEnvelope`, always include `attachment_id: None` (or the actual value). This field was added to both structs.

### Stale JWKS cache after OIDC issuer update

**Symptom**: Tokens from new issuer fail with `invalid_token` after `PUT /admin/tenants/{id}/oidc`.
**Cause**: JWKS cache has the old entry.
**Fix**: The `update_oidc_issuer` admin handler calls `jwks_cache.invalidate(tenant_id, old_jwks_url)` — ensure this is present. Cache TTL is 5 minutes otherwise.

---

## Appendix: Crate Responsibilities Summary

| Crate | Owns | Does NOT own |
|---|---|---|
| `common` | Domain types, error codes | Any I/O or logic |
| `tenant` | Tenant CRUD, TenantUsage | Auth, JWKS |
| `auth` | JWT validation, JWKS cache, TenantRegistry, Tower middleware | Any business logic |
| `kds` | Key bundle storage, SPK signature verification | Auth, messaging |
| `messaging` | Message storage, delivery state, offline queue | Groups, auth |
| `groups` | Group membership, SKDM storage | Messaging (delegates via trait) |
| `realtime` | WS session registry, offline queue traits | Database access |
| `observability` | Prometheus metrics, /health, TracingLayer | Business logic |
| `transport` | (empty placeholder) | — |
| `api` | Router wiring, handler functions, `main()` | Domain logic (delegates to crates above) |
