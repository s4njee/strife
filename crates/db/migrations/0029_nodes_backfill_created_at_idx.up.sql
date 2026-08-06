-- no-transaction
-- Every backfill refill pass walks nodes in (created_at, id) order to find the
-- next batch of email-shaped files after the campaign cursor. Without a
-- matching btree the planner parallel-scans all file nodes and sorts them on
-- each pass (measured at ~40-80s against the live library). Partial to
-- kind='file' AND lifecycle_state='active' because those are the only rows a
-- backfill ever visits. CONCURRENTLY so the live archive keeps accepting
-- writes while the index builds.
CREATE INDEX CONCURRENTLY IF NOT EXISTS nodes_backfill_created_at_idx
    ON nodes (created_at, id)
    WHERE kind = 'file' AND lifecycle_state = 'active';
