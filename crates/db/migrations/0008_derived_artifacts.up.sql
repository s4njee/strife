CREATE TYPE artifact_type AS ENUM ('thumbnail', 'preview');
CREATE TYPE artifact_state AS ENUM ('generating', 'ready', 'failed');

CREATE TABLE derived_artifacts (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    artifact_type artifact_type NOT NULL,
    format TEXT NOT NULL,
    width INTEGER,
    height INTEGER,
    storage_key TEXT NOT NULL,
    byte_size BIGINT NOT NULL DEFAULT 0,
    generator_version TEXT NOT NULL,
    state artifact_state NOT NULL DEFAULT 'generating',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id, artifact_type),
    CONSTRAINT derived_artifacts_dimensions_nonnegative CHECK ((width IS NULL OR width >= 0) AND (height IS NULL OR height >= 0)),
    CONSTRAINT derived_artifacts_size_nonnegative CHECK (byte_size >= 0)
);
