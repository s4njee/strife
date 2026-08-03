CREATE TYPE email_extraction_status AS ENUM (
    'pending',
    'completed',
    'failed',
    'skipped',
    'unsupported'
);

CREATE TYPE email_address_role AS ENUM (
    'from',
    'sender',
    'reply_to',
    'to',
    'cc',
    'bcc'
);

-- One parsed projection per `.eml` node. The node remains the canonical
-- original; every row here is regenerable and cascades with it.
CREATE TABLE email_messages (
    node_id UUID PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    status email_extraction_status NOT NULL DEFAULT 'pending',
    parser_name TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    message_id TEXT,
    normalized_message_id TEXT,
    in_reply_to TEXT,
    reference_ids TEXT[] NOT NULL DEFAULT '{}',
    subject TEXT,
    normalized_subject TEXT,
    sent_at TIMESTAMPTZ,
    received_at TIMESTAMPTZ,
    body_text TEXT NOT NULL DEFAULT '',
    body_html TEXT,
    preview_text TEXT NOT NULL DEFAULT '',
    content_hash TEXT,
    thread_group_id UUID,
    duplicate_group_id UUID,
    provider_thread_id TEXT,
    attachment_count INTEGER NOT NULL DEFAULT 0,
    warnings TEXT[] NOT NULL DEFAULT '{}',
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT email_messages_attachment_count_nonnegative
        CHECK (attachment_count >= 0),
    CONSTRAINT email_messages_duration_nonnegative
        CHECK (duration_ms IS NULL OR duration_ms >= 0)
);

-- Thread and duplicate identifiers are grouping hints, never uniqueness
-- constraints: several originals legitimately share both.
CREATE INDEX email_messages_thread_group_idx
    ON email_messages (thread_group_id) WHERE thread_group_id IS NOT NULL;
CREATE INDEX email_messages_duplicate_group_idx
    ON email_messages (duplicate_group_id) WHERE duplicate_group_id IS NOT NULL;
CREATE INDEX email_messages_normalized_message_id_idx
    ON email_messages (normalized_message_id)
    WHERE normalized_message_id IS NOT NULL;
CREATE INDEX email_messages_content_hash_idx
    ON email_messages (content_hash) WHERE content_hash IS NOT NULL;
CREATE INDEX email_messages_sent_at_idx ON email_messages (sent_at, node_id);
CREATE INDEX email_messages_status_idx ON email_messages (status);
CREATE INDEX email_messages_parser_version_idx ON email_messages (parser_version);

-- Addresses stay structured and ordered by role rather than flattened into one
-- string, so `from`/`to`/`cc` filters can be exact and indexed.
CREATE TABLE email_addresses (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    role email_address_role NOT NULL,
    position INTEGER NOT NULL,
    display_name TEXT,
    address TEXT NOT NULL,
    CONSTRAINT email_addresses_position_nonnegative CHECK (position >= 0),
    UNIQUE (node_id, role, position)
);

CREATE INDEX email_addresses_address_idx ON email_addresses (address, role);
CREATE INDEX email_addresses_node_idx ON email_addresses (node_id);

-- Repeated headers such as `Received` must survive, so ordering is part of the
-- key rather than the header name alone.
CREATE TABLE email_headers (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    value TEXT NOT NULL,
    CONSTRAINT email_headers_position_nonnegative CHECK (position >= 0),
    UNIQUE (node_id, position)
);

CREATE INDEX email_headers_name_idx ON email_headers (node_id, normalized_name);

CREATE TABLE email_labels (
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    PRIMARY KEY (node_id, label)
);

CREATE INDEX email_labels_label_idx ON email_labels (label);

-- The attachment manifest is recorded during parsing. Materializing bytes into
-- artifacts is separate work; `part_path` identifies the MIME part until then.
CREATE TABLE email_attachments (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    part_path TEXT NOT NULL,
    position INTEGER NOT NULL,
    filename TEXT,
    media_type TEXT NOT NULL,
    disposition TEXT,
    content_id TEXT,
    transfer_encoding TEXT,
    decoded_size BIGINT,
    checksum_sha256 TEXT,
    is_inline BOOLEAN NOT NULL DEFAULT false,
    is_message BOOLEAN NOT NULL DEFAULT false,
    extraction_status email_extraction_status NOT NULL DEFAULT 'pending',
    warnings TEXT[] NOT NULL DEFAULT '{}',
    CONSTRAINT email_attachments_position_nonnegative CHECK (position >= 0),
    CONSTRAINT email_attachments_size_nonnegative
        CHECK (decoded_size IS NULL OR decoded_size >= 0),
    UNIQUE (node_id, part_path)
);

CREATE INDEX email_attachments_node_idx ON email_attachments (node_id, position);
CREATE INDEX email_attachments_filename_idx
    ON email_attachments (filename) WHERE filename IS NOT NULL;
