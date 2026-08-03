ALTER TABLE jobs
    ADD COLUMN import_source_id UUID REFERENCES import_sources (id) ON DELETE CASCADE;

ALTER TABLE jobs
    ADD CONSTRAINT jobs_import_source_matches_type CHECK (
        (job_type = 'import_scan') = (import_source_id IS NOT NULL)
    );

DROP INDEX jobs_active_type_target_unique;

CREATE UNIQUE INDEX jobs_active_type_target_unique
    ON jobs (job_type, target_node_id)
    WHERE state IN ('pending', 'leased') AND job_type <> 'import_scan';

CREATE UNIQUE INDEX jobs_active_import_source_unique
    ON jobs (import_source_id)
    WHERE state IN ('pending', 'leased') AND job_type = 'import_scan';
