CREATE TYPE metadata_status AS ENUM ('pending', 'completed', 'failed', 'unsupported');
CREATE TYPE media_stream_type AS ENUM ('video', 'audio', 'subtitle');
CREATE TYPE media_kind AS ENUM ('document', 'image', 'video', 'audio', 'other');

CREATE TABLE metadata_records (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    extractor_name TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    status metadata_status NOT NULL DEFAULT 'pending',
    raw_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    warnings TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id, extractor_name)
);

CREATE TABLE media_streams (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes (id) ON DELETE CASCADE,
    stream_index INTEGER NOT NULL,
    stream_type media_stream_type NOT NULL,
    codec TEXT NOT NULL,
    width INTEGER,
    height INTEGER,
    duration_ms BIGINT,
    bitrate_bps BIGINT,
    frame_rate TEXT,
    language TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (node_id, stream_index),
    CONSTRAINT media_streams_dimensions_nonnegative CHECK (
        (width IS NULL OR width >= 0) AND (height IS NULL OR height >= 0)
    ),
    CONSTRAINT media_streams_duration_nonnegative CHECK (duration_ms IS NULL OR duration_ms >= 0),
    CONSTRAINT media_streams_bitrate_nonnegative CHECK (bitrate_bps IS NULL OR bitrate_bps >= 0)
);

CREATE TABLE node_metadata (
    node_id UUID PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    detected_mime TEXT NOT NULL,
    media_kind media_kind NOT NULL,
    duration_ms BIGINT,
    width INTEGER,
    height INTEGER,
    capture_time TIMESTAMPTZ,
    page_count INTEGER,
    orientation INTEGER,
    has_gps BOOLEAN NOT NULL DEFAULT false,
    gps_latitude DOUBLE PRECISION,
    gps_longitude DOUBLE PRECISION,
    camera_make TEXT,
    camera_model TEXT,
    document_title TEXT,
    document_author TEXT,
    document_created_at TIMESTAMPTZ,
    document_modified_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT node_metadata_dimensions_nonnegative CHECK (
        (width IS NULL OR width >= 0) AND (height IS NULL OR height >= 0)
    ),
    CONSTRAINT node_metadata_duration_nonnegative CHECK (duration_ms IS NULL OR duration_ms >= 0),
    CONSTRAINT node_metadata_page_count_nonnegative CHECK (page_count IS NULL OR page_count >= 0),
    CONSTRAINT node_metadata_gps_pair CHECK (
        (gps_latitude IS NULL) = (gps_longitude IS NULL)
        AND has_gps = (gps_latitude IS NOT NULL)
    )
);
