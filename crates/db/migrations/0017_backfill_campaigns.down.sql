DROP TABLE job_claim_fairness;
DROP TRIGGER jobs_record_backfill_outcome ON jobs;
DROP FUNCTION record_backfill_job_outcome();
DROP TRIGGER jobs_release_resource_lease_before_delete ON jobs;
DROP FUNCTION release_resource_lease_before_job_delete();
DROP TABLE worker_resource_leases;
DROP INDEX jobs_campaign_state_idx;
DROP INDEX jobs_claim_origin_idx;
ALTER TABLE jobs
    DROP CONSTRAINT jobs_backfill_campaign_required,
    DROP COLUMN resource_class,
    DROP COLUMN campaign_id,
    DROP COLUMN origin;
DROP TABLE backfill_campaign_events;
DROP TABLE backfill_campaigns;
DROP TYPE backfill_state;
DROP TYPE backfill_kind;
DROP TYPE job_resource_class;
DROP TYPE job_origin;
