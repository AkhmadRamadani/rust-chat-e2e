//! KDS REST handlers — Key Distribution Server endpoints.
//!
//! # Route table
//!
//! | Method | Path | Handler |
//! |--------|------|---------|
//! | `POST` | `/users/{userID}/devices` | [`register_device`] |
//! | `GET` | `/users/{userID}/key-bundle` | [`get_key_bundle`] |
//! | `PUT` | `/users/{userID}/devices/{deviceID}/one-time-prekeys` | [`replenish_otpks`] |
//! | `PUT` | `/users/{userID}/devices/{deviceID}/signed-prekey` | [`rotate_signed_prekey`] |

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use auth::AuthenticatedUser;
use common::{
    error_codes, ApiError, Curve25519PublicKey, DeviceId, Ed25519Signature, KeyBundle,
    KeyBundleResponse, OneTimePreKey, RtEvent, UserId,
};
use kds::{verify_signed_prekey, KdsError, KdsRepository};
use realtime::{WebTransportManager};

// ── Shared handler state ──────────────────────────────────────────────────────

/// Axum state shared across all KDS handlers.
#[derive(Clone)]
pub struct KdsState {
    /// KDS repository (PostgreSQL).
    pub repo: Arc<dyn KdsRepository>,
    /// WebTransport manager for delivering real-time events (e.g. `low_otpk`).
    pub wt_manager: Arc<dyn WebTransportManager>,
}

impl std::fmt::Debug for KdsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdsState").finish_non_exhaustive()
    }
}

// ── Error handling ────────────────────────────────────────────────────────────

/// KDS-specific handler errors mapped to structured `ApiError` responses.
#[derive(Debug)]
pub enum KdsHandlerError {
    /// HTTP 422 — SignedPreKey signature verification failed.
    InvalidSignature,
    /// HTTP 409 — user already has 5 devices.
    DeviceLimitReached,
    /// HTTP 403 — authenticated user does not own the target resource.
    Forbidden,
    /// HTTP 404 — requested resource not found.
    NotFound,
    /// HTTP 400 — invalid request parameters.
    BadRequest(String),
    /// HTTP 503 — storage layer error.
    Storage(String),
}

impl IntoResponse for KdsHandlerError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match self {
            KdsHandlerError::InvalidSignature => (
                StatusCode::UNPROCESSABLE_ENTITY,
                error_codes::INVALID_SIGNED_PREKEY_SIGNATURE,
                "The SignedPreKey signature could not be verified against the IdentityKey.",
            ),
            KdsHandlerError::DeviceLimitReached => (
                StatusCode::CONFLICT,
                error_codes::DEVICE_LIMIT_REACHED,
                "This user already has 5 registered devices.",
            ),
            KdsHandlerError::Forbidden => (
                StatusCode::FORBIDDEN,
                error_codes::FORBIDDEN,
                "You are not authorised to access this resource.",
            ),
            KdsHandlerError::NotFound => (
                StatusCode::NOT_FOUND,
                error_codes::NOT_FOUND,
                "The requested resource does not exist.",
            ),
            KdsHandlerError::BadRequest(ref msg) => {
                let owned_msg = msg.clone();
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        error_code: error_codes::BAD_REQUEST.to_string(),
                        message: owned_msg,
                        request_id: Uuid::new_v4(),
                    }),
                )
                    .into_response();
            }
            KdsHandlerError::Storage(ref msg) => {
                tracing::error!("KDS storage error: {msg}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ApiError {
                        error_code: error_codes::STORAGE_UNAVAILABLE.to_string(),
                        message: "A storage error occurred; please retry.".to_string(),
                        request_id: Uuid::new_v4(),
                    }),
                )
                    .into_response();
            }
        };

        (
            status,
            Json(ApiError {
                error_code: error_code.to_string(),
                message: message.to_string(),
                request_id: Uuid::new_v4(),
            }),
        )
            .into_response()
    }
}

impl From<KdsError> for KdsHandlerError {
    fn from(err: KdsError) -> Self {
        match err {
            KdsError::DeviceLimitReached => KdsHandlerError::DeviceLimitReached,
            KdsError::InvalidSignature => KdsHandlerError::InvalidSignature,
            KdsError::NotFound => KdsHandlerError::NotFound,
            KdsError::Database(e) => KdsHandlerError::Storage(e.to_string()),
        }
    }
}

// ── POST /users/{userID}/devices ──────────────────────────────────────────────

/// Response body for a successful device registration.
#[derive(Debug, Serialize)]
pub struct RegisterDeviceResponse {
    /// The server-assigned device UUID.
    pub device_id: Uuid,
}

/// Register a new device for the authenticated user.
///
/// # Steps
/// 1. Verify the `SignedPreKey` Ed25519 signature → HTTP 422 on failure.
/// 2. Check the device count for this tenant-user → HTTP 409 if at limit (5).
/// 3. Persist the device and its key material → HTTP 201 with `{ device_id }`.
///
/// # Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7
pub async fn register_device(
    State(state): State<KdsState>,
    auth_user: AuthenticatedUser,
    Path(user_id): Path<String>,
    Json(bundle): Json<KeyBundle>,
) -> Result<(StatusCode, Json<RegisterDeviceResponse>), KdsHandlerError> {
    // Ensure the authenticated user matches the path parameter.
    if auth_user.user_id.0 != user_id {
        return Err(KdsHandlerError::Forbidden);
    }

    // Validate: between 0 and 100 OTPKs.
    if bundle.one_time_prekeys.len() > 100 {
        return Err(KdsHandlerError::BadRequest(
            "A registration request may include at most 100 one-time pre-keys.".to_string(),
        ));
    }

    // 1. Verify the SignedPreKey signature before touching the database.
    verify_signed_prekey(&bundle.identity_key, &bundle.signed_prekey, &bundle.signed_prekey_sig)
        .map_err(|_| KdsHandlerError::InvalidSignature)?;

    let tenant_id = auth_user.tenant_id;
    let uid = UserId(user_id);

    // 2. Enforce 5-device limit (register_device also checks, but we check
    //    here first to return the correct error code before touching the DB
    //    transactionally).
    let device_count = state
        .repo
        .get_device_count(tenant_id, uid.clone())
        .await
        .map_err(KdsHandlerError::from)?;

    if device_count >= 5 {
        return Err(KdsHandlerError::DeviceLimitReached);
    }

    // 3. Persist device + key material.
    let device_id = state
        .repo
        .register_device(tenant_id, uid, bundle)
        .await
        .map_err(KdsHandlerError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(RegisterDeviceResponse {
            device_id: device_id.0,
        }),
    ))
}

// ── GET /users/{userID}/key-bundle ────────────────────────────────────────────

/// OTPK-depleted HTTP header name.
const HEADER_OTPK_WARNING: &str = "x-otpk-warning";

/// Fetch a key bundle for a user, atomically consuming one OTPK.
///
/// # Steps
/// 1. Call `fetch_key_bundle` (atomically consumes one OTPK).
/// 2. If `one_time_prekey` is `None`, add `x-otpk-warning: depleted` header.
/// 3. After the fetch check remaining OTPK count; if < 10, deliver a
///    `LowOtpk` real-time event to the owning device.
///
/// # Requirements: 4.1, 4.2, 4.3, 4.4
pub async fn get_key_bundle(
    State(state): State<KdsState>,
    _auth_user: AuthenticatedUser,
    Path(user_id): Path<String>,
) -> Result<(StatusCode, HeaderMap, Json<KeyBundleResponse>), KdsHandlerError> {
    let uid = UserId(user_id);
    // tenant_id is resolved from the auth middleware — we need it scoped to the
    // path param user here (any authenticated tenant user may fetch another
    // user's key bundle within the same tenant).
    let tenant_id = _auth_user.tenant_id;

    // 1. Atomically fetch one key bundle (consumes one OTPK when available).
    let bundle = state
        .repo
        .fetch_key_bundle(tenant_id, uid)
        .await
        .map_err(KdsHandlerError::from)?;

    // 2. Set warning header when the OTPK pool is depleted.
    let mut headers = HeaderMap::new();
    if bundle.one_time_prekey.is_none() {
        headers.insert(
            HEADER_OTPK_WARNING,
            HeaderValue::from_static("depleted"),
        );
    }

    // 3. Check OTPK count and emit `low_otpk` event if below threshold.
    let device_id = bundle.device_id;
    let otpk_count = state
        .repo
        .get_otpk_count(tenant_id, device_id)
        .await
        .unwrap_or(0); // non-fatal: best-effort notification

    if otpk_count < 10 {
        let event = RtEvent::LowOtpk {
            device_id,
            count: otpk_count as u32,
        };
        // Fire-and-forget: failure to notify is non-fatal.
        let _ = state.wt_manager.deliver(tenant_id, device_id, event).await;
    }

    Ok((StatusCode::OK, headers, Json(bundle)))
}

// ── PUT /users/{userID}/devices/{deviceID}/one-time-prekeys ───────────────────

/// Request body for replenishing one-time pre-keys.
#[derive(Debug, Deserialize)]
pub struct ReplenishOtpksRequest {
    /// New one-time pre-keys to append to the device's pool (1–100).
    pub one_time_prekeys: Vec<OneTimePreKey>,
}

/// Response body for a successful OTPK replenishment.
#[derive(Debug, Serialize)]
pub struct ReplenishOtpksResponse {
    /// Updated total count of unconsumed OTPKs after the upload.
    pub total_count: i64,
}

/// Append new one-time pre-keys to a device's pool.
///
/// # Requirements: 4.5
pub async fn replenish_otpks(
    State(state): State<KdsState>,
    auth_user: AuthenticatedUser,
    Path((user_id, device_id)): Path<(String, Uuid)>,
    Json(body): Json<ReplenishOtpksRequest>,
) -> Result<Json<ReplenishOtpksResponse>, KdsHandlerError> {
    // Ownership check: the authenticated user must own the device.
    if auth_user.user_id.0 != user_id {
        return Err(KdsHandlerError::Forbidden);
    }

    // Validate key count: 1–100 (Requirement 3.4 / 4.5).
    if body.one_time_prekeys.is_empty() {
        return Err(KdsHandlerError::BadRequest(
            "At least one one-time pre-key must be provided.".to_string(),
        ));
    }
    if body.one_time_prekeys.len() > 100 {
        return Err(KdsHandlerError::BadRequest(
            "At most 100 one-time pre-keys may be uploaded per request.".to_string(),
        ));
    }

    let tenant_id = auth_user.tenant_id;
    let did = DeviceId(device_id);

    let total_count = state
        .repo
        .replenish_otpks(tenant_id, did, body.one_time_prekeys)
        .await
        .map_err(KdsHandlerError::from)?;

    Ok(Json(ReplenishOtpksResponse { total_count }))
}

// ── PUT /users/{userID}/devices/{deviceID}/signed-prekey ──────────────────────

/// Request body for rotating the signed pre-key.
#[derive(Debug, Deserialize)]
pub struct RotateSignedPrekeyRequest {
    /// New signed pre-key identifier.
    pub signed_prekey_id: u64,
    /// New Curve25519 signed pre-key public value.
    pub signed_prekey: Curve25519PublicKey,
    /// Ed25519 signature over the new signed pre-key bytes, produced with the
    /// device's identity key.
    pub signed_prekey_sig: Ed25519Signature,
}

/// Rotate the signed pre-key for a specific device.
///
/// The new signature is verified against the stored IdentityKey before
/// overwriting the existing SPK.  On failure the existing SPK is unchanged.
///
/// # Requirements: 4.6, 4.7
pub async fn rotate_signed_prekey(
    State(state): State<KdsState>,
    auth_user: AuthenticatedUser,
    Path((user_id, device_id)): Path<(String, Uuid)>,
    Json(body): Json<RotateSignedPrekeyRequest>,
) -> Result<StatusCode, KdsHandlerError> {
    // Ownership check.
    if auth_user.user_id.0 != user_id {
        return Err(KdsHandlerError::Forbidden);
    }

    let tenant_id = auth_user.tenant_id;
    let did = DeviceId(device_id);

    // Fetch the stored identity key to verify the new signature against.
    let identity_key = state
        .repo
        .get_identity_key(tenant_id, did)
        .await
        .map_err(KdsHandlerError::from)?;

    // Verify signature — do NOT write if this fails (Requirement 4.7).
    verify_signed_prekey(&identity_key, &body.signed_prekey, &body.signed_prekey_sig)
        .map_err(|_| KdsHandlerError::InvalidSignature)?;

    // Persist the new SPK.
    state
        .repo
        .rotate_signed_prekey(
            tenant_id,
            did,
            body.signed_prekey_id,
            body.signed_prekey,
            body.signed_prekey_sig,
        )
        .await
        .map_err(KdsHandlerError::from)?;

    Ok(StatusCode::OK)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_device_response_serialises() {
        let resp = RegisterDeviceResponse {
            device_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("device_id"));
    }

    #[test]
    fn replenish_otpks_response_serialises() {
        let resp = ReplenishOtpksResponse { total_count: 42 };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("total_count"));
        assert!(json.contains("42"));
    }

    #[test]
    fn kds_error_device_limit_converts() {
        let err = KdsHandlerError::from(KdsError::DeviceLimitReached);
        assert!(matches!(err, KdsHandlerError::DeviceLimitReached));
    }

    #[test]
    fn kds_error_invalid_sig_converts() {
        let err = KdsHandlerError::from(KdsError::InvalidSignature);
        assert!(matches!(err, KdsHandlerError::InvalidSignature));
    }

    #[test]
    fn kds_error_not_found_converts() {
        let err = KdsHandlerError::from(KdsError::NotFound);
        assert!(matches!(err, KdsHandlerError::NotFound));
    }
}
