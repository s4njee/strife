-- Keep the hot queue indexes proportional to runnable work rather than history.
CREATE INDEX jobs_claim_pending_idx
    ON jobs (job_type, origin, priority DESC, created_at, id)
    WHERE state = 'pending';

CREATE INDEX jobs_expired_lease_idx
    ON jobs (lease_expires_at)
    WHERE state = 'leased';
