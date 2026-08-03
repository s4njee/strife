ALTER TABLE email_messages
    DROP COLUMN IF EXISTS thread_reason,
    DROP COLUMN IF EXISTS duplicate_reason,
    DROP COLUMN IF EXISTS thread_conflict;

DROP TYPE IF EXISTS email_thread_reason;
DROP TYPE IF EXISTS email_duplicate_reason;
