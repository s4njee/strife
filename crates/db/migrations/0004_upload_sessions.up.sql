CREATE TYPE upload_session_state AS ENUM (
    'active',
    'finalizing',
    'completed',
    'cancelled',
    'expired'
);

CREATE TABLE upload_sessions (
    id UUID PRIMARY KEY,
    target_folder_id UUID NOT NULL REFERENCES nodes (id) ON DELETE RESTRICT,
    display_name TEXT NOT NULL,
    expected_byte_size BIGINT,
    received_bytes BIGINT NOT NULL DEFAULT 0,
    staging_key TEXT NOT NULL UNIQUE,
    state upload_session_state NOT NULL DEFAULT 'active',
    checksum_sha256 TEXT,
    completed_node_id UUID UNIQUE REFERENCES nodes (id) ON DELETE RESTRICT,
    source_created_at TIMESTAMPTZ,
    source_modified_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT upload_sessions_name_not_empty CHECK (display_name <> ''),
    CONSTRAINT upload_sessions_expected_size_nonnegative CHECK (
        expected_byte_size IS NULL OR expected_byte_size >= 0
    ),
    CONSTRAINT upload_sessions_received_bytes_nonnegative CHECK (received_bytes >= 0)
);

CREATE UNIQUE INDEX upload_sessions_active_folder_name_unique
    ON upload_sessions (target_folder_id, display_name)
    WHERE state IN ('active', 'finalizing');

CREATE TABLE upload_chunks (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES upload_sessions (id) ON DELETE CASCADE,
    start_byte BIGINT NOT NULL,
    end_byte BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT upload_chunks_valid_range CHECK (
        start_byte >= 0 AND end_byte >= start_byte
    ),
    CONSTRAINT upload_chunks_exact_range_unique UNIQUE (
        session_id,
        start_byte,
        end_byte
    )
);

CREATE INDEX upload_chunks_session_start_idx
    ON upload_chunks (session_id, start_byte);
