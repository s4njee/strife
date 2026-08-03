CREATE TYPE job_origin AS ENUM ('foreground', 'repair', 'backfill');
CREATE TYPE job_resource_class AS ENUM ('light', 'extractor', 'preview', 'heavy_cpu', 'heavy_io');
CREATE TYPE backfill_kind AS ENUM ('email', 'ocr', 'attachment_text', 'attachment_ocr');
CREATE TYPE backfill_state AS ENUM ('draft', 'paused', 'running', 'draining', 'completed', 'cancelled', 'failed');

CREATE TABLE backfill_campaigns (
    id UUID PRIMARY KEY,
    kind backfill_kind NOT NULL,
    state backfill_state NOT NULL DEFAULT 'draft',
    candidate_definition JSONB NOT NULL DEFAULT '{}'::jsonb,
    snapshot_before TIMESTAMPTZ,
    cursor_created_at TIMESTAMPTZ,
    cursor_node_id UUID,
    batch_size INTEGER NOT NULL DEFAULT 100,
    max_queued INTEGER NOT NULL DEFAULT 500,
    max_running INTEGER NOT NULL DEFAULT 1,
    resource_class job_resource_class NOT NULL,
    foreground_fairness INTEGER NOT NULL DEFAULT 20,
    candidate_count BIGINT NOT NULL DEFAULT 0,
    enqueued_count BIGINT NOT NULL DEFAULT 0,
    completed_count BIGINT NOT NULL DEFAULT 0,
    failed_count BIGINT NOT NULL DEFAULT 0,
    skipped_count BIGINT NOT NULL DEFAULT 0,
    created_by_version TEXT NOT NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    paused_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    CONSTRAINT backfill_campaign_batch_size_positive CHECK (batch_size > 0),
    CONSTRAINT backfill_campaign_max_queued_positive CHECK (max_queued > 0),
    CONSTRAINT backfill_campaign_max_running_positive CHECK (max_running > 0),
    CONSTRAINT backfill_campaign_fairness_positive CHECK (foreground_fairness > 0),
    CONSTRAINT backfill_campaign_counts_nonnegative CHECK (
        candidate_count >= 0 AND enqueued_count >= 0 AND completed_count >= 0
        AND failed_count >= 0 AND skipped_count >= 0
    )
);

CREATE INDEX backfill_campaigns_state_created_idx
    ON backfill_campaigns (state, created_at, id);

CREATE TABLE backfill_campaign_events (
    id BIGSERIAL PRIMARY KEY,
    campaign_id UUID NOT NULL REFERENCES backfill_campaigns (id) ON DELETE CASCADE,
    old_state backfill_state,
    new_state backfill_state,
    event_type TEXT NOT NULL,
    reason TEXT,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX backfill_campaign_events_campaign_id_idx
    ON backfill_campaign_events (campaign_id, id);

ALTER TABLE jobs
    ADD COLUMN origin job_origin NOT NULL DEFAULT 'foreground',
    ADD COLUMN campaign_id UUID REFERENCES backfill_campaigns (id) ON DELETE SET NULL,
    ADD COLUMN resource_class job_resource_class NOT NULL DEFAULT 'light',
    ADD CONSTRAINT jobs_backfill_campaign_required CHECK (
        (origin = 'backfill' AND campaign_id IS NOT NULL)
        OR (origin <> 'backfill' AND campaign_id IS NULL)
    );

CREATE INDEX jobs_claim_origin_idx
    ON jobs (job_type, state, origin, priority DESC, created_at, id);
CREATE INDEX jobs_campaign_state_idx
    ON jobs (campaign_id, state) WHERE campaign_id IS NOT NULL;

CREATE TABLE worker_resource_leases (
    resource_class job_resource_class NOT NULL,
    slot_number INTEGER NOT NULL,
    lease_owner TEXT,
    job_id UUID REFERENCES jobs (id) ON DELETE SET NULL,
    lease_expires_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (resource_class, slot_number),
    CONSTRAINT worker_resource_lease_slot_positive CHECK (slot_number > 0),
    CONSTRAINT worker_resource_lease_fields_together CHECK (
        (lease_owner IS NULL AND job_id IS NULL AND lease_expires_at IS NULL)
        OR (lease_owner IS NOT NULL AND job_id IS NOT NULL AND lease_expires_at IS NOT NULL)
    )
);

INSERT INTO worker_resource_leases (resource_class, slot_number)
VALUES ('extractor', 1), ('preview', 1), ('heavy_cpu', 1), ('heavy_io', 1);

CREATE FUNCTION release_resource_lease_before_job_delete()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    UPDATE worker_resource_leases
    SET lease_owner = NULL, job_id = NULL, lease_expires_at = NULL, updated_at = now()
    WHERE job_id = OLD.id;
    RETURN OLD;
END;
$$;

CREATE TRIGGER jobs_release_resource_lease_before_delete
BEFORE DELETE ON jobs
FOR EACH ROW
EXECUTE FUNCTION release_resource_lease_before_job_delete();

CREATE FUNCTION record_backfill_job_outcome()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.campaign_id IS NOT NULL AND OLD.state IS DISTINCT FROM NEW.state THEN
        IF NEW.state = 'completed' THEN
            UPDATE backfill_campaigns
            SET completed_count = completed_count + 1, updated_at = now()
            WHERE id = NEW.campaign_id;
        ELSIF NEW.state = 'failed' THEN
            UPDATE backfill_campaigns
            SET failed_count = failed_count + 1, updated_at = now()
            WHERE id = NEW.campaign_id;
        ELSIF NEW.state = 'skipped' THEN
            UPDATE backfill_campaigns
            SET skipped_count = skipped_count + 1, updated_at = now()
            WHERE id = NEW.campaign_id;
        END IF;
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER jobs_record_backfill_outcome
AFTER UPDATE OF state ON jobs
FOR EACH ROW
EXECUTE FUNCTION record_backfill_job_outcome();

CREATE TABLE job_claim_fairness (
    job_type job_type PRIMARY KEY,
    foreground_claims_since_backfill INTEGER NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT job_claim_fairness_nonnegative CHECK (foreground_claims_since_backfill >= 0)
);
