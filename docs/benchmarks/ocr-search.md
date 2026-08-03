# OCR Search Benchmark

This benchmark establishes an initial PostgreSQL full-text-search baseline for Story 15.1. It is a development measurement, not a production service-level objective.

## Dataset and method

- Date: 2026-08-02
- Host architecture: ARM64 macOS, PostgreSQL 17.10 in the development Docker Compose service
- Corpus: 1,000 active documents, 10 pages per document, 10,000 generated page rows total
- Match selectivity: one marker term on page 7 of every hundredth document, producing 10 matches
- Query: English `websearch_to_tsquery`, page-level `search_vector` match, active-node join, `ts_rank_cd` relevance ordering, and a 25-row limit
- Isolation: the seed, `ANALYZE`, and query ran in one transaction and ended with `ROLLBACK`; no benchmark rows remain

## Result

PostgreSQL used `document_text_pages_search_vector_idx` through a bitmap index scan. The measured planning time was **0.406 ms** and execution time was **1.223 ms**. The query returned all 10 matching pages and touched 153 shared buffers; the final relevance sort used 25 kB.

This result supports keeping the first document-text search implementation in PostgreSQL. It is not evidence that a dedicated search service will never be needed: repeat the benchmark with a production-sized corpus, realistic OCR text distributions, concurrent requests, and Raspberry Pi storage before making that decision.

## Reproduction query

The benchmark inserts deterministic UUIDs for 1,000 documents and 10 pages each, runs `ANALYZE document_text_pages`, then executes:

```sql
EXPLAIN (ANALYZE, BUFFERS)
SELECT
    p.node_id,
    p.page_number,
    ts_rank_cd(
        p.search_vector,
        websearch_to_tsquery('english', 'benchmarkneedle')
    ) AS score
FROM document_text_pages AS p
JOIN nodes AS n ON n.id = p.node_id
WHERE p.search_vector @@ websearch_to_tsquery('english', 'benchmarkneedle')
  AND n.lifecycle_state = 'active'
ORDER BY score DESC, p.id
LIMIT 25;
```

Keep the seed and measurement inside `BEGIN` / `ROLLBACK` when repeating it against a development database.
