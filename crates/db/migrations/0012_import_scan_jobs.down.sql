DROP INDEX jobs_active_import_source_unique;
DROP INDEX jobs_active_type_target_unique;

DELETE FROM jobs WHERE job_type = 'import_scan';

ALTER TABLE jobs DROP CONSTRAINT jobs_import_source_matches_type;
ALTER TABLE jobs DROP COLUMN import_source_id;

CREATE UNIQUE INDEX jobs_active_type_target_unique
    ON jobs (job_type, target_node_id)
    WHERE state IN ('pending', 'leased');
