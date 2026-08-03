CREATE TYPE document_text_source AS ENUM ('embedded', 'ocr');
CREATE TYPE document_text_status AS ENUM (
    'pending',
    'completed',
    'failed',
    'skipped',
    'unsupported'
);

CREATE TABLE document_text (
    node_id UUID PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    source document_text_source NOT NULL,
    status document_text_status NOT NULL DEFAULT 'pending',
    language TEXT NOT NULL,
    engine_name TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    page_count INTEGER,
    mean_confidence REAL,
    char_count INTEGER NOT NULL DEFAULT 0,
    warnings TEXT[] NOT NULL DEFAULT '{}',
    duration_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT document_text_page_count_nonnegative CHECK (
        page_count IS NULL OR page_count >= 0
    ),
    CONSTRAINT document_text_mean_confidence_range CHECK (
        mean_confidence IS NULL OR mean_confidence BETWEEN 0.0 AND 100.0
    ),
    CONSTRAINT document_text_char_count_nonnegative CHECK (char_count >= 0),
    CONSTRAINT document_text_duration_nonnegative CHECK (
        duration_ms IS NULL OR duration_ms >= 0
    )
);

CREATE TABLE document_text_pages (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    page_number INTEGER NOT NULL,
    content TEXT NOT NULL,
    confidence REAL,
    width INTEGER,
    height INTEGER,
    UNIQUE (node_id, page_number),
    CONSTRAINT document_text_pages_number_positive CHECK (page_number >= 1),
    CONSTRAINT document_text_pages_confidence_range CHECK (
        confidence IS NULL OR confidence BETWEEN 0.0 AND 100.0
    ),
    CONSTRAINT document_text_pages_dimensions_nonnegative CHECK (
        (width IS NULL OR width >= 0) AND (height IS NULL OR height >= 0)
    )
);

CREATE INDEX document_text_status_idx ON document_text (status);
CREATE INDEX document_text_pages_node_page_idx
    ON document_text_pages (node_id, page_number);
