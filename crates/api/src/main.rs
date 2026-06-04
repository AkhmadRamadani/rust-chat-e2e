// crates/api — Axum router wiring (binary entry point)
//
// Task 13.1: Assemble the Axum router with all route handlers and middleware layers.
//
// Layers applied (outermost → innermost):
//   1. `TracingLayer`     — structured JSON request spans (observability crate)
//   2. `CatchPanicLayer`  — converts any handler panic to HTTP 500 `internal_error`
//
// Per-sub-router layers:
//   - `/admin/...`    — `AdminAuthLayer` (ADMIN_TOKEN env var check)
//   - tenant routes   — `AuthLayer` (per-tenant OIDC bearer-token validation)
//
// State wired as Axum `State`:
//   - `AdminState`         — admin handlers
//   - `Arc<RefreshState>`  — POST /auth/refresh
//   - `KdsState`           — KDS handlers
//   - `ConversationState`  — conversation handlers
//   - `GroupState`         — group handlers
//   - `MetricsRegistry`    — GET /metrics
//   - `HealthState`        — GET /health
//
// `WebTransportManager` is carried inside `KdsState`, `ConversationState`,
// and `GroupState` rather than as a top-level state entry so that each
// handler sub-router can clone the Arc it needs without global state merging.

mod admin;
mod admin_auth;
mod attachments;
mod conversations;
mod error;
mod groups;
mod kds;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use tower_http::catch_panic::CatchPanicLayer;

use admin_auth::AdminAuthLayer;
use auth::{AuthLayer, PgTokenStore, RefreshState};
use common::{error_codes, ApiError};
use observability::{HealthState, MetricsRegistry, TracingLayer, init_tracing};
use realtime::{WsSessionManager, handle_ws_session};
use uuid::Uuid;

use ::kds::PgKdsRepository;
use ::groups::PgGroupRepository;
use ::messaging::PgMessagingRepository;
use ::tenant::PgTenantRepository;

// ── Panic → HTTP 500 conversion ───────────────────────────────────────────────

/// Convert a caught panic into an HTTP 500 response carrying a structured
/// `ApiError` JSON body with `error_code: "internal_error"`.
///
/// This function is passed to [`CatchPanicLayer::custom`] so that panics in
/// any downstream handler are converted to a well-formed API error rather
/// than a plain-text "Service panicked" response.
///
/// Requirements: 11.1
fn handle_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response<Body> {
    let message = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "An internal server error occurred.".to_string()
    };

    tracing::error!(panic_message = %message, "handler panicked");

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error_code: error_codes::INTERNAL_ERROR.to_string(),
            message: "An internal server error occurred.".to_string(),
            request_id: Uuid::new_v4(),
        }),
    )
        .into_response()
}

// ── WebSocket state ───────────────────────────────────────────────────────────

/// Shared state for the `/ws` handler.
#[derive(Clone)]
pub struct WsState {
    manager: Arc<WsSessionManager>,
    registry: Arc<auth::TenantRegistry>,
    jwks_cache: Arc<auth::JwksCache>,
    offline_queue: Arc<dyn realtime::OfflineQueueDrain>,
    offline_enqueue: Arc<dyn realtime::OfflineEnqueue>,
}

/// Query parameters for `GET /ws`.
#[derive(Deserialize)]
struct WsQuery {
    /// Bearer token passed as a query param since browser `WebSocket` API
    /// does not support custom headers.
    token: String,
}

/// `GET /ws?token=<jwt>` — upgrade to a WebSocket real-time session.
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsQuery>,
    State(state): State<WsState>,
) -> Response<Body> {
    // Validate the JWT before upgrading.
    let user = match auth::validate_bearer_token(
        &params.token,
        &state.registry,
        &state.jwks_cache,
    )
    .await
    {
        Ok(u) => u,
        Err(e) => return e.into_response(),
    };

    let device_id = match user.device_id {
        Some(d) => d,
        // Use a nil UUID when no device_id is in the token; the client should
        // pass a device_id query param in a real implementation.
        None => common::DeviceId(Uuid::nil()),
    };

    let manager = Arc::clone(&state.manager);
    let offline_queue = Arc::clone(&state.offline_queue);
    let offline_enqueue = Arc::clone(&state.offline_enqueue);

    ws.on_upgrade(move |socket| async move {
        handle_ws_session(
            socket,
            manager,
            user.tenant_id,
            user.user_id,
            device_id,
            offline_queue,
            offline_enqueue,
        )
        .await;
    })
}

// ── Router assembly ───────────────────────────────────────────────────────────

/// Assemble the complete Axum [`Router`] with all REST and WebSocket routes.
///
/// All route groups, middleware layers, and shared state are wired here.
///
/// Requirements: 0.6, 1.1, 2.1, 11.1
pub fn build_router(
    admin_state: admin::AdminState,
    refresh_state: Arc<RefreshState>,
    kds_state: kds::KdsState,
    conversation_state: conversations::ConversationState,
    group_state: groups::GroupState,
    metrics_registry: MetricsRegistry,
    health_state: HealthState,
    ws_state: WsState,
    attachment_state: attachments::AttachmentState,
) -> Router {
    // ── Shared auth handles (cloned for each sub-router) ───────────────────
    let tenant_registry = Arc::new(admin_state.registry.clone());
    let jwks_cache = Arc::new(admin_state.jwks_cache.clone());

    // ── Platform routes (no auth required) ────────────────────────────────
    let platform_routes = Router::new()
        .route("/health", get(observability::health_handler))
        .with_state(health_state)
        .route("/metrics", get(observability::metrics_handler))
        .with_state(metrics_registry.clone());

    // ── Auth routes (public — no bearer-token middleware) ──────────────────
    let auth_routes = Router::new()
        .route("/auth/refresh", post(auth::refresh_access_token))
        .with_state(refresh_state);

    // ── Admin routes (/admin/...) protected by AdminAuthLayer ──────────────
    let admin_routes = Router::new()
        .route("/admin/tenants", post(admin::create_tenant))
        .route("/admin/tenants/:tenant_id", delete(admin::deactivate_tenant))
        .route("/admin/tenants/:tenant_id/oidc", put(admin::update_oidc_issuer))
        .route("/admin/tenants/:tenant_id/usage", get(admin::get_tenant_usage))
        .with_state(admin_state)
        .layer(AdminAuthLayer::from_env());

    // ── KDS routes — protected by AuthLayer ───────────────────────────────
    let kds_routes = Router::new()
        .route("/users/:user_id/devices", post(kds::register_device))
        .route("/users/:user_id/key-bundle", get(kds::get_key_bundle))
        .route(
            "/users/:user_id/devices/:device_id/one-time-prekeys",
            put(kds::replenish_otpks),
        )
        .route(
            "/users/:user_id/devices/:device_id/signed-prekey",
            put(kds::rotate_signed_prekey),
        )
        .with_state(kds_state)
        .layer(AuthLayer::new(
            Arc::clone(&tenant_registry),
            Arc::clone(&jwks_cache),
        ));

    // ── Conversation routes — protected by AuthLayer ───────────────────────
    let conversation_routes = Router::new()
        .route("/conversations", post(conversations::create_conversation))
        .route(
            "/conversations/:conversation_id/messages",
            post(conversations::send_message).get(conversations::get_messages),
        )
        .with_state(conversation_state)
        .layer(AuthLayer::new(
            Arc::clone(&tenant_registry),
            Arc::clone(&jwks_cache),
        ));

    // ── Group routes — protected by AuthLayer ─────────────────────────────
    let group_routes = Router::new()
        .route("/groups", post(groups::create_group))
        .route(
            "/groups/:conversation_id/messages",
            post(groups::send_group_message),
        )
        .route(
            "/groups/:conversation_id/members",
            post(groups::add_group_member),
        )
        .route(
            "/groups/:conversation_id/members/:user_id",
            delete(groups::remove_group_member),
        )
        .route(
            "/groups/:conversation_id/sender-key-distribution",
            post(groups::distribute_sender_key),
        )
        .with_state(group_state)
        .layer(AuthLayer::new(
            Arc::clone(&tenant_registry),
            Arc::clone(&jwks_cache),
        ));

    // ── Assemble the top-level router ──────────────────────────────────────
    //
    // Global middleware stack — applied by calling `.layer()` from innermost
    // to outermost (Axum applies the last `.layer()` call outermost):
    //
    //   Layer order (outermost first in logical request flow):
    //   1. TracingLayer    — structured JSON request tracing spans
    //   2. CatchPanicLayer — convert handler panics to HTTP 500 `internal_error`
    //
    // We apply CatchPanicLayer first (it becomes the inner middleware that
    // receives requests before TracingLayer — wait, Axum stacks layers so that
    // the LAST `.layer()` call is outermost). So:
    //   .layer(CatchPanicLayer)  — inner (applied first)
    //   .layer(TracingLayer)     — outer (applied second, wraps CatchPanic)
    //
    // Because TracingLayer's Service impl requires the inner service to have
    // Response = Response<Body>, we place TracingLayer outermost around the
    // unmodified Axum router (before any body-erasing layers), and apply
    // CatchPanicLayer separately as an inner layer.
    // ── Attachment routes — protected by AuthLayer ────────────────────────
    let attachment_routes = Router::new()
        .route("/attachments", post(attachments::upload_attachment))
        .route("/attachments/:attachment_id", get(attachments::download_attachment))
        .with_state(attachment_state)
        .layer(AuthLayer::new(
            Arc::clone(&tenant_registry),
            Arc::clone(&jwks_cache),
        ));

    // ── WebSocket real-time route — authenticated via token query param ───
    let ws_routes = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(ws_state);

    Router::new()
        .merge(platform_routes)
        .merge(auth_routes)
        .merge(admin_routes)
        .merge(kds_routes)
        .merge(conversation_routes)
        .merge(group_routes)
        .merge(attachment_routes)
        .merge(ws_routes)
        // CatchPanicLayer is applied first (innermost position after routes),
        // but TracingLayer is placed around the whole thing via axum's layer
        // stacking. We apply them as separate layers so Axum can normalize
        // the body types between them.
        .layer(CatchPanicLayer::custom(handle_panic))
        .layer(TracingLayer::new())
}

// ── Binary entry point ────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    init_tracing();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url    = std::env::var("REDIS_URL").expect("REDIS_URL must be set");
    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .expect("BIND_ADDR must be a valid socket address");

    // ── Connect to PostgreSQL ─────────────────────────────────────────────────
    tracing::info!("Connecting to PostgreSQL...");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");
    tracing::info!("PostgreSQL connected");

    // ── Connect to Redis ──────────────────────────────────────────────────────
    tracing::info!("Connecting to Redis...");
    let redis_client = redis::Client::open(redis_url)
        .expect("Invalid REDIS_URL");
    redis_client
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to connect to Redis");
    tracing::info!("Redis connected");

    // ── Real-time session manager (WebSocket) ─────────────────────────────────
    let ws_manager = Arc::new(WsSessionManager::new());
    let rt_manager: Arc<dyn realtime::WebTransportManager> = Arc::clone(&ws_manager) as _;

    // ── Metrics ───────────────────────────────────────────────────────────────
    let metrics_registry = MetricsRegistry::new();

    // ── Tenant / admin state ──────────────────────────────────────────────────
    let tenant_registry = auth::TenantRegistry::new();
    tenant_registry.load_all(&pool).await;
    tracing::info!("Tenant registry loaded from database");

    let jwks_cache   = auth::JwksCache::new();
    let tenant_repo  = Arc::new(PgTenantRepository::new(pool.clone()));
    let token_store  = Arc::new(PgTokenStore::new(pool.clone()));
    let admin_state  = admin::AdminState {
        repo:       tenant_repo,
        registry:   tenant_registry.clone(),
        jwks_cache: jwks_cache.clone(),
    };
    let refresh_state = Arc::new(RefreshState { token_store });

    // ── Offline queue adapters (PgMessagingRepository satisfies both traits) ──
    let messaging_repo_ws = Arc::new(PgMessagingRepository::new(pool.clone()));

    // ── Attachment routes — protected by AuthLayer ────────────────────────
    let attachment_state = attachments::AttachmentState {
        pool: pool.clone(),
        storage_dir: std::path::PathBuf::from(
            std::env::var("ATTACHMENT_DIR").unwrap_or_else(|_| "/app/attachments".to_string()),
        ),
    };
    // Ensure the storage directory exists at startup
    tokio::fs::create_dir_all(&attachment_state.storage_dir)
        .await
        .expect("Failed to create attachment storage directory");

    let kds_state = kds::KdsState {
        repo:       Arc::new(PgKdsRepository::new(pool.clone())),
        wt_manager: Arc::clone(&rt_manager),
    };
    let conversation_state = conversations::ConversationState {
        repo:       Arc::new(PgMessagingRepository::new(pool.clone())),
        wt_manager: Arc::clone(&rt_manager),
    };
    let group_state = groups::GroupState {
        group_repo:    Arc::new(PgGroupRepository::new(pool.clone())),
        messaging_repo: Arc::new(PgMessagingRepository::new(pool.clone())),
        wt_manager:    Arc::clone(&rt_manager),
    };
    let health_state = HealthState {
        pool:  pool.clone(),
        redis: redis_client,
    };

    // ── WebSocket state ───────────────────────────────────────────────────────
    let ws_state = WsState {
        manager:         Arc::clone(&ws_manager),
        registry:        Arc::new(tenant_registry),
        jwks_cache:      Arc::new(jwks_cache),
        offline_queue:   messaging_repo_ws.clone() as Arc<dyn realtime::OfflineQueueDrain>,
        offline_enqueue: messaging_repo_ws         as Arc<dyn realtime::OfflineEnqueue>,
    };

    // ── Build router ──────────────────────────────────────────────────────────
    let router = build_router(
        admin_state,
        refresh_state,
        kds_state,
        conversation_state,
        group_state,
        metrics_registry,
        health_state,
        ws_state,
        attachment_state,
    );

    // ── Single HTTP/1.1 + WebSocket listener ─────────────────────────────────
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("Failed to bind listener");
    tracing::info!(%bind_addr, "rust-e2e-chat-api listening (HTTP/1.1 + WebSocket)");

    axum::serve(listener, router)
        .await
        .expect("Server error");
}

// ── Integration smoke-tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use auth::{JwksCache, RefreshState, TenantRegistry};
    use auth::token_store::{RefreshTokenData, TokenStore, TokenStoreError};
    use common::{
        ConversationId, ConversationMember, Curve25519PublicKey, DeviceId, Ed25519Signature,
        KeyBundle, KeyBundleResponse, MessageEnvelope, NewMessageEnvelope, OneTimePreKey,
        TenantId, UserId,
    };
    use async_trait::async_trait;
    use ::kds::{KdsError, KdsRepository};
    use ::messaging::{EnqueueResult, GetMessagesParams, MessagingError, MessagingRepository};
    use ::groups::{GroupError, GroupRepository};
    use uuid::Uuid;

    // ── Helper: build a router for compilation/shape tests ────────────────

    fn make_test_router() -> Router {
        let admin_state = admin::AdminState {
            repo: Arc::new(MockTenantRepo),
            registry: TenantRegistry::new(),
            jwks_cache: JwksCache::new(),
        };
        let refresh_state = Arc::new(RefreshState {
            token_store: Arc::new(MockTokenStore),
        });
        let kds_state = kds::KdsState {
            repo: Arc::new(MockKdsRepo),
            wt_manager: Arc::new(realtime::NoopWebTransportManager),
        };
        let conversation_state = conversations::ConversationState {
            repo: Arc::new(MockMessagingRepo),
            wt_manager: Arc::new(realtime::NoopWebTransportManager),
        };
        let group_state = groups::GroupState {
            group_repo: Arc::new(MockGroupRepo),
            messaging_repo: Arc::new(MockMessagingRepo),
            wt_manager: Arc::new(realtime::NoopWebTransportManager),
        };
        let metrics_registry = MetricsRegistry::new();

        // HealthState requires a real PgPool and redis::Client — we cannot
        // construct those without a database in unit tests.  We use a separate
        // router builder variant that omits the /health route for unit tests.
        build_router_without_health(
            admin_state,
            refresh_state,
            kds_state,
            conversation_state,
            group_state,
            metrics_registry,
        )
    }

    /// Build a router without the `HealthState`-dependent `/health` route.
    fn build_router_without_health(
        admin_state: admin::AdminState,
        refresh_state: Arc<RefreshState>,
        kds_state: kds::KdsState,
        conversation_state: conversations::ConversationState,
        group_state: groups::GroupState,
        metrics_registry: MetricsRegistry,
    ) -> Router {
        let tenant_registry = Arc::new(admin_state.registry.clone());
        let jwks_cache = Arc::new(admin_state.jwks_cache.clone());

        // Dummy WS state for tests — no real sessions.
        struct NoopDrain;
        struct NoopEnqueue;
        #[async_trait::async_trait]
        impl realtime::OfflineQueueDrain for NoopDrain {
            async fn drain_for_device(&self, _: common::TenantId, _: common::DeviceId) -> Result<Vec<common::MessageEnvelope>, String> { Ok(vec![]) }
        }
        #[async_trait::async_trait]
        impl realtime::OfflineEnqueue for NoopEnqueue {
            async fn enqueue(&self, _: common::TenantId, _: common::DeviceId, _: common::MessageEnvelope) -> Result<realtime::EnqueueResult, String> { Ok(realtime::EnqueueResult::Queued) }
        }
        let ws_state = WsState {
            manager: Arc::new(realtime::WsSessionManager::new()),
            registry: Arc::clone(&tenant_registry),
            jwks_cache: Arc::clone(&jwks_cache),
            offline_queue: Arc::new(NoopDrain),
            offline_enqueue: Arc::new(NoopEnqueue),
        };

        let platform_routes = Router::new()
            .route("/metrics", get(observability::metrics_handler))
            .with_state(metrics_registry);

        let auth_routes = Router::new()
            .route("/auth/refresh", post(auth::refresh_access_token))
            .with_state(refresh_state);

        let admin_routes = Router::new()
            .route("/admin/tenants", post(admin::create_tenant))
            .route("/admin/tenants/:tenant_id", delete(admin::deactivate_tenant))
            .route("/admin/tenants/:tenant_id/oidc", put(admin::update_oidc_issuer))
            .route("/admin/tenants/:tenant_id/usage", get(admin::get_tenant_usage))
            .with_state(admin_state)
            .layer(AdminAuthLayer::from_env());

        let kds_routes = Router::new()
            .route("/users/:user_id/devices", post(kds::register_device))
            .route("/users/:user_id/key-bundle", get(kds::get_key_bundle))
            .route(
                "/users/:user_id/devices/:device_id/one-time-prekeys",
                put(kds::replenish_otpks),
            )
            .route(
                "/users/:user_id/devices/:device_id/signed-prekey",
                put(kds::rotate_signed_prekey),
            )
            .with_state(kds_state)
            .layer(AuthLayer::new(
                Arc::clone(&tenant_registry),
                Arc::clone(&jwks_cache),
            ));

        let conversation_routes = Router::new()
            .route("/conversations", post(conversations::create_conversation))
            .route(
                "/conversations/:conversation_id/messages",
                post(conversations::send_message).get(conversations::get_messages),
            )
            .with_state(conversation_state)
            .layer(AuthLayer::new(
                Arc::clone(&tenant_registry),
                Arc::clone(&jwks_cache),
            ));

        let group_routes = Router::new()
            .route("/groups", post(groups::create_group))
            .route(
                "/groups/:conversation_id/messages",
                post(groups::send_group_message),
            )
            .route(
                "/groups/:conversation_id/members",
                post(groups::add_group_member),
            )
            .route(
                "/groups/:conversation_id/members/:user_id",
                delete(groups::remove_group_member),
            )
            .route(
                "/groups/:conversation_id/sender-key-distribution",
                post(groups::distribute_sender_key),
            )
            .with_state(group_state)
            .layer(AuthLayer::new(
                Arc::clone(&tenant_registry),
                Arc::clone(&jwks_cache),
            ));

        let ws_routes = Router::new()
            .route("/ws", get(ws_handler))
            .with_state(ws_state);

        // Dummy attachment state for tests
        let attachment_state = attachments::AttachmentState {
            pool: {
                // We can't create a real pool in unit tests; use a fake path.
                // The routes won't be called in these tests.
                sqlx::postgres::PgPool::connect_lazy("postgresql://localhost/test").unwrap()
            },
            storage_dir: std::path::PathBuf::from("/tmp/test-attachments"),
        };

        let attachment_routes = Router::new()
            .route("/attachments", post(attachments::upload_attachment))
            .route("/attachments/:attachment_id", get(attachments::download_attachment))
            .with_state(attachment_state)
            .layer(AuthLayer::new(
                Arc::clone(&tenant_registry),
                Arc::clone(&jwks_cache),
            ));

        Router::new()
            .merge(platform_routes)
            .merge(auth_routes)
            .merge(admin_routes)
            .merge(kds_routes)
            .merge(conversation_routes)
            .merge(group_routes)
            .merge(attachment_routes)
            .merge(ws_routes)
            .layer(CatchPanicLayer::custom(handle_panic))
            .layer(TracingLayer::new())
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    /// Verify the full router compiles and can be constructed without panicking.
    #[test]
    fn full_router_compiles() {
        let _router = make_test_router();
    }

    /// The router should return HTTP 401 for a tenant-protected route when
    /// no Authorization header is present (AuthLayer blocks the request).
    #[tokio::test]
    async fn protected_route_returns_401_without_token() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

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

    /// The admin route should return 401 when the admin token is wrong,
    /// or 500 when ADMIN_TOKEN is not configured (misconfiguration).
    ///
    /// In test environments ADMIN_TOKEN is not set, so we expect HTTP 500.
    #[tokio::test]
    async fn admin_route_rejects_missing_token() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let router = make_test_router();

        let req = Request::builder()
            .method("POST")
            .uri("/admin/tenants")
            .header("Content-Type", "application/json")
            .body(Body::from(r#"{"name":"test","oidc_issuer":"https://test.example.com"}"#))
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        // When ADMIN_TOKEN is not set: HTTP 500 (misconfiguration).
        // When ADMIN_TOKEN is set but missing: HTTP 401.
        // Either way, the request is rejected (not 200/201).
        assert_ne!(resp.status(), StatusCode::OK);
        assert_ne!(resp.status(), StatusCode::CREATED);
    }

    /// The /metrics endpoint should return HTTP 200 with Prometheus text format.
    #[tokio::test]
    async fn metrics_endpoint_returns_200() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let router = make_test_router();

        let req = Request::builder()
            .method("GET")
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// A panic in a handler must be caught and converted to HTTP 500 with
    /// the `internal_error` error code.
    #[tokio::test]
    async fn panic_is_caught_and_returns_500() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        // Build a minimal router with a handler that always panics.
        // The closure explicitly returns `axum::response::Response` to avoid
        // never-type fallback issues.
        async fn panicking_handler() -> axum::response::Response {
            panic!("intentional test panic");
        }

        let panic_router = Router::new()
            .route("/panic", get(panicking_handler))
            .layer(CatchPanicLayer::custom(handle_panic))
            .layer(TracingLayer::new());

        let req = Request::builder()
            .method("GET")
            .uri("/panic")
            .body(Body::empty())
            .unwrap();

        let resp = panic_router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Verify the response body contains the expected error_code.
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["error_code"], "internal_error");
    }

    // ── Mock implementations ──────────────────────────────────────────────

    struct MockTokenStore;

    #[async_trait]
    impl TokenStore for MockTokenStore {
        async fn store_refresh_token(
            &self,
            _tenant_id: TenantId,
            _data: RefreshTokenData,
        ) -> Result<(), TokenStoreError> {
            Ok(())
        }

        async fn revoke(
            &self,
            _tenant_id: TenantId,
            _jti: &str,
        ) -> Result<(), TokenStoreError> {
            Ok(())
        }

        async fn is_revoked(
            &self,
            _tenant_id: TenantId,
            _jti: &str,
        ) -> Result<bool, TokenStoreError> {
            Ok(false)
        }
    }

    struct MockTenantRepo;

    #[async_trait::async_trait]
    impl tenant::TenantRepository for MockTenantRepo {
        async fn create_tenant(
            &self,
            name: &str,
            oidc_issuer: &str,
        ) -> Result<common::TenantConfig, tenant::TenantRepositoryError> {
            Ok(common::TenantConfig {
                tenant_id: TenantId(Uuid::new_v4()),
                name: name.to_string(),
                oidc_issuer: oidc_issuer.to_string(),
                active: true,
            })
        }

        async fn get_by_issuer(
            &self,
            _iss: &str,
        ) -> Result<Option<common::TenantConfig>, tenant::TenantRepositoryError> {
            Ok(None)
        }

        async fn get_by_id(
            &self,
            tenant_id: TenantId,
        ) -> Result<Option<common::TenantConfig>, tenant::TenantRepositoryError> {
            Ok(Some(common::TenantConfig {
                tenant_id,
                name: "Mock Tenant".to_string(),
                oidc_issuer: "https://mock.example.com".to_string(),
                active: true,
            }))
        }

        async fn deactivate_tenant(
            &self,
            _tenant_id: TenantId,
        ) -> Result<(), tenant::TenantRepositoryError> {
            Ok(())
        }

        async fn update_oidc_issuer(
            &self,
            _tenant_id: TenantId,
            _new_issuer: &str,
        ) -> Result<(), tenant::TenantRepositoryError> {
            Ok(())
        }

        async fn get_usage(
            &self,
            _tenant_id: TenantId,
        ) -> Result<tenant::TenantUsage, tenant::TenantRepositoryError> {
            Ok(tenant::TenantUsage {
                user_count: 0,
                device_count: 0,
                message_count_30d: 0,
                active_wt_sessions: 0,
            })
        }
    }

    struct MockKdsRepo;

    #[async_trait]
    impl KdsRepository for MockKdsRepo {
        async fn register_device(
            &self,
            _tenant_id: TenantId,
            _user_id: UserId,
            _bundle: KeyBundle,
        ) -> Result<DeviceId, KdsError> {
            Ok(DeviceId(Uuid::new_v4()))
        }

        async fn fetch_key_bundle(
            &self,
            _tenant_id: TenantId,
            _user_id: UserId,
        ) -> Result<KeyBundleResponse, KdsError> {
            Err(KdsError::NotFound)
        }

        async fn replenish_otpks(
            &self,
            _tenant_id: TenantId,
            _device_id: DeviceId,
            _keys: Vec<OneTimePreKey>,
        ) -> Result<i64, KdsError> {
            Ok(0)
        }

        async fn rotate_signed_prekey(
            &self,
            _tenant_id: TenantId,
            _device_id: DeviceId,
            _signed_prekey_id: u64,
            _signed_prekey: Curve25519PublicKey,
            _signed_prekey_sig: Ed25519Signature,
        ) -> Result<(), KdsError> {
            Ok(())
        }

        async fn get_otpk_count(
            &self,
            _tenant_id: TenantId,
            _device_id: DeviceId,
        ) -> Result<i64, KdsError> {
            Ok(0)
        }

        async fn get_device_count(
            &self,
            _tenant_id: TenantId,
            _user_id: UserId,
        ) -> Result<i64, KdsError> {
            Ok(0)
        }

        async fn get_identity_key(
            &self,
            _tenant_id: TenantId,
            _device_id: DeviceId,
        ) -> Result<Curve25519PublicKey, KdsError> {
            Err(KdsError::NotFound)
        }
    }

    struct MockMessagingRepo;

    #[async_trait]
    impl MessagingRepository for MockMessagingRepo {
        async fn find_direct_conversation(
            &self,
            _tenant_id: TenantId,
            _user_a: &UserId,
            _user_b: &UserId,
        ) -> Result<Option<ConversationId>, MessagingError> {
            Ok(None)
        }

        async fn create_direct_conversation(
            &self,
            _tenant_id: TenantId,
            _user_a: UserId,
            _device_a: DeviceId,
            _user_b: UserId,
            _device_b: DeviceId,
        ) -> Result<ConversationId, MessagingError> {
            Ok(ConversationId(Uuid::new_v4()))
        }

        async fn is_participant(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _user_id: &UserId,
        ) -> Result<bool, MessagingError> {
            Ok(true)
        }

        async fn store_envelope(
            &self,
            _tenant_id: TenantId,
            _envelope: NewMessageEnvelope,
        ) -> Result<MessageEnvelope, MessagingError> {
            Err(MessagingError::ConversationNotFound)
        }

        async fn get_messages(
            &self,
            _tenant_id: TenantId,
            _params: GetMessagesParams,
        ) -> Result<Vec<MessageEnvelope>, MessagingError> {
            Ok(vec![])
        }

        async fn mark_delivered(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _device_id: DeviceId,
            _seq: u64,
        ) -> Result<(), MessagingError> {
            Ok(())
        }

        async fn enqueue_offline(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _device_id: DeviceId,
            _seq: u64,
        ) -> Result<EnqueueResult, MessagingError> {
            Ok(EnqueueResult::Queued)
        }

        async fn drain_offline_queue(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _device_id: DeviceId,
        ) -> Result<Vec<MessageEnvelope>, MessagingError> {
            Ok(vec![])
        }
    }

    struct MockGroupRepo;

    #[async_trait]
    impl GroupRepository for MockGroupRepo {
        async fn create_group(
            &self,
            _tenant_id: TenantId,
            initial_members: Vec<ConversationMember>,
        ) -> Result<(ConversationId, Vec<ConversationMember>), GroupError> {
            Ok((ConversationId(Uuid::new_v4()), initial_members))
        }

        async fn add_member(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _user_id: UserId,
            _device_id: DeviceId,
        ) -> Result<(), GroupError> {
            Ok(())
        }

        async fn remove_member(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _user_id: UserId,
        ) -> Result<(), GroupError> {
            Ok(())
        }

        async fn is_member(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _user_id: UserId,
        ) -> Result<bool, GroupError> {
            Ok(false)
        }

        async fn get_members(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
        ) -> Result<Vec<ConversationMember>, GroupError> {
            Ok(vec![])
        }

        async fn store_skdm(
            &self,
            _tenant_id: TenantId,
            _conversation_id: ConversationId,
            _sender_user_id: UserId,
            _sender_device_id: DeviceId,
            _recipients: Vec<(UserId, DeviceId, Vec<u8>)>,
        ) -> Result<(), GroupError> {
            Ok(())
        }
    }
}
