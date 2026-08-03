DROP TRIGGER IF EXISTS metadata_job_events ON jobs;
DROP FUNCTION IF EXISTS record_metadata_job_event();
DROP INDEX IF EXISTS jobs_metadata_state_idx;
DROP INDEX IF EXISTS metadata_events_created_idx;
DROP TABLE IF EXISTS metadata_events;
