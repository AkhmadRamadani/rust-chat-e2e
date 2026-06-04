-- Migration 0: Tenant registry
-- Creates the tenants table and its index on oidc_issuer

CREATE TABLE tenants (
    tenant_id    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT NOT NULL,
    oidc_issuer  TEXT NOT NULL UNIQUE,   -- JWT iss claim; used to resolve tenant
    active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_tenants_issuer ON tenants(oidc_issuer);
