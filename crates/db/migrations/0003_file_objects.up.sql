CREATE TYPE file_upload_state AS ENUM ('staging', 'finalized');

CREATE TABLE file_objects (
    id UUID PRIMARY KEY,
    node_id UUID REFERENCES nodes (id) ON DELETE RESTRICT,
    storage_key TEXT NOT NULL,
    byte_size BIGINT NOT NULL,
    mime_type TEXT,
    checksum_sha256 TEXT,
    upload_state file_upload_state NOT NULL DEFAULT 'staging',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT file_objects_nonnegative_size CHECK (byte_size >= 0),
    CONSTRAINT file_objects_finalized_node_required CHECK (
        upload_state <> 'finalized' OR node_id IS NOT NULL
    )
);

CREATE UNIQUE INDEX file_objects_one_finalized_per_node
    ON file_objects (node_id)
    WHERE upload_state = 'finalized';
