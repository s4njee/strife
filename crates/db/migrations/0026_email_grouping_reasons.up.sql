-- Records *why* a message was placed in a thread or duplicate group.
--
-- Grouping archived mail is inference, not fact: a ten-year export contains
-- messages with no Message-ID, replies whose parents were never exported, and
-- provider thread ids that disagree with the RFC headers. Storing the reason
-- makes a surprising grouping explainable instead of mysterious, and lets a
-- weaker basis be filtered out later without recomputing everything.

CREATE TYPE email_thread_reason AS ENUM (
    'provider',   -- Gmail thread id, taken as authoritative
    'references', -- RFC References/In-Reply-To root
    'message_id', -- the message's own id; a thread of one so far
    'subject',    -- normalized subject fallback, the weakest basis
    'none'        -- nothing to group on
);

CREATE TYPE email_duplicate_reason AS ENUM (
    'message_id',   -- normalized Message-ID, the strong basis
    'content_hash', -- canonical content hash fallback
    'none'
);

ALTER TABLE email_messages
    ADD COLUMN thread_reason email_thread_reason NOT NULL DEFAULT 'none',
    ADD COLUMN duplicate_reason email_duplicate_reason NOT NULL DEFAULT 'none',
    -- True when a provider thread id was used but the RFC headers point at a
    -- different thread. The provider id still wins, because Gmail knows things
    -- the headers do not, but the disagreement is recorded rather than hidden.
    ADD COLUMN thread_conflict BOOLEAN NOT NULL DEFAULT false;
