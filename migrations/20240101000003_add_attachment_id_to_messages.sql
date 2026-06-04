-- Migration 3: Add attachment_id to message_envelopes
-- Nullable — most messages have no attachment.

ALTER TABLE message_envelopes
    ADD COLUMN attachment_id UUID REFERENCES attachments(attachment_id) ON DELETE SET NULL;

CREATE INDEX idx_msg_attachment ON message_envelopes(attachment_id)
    WHERE attachment_id IS NOT NULL;
