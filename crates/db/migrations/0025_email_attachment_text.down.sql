DROP TRIGGER IF EXISTS email_attachment_text_search_vector ON email_attachment_text;
DROP TABLE IF EXISTS email_attachment_text;
DROP TYPE IF EXISTS email_attachment_text_source;

ALTER TABLE email_attachment_artifacts
    DROP CONSTRAINT IF EXISTS email_attachment_artifacts_text_bytes_nonnegative,
    DROP COLUMN IF EXISTS text_status,
    DROP COLUMN IF EXISTS text_extractor_name,
    DROP COLUMN IF EXISTS text_extractor_version,
    DROP COLUMN IF EXISTS text_bytes,
    DROP COLUMN IF EXISTS text_warnings;

DROP INDEX IF EXISTS email_attachment_artifacts_text_status_idx;

-- Restores the 0020 definition without the attachment-text contribution.
CREATE OR REPLACE FUNCTION email_search_vector(p_node_id UUID, p_subject TEXT, p_body TEXT)
RETURNS tsvector AS $$
DECLARE
    v_subject TEXT := coalesce(p_subject, '');
    v_body TEXT := coalesce(p_body, '');
    v_primary TEXT;
    v_recipients TEXT;
    v_labels TEXT;
    v_filenames TEXT;
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

    RETURN
        setweight(to_tsvector('english', v_subject), 'A') ||
        setweight(to_tsvector('simple', v_subject), 'A') ||
        setweight(to_tsvector('simple', v_primary), 'A') ||
        setweight(to_tsvector('simple', v_recipients), 'B') ||
        setweight(to_tsvector('simple', v_labels), 'B') ||
        setweight(to_tsvector('simple', v_filenames), 'B') ||
        setweight(to_tsvector('english', v_body), 'C');
END;
$$ LANGUAGE plpgsql STABLE;
