-- Migration 1: All application tables
-- Every table includes tenant_id UUID NOT NULL REFERENCES tenants(tenant_id)
-- All indexes use tenant_id as the leading column

-- ── Users and devices ────────────────────────────────────────────────────────
CREATE TABLE users (
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    user_id     TEXT NOT NULL,           -- OIDC sub claim (unique within tenant)
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, user_id)
);
CREATE INDEX idx_users_tenant ON users(tenant_id);

CREATE TABLE devices (
    tenant_id           UUID NOT NULL REFERENCES tenants(tenant_id),
    device_id           UUID NOT NULL DEFAULT gen_random_uuid(),
    user_id             TEXT NOT NULL,
    identity_key        BYTEA NOT NULL,     -- Curve25519 public key (32 bytes)
    signed_prekey_id    BIGINT NOT NULL,
    signed_prekey       BYTEA NOT NULL,     -- Curve25519 public key
    signed_prekey_sig   BYTEA NOT NULL,     -- Ed25519 signature (64 bytes)
    otpk_count          INT NOT NULL DEFAULT 0,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, device_id),
    FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, user_id),
    UNIQUE (tenant_id, user_id, device_id)
);
CREATE INDEX idx_devices_tenant_user ON devices(tenant_id, user_id);

-- ── One-time pre-keys ────────────────────────────────────────────────────────
CREATE TABLE one_time_prekeys (
    otpk_id     BIGSERIAL PRIMARY KEY,
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    device_id   UUID NOT NULL,
    key_id      BIGINT NOT NULL,
    public_key  BYTEA NOT NULL,            -- Curve25519 public key
    consumed    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (tenant_id, device_id, key_id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES devices(tenant_id, device_id) ON DELETE CASCADE
);
CREATE INDEX idx_otpk_tenant_device_available
    ON one_time_prekeys(tenant_id, device_id) WHERE consumed = FALSE;

-- ── Conversations (1:1 and group) ────────────────────────────────────────────
CREATE TABLE conversations (
    tenant_id       UUID NOT NULL REFERENCES tenants(tenant_id),
    conversation_id UUID NOT NULL DEFAULT gen_random_uuid(),
    kind            TEXT NOT NULL CHECK (kind IN ('direct', 'group')),
    last_seq        BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, conversation_id)
);
CREATE INDEX idx_conversations_tenant ON conversations(tenant_id);

-- ── Conversation participants ────────────────────────────────────────────────
CREATE TABLE conversation_members (
    tenant_id       UUID NOT NULL REFERENCES tenants(tenant_id),
    conversation_id UUID NOT NULL,
    user_id         TEXT NOT NULL,
    device_id       UUID NOT NULL,
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (tenant_id, conversation_id, user_id, device_id),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations(tenant_id, conversation_id),
    FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, user_id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES devices(tenant_id, device_id)
);
CREATE INDEX idx_conv_members_tenant_conv
    ON conversation_members(tenant_id, conversation_id);
CREATE INDEX idx_conv_members_tenant_user
    ON conversation_members(tenant_id, user_id);

-- ── Message envelopes ────────────────────────────────────────────────────────
CREATE TABLE message_envelopes (
    id                  BIGSERIAL PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenants(tenant_id),
    conversation_id     UUID NOT NULL,
    seq                 BIGINT NOT NULL,    -- per-conversation monotonic sequence
    sender_user_id      TEXT NOT NULL,
    sender_device_id    UUID NOT NULL,
    recipient_user_id   TEXT,               -- NULL for group messages
    recipient_device_id UUID,               -- NULL for group messages
    ciphertext          BYTEA NOT NULL,
    protocol_header     JSONB NOT NULL,     -- DR header or SenderKey ratchet header
    server_ts           BIGINT NOT NULL,    -- Unix epoch milliseconds
    UNIQUE (tenant_id, conversation_id, seq),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations(tenant_id, conversation_id)
);
CREATE INDEX idx_msg_tenant_conv_seq
    ON message_envelopes(tenant_id, conversation_id, seq);
CREATE INDEX idx_msg_tenant_recipient
    ON message_envelopes(tenant_id, recipient_device_id, conversation_id, seq)
    WHERE recipient_device_id IS NOT NULL;

-- ── Per-device delivery state ────────────────────────────────────────────────
CREATE TABLE delivery_state (
    tenant_id           UUID NOT NULL REFERENCES tenants(tenant_id),
    conversation_id     UUID NOT NULL,
    device_id           UUID NOT NULL,
    last_delivered_seq  BIGINT NOT NULL DEFAULT 0,
    last_acked_seq      BIGINT NOT NULL DEFAULT 0,
    dropped_count       BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, conversation_id, device_id),
    FOREIGN KEY (tenant_id, device_id) REFERENCES devices(tenant_id, device_id)
);

-- ── SenderKey distribution messages (one row per recipient device) ───────────
CREATE TABLE sender_key_distributions (
    id                  BIGSERIAL PRIMARY KEY,
    tenant_id           UUID NOT NULL REFERENCES tenants(tenant_id),
    conversation_id     UUID NOT NULL,
    sender_user_id      TEXT NOT NULL,
    sender_device_id    UUID NOT NULL,
    recipient_user_id   TEXT NOT NULL,
    recipient_device_id UUID NOT NULL,
    encrypted_skdm      BYTEA NOT NULL,    -- encrypted SenderKey distribution message
    delivered           BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    FOREIGN KEY (tenant_id, conversation_id)
        REFERENCES conversations(tenant_id, conversation_id)
);
CREATE INDEX idx_skdm_tenant_conv
    ON sender_key_distributions(tenant_id, conversation_id);
CREATE INDEX idx_skdm_tenant_recipient
    ON sender_key_distributions(tenant_id, recipient_device_id, conversation_id);

-- ── Token revocation (refresh tokens) ───────────────────────────────────────
CREATE TABLE refresh_tokens (
    jti         TEXT PRIMARY KEY,          -- JWT ID claim
    tenant_id   UUID NOT NULL REFERENCES tenants(tenant_id),
    user_id     TEXT NOT NULL,
    device_id   UUID NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked     BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE INDEX idx_refresh_tokens_tenant ON refresh_tokens(tenant_id, user_id);
