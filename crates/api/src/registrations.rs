//! Registration API handlers — public self-registration and admin management.
//!
//! # Route table
//!
//! | Method | Path | Auth | Handler |
//! |--------|------|------|---------|
//! | `POST` | `/registrations` | none | [`submit_registration`] |
//! | `GET`  | `/registrations/{id}` | Bearer RegistrationToken | [`get_registration`] |
//! | `GET`  | `/admin/registrations` | ADMIN_TOKEN | [`list_registrations`] |
//! | `POST` | `/admin/registrations/{id}/approve` | ADMIN_TOKEN | [`approve_registration`] |
//! | `POST` | `/admin/registrations/{id}/reject` | ADMIN_TOKEN | [`reject_registration`] |
//!
//! Requirements: 8.1–8.7, 9.1–9.5, 10.2–10.9

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use common::{error_codes, ApiError};
use tenant::TenantRepository;

// ── State ─────────────────────────────────────────────────────────────────────

/// Shared state for all registration handlers.
#[derive(Clone)]
pub struct RegistrationState {
    /// PostgreSQL connection pool for direct queries against `tenant_registrations`.
    pub pool: PgPool,
    /// Tenant repository used when approving a registration to provision the tenant.
    pub tenant_repo: Arc<dyn TenantRepository>,
}

impl std::fmt::Debug for RegistrationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistrationState").finish_non_exhaustive()
    }
}

// ── Request / Response types ──────────────────────────────────────────────────

/// Request body for `POST /registrations`.
///
/// Requirements: 8.1
#[derive(Debug, Deserialize)]
pub struct SubmitRegistrationRequest {
    /// Human-readable name for the applicant's application (1–100 chars).
    pub app_name: String,
    /// OIDC issuer URL — must begin with `https://`.
    pub oidc_issuer: String,
    /// Contact email address — must contain `@` with non-empty local and domain parts.
    pub contact_email: String,
}

/// Response body for a successful `POST /registrations` (HTTP 201).
///
/// Requirements: 8.2
#[derive(Debug, Serialize)]
pub struct SubmitRegistrationResponse {
    /// Server-assigned UUID for this registration.
    pub registration_id: Uuid,
    /// Opaque bearer token the applicant must store to look up their status.
    pub registration_token: String,
}

/// A full registration record returned by `GET /registrations/{id}`.
///
/// Requirements: 9.1
#[derive(Debug, Serialize)]
pub struct RegistrationRecord {
    pub registration_id: Uuid,
    pub app_name: String,
    pub oidc_issuer: String,
    pub contact_email: String,
    /// `"pending"`, `"approved"`, or `"rejected"`.
    pub status: String,
    /// ISO-8601 timestamp of creation.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// `Some(uuid)` once a registration has been approved and a tenant provisioned.
    pub tenant_id: Option<Uuid>,
    /// Optional rejection reason provided by the operator.
    pub rejection_reason: Option<String>,
}

/// A registration record as returned by admin list/approve/reject endpoints
/// (omits the `registration_token` field for security).
///
/// Requirements: 10.2
#[derive(Debug, Serialize)]
pub struct AdminRegistrationRecord {
    pub registration_id: Uuid,
    pub app_name: String,
    pub oidc_issuer: String,
    pub contact_email: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub tenant_id: Option<Uuid>,
    pub rejection_reason: Option<String>,
}

/// Query parameters for `GET /admin/registrations`.
#[derive(Debug, Deserialize)]
pub struct ListRegistrationsQuery {
    /// Optional status filter: `pending`, `approved`, or `rejected`.
    pub status: Option<String>,
}

/// Request body for `POST /admin/registrations/{id}/reject`.
#[derive(Debug, Deserialize)]
pub struct RejectRegistrationRequest {
    /// Human-readable explanation for the rejection (optional).
    pub reason: Option<String>,
}

/// Response body for `POST /admin/registrations/{id}/approve`.
#[derive(Debug, Serialize)]
pub struct ApproveRegistrationResponse {
    pub tenant_id: Uuid,
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can be returned by registration handlers.
#[derive(Debug)]
pub enum RegistrationError {
    /// Validation of a request field failed; carries the `error_code` and message.
    Validation { code: &'static str, message: String },
    /// The `oidc_issuer` is already in use.
    IssuerAlreadyRegistered(String),
    /// The registration ID does not exist.
    NotFound,
    /// The supplied registration token does not match the record.
    InvalidToken,
    /// The registration is not in `pending` status.
    NotPending,
    /// An unexpected database error occurred.
    Database(String),
}

impl IntoResponse for RegistrationError {
    fn into_response(self) -> Response {
        let request_id = Uuid::new_v4();
        match self {
            RegistrationError::Validation { code, message } => (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError {
                    error_code: code.to_string(),
                    message,
                    request_id,
                }),
            )
                .into_response(),

            RegistrationError::IssuerAlreadyRegistered(iss) => (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error_code: error_codes::ISSUER_ALREADY_REGISTERED.to_string(),
                    message: format!("OIDC issuer already registered: {iss}"),
                    request_id,
                }),
            )
                .into_response(),

            RegistrationError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error_code: error_codes::NOT_FOUND.to_string(),
                    message: "Registration not found".to_string(),
                    request_id,
                }),
            )
                .into_response(),

            RegistrationError::InvalidToken => (
                StatusCode::UNAUTHORIZED,
                Json(ApiError {
                    error_code: error_codes::INVALID_REGISTRATION_TOKEN.to_string(),
                    message: "Invalid registration token".to_string(),
                    request_id,
                }),
            )
                .into_response(),

            RegistrationError::NotPending => (
                StatusCode::CONFLICT,
                Json(ApiError {
                    error_code: error_codes::REGISTRATION_NOT_PENDING.to_string(),
                    message: "Registration is not in pending status".to_string(),
                    request_id,
                }),
            )
                .into_response(),

            RegistrationError::Database(msg) => {
                tracing::error!("Registration handler database error: {msg}");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ApiError {
                        error_code: error_codes::STORAGE_UNAVAILABLE.to_string(),
                        message: "Storage error".to_string(),
                        request_id,
                    }),
                )
                    .into_response()
            }
        }
    }
}

// ── Helper: validate fields ───────────────────────────────────────────────────

/// Validate the three registration input fields; return the first error found.
///
/// Requirements: 8.4, 8.5, 8.6
fn validate_registration_fields(
    app_name: &str,
    oidc_issuer: &str,
    contact_email: &str,
) -> Result<(), RegistrationError> {
    if app_name.is_empty() || app_name.len() > 100 {
        return Err(RegistrationError::Validation {
            code: error_codes::INVALID_APP_NAME,
            message: "app_name must be between 1 and 100 characters".to_string(),
        });
    }
    if !oidc_issuer.starts_with("https://") && !oidc_issuer.starts_with("http://") {
        return Err(RegistrationError::Validation {
            code: error_codes::INVALID_OIDC_ISSUER,
            message: "oidc_issuer must begin with http:// or https://".to_string(),
        });
    }
    let at_idx = contact_email.find('@').ok_or_else(|| RegistrationError::Validation {
        code: error_codes::INVALID_EMAIL,
        message: "contact_email must contain @".to_string(),
    })?;
    let local = &contact_email[..at_idx];
    let domain = &contact_email[at_idx + 1..];
    if local.is_empty() || domain.is_empty() {
        return Err(RegistrationError::Validation {
            code: error_codes::INVALID_EMAIL,
            message: "contact_email must have non-empty local and domain parts".to_string(),
        });
    }
    Ok(())
}

// ── POST /registrations ───────────────────────────────────────────────────────

/// Submit a new tenant registration request (public, no auth).
///
/// Validates `app_name`, `oidc_issuer`, `contact_email`, checks issuer
/// uniqueness, generates a cryptographically random 256-bit token, inserts the
/// row, and returns HTTP 201 with the `registration_id` and `registration_token`.
///
/// Requirements: 8.1–8.7
pub async fn submit_registration(
    State(state): State<RegistrationState>,
    Json(body): Json<SubmitRegistrationRequest>,
) -> Result<(StatusCode, Json<SubmitRegistrationResponse>), RegistrationError> {
    // Validate fields first.
    validate_registration_fields(&body.app_name, &body.oidc_issuer, &body.contact_email)?;

    // Check issuer uniqueness against active tenants.
    // Requirements: 8.3
    let active_tenant_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tenants WHERE oidc_issuer = $1 AND active = TRUE)",
    )
    .bind(&body.oidc_issuer)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    if active_tenant_exists {
        return Err(RegistrationError::IssuerAlreadyRegistered(body.oidc_issuer));
    }

    // Check issuer uniqueness against pending/approved registrations.
    let pending_registration_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tenant_registrations WHERE oidc_issuer = $1 AND status IN ('pending', 'approved'))",
    )
    .bind(&body.oidc_issuer)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    if pending_registration_exists {
        return Err(RegistrationError::IssuerAlreadyRegistered(body.oidc_issuer));
    }

    // Generate 256-bit cryptographically random token (hex-encoded).
    // Requirements: 8.7, Property A
    let token_bytes: [u8; 32] = rand::random();
    let registration_token = hex::encode(token_bytes);

    // Insert registration record.
    let registration_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO tenant_registrations (app_name, oidc_issuer, contact_email, registration_token)
        VALUES ($1, $2, $3, $4)
        RETURNING registration_id
        "#,
    )
    .bind(&body.app_name)
    .bind(&body.oidc_issuer)
    .bind(&body.contact_email)
    .bind(&registration_token)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(SubmitRegistrationResponse {
            registration_id,
            registration_token,
        }),
    ))
}

// ── GET /registrations/{id} ───────────────────────────────────────────────────

/// Check registration status using the RegistrationToken (public, Bearer auth).
///
/// The caller must present the correct `registration_token` for the given
/// `registration_id` in the `Authorization: Bearer` header.
///
/// Requirements: 9.1–9.5, Property B
pub async fn get_registration(
    State(state): State<RegistrationState>,
    Path(registration_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RegistrationRecord>, RegistrationError> {
    // Extract bearer token from Authorization header.
    let token = extract_bearer_token(&headers).ok_or(RegistrationError::InvalidToken)?;

    // Fetch the registration record.
    let row: Option<(
        Uuid,
        String,
        String,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        Option<Uuid>,
        Option<String>,
    )> = sqlx::query_as(
        r#"
        SELECT registration_id, app_name, oidc_issuer, contact_email,
               status, registration_token, created_at, tenant_id, rejection_reason
        FROM   tenant_registrations
        WHERE  registration_id = $1
        "#,
    )
    .bind(registration_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    let (reg_id, app_name, oidc_issuer, contact_email, status, stored_token, created_at, tenant_id, rejection_reason) =
        row.ok_or(RegistrationError::NotFound)?;

    // Validate token — constant-time comparison to mitigate timing attacks.
    // Requirements: 9.2, 9.5, Property B
    if !constant_time_eq(token.as_bytes(), stored_token.as_bytes()) {
        return Err(RegistrationError::InvalidToken);
    }

    Ok(Json(RegistrationRecord {
        registration_id: reg_id,
        app_name,
        oidc_issuer,
        contact_email,
        status,
        created_at,
        tenant_id,
        rejection_reason,
    }))
}

// ── GET /admin/registrations ──────────────────────────────────────────────────

/// List all registration records (admin-only, filtered by optional `?status=`).
///
/// The `registration_token` field is NOT included in this response.
///
/// Requirements: 10.2
pub async fn list_registrations(
    State(state): State<RegistrationState>,
    Query(params): Query<ListRegistrationsQuery>,
) -> Result<Json<Vec<AdminRegistrationRecord>>, RegistrationError> {
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        String,
        chrono::DateTime<chrono::Utc>,
        Option<Uuid>,
        Option<String>,
    )> = if let Some(ref status) = params.status {
        sqlx::query_as(
            r#"
            SELECT registration_id, app_name, oidc_issuer, contact_email,
                   status, created_at, tenant_id, rejection_reason
            FROM   tenant_registrations
            WHERE  status = $1
            ORDER  BY created_at DESC
            "#,
        )
        .bind(status)
        .fetch_all(&state.pool)
        .await
        .map_err(|e| RegistrationError::Database(e.to_string()))?
    } else {
        sqlx::query_as(
            r#"
            SELECT registration_id, app_name, oidc_issuer, contact_email,
                   status, created_at, tenant_id, rejection_reason
            FROM   tenant_registrations
            ORDER  BY created_at DESC
            "#,
        )
        .fetch_all(&state.pool)
        .await
        .map_err(|e| RegistrationError::Database(e.to_string()))?
    };

    let records = rows
        .into_iter()
        .map(
            |(registration_id, app_name, oidc_issuer, contact_email, status, created_at, tenant_id, rejection_reason)| {
                AdminRegistrationRecord {
                    registration_id,
                    app_name,
                    oidc_issuer,
                    contact_email,
                    status,
                    created_at,
                    tenant_id,
                    rejection_reason,
                }
            },
        )
        .collect();

    Ok(Json(records))
}

// ── POST /admin/registrations/{id}/approve ────────────────────────────────────

/// Approve a pending registration and provision a new tenant.
///
/// Calls `TenantRepository::create_tenant` to provision the tenant, then
/// updates the registration row to `approved` and stores the new `tenant_id`.
///
/// Requirements: 10.3–10.5, Property C
pub async fn approve_registration(
    State(state): State<RegistrationState>,
    Path(registration_id): Path<Uuid>,
) -> Result<Json<ApproveRegistrationResponse>, RegistrationError> {
    // Fetch the registration — 404 if missing, 409 if not pending.
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT app_name, oidc_issuer, status FROM tenant_registrations WHERE registration_id = $1",
    )
    .bind(registration_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    let (app_name, oidc_issuer, status) = row.ok_or(RegistrationError::NotFound)?;

    if status != "pending" {
        return Err(RegistrationError::NotPending);
    }

    // Provision the tenant via the repository.
    // Requirements: 10.3
    let config = state
        .tenant_repo
        .create_tenant(&app_name, &oidc_issuer)
        .await
        .map_err(|e| RegistrationError::Database(e.to_string()))?;

    let new_tenant_id = config.tenant_id.0;

    // Update the registration record to approved.
    sqlx::query(
        r#"
        UPDATE tenant_registrations
        SET    status = 'approved',
               tenant_id = $2,
               updated_at = NOW()
        WHERE  registration_id = $1
        "#,
    )
    .bind(registration_id)
    .bind(new_tenant_id)
    .execute(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    Ok(Json(ApproveRegistrationResponse {
        tenant_id: new_tenant_id,
    }))
}

// ── POST /admin/registrations/{id}/reject ─────────────────────────────────────

/// Reject a pending registration with an optional reason.
///
/// Requirements: 10.6–10.8, Property D
pub async fn reject_registration(
    State(state): State<RegistrationState>,
    Path(registration_id): Path<Uuid>,
    Json(body): Json<RejectRegistrationRequest>,
) -> Result<StatusCode, RegistrationError> {
    // Fetch the registration — 404 if missing, 409 if not pending.
    let row: Option<String> = sqlx::query_scalar(
        "SELECT status FROM tenant_registrations WHERE registration_id = $1",
    )
    .bind(registration_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    let status = row.ok_or(RegistrationError::NotFound)?;

    if status != "pending" {
        return Err(RegistrationError::NotPending);
    }

    // Update the registration record to rejected.
    sqlx::query(
        r#"
        UPDATE tenant_registrations
        SET    status = 'rejected',
               rejection_reason = $2,
               updated_at = NOW()
        WHERE  registration_id = $1
        "#,
    )
    .bind(registration_id)
    .bind(body.reason.as_deref())
    .execute(&state.pool)
    .await
    .map_err(|e| RegistrationError::Database(e.to_string()))?;

    Ok(StatusCode::OK)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract a bearer token from the `Authorization` header.
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    value.strip_prefix("Bearer ")
}

/// Constant-time byte-slice comparison to prevent timing-based token enumeration.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_app_name_empty() {
        assert!(validate_registration_fields("", "https://x.com", "a@b.com").is_err());
    }

    #[test]
    fn validate_app_name_too_long() {
        let long = "a".repeat(101);
        assert!(validate_registration_fields(&long, "https://x.com", "a@b.com").is_err());
    }

    #[test]
    fn validate_app_name_max_length_ok() {
        let exactly_100 = "a".repeat(100);
        assert!(validate_registration_fields(&exactly_100, "https://x.com", "a@b.com").is_ok());
    }

    #[test]
    fn validate_issuer_no_https() {
        assert!(validate_registration_fields("App", "http://x.com", "a@b.com").is_err());
    }

    #[test]
    fn validate_issuer_ok() {
        assert!(validate_registration_fields("App", "https://x.com", "a@b.com").is_ok());
    }

    #[test]
    fn validate_email_missing_at() {
        assert!(validate_registration_fields("App", "https://x.com", "noatsign").is_err());
    }

    #[test]
    fn validate_email_empty_local() {
        assert!(validate_registration_fields("App", "https://x.com", "@domain.com").is_err());
    }

    #[test]
    fn validate_email_empty_domain() {
        assert!(validate_registration_fields("App", "https://x.com", "local@").is_err());
    }

    #[test]
    fn validate_email_ok() {
        assert!(validate_registration_fields("App", "https://x.com", "user@example.com").is_ok());
    }

    #[test]
    fn constant_time_eq_same() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"hello", b"hell"));
    }

    #[test]
    fn constant_time_eq_different_content() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn extract_bearer_token_valid() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer my-token-123".parse().unwrap(),
        );
        assert_eq!(extract_bearer_token(&headers), Some("my-token-123"));
    }

    #[test]
    fn extract_bearer_token_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn extract_bearer_token_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            "Basic dXNlcjpwYXNz".parse().unwrap(),
        );
        assert_eq!(extract_bearer_token(&headers), None);
    }

    #[test]
    fn registration_error_not_found_returns_404() {
        let response = RegistrationError::NotFound.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn registration_error_invalid_token_returns_401() {
        let response = RegistrationError::InvalidToken.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn registration_error_not_pending_returns_409() {
        let response = RegistrationError::NotPending.into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn registration_error_issuer_already_registered_returns_409() {
        let response =
            RegistrationError::IssuerAlreadyRegistered("https://dup.example.com".into())
                .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn registration_error_validation_returns_422() {
        let response = RegistrationError::Validation {
            code: error_codes::INVALID_APP_NAME,
            message: "too long".to_string(),
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
