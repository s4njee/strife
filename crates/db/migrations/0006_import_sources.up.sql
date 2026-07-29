CREATE TYPE import_entry_state AS ENUM (
    'discovered',
    'stable',
    'importing',
    'imported',
    'failed'
);

CREATE TABLE import_sources (
    id UUID PRIMARY KEY,
    watch_path TEXT NOT NULL UNIQUE,
    destination_folder_id UUID NOT NULL REFERENCES nodes (id) ON DELETE RESTRICT,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_scan_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE import_entries (
    id UUID PRIMARY KEY,
    source_id UUID NOT NULL REFERENCES import_sources (id) ON DELETE CASCADE,
    source_path TEXT NOT NULL,
    source_size BIGINT NOT NULL,
    source_modified_at TIMESTAMPTZ NOT NULL,
    source_checksum TEXT,
    state import_entry_state NOT NULL DEFAULT 'discovered',
    resulting_node_id UUID REFERENCES nodes (id) ON DELETE SET NULL,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT import_entries_nonnegative_size CHECK (source_size >= 0),
    CONSTRAINT import_entries_source_path_relative CHECK (
        source_path <> '' AND left(source_path, 1) <> '/'
    ),
    UNIQUE (source_id, source_path)
);

CREATE INDEX import_entries_pending_idx
    ON import_entries (source_id, state, created_at)
    WHERE state IN ('discovered', 'stable', 'importing');

INSERT INTO import_sources (id, watch_path, destination_folder_id)
VALUES (
    '00000000-0000-0000-0000-000000000003',
    '/mnt/ext/watch',
    '00000000-0000-0000-0000-000000000001'
)
ON CONFLICT (watch_path) DO NOTHING;
