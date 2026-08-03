-- Additive only. The column starts NULL on existing rows and is populated in
-- bounded batches by `backfill_email_search_vectors`, never by a table-wide
-- rewrite during migration or API startup.
ALTER TABLE email_messages ADD COLUMN search_vector tsvector;

-- Builds one message's weighted search vector from every contributing table.
--
-- Prose uses the `english` configuration so "meeting" matches "meetings".
-- Addresses, labels, attachment filenames, and message ids use `simple`:
-- stemming would mangle `a.reyes@example.test` into tokens that no exact
-- address filter could ever match again.
-- Subject and body arrive as arguments rather than being re-selected: during
-- a BEFORE INSERT trigger the message row does not exist yet, so a self-select
-- would silently index an empty subject and body.
CREATE FUNCTION email_search_vector(p_node_id UUID, p_subject TEXT, p_body TEXT)
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

-- On insert the dependent rows do not exist yet, so this computes subject and
-- body only. The dependent-table triggers below touch the message afterwards,
-- which re-fires this and picks up addresses, labels, and filenames.
CREATE FUNCTION email_messages_refresh_search_vector() RETURNS trigger AS $$
BEGIN
    NEW.search_vector := email_search_vector(NEW.node_id, NEW.subject, NEW.body_text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER email_messages_search_vector
    BEFORE INSERT OR UPDATE ON email_messages
    FOR EACH ROW EXECUTE FUNCTION email_messages_refresh_search_vector();

-- Touches the owning message so its BEFORE trigger recomputes. The UPDATE
-- fires only the BEFORE trigger, which does not itself update, so this cannot
-- recurse.
CREATE FUNCTION email_dependent_touch_message() RETURNS trigger AS $$
DECLARE
    v_node_id UUID;
BEGIN
    v_node_id := coalesce(NEW.node_id, OLD.node_id);
    UPDATE email_messages SET updated_at = updated_at WHERE node_id = v_node_id;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER email_addresses_search_vector
    AFTER INSERT OR UPDATE OR DELETE ON email_addresses
    FOR EACH ROW EXECUTE FUNCTION email_dependent_touch_message();

CREATE TRIGGER email_labels_search_vector
    AFTER INSERT OR UPDATE OR DELETE ON email_labels
    FOR EACH ROW EXECUTE FUNCTION email_dependent_touch_message();

CREATE TRIGGER email_attachments_search_vector
    AFTER INSERT OR UPDATE OR DELETE ON email_attachments
    FOR EACH ROW EXECUTE FUNCTION email_dependent_touch_message();

-- Created empty here because the column is empty. Populating the archive is a
-- separate bounded operation; see `backfill.md` for the concurrent rebuild used
-- when the index must be recreated against a full archive.
CREATE INDEX email_messages_search_vector_idx
    ON email_messages USING GIN (search_vector);

-- Supports the attachment-presence filter without scanning the manifest.
CREATE INDEX email_messages_attachment_presence_idx
    ON email_messages (attachment_count) WHERE attachment_count > 0;
