//! Attachment upload and download handlers.
//!
//! # Routes
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST   | `/attachments` | Upload a file; returns `{ attachment_id }` |
//! | GET    | `/attachments/{attachment_id}` | Download a file by ID |
//!
//! Files are stored under `ATTACHMENT_DIR` (default `/app/attachments`).
//! Each file is saved as `{tenant_id}/{attachment_id}` to enforce isolation.

use std::path::PathBuf;

use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use sqlx::PgPool;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use auth::AuthenticatedUser;
use common::{error_codes, ApiError};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AttachmentState {
    pub pool: PgPool,
    pub storage_dir: PathBuf,
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub attachment_id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
}

// ── POST /attachments ─────────────────────────────────────────────────────────

/// Upload a file attachment.
///
/// Accepts `multipart/form-data` with a single field named `file`.
/// Returns `{ attachment_id }` which can be referenced in a message envelope.
pub async fn upload_attachment(
    State(state): State<AttachmentState>,
    auth_user: AuthenticatedUser,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResponse>), Response> {
    let tenant_id = auth_user.tenant_id;
    let uploader_id = auth_user.user_id.0.clone();

    // Extract the first file field from the multipart body.
    let field = loop {
        match multipart.next_field().await {
            Ok(Some(f)) => break f,
            Ok(None) => return Err(err_response(StatusCode::BAD_REQUEST, "no file field in multipart body")),
            Err(e) => return Err(err_response(StatusCode::BAD_REQUEST, &format!("multipart error: {e}"))),
        }
    };

    let filename = field
        .file_name()
        .unwrap_or("attachment")
        .to_string()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect::<String>();
    let content_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_string();

    let data = field.bytes().await.map_err(|e| {
        err_response(StatusCode::BAD_REQUEST, &format!("failed to read file: {e}"))
    })?;
    let size_bytes = data.len() as u64;

    if size_bytes == 0 {
        return Err(err_response(StatusCode::BAD_REQUEST, "uploaded file is empty"));
    }
    // 100 MB limit
    if size_bytes > 100 * 1024 * 1024 {
        return Err(err_response(StatusCode::PAYLOAD_TOO_LARGE, "file exceeds 100 MB limit"));
    }

    let attachment_id = Uuid::new_v4();

    // Store under {storage_dir}/{tenant_id}/{attachment_id}
    let tenant_dir = state.storage_dir.join(tenant_id.0.to_string());
    fs::create_dir_all(&tenant_dir).await.map_err(|e| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("storage error: {e}"))
    })?;

    let storage_path = format!("{}/{}", tenant_id.0, attachment_id);
    let file_path = state.storage_dir.join(&storage_path);

    let mut file = fs::File::create(&file_path).await.map_err(|e| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("storage error: {e}"))
    })?;
    file.write_all(&data).await.map_err(|e| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("write error: {e}"))
    })?;

    // Persist metadata to the attachments table.
    sqlx::query(
        r#"
        INSERT INTO attachments
            (attachment_id, tenant_id, uploader_id, filename, content_type, size_bytes, storage_path)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(attachment_id)
    .bind(tenant_id.0)
    .bind(&uploader_id)
    .bind(&filename)
    .bind(&content_type)
    .bind(size_bytes as i64)
    .bind(&storage_path)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("db error: {e}"))
    })?;

    tracing::info!(
        tenant_id = %tenant_id.0,
        attachment_id = %attachment_id,
        filename = %filename,
        size_bytes = size_bytes,
        "attachment uploaded"
    );

    Ok((
        StatusCode::CREATED,
        Json(UploadResponse { attachment_id, filename, content_type, size_bytes }),
    ))
}

// ── GET /attachments/{attachment_id} ─────────────────────────────────────────

/// Download an attachment by ID.
///
/// Only accessible to users within the same tenant as the uploader.
pub async fn download_attachment(
    State(state): State<AttachmentState>,
    auth_user: AuthenticatedUser,
    Path(attachment_id): Path<Uuid>,
) -> Result<Response, Response> {
    let tenant_id = auth_user.tenant_id;

    // Fetch metadata — also verifies the attachment belongs to this tenant.
    let row = sqlx::query_as::<_, (String, String, i64, String)>(
        r#"
        SELECT filename, content_type, size_bytes, storage_path
        FROM   attachments
        WHERE  attachment_id = $1
          AND  tenant_id     = $2
        "#,
    )
    .bind(attachment_id)
    .bind(tenant_id.0)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?
    .ok_or_else(|| err_response(StatusCode::NOT_FOUND, "attachment not found"))?;

    let (filename, content_type, _size_bytes, storage_path) = row;

    let file_path = state.storage_dir.join(&storage_path);
    let data = fs::read(&file_path).await.map_err(|e| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("read error: {e}"))
    })?;

    // Build response with correct Content-Type and Content-Disposition.
    let safe_name = filename
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect::<String>();
    let disposition = format!("attachment; filename=\"{safe_name}\"");

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_str(&content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")))
        .header(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).map_err(|_| err_response(StatusCode::INTERNAL_SERVER_ERROR, "invalid filename"))?)
        .header(header::CONTENT_LENGTH, data.len().to_string())
        .body(Body::from(data))
        .map_err(|e| err_response(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;

    Ok(response)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn err_response(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(ApiError {
            error_code: error_codes::BAD_REQUEST.to_string(),
            message: message.to_string(),
            request_id: Uuid::new_v4(),
        }),
    )
        .into_response()
}
