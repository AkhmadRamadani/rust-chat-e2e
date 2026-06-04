// crates/kds — Key Distribution Server
// Repository trait and PostgreSQL implementation.

use async_trait::async_trait;
use common::{
    Curve25519PublicKey, DeviceId, Ed25519Signature, KeyBundle, KeyBundleResponse, OneTimePreKey,
    OtpkWarning, TenantId, UserId,
};
use sqlx::{PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum KdsError {
    #[error("device limit reached: a user may not register more than 5 devices")]
    DeviceLimitReached,

    #[error("invalid signed pre-key signature")]
    InvalidSignature,

    #[error("resource not found")]
    NotFound,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ── Repository trait ──────────────────────────────────────────────────────────

/// All KDS storage operations.  Every method is scoped to `tenant_id` so that
/// no cross-tenant data leakage is possible at the repository level.
#[async_trait]
pub trait KdsRepository: Send + Sync {
    /// Register a new device for `user_id` under `tenant_id`.
    ///
    /// - Upserts the `users` row.
    /// - Counts existing devices for `(tenant_id, user_id)`; returns
    ///   [`KdsError::DeviceLimitReached`] if already at 5.
    /// - Inserts the `devices` row and the initial one-time pre-keys.
    /// - Returns the newly assigned [`DeviceId`].
    async fn register_device(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
        bundle: KeyBundle,
    ) -> Result<DeviceId, KdsError>;

    /// Atomically fetch one key bundle for any device belonging to `user_id`.
    ///
    /// Selects one unconsumed OTPK with `FOR UPDATE SKIP LOCKED`, marks it
    /// `consumed = true`, and returns the full [`KeyBundleResponse`].  When no
    /// OTPK is available the response still contains the identity / signed
    /// pre-key fields with `one_time_prekey = None` and
    /// `otpk_warning = Some(OtpkWarning::Depleted)`.
    async fn fetch_key_bundle(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<KeyBundleResponse, KdsError>;

    /// Append new one-time pre-keys to the device's pool.
    ///
    /// Returns the updated total count of **unconsumed** OTPKs after the
    /// insert (i.e. `SELECT COUNT(*) … WHERE consumed = FALSE`).
    async fn replenish_otpks(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        keys: Vec<OneTimePreKey>,
    ) -> Result<i64, KdsError>;

    /// Replace the signed pre-key on the device row.
    ///
    /// The caller is responsible for verifying the signature before calling
    /// this method.
    async fn rotate_signed_prekey(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        signed_prekey_id: u64,
        signed_prekey: Curve25519PublicKey,
        signed_prekey_sig: Ed25519Signature,
    ) -> Result<(), KdsError>;

    /// Return the count of unconsumed OTPKs for the given device.
    async fn get_otpk_count(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<i64, KdsError>;

    /// Return the number of devices registered for `(tenant_id, user_id)`.
    async fn get_device_count(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<i64, KdsError>;

    /// Return the stored `IdentityKey` (Curve25519 public key) for a specific device.
    ///
    /// Used by the SPK rotation handler to verify the new signature before
    /// writing.  Returns [`KdsError::NotFound`] when no matching device exists.
    async fn get_identity_key(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<Curve25519PublicKey, KdsError>;
}

// ── PostgreSQL implementation ─────────────────────────────────────────────────

/// Production repository backed by a `sqlx::PgPool`.
#[derive(Clone)]
pub struct PgKdsRepository {
    pool: PgPool,
}

impl PgKdsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// Maximum devices per user (Requirement 3.6).
const DEVICE_LIMIT: i64 = 5;

#[async_trait]
impl KdsRepository for PgKdsRepository {
    // ── register_device ───────────────────────────────────────────────────────

    async fn register_device(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
        bundle: KeyBundle,
    ) -> Result<DeviceId, KdsError> {
        let mut tx = self.pool.begin().await?;

        // 1. Upsert user row (no-op if it already exists).
        sqlx::query(
            r#"
            INSERT INTO users (tenant_id, user_id)
            VALUES ($1, $2)
            ON CONFLICT (tenant_id, user_id) DO NOTHING
            "#,
        )
        .bind(tenant_id.0)
        .bind(&user_id.0)
        .execute(&mut *tx)
        .await?;

        // 2. Count existing devices for this tenant-user; enforce limit.
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*) FROM devices
            WHERE tenant_id = $1 AND user_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(&user_id.0)
        .fetch_one(&mut *tx)
        .await?;

        if count >= DEVICE_LIMIT {
            return Err(KdsError::DeviceLimitReached);
        }

        // 3. Insert device row.
        let device_id = Uuid::new_v4();
        let identity_key_bytes: Vec<u8> = bundle.identity_key.0.to_vec();
        let signed_prekey_bytes: Vec<u8> = bundle.signed_prekey.0.to_vec();
        let signed_prekey_sig_bytes: Vec<u8> = bundle.signed_prekey_sig.0.to_vec();
        let signed_prekey_id = bundle.signed_prekey_id as i64;
        let otpk_count = bundle.one_time_prekeys.len() as i32;

        sqlx::query(
            r#"
            INSERT INTO devices
                (tenant_id, device_id, user_id,
                 identity_key, signed_prekey_id, signed_prekey, signed_prekey_sig,
                 otpk_count)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id)
        .bind(&user_id.0)
        .bind(&identity_key_bytes)
        .bind(signed_prekey_id)
        .bind(&signed_prekey_bytes)
        .bind(&signed_prekey_sig_bytes)
        .bind(otpk_count)
        .execute(&mut *tx)
        .await?;

        // 4. Bulk-insert initial OTPKs (if any).
        for otpk in &bundle.one_time_prekeys {
            let key_id = otpk.key_id as i64;
            let public_key_bytes: Vec<u8> = otpk.public_key.0.to_vec();
            sqlx::query(
                r#"
                INSERT INTO one_time_prekeys (tenant_id, device_id, key_id, public_key)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(tenant_id.0)
            .bind(device_id)
            .bind(key_id)
            .bind(&public_key_bytes)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(DeviceId(device_id))
    }

    // ── fetch_key_bundle ──────────────────────────────────────────────────────

    async fn fetch_key_bundle(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<KeyBundleResponse, KdsError> {
        // Pick any one device for this user (oldest first).
        let device_row = sqlx::query(
            r#"
            SELECT device_id,
                   identity_key,
                   signed_prekey_id,
                   signed_prekey,
                   signed_prekey_sig
            FROM devices
            WHERE tenant_id = $1 AND user_id = $2
            ORDER BY created_at
            LIMIT 1
            "#,
        )
        .bind(tenant_id.0)
        .bind(&user_id.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(KdsError::NotFound)?;

        let device_uuid: Uuid = device_row.try_get("device_id").map_err(KdsError::Database)?;
        let identity_key_raw: Vec<u8> = device_row.try_get("identity_key").map_err(KdsError::Database)?;
        let signed_prekey_id_raw: i64 = device_row.try_get("signed_prekey_id").map_err(KdsError::Database)?;
        let signed_prekey_raw: Vec<u8> = device_row.try_get("signed_prekey").map_err(KdsError::Database)?;
        let signed_prekey_sig_raw: Vec<u8> = device_row.try_get("signed_prekey_sig").map_err(KdsError::Database)?;

        // Atomically consume one OTPK using FOR UPDATE SKIP LOCKED inside a
        // transaction so concurrent fetches never return the same OTPK.
        let mut tx = self.pool.begin().await?;

        let otpk_row = sqlx::query(
            r#"
            SELECT otpk_id, key_id, public_key
            FROM one_time_prekeys
            WHERE tenant_id = $1
              AND device_id = $2
              AND consumed = FALSE
            ORDER BY otpk_id
            LIMIT 1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_uuid)
        .fetch_optional(&mut *tx)
        .await?;

        let one_time_prekey = if let Some(row) = otpk_row {
            let otpk_id: i64 = row.try_get("otpk_id").map_err(KdsError::Database)?;
            let key_id: i64 = row.try_get("key_id").map_err(KdsError::Database)?;
            let public_key_raw: Vec<u8> = row.try_get("public_key").map_err(KdsError::Database)?;

            // Mark the chosen OTPK as consumed.
            sqlx::query(
                r#"
                UPDATE one_time_prekeys
                SET consumed = TRUE
                WHERE otpk_id = $1
                "#,
            )
            .bind(otpk_id)
            .execute(&mut *tx)
            .await?;

            // Decrement the cached count on the device row.
            sqlx::query(
                r#"
                UPDATE devices
                SET otpk_count = GREATEST(otpk_count - 1, 0)
                WHERE tenant_id = $1 AND device_id = $2
                "#,
            )
            .bind(tenant_id.0)
            .bind(device_uuid)
            .execute(&mut *tx)
            .await?;

            let key_bytes: [u8; 32] = public_key_raw
                .try_into()
                .map_err(|_| KdsError::NotFound)?;

            Some(OneTimePreKey {
                key_id: key_id as u64,
                public_key: Curve25519PublicKey(key_bytes),
            })
        } else {
            None
        };

        tx.commit().await?;

        // Build response from the device row.
        let identity_key_bytes: [u8; 32] = identity_key_raw
            .try_into()
            .map_err(|_| KdsError::NotFound)?;
        let signed_prekey_bytes: [u8; 32] = signed_prekey_raw
            .try_into()
            .map_err(|_| KdsError::NotFound)?;
        let signed_prekey_sig_bytes: [u8; 64] = signed_prekey_sig_raw
            .try_into()
            .map_err(|_| KdsError::NotFound)?;

        let otpk_warning = if one_time_prekey.is_none() {
            Some(OtpkWarning::Depleted)
        } else {
            None
        };

        Ok(KeyBundleResponse {
            device_id: DeviceId(device_uuid),
            identity_key: Curve25519PublicKey(identity_key_bytes),
            signed_prekey_id: signed_prekey_id_raw as u64,
            signed_prekey: Curve25519PublicKey(signed_prekey_bytes),
            signed_prekey_sig: Ed25519Signature(signed_prekey_sig_bytes),
            one_time_prekey,
            otpk_warning,
        })
    }

    // ── replenish_otpks ───────────────────────────────────────────────────────

    async fn replenish_otpks(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        keys: Vec<OneTimePreKey>,
    ) -> Result<i64, KdsError> {
        let mut tx = self.pool.begin().await?;

        for otpk in &keys {
            let key_id = otpk.key_id as i64;
            let public_key_bytes: Vec<u8> = otpk.public_key.0.to_vec();
            sqlx::query(
                r#"
                INSERT INTO one_time_prekeys (tenant_id, device_id, key_id, public_key)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (tenant_id, device_id, key_id) DO NOTHING
                "#,
            )
            .bind(tenant_id.0)
            .bind(device_id.0)
            .bind(key_id)
            .bind(&public_key_bytes)
            .execute(&mut *tx)
            .await?;
        }

        // Count total unconsumed OTPKs after the insert.
        let new_count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM one_time_prekeys
            WHERE tenant_id = $1
              AND device_id = $2
              AND consumed = FALSE
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id.0)
        .fetch_one(&mut *tx)
        .await?;

        // Keep the cached counter in sync.
        sqlx::query(
            r#"
            UPDATE devices
            SET otpk_count = $3
            WHERE tenant_id = $1 AND device_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id.0)
        .bind(new_count as i32)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(new_count)
    }

    // ── rotate_signed_prekey ──────────────────────────────────────────────────

    async fn rotate_signed_prekey(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
        signed_prekey_id: u64,
        signed_prekey: Curve25519PublicKey,
        signed_prekey_sig: Ed25519Signature,
    ) -> Result<(), KdsError> {
        let spk_id = signed_prekey_id as i64;
        let spk_bytes: Vec<u8> = signed_prekey.0.to_vec();
        let sig_bytes: Vec<u8> = signed_prekey_sig.0.to_vec();

        let result = sqlx::query(
            r#"
            UPDATE devices
            SET signed_prekey_id  = $3,
                signed_prekey     = $4,
                signed_prekey_sig = $5
            WHERE tenant_id = $1 AND device_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id.0)
        .bind(spk_id)
        .bind(&spk_bytes)
        .bind(&sig_bytes)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(KdsError::NotFound);
        }

        Ok(())
    }

    // ── get_otpk_count ────────────────────────────────────────────────────────

    async fn get_otpk_count(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<i64, KdsError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM one_time_prekeys
            WHERE tenant_id = $1
              AND device_id = $2
              AND consumed = FALSE
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    // ── get_device_count ──────────────────────────────────────────────────────

    async fn get_device_count(
        &self,
        tenant_id: TenantId,
        user_id: UserId,
    ) -> Result<i64, KdsError> {
        let count: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM devices
            WHERE tenant_id = $1 AND user_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    // ── get_identity_key ──────────────────────────────────────────────────────

    async fn get_identity_key(
        &self,
        tenant_id: TenantId,
        device_id: DeviceId,
    ) -> Result<Curve25519PublicKey, KdsError> {
        let identity_key_raw: Vec<u8> = sqlx::query_scalar(
            r#"
            SELECT identity_key
            FROM devices
            WHERE tenant_id = $1 AND device_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(device_id.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(KdsError::NotFound)?;

        let key_bytes: [u8; 32] = identity_key_raw
            .try_into()
            .map_err(|_| KdsError::NotFound)?;

        Ok(Curve25519PublicKey(key_bytes))
    }
}

// ── Signature verification ────────────────────────────────────────────────────

/// Verify the Ed25519 signature on a `SignedPreKey` public value against the
/// device's `IdentityKey`.
///
/// Returns `Ok(())` when valid, `Err(KdsError::InvalidSignature)` otherwise.
pub fn verify_signed_prekey(
    identity_key: &Curve25519PublicKey,
    signed_prekey: &Curve25519PublicKey,
    signature: &Ed25519Signature,
) -> Result<(), KdsError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    // The identity key in an X3DH bundle is a Curve25519 key, but the
    // signature is produced with the corresponding Ed25519 key.  In the Signal
    // Protocol the identity key pair is simultaneously a Curve25519 key pair
    // (for X3DH DH operations) and an Ed25519 key pair (for signing the SPK).
    // On the wire both representations share the same 32-byte scalar.
    let verifying_key = VerifyingKey::from_bytes(&identity_key.0)
        .map_err(|_| KdsError::InvalidSignature)?;

    let sig = Signature::from_bytes(&signature.0);

    verifying_key
        .verify(&signed_prekey.0, &sig)
        .map_err(|_| KdsError::InvalidSignature)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn generate_identity_keypair() -> (SigningKey, Curve25519PublicKey) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_bytes: [u8; 32] = signing_key.verifying_key().to_bytes();
        (signing_key, Curve25519PublicKey(verifying_bytes))
    }

    fn make_spk(signing_key: &SigningKey) -> (Curve25519PublicKey, Ed25519Signature) {
        // Use a fixed "signed prekey" public value for simplicity.
        let spk_bytes = [0x42u8; 32];
        let sig = signing_key.sign(&spk_bytes);
        (
            Curve25519PublicKey(spk_bytes),
            Ed25519Signature(sig.to_bytes()),
        )
    }

    #[test]
    fn verify_signed_prekey_valid() {
        let (signing_key, identity_key) = generate_identity_keypair();
        let (spk, sig) = make_spk(&signing_key);
        assert!(verify_signed_prekey(&identity_key, &spk, &sig).is_ok());
    }

    #[test]
    fn verify_signed_prekey_tampered_signature() {
        let (signing_key, identity_key) = generate_identity_keypair();
        let (spk, mut sig) = make_spk(&signing_key);
        // Flip a bit in the signature.
        sig.0[0] ^= 0xFF;
        assert!(matches!(
            verify_signed_prekey(&identity_key, &spk, &sig),
            Err(KdsError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_signed_prekey_wrong_identity_key() {
        let (signing_key, _identity_key_correct) = generate_identity_keypair();
        let (_other_key, wrong_identity_key) = generate_identity_keypair();
        let (spk, sig) = make_spk(&signing_key);
        assert!(matches!(
            verify_signed_prekey(&wrong_identity_key, &spk, &sig),
            Err(KdsError::InvalidSignature)
        ));
    }

    #[test]
    fn verify_signed_prekey_tampered_spk() {
        let (signing_key, identity_key) = generate_identity_keypair();
        let (mut spk, sig) = make_spk(&signing_key);
        // Modify the signed pre-key bytes after signing.
        spk.0[0] ^= 0xFF;
        assert!(matches!(
            verify_signed_prekey(&identity_key, &spk, &sig),
            Err(KdsError::InvalidSignature)
        ));
    }
}
