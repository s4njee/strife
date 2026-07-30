CREATE TABLE trash_entries (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL UNIQUE REFERENCES nodes (id) ON DELETE CASCADE,
    original_parent_id UUID REFERENCES nodes (id) ON DELETE SET NULL,
    trashed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    scheduled_purge_at TIMESTAMPTZ NOT NULL DEFAULT (now() + INTERVAL '30 days'),
    CONSTRAINT trash_entries_purge_after_trash CHECK (scheduled_purge_at >= trashed_at)
);

CREATE INDEX trash_entries_scheduled_purge_at_idx
    ON trash_entries (scheduled_purge_at);

CREATE INDEX trash_entries_trashed_at_idx
    ON trash_entries (trashed_at DESC);
