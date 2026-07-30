CREATE TABLE favorites (
    node_id UUID PRIMARY KEY REFERENCES nodes (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX favorites_created_at_idx ON favorites (created_at DESC);
