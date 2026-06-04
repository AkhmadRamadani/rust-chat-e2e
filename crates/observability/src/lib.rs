// crates/observability — Prometheus metrics, health check, and structured tracing.
//
// Task 12.1: `MetricsRegistry` with all Prometheus counters, gauges, and histograms.
// Task 12.2: `GET /metrics` Axum handler (Prometheus text exposition format).
// Task 12.3: `check_health` + `GET /health` Axum handler.
// Task 12.4: `TracingLayer` Tower middleware + `init_tracing` initializer.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use prometheus_client::{
    encoding::EncodeLabelSet,
    metrics::{
        counter::Counter,
        family::Family,
        gauge::Gauge,
        histogram::{exponential_buckets, Histogram},
    },
    registry::Registry,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower::{Layer, Service};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Task 12.1: MetricsRegistry
// ─────────────────────────────────────────────────────────────────────────────

// ── Label sets ────────────────────────────────────────────────────────────────

/// Labels carrying only a `tenant_id` dimension.
///
/// Used by `messages_received`, `messages_delivered`, `wt_sessions_active`,
/// and `auth_failures`.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct TenantLabels {
    pub tenant_id: String,
}

/// Labels carrying both `tenant_id` and `device_id` dimensions.
///
/// Used by `otpk_pool_level`.
#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct TenantDeviceLabels {
    pub tenant_id: String,
    pub device_id: String,
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Holds every Prometheus metric the server exposes.
///
/// All per-tenant metrics carry a `tenant_id` label. The struct is cheaply
/// clonable via the inner `Arc` so it can be shared between the `/metrics`
/// handler and the Tower tracing middleware.
///
/// Requirements: 11.4
#[derive(Clone)]
pub struct MetricsRegistry {
    inner: Arc<MetricsInner>,
}

struct MetricsInner {
    /// The underlying `prometheus-client` registry.
    pub registry: RwLock<Registry>,

    // Counters
    pub messages_received: Family<TenantLabels, Counter>,
    pub messages_delivered: Family<TenantLabels, Counter>,
    pub auth_failures: Family<TenantLabels, Counter>,

    // Gauges
    pub wt_sessions_active: Family<TenantLabels, Gauge>,
    pub otpk_pool_level: Family<TenantDeviceLabels, Gauge>,

    // Histograms
    pub request_latency_ms: Histogram,
}

impl MetricsRegistry {
    /// Create a new `MetricsRegistry` with all metrics registered.
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let messages_received = Family::<TenantLabels, Counter>::default();
        registry.register(
            "messages_received",
            "Total number of messages received by the server",
            messages_received.clone(),
        );

        let messages_delivered = Family::<TenantLabels, Counter>::default();
        registry.register(
            "messages_delivered",
            "Total number of messages delivered to a device",
            messages_delivered.clone(),
        );

        let auth_failures = Family::<TenantLabels, Counter>::default();
        registry.register(
            "auth_failures",
            "Total number of authentication failures (HTTP 401)",
            auth_failures.clone(),
        );

        let wt_sessions_active = Family::<TenantLabels, Gauge>::default();
        registry.register(
            "wt_sessions_active",
            "Number of currently active WebTransport sessions",
            wt_sessions_active.clone(),
        );

        let otpk_pool_level = Family::<TenantDeviceLabels, Gauge>::default();
        registry.register(
            "otpk_pool_level",
            "Current one-time pre-key pool level for a device",
            otpk_pool_level.clone(),
        );

        // Exponential buckets: 1, 2, 4, 8, … ms — 16 buckets covers up to ~32 s.
        let request_latency_ms = Histogram::new(exponential_buckets(1.0, 2.0, 16));
        registry.register(
            "request_latency_ms",
            "HTTP request latency in milliseconds",
            request_latency_ms.clone(),
        );

        MetricsRegistry {
            inner: Arc::new(MetricsInner {
                registry: RwLock::new(registry),
                messages_received,
                messages_delivered,
                auth_failures,
                wt_sessions_active,
                otpk_pool_level,
                request_latency_ms,
            }),
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Increment `messages_received` for the given tenant.
    pub fn inc_messages_received(&self, tenant_id: &str) {
        self.inner
            .messages_received
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .inc();
    }

    /// Increment `messages_delivered` for the given tenant.
    pub fn inc_messages_delivered(&self, tenant_id: &str) {
        self.inner
            .messages_delivered
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .inc();
    }

    /// Increment `auth_failures` for the given tenant.
    pub fn inc_auth_failures(&self, tenant_id: &str) {
        self.inner
            .auth_failures
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .inc();
    }

    /// Increment `wt_sessions_active` when a WebTransport session is opened.
    pub fn inc_wt_sessions(&self, tenant_id: &str) {
        self.inner
            .wt_sessions_active
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .inc();
    }

    /// Decrement `wt_sessions_active` when a WebTransport session is closed.
    pub fn dec_wt_sessions(&self, tenant_id: &str) {
        self.inner
            .wt_sessions_active
            .get_or_create(&TenantLabels { tenant_id: tenant_id.to_string() })
            .dec();
    }

    /// Set `otpk_pool_level` for a specific (tenant, device) pair.
    pub fn set_otpk_pool_level(&self, tenant_id: &str, device_id: &str, level: i64) {
        self.inner
            .otpk_pool_level
            .get_or_create(&TenantDeviceLabels {
                tenant_id: tenant_id.to_string(),
                device_id: device_id.to_string(),
            })
            .set(level);
    }

    /// Record a request latency observation (in milliseconds).
    pub fn observe_request_latency(&self, latency_ms: f64) {
        self.inner.request_latency_ms.observe(latency_ms);
    }

    /// Provide read access to the underlying `prometheus-client` registry so
    /// the `/metrics` handler can encode and serve it.
    pub fn with_registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Registry) -> R,
    {
        let guard = self.inner.registry.read().expect("registry lock poisoned");
        f(&*guard)
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 12.2: GET /metrics handler
// ─────────────────────────────────────────────────────────────────────────────

/// Axum handler for `GET /metrics`.
///
/// Encodes the `MetricsRegistry` in Prometheus text exposition format and
/// returns it with `Content-Type: text/plain; version=0.0.4; charset=utf-8`.
///
/// Requirements: 11.4
pub async fn metrics_handler(
    axum::extract::State(registry): axum::extract::State<MetricsRegistry>,
) -> Response {
    use axum::http::header;
    use prometheus_client::encoding::text::encode;

    let mut buffer = String::new();

    match registry.with_registry(|reg| encode(&mut buffer, reg)) {
        Ok(()) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
            buffer,
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to encode Prometheus metrics");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to encode metrics").into_response()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 12.3: Health check
// ─────────────────────────────────────────────────────────────────────────────

/// Status of a single subsystem in the health response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubsystemStatus {
    Ok,
    Error,
}

impl SubsystemStatus {
    fn is_ok(&self) -> bool {
        *self == SubsystemStatus::Ok
    }
}

/// JSON body returned by `GET /health`.
///
/// - `quic_listener`  — static "ok" (the request arriving proves the listener is up).
/// - `kds_storage`    — probes PostgreSQL with `SELECT 1`.
/// - `message_queue`  — probes Redis with `PING`.
///
/// Requirements: 11.2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub quic_listener: SubsystemStatus,
    pub kds_storage: SubsystemStatus,
    pub message_queue: SubsystemStatus,
}

/// Probe PostgreSQL and Redis and return a populated [`HealthResponse`].
///
/// Requirements: 11.2
pub async fn check_health(pool: &PgPool, redis_client: &redis::Client) -> HealthResponse {
    // PostgreSQL probe
    let kds_storage = match sqlx::query("SELECT 1").execute(pool).await {
        Ok(_) => SubsystemStatus::Ok,
        Err(e) => {
            tracing::warn!(error = %e, "health: PostgreSQL probe failed");
            SubsystemStatus::Error
        }
    };

    // Redis probe
    let message_queue = match redis_client.get_multiplexed_async_connection().await {
        Ok(mut conn) => {
            let pong: Result<String, _> = redis::cmd("PING").query_async(&mut conn).await;
            match pong {
                Ok(_) => SubsystemStatus::Ok,
                Err(e) => {
                    tracing::warn!(error = %e, "health: Redis PING failed");
                    SubsystemStatus::Error
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "health: Redis connection failed");
            SubsystemStatus::Error
        }
    };

    HealthResponse {
        quic_listener: SubsystemStatus::Ok,
        kds_storage,
        message_queue,
    }
}

/// Axum shared state for the health handler.
#[derive(Clone)]
pub struct HealthState {
    pub pool: PgPool,
    pub redis: redis::Client,
}

/// Axum handler for `GET /health`.
///
/// Returns HTTP 200 when all subsystems are healthy, HTTP 503 when a critical
/// backend reports an error. The `quic_listener` field is always `"ok"`.
///
/// Requirements: 11.2
pub async fn health_handler(
    axum::extract::State(state): axum::extract::State<HealthState>,
) -> Response {
    let health = check_health(&state.pool, &state.redis).await;

    let status = if health.kds_storage.is_ok() && health.message_queue.is_ok() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status, Json(health)).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 12.4: Structured JSON request tracing middleware
// ─────────────────────────────────────────────────────────────────────────────

/// Tower [`Layer`] that instruments each request with a structured tracing span.
///
/// Each span contains:
/// - `request_id` (UUID v4, threaded through all layers)
/// - `path` (HTTP request path)
/// - `tenant_id`, `user_id`, `device_id` (from `AuthenticatedUser` extension, if present)
/// - `http_status` and `latency_ms` (recorded after the response)
///
/// Requirements: 11.1, 11.3
#[derive(Debug, Clone, Default)]
pub struct TracingLayer;

impl TracingLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TracingLayer {
    type Service = TracingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TracingService { inner }
    }
}

/// Tower [`Service`] produced by [`TracingLayer`].
#[derive(Debug, Clone)]
pub struct TracingService<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for TracingService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let request_id = Uuid::new_v4();
        let path = req.uri().path().to_string();

        // Extract identity fields from AuthenticatedUser if present (auth routes
        // inject this; /health and /metrics do not).
        let auth_user = req.extensions().get::<auth::AuthenticatedUser>().cloned();
        let tenant_id = auth_user
            .as_ref()
            .map(|u| u.tenant_id.0.to_string())
            .unwrap_or_else(|| "none".to_string());
        let user_id = auth_user
            .as_ref()
            .map(|u| u.user_id.0.clone())
            .unwrap_or_else(|| "none".to_string());
        let device_id = auth_user
            .as_ref()
            .and_then(|u| u.device_id.map(|d| d.0.to_string()))
            .unwrap_or_else(|| "none".to_string());

        let span = tracing::info_span!(
            "http_request",
            request_id = %request_id,
            path        = %path,
            tenant_id   = %tenant_id,
            user_id     = %user_id,
            device_id   = %device_id,
            http_status = tracing::field::Empty,
            latency_ms  = tracing::field::Empty,
        );

        let mut inner = self.inner.clone();
        let start = Instant::now();

        Box::pin(async move {
            let result = {
                use tracing::Instrument as _;
                inner.call(req).instrument(span.clone()).await
            };

            let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
            let http_status = match &result {
                Ok(resp) => resp.status().as_u16(),
                Err(_) => 500,
            };

            span.record("http_status", http_status);
            span.record("latency_ms", latency_ms);

            result
        })
    }
}

/// Initialize the global `tracing-subscriber` with JSON output.
///
/// Call once at application startup before any tracing events are emitted.
/// Respects the `RUST_LOG` environment variable; defaults to `info`.
///
/// Requirements: 11.1, 11.3
pub fn init_tracing() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true)
        .with_span_list(false);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(json_layer)
        .init();
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registry() -> MetricsRegistry {
        MetricsRegistry::new()
    }

    #[test]
    fn messages_received_increments() {
        let r = make_registry();
        r.inc_messages_received("tenant-a");
        r.inc_messages_received("tenant-a");
        r.inc_messages_received("tenant-b");

        let a = r
            .inner
            .messages_received
            .get_or_create(&TenantLabels { tenant_id: "tenant-a".into() })
            .get();
        let b = r
            .inner
            .messages_received
            .get_or_create(&TenantLabels { tenant_id: "tenant-b".into() })
            .get();
        assert_eq!(a, 2);
        assert_eq!(b, 1);
    }

    #[test]
    fn messages_delivered_increments() {
        let r = make_registry();
        r.inc_messages_delivered("tenant-x");
        let v = r
            .inner
            .messages_delivered
            .get_or_create(&TenantLabels { tenant_id: "tenant-x".into() })
            .get();
        assert_eq!(v, 1);
    }

    #[test]
    fn auth_failures_increments() {
        let r = make_registry();
        r.inc_auth_failures("tenant-a");
        r.inc_auth_failures("tenant-a");
        let v = r
            .inner
            .auth_failures
            .get_or_create(&TenantLabels { tenant_id: "tenant-a".into() })
            .get();
        assert_eq!(v, 2);
    }

    #[test]
    fn wt_sessions_inc_dec() {
        let r = make_registry();
        r.inc_wt_sessions("tenant-a");
        r.inc_wt_sessions("tenant-a");
        r.dec_wt_sessions("tenant-a");
        let v = r
            .inner
            .wt_sessions_active
            .get_or_create(&TenantLabels { tenant_id: "tenant-a".into() })
            .get();
        assert_eq!(v, 1);
    }

    #[test]
    fn otpk_pool_level_set() {
        let r = make_registry();
        r.set_otpk_pool_level("tenant-a", "device-1", 42);
        r.set_otpk_pool_level("tenant-a", "device-1", 7);
        let v = r
            .inner
            .otpk_pool_level
            .get_or_create(&TenantDeviceLabels {
                tenant_id: "tenant-a".into(),
                device_id: "device-1".into(),
            })
            .get();
        assert_eq!(v, 7);
    }

    #[test]
    fn request_latency_observe_does_not_panic() {
        let r = make_registry();
        r.observe_request_latency(1.0);
        r.observe_request_latency(50.0);
        r.observe_request_latency(2000.0);
    }

    #[test]
    fn registry_is_clonable_and_shared() {
        let r = make_registry();
        let r2 = r.clone();
        r.inc_messages_received("tenant-clone");
        let v = r2
            .inner
            .messages_received
            .get_or_create(&TenantLabels { tenant_id: "tenant-clone".into() })
            .get();
        assert_eq!(v, 1);
    }

    #[test]
    fn tenant_labels_are_isolated() {
        let r = make_registry();
        r.inc_auth_failures("tenant-a");
        r.inc_auth_failures("tenant-a");
        r.inc_auth_failures("tenant-b");
        let a = r
            .inner
            .auth_failures
            .get_or_create(&TenantLabels { tenant_id: "tenant-a".into() })
            .get();
        let b = r
            .inner
            .auth_failures
            .get_or_create(&TenantLabels { tenant_id: "tenant-b".into() })
            .get();
        assert_eq!(a, 2);
        assert_eq!(b, 1);
    }

    // ── Health check tests ────────────────────────────────────────────────────

    #[test]
    fn health_response_serialises_all_ok() {
        let resp = HealthResponse {
            quic_listener: SubsystemStatus::Ok,
            kds_storage: SubsystemStatus::Ok,
            message_queue: SubsystemStatus::Ok,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["quic_listener"], "ok");
        assert_eq!(json["kds_storage"], "ok");
        assert_eq!(json["message_queue"], "ok");
    }

    #[test]
    fn health_response_serialises_degraded() {
        let resp = HealthResponse {
            quic_listener: SubsystemStatus::Ok,
            kds_storage: SubsystemStatus::Error,
            message_queue: SubsystemStatus::Error,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["kds_storage"], "error");
        assert_eq!(json["message_queue"], "error");
    }

    #[test]
    fn health_response_round_trips() {
        let original = HealthResponse {
            quic_listener: SubsystemStatus::Ok,
            kds_storage: SubsystemStatus::Ok,
            message_queue: SubsystemStatus::Error,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(original.quic_listener, restored.quic_listener);
        assert_eq!(original.kds_storage, restored.kds_storage);
        assert_eq!(original.message_queue, restored.message_queue);
    }

    #[test]
    fn subsystem_status_is_ok() {
        assert!(SubsystemStatus::Ok.is_ok());
        assert!(!SubsystemStatus::Error.is_ok());
    }

    // ── TracingService smoke test ─────────────────────────────────────────────

    #[tokio::test]
    async fn tracing_service_passes_request_through() {
        use tower::ServiceExt;

        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, std::convert::Infallible>(
                axum::http::Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap(),
            )
        });

        let mut svc = TracingLayer::new().layer(inner);
        let req = Request::builder().uri("/ping").body(Body::empty()).unwrap();
        let resp = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn tracing_service_handles_authenticated_user() {
        use auth::AuthenticatedUser;
        use common::{DeviceId, TenantId, UserId};
        use tower::ServiceExt;

        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, std::convert::Infallible>(
                axum::http::Response::builder()
                    .status(StatusCode::CREATED)
                    .body(Body::empty())
                    .unwrap(),
            )
        });

        let mut svc = TracingLayer::new().layer(inner);

        let mut req = Request::builder()
            .uri("/conversations")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(AuthenticatedUser {
            tenant_id: TenantId(Uuid::new_v4()),
            user_id: UserId("user-123".to_string()),
            device_id: Some(DeviceId(Uuid::new_v4())),
        });

        let resp = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}
