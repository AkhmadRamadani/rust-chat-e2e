-- Migration 2: Attachments table
-- Stores file metadata; the actual bytes live on the filesystem or object store.

CREATE TABLE attachments (
    attachment_id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL REFERENCES tenants(tenant_id),
    uploader_id     TEXT NOT NULL,              -- user_id of the uploader
    filename        TEXT NOT NULL,              -- original client filename
    content_type    TEXT NOT NULL,              -- MIME type e.g. image/jpeg
    size_bytes      BIGINT NOT NULL,
    storage_path    TEXT NOT NULL UNIQUE,       -- path on disk / object key
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_attachments_tenant ON attachments(tenant_id, attachment_id);
