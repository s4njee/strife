-- Text extracted from attachment artifacts, indexed alongside the message that
-- carried them so a message can be found by the document it attached.
--
-- Like every other email projection this is disposable: the .eml original is
-- canonical and every row here can be rebuilt from it.

CREATE TYPE email_attachment_text_source AS ENUM ('embedded', 'ocr');

CREATE TABLE email_attachment_text (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES email_messages (node_id) ON DELETE CASCADE,
    part_path TEXT NOT NULL,
    -- Page-oriented formats keep their pagination so a hit can say where in a
    -- document it was found. Single-page sources use 1.
    page_number INTEGER NOT NULL DEFAULT 1,
    content TEXT NOT NULL,
    source email_attachment_text_source NOT NULL,
    -- Recorded per page because a mixed PDF can have embedded text on one page
    -- and an OCR'd scan on the next.
    confidence REAL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id, part_path, page_number),
    CONSTRAINT email_attachment_text_page_positive CHECK (page_number >= 1)
);

CREATE INDEX email_attachment_text_node_idx
    ON email_attachment_text (node_id, part_path, page_number);

-- Extraction outcome lives on the artifact, one row per attachment, rather
-- than being repeated on every page of text it produced.
ALTER TABLE email_attachment_artifacts
    ADD COLUMN text_status email_extraction_status NOT NULL DEFAULT 'pending',
    ADD COLUMN text_extractor_name TEXT,
    ADD COLUMN text_extractor_version TEXT,
    ADD COLUMN text_bytes BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN text_warnings TEXT[] NOT NULL DEFAULT '{}',
    ADD CONSTRAINT email_attachment_artifacts_text_bytes_nonnegative
        CHECK (text_bytes >= 0);

-- Finds attachments needing extraction without scanning ready ones.
CREATE INDEX email_attachment_artifacts_text_status_idx
    ON email_attachment_artifacts (text_status)
    WHERE text_status = 'pending';

-- Rebuilt to add attachment text at weight D. The ordering of the weights is
-- the ranking policy: a term in the subject outranks the same term in the body,
-- and a term found only inside an attached document ranks below both, because
-- the message is being searched, not the attachment.
CREATE OR REPLACE FUNCTION email_search_vector(p_node_id UUID, p_subject TEXT, p_body TEXT)
RETURNS tsvector AS $$
DECLARE
    v_subject TEXT := coalesce(p_subject, '');
    v_body TEXT := coalesce(p_body, '');
    v_primary TEXT;
    v_recipients TEXT;
    v_labels TEXT;
    v_filenames TEXT;
    v_attachment_text TEXT;
BEGIN
    SELECT coalesce(string_agg(
               coalesce(display_name, '') || ' ' || address, ' '), '')
      INTO v_primary
      FROM email_addresses
     WHERE node_id = p_node_id AND role IN ('from', 'sender', 'reply_to');

    SELECT coalesce(string_agg(
               coalesce(display_name, '') || ' ' || address, ' '), '')
      INTO v_recipients
      FROM email_addresses
     WHERE node_id = p_node_id AND role IN ('to', 'cc', 'bcc');

    SELECT coalesce(string_agg(label, ' '), '')
      INTO v_labels FROM email_labels WHERE node_id = p_node_id;

    SELECT coalesce(string_agg(filename, ' '), '')
      INTO v_filenames
      FROM email_attachments
     WHERE node_id = p_node_id AND filename IS NOT NULL;

    SELECT coalesce(string_agg(content, ' ' ORDER BY part_path, page_number), '')
      INTO v_attachment_text
      FROM email_attachment_text
     WHERE node_id = p_node_id;

    RETURN
        setweight(to_tsvector('english', v_subject), 'A') ||
        setweight(to_tsvector('simple', v_subject), 'A') ||
        setweight(to_tsvector('simple', v_primary), 'A') ||
        setweight(to_tsvector('simple', v_recipients), 'B') ||
        setweight(to_tsvector('simple', v_labels), 'B') ||
        setweight(to_tsvector('simple', v_filenames), 'B') ||
        setweight(to_tsvector('english', v_body), 'C') ||
        setweight(to_tsvector('english', v_attachment_text), 'D');
END;
$$ LANGUAGE plpgsql STABLE;

CREATE TRIGGER email_attachment_text_search_vector
    AFTER INSERT OR UPDATE OR DELETE ON email_attachment_text
    FOR EACH ROW EXECUTE FUNCTION email_dependent_touch_message();
