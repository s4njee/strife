CREATE TABLE metadata_events (
    id BIGSERIAL PRIMARY KEY,
    job_id UUID REFERENCES jobs (id) ON DELETE SET NULL,
    node_id UUID REFERENCES nodes (id) ON DELETE SET NULL,
    node_name TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'running', 'retrying', 'completed', 'failed', 'skipped', 'cancelled')
    ),
    attempt INTEGER NOT NULL,
    extractor_name TEXT,
    duration_ms BIGINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT metadata_events_attempt_nonnegative CHECK (attempt >= 0),
    CONSTRAINT metadata_events_duration_nonnegative CHECK (
        duration_ms IS NULL OR duration_ms >= 0
    )
);

CREATE INDEX metadata_events_created_idx ON metadata_events (created_at, id);
CREATE INDEX jobs_metadata_state_idx ON jobs (state)
    WHERE job_type = 'metadata_extraction';

CREATE FUNCTION record_metadata_job_event()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    event_state TEXT;
    event_duration_ms BIGINT;
    event_extractor TEXT;
BEGIN
    IF NEW.job_type <> 'metadata_extraction' THEN
        RETURN NEW;
    END IF;

    IF TG_OP = 'INSERT' THEN
        event_state := 'queued';
    ELSIF NEW.state = OLD.state THEN
        RETURN NEW;
    ELSE
        event_state := CASE
            WHEN NEW.state = 'pending' AND OLD.state = 'leased' THEN 'retrying'
            WHEN NEW.state = 'leased' THEN 'running'
            WHEN NEW.state = 'completed' THEN 'completed'
            WHEN NEW.state = 'failed' THEN 'failed'
            WHEN NEW.state = 'skipped' THEN 'skipped'
            WHEN NEW.state = 'cancelled' THEN 'cancelled'
            ELSE NULL
        END;
    END IF;

    IF event_state IS NULL THEN
        RETURN NEW;
    END IF;

    IF event_state IN ('completed', 'failed', 'skipped') THEN
        SELECT extractor_name
        INTO event_extractor
        FROM metadata_records
        WHERE node_id = NEW.target_node_id
          AND extractor_name <> 'mime'
        ORDER BY updated_at DESC, extractor_name
        LIMIT 1;
    END IF;

    IF event_state IN ('completed', 'failed', 'skipped', 'retrying') THEN
        SELECT GREATEST(
            0,
            (EXTRACT(EPOCH FROM (clock_timestamp() - created_at)) * 1000)::BIGINT
        )
        INTO event_duration_ms
        FROM metadata_events
        WHERE job_id = NEW.id AND state = 'running'
        ORDER BY id DESC
        LIMIT 1;
    END IF;

    INSERT INTO metadata_events (
        job_id, node_id, node_name, state, attempt, extractor_name,
        duration_ms, error_message
    )
    SELECT
        NEW.id,
        node.id,
        node.name,
        event_state,
        NEW.attempts,
        event_extractor,
        event_duration_ms,
        CASE
            WHEN event_state IN ('failed', 'retrying', 'skipped') THEN NEW.last_error
            ELSE NULL
        END
    FROM nodes AS node
    WHERE node.id = NEW.target_node_id;

    RETURN NEW;
END;
$$;

CREATE TRIGGER metadata_job_events
AFTER INSERT OR UPDATE OF state ON jobs
FOR EACH ROW
EXECUTE FUNCTION record_metadata_job_event();
