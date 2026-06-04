-- Migration: Create tenant_registrations table
-- Requirements: 10.9

CREATE TABLE tenant_registrations (
    registration_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    app_name          TEXT NOT NULL,
    oidc_issuer       TEXT NOT NULL,
    contact_email     TEXT NOT NULL,
    status            TEXT NOT NULL DEFAULT 'pending'
                        CHECK (status IN ('pending', 'approved', 'rejected')),
    registration_token TEXT NOT NULL UNIQUE,
    tenant_id         UUID REFERENCES tenants(tenant_id),
    rejection_reason  TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_registrations_status ON tenant_registrations(status);
CREATE INDEX idx_registrations_issuer ON tenant_registrations(oidc_issuer);
