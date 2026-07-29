CREATE TYPE job_type AS ENUM (
    'metadata_extraction',
    'preview_generation',
    'trash_cleanup',
    'permanent_deletion'
);

CREATE TYPE job_state AS ENUM (
    'pending',
    'leased',
    'completed',
    'failed',
    'cancelled'
);

CREATE TABLE jobs (
    id UUID PRIMARY KEY,
    job_type job_type NOT NULL,
    target_node_id UUID NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    state job_state NOT NULL DEFAULT 'pending',
    priority INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    lease_owner TEXT,
    lease_expires_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT jobs_attempts_nonnegative CHECK (attempts >= 0),
    CONSTRAINT jobs_max_attempts_positive CHECK (max_attempts > 0)
);

CREATE UNIQUE INDEX jobs_active_type_target_unique
    ON jobs (job_type, target_node_id)
    WHERE state IN ('pending', 'leased');
