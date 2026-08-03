ALTER TABLE document_text_pages
    ADD COLUMN search_vector TSVECTOR
    GENERATED ALWAYS AS (to_tsvector('english', COALESCE(content, ''))) STORED;

CREATE INDEX document_text_pages_search_vector_idx
    ON document_text_pages USING GIN (search_vector);
