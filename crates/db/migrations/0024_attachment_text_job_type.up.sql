-- Split from 0025 because PostgreSQL refuses to use a new enum value inside the
-- transaction that added it, and sqlx runs each migration in one transaction.
ALTER TYPE job_type ADD VALUE IF NOT EXISTS 'attachment_extraction';
