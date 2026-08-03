CREATE TABLE ocr_engine_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    engine_name TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    language TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ocr_events (
    id BIGSERIAL PRIMARY KEY,
    node_id UUID REFERENCES nodes (id) ON DELETE SET NULL,
    node_name TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('running', 'completed', 'failed', 'skipped', 'unsupported')
    ),
    page_count INTEGER,
    mean_confidence REAL,
    warning TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT ocr_events_page_count_nonnegative CHECK (
        page_count IS NULL OR page_count >= 0
    ),
    CONSTRAINT ocr_events_confidence_range CHECK (
        mean_confidence IS NULL OR mean_confidence BETWEEN 0.0 AND 100.0
    )
);

CREATE INDEX jobs_ocr_state_idx ON jobs (state)
    WHERE job_type = 'ocr';
CREATE INDEX document_text_engine_version_idx
    ON document_text (engine_version, status);
