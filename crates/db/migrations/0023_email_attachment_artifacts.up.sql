-- Decoded attachment bytes promoted to managed, regenerable artifacts.
--
-- Artifacts are disposable: the .eml original remains the canonical source and
-- every row here can be rebuilt by reparsing it. Deleting the message deletes
-- its artifacts, and the storage objects are reclaimed by the same permanent
-- deletion path that handles other derived artifacts.

CREATE TYPE email_artifact_state AS ENUM ('pending', 'ready', 'failed', 'skipped');

CREATE TABLE email_attachment_artifacts (
    -- Deterministic: a UUIDv5 of (message node, MIME part path). The primary
    -- key doubles as the storage object id, so an attachment's location is a
    -- pure function of its identity within its message. A sender-supplied
    -- filename never participates.
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    part_path TEXT NOT NULL,
    state email_artifact_state NOT NULL DEFAULT 'pending',
    storage_key TEXT,
    media_type TEXT NOT NULL,
    byte_size BIGINT NOT NULL DEFAULT 0,
    checksum_sha256 TEXT,
    -- Nesting depth of the source part; 0 is a top-level attachment.
    depth INTEGER NOT NULL DEFAULT 0,
    is_message BOOLEAN NOT NULL DEFAULT false,
    materializer_version TEXT NOT NULL,
    warnings TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id, part_path),
    CONSTRAINT email_attachment_artifacts_size_nonnegative CHECK (byte_size >= 0),
    CONSTRAINT email_attachment_artifacts_depth_nonnegative CHECK (depth >= 0),
    -- A ready artifact must say where its bytes are; anything else must not
    -- claim a location it may not have written.
    CONSTRAINT email_attachment_artifacts_ready_has_key
        CHECK (state <> 'ready' OR storage_key IS NOT NULL)
);

CREATE INDEX email_attachment_artifacts_node_idx
    ON email_attachment_artifacts (node_id, part_path);
CREATE INDEX email_attachment_artifacts_state_idx
    ON email_attachment_artifacts (state)
    WHERE state <> 'ready';
