CREATE TYPE node_kind AS ENUM ('folder', 'file');
CREATE TYPE node_lifecycle_state AS ENUM ('active', 'trashed', 'deleted');

CREATE TABLE nodes (
    id UUID PRIMARY KEY,
    parent_id UUID REFERENCES nodes (id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    kind node_kind NOT NULL,
    lifecycle_state node_lifecycle_state NOT NULL DEFAULT 'active',
    source_created_at TIMESTAMPTZ,
    source_modified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT nodes_not_own_parent CHECK (id <> parent_id)
);

CREATE UNIQUE INDEX nodes_active_sibling_name_unique
    ON nodes (parent_id, name)
    WHERE lifecycle_state = 'active';

INSERT INTO nodes (id, parent_id, name, kind)
VALUES ('00000000-0000-0000-0000-000000000001', NULL, 'root', 'folder')
ON CONFLICT (id) DO NOTHING;
