-- Durable email extraction activity for the operator console.
--
-- Deliberately narrow. These rows are shown live and kept indefinitely, so they
-- carry identifiers and measurements but never message content: no body, no
-- addresses, no raw headers. The subject is the one exception and is bounded
-- and optional, because a console that cannot name the message being processed
-- is not much use during a ten-year backfill.

CREATE TABLE email_events (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID REFERENCES nodes (id) ON DELETE SET NULL,
    node_name TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('running', 'completed', 'failed', 'skipped', 'unsupported')
    ),
    -- Truncated by the writer. Present only for successfully parsed messages,
    -- where it is the same string already shown in search results.
    subject TEXT,
    attachment_count INTEGER,
    duration_ms BIGINT,
    -- Whether this came from a historical campaign, so the console can separate
    -- backfill progress from new mail without joining the jobs table.
    origin job_origin NOT NULL DEFAULT 'foreground',
    campaign_id UUID REFERENCES backfill_campaigns (id) ON DELETE SET NULL,
    warning TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT email_events_attachment_count_nonnegative CHECK (
        attachment_count IS NULL OR attachment_count >= 0
    ),
    CONSTRAINT email_events_duration_nonnegative CHECK (
        duration_ms IS NULL OR duration_ms >= 0
    )
);

-- The stream reads strictly forward from a cursor.
CREATE INDEX email_events_id_idx ON email_events (id);
