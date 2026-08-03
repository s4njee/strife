# Email search benchmark

Records whether PostgreSQL full-text search meets Strife's latency targets at
archive scale, so the "dedicated search service" question in
[`deferred.md`](../../deferred.md) is decided on measurement rather than
intuition. ADR 0009 requires this before OpenSearch or Elasticsearch is
reconsidered.

> [!IMPORTANT]
> **Status: harness landed, production-scale run outstanding.** The generator
> and measurement procedure below are committed and runnable. The numbers this
> document must eventually carry — cold and warm latencies at ≥100,000
> messages on Orion — have not been captured yet. Do not cite this file as
> evidence that PostgreSQL is sufficient until the results table is filled in.

## Why this is not yet run

Story 19.5 sits in Epic 19, but the measurement it wants is rollout evidence,
not development evidence. Benchmarking an index before the parser, ranking
weights, and filter set are stable measures a moving target. The run belongs
immediately before the Phase 7 email canaries in [`backfill.md`](../backfill.md),
alongside Story 22.5's validation, where its thresholds actually gate a
decision.

## Corpus generation

The generator produces synthetic messages only. No personal mailbox content is
copied into the repository or into a benchmark database.

```bash
cargo run --release -p strife-db --example seed_email_benchmark -- \
  --database-url "$DATABASE_URL" --messages 100000
```

The generated corpus deliberately mirrors archive shape rather than uniform
random text:

- body length follows a long-tailed distribution, since a handful of very large
  messages dominate index size;
- correspondent cardinality is bounded — a real archive has a few hundred
  frequent correspondents and a long tail of one-offs;
- labels are drawn from a small set with skewed frequency;
- roughly one message in five carries an attachment;
- sent dates spread across ten years so date filters and year facets are
  exercised realistically.

## What to record

Fill in every section before drawing a conclusion.

### Environment

| Field | Value |
| --- | --- |
| Host | |
| CPU / cores | |
| RAM | |
| Storage | |
| PostgreSQL version | |
| `shared_buffers` | |
| `work_mem` | |
| `effective_cache_size` | |

### Corpus

| Field | Value |
| --- | --- |
| Messages | |
| Total body bytes | |
| Body bytes p50 / p95 / max | |
| Distinct correspondents | |
| Distinct labels | |
| Messages with attachments | |
| `email_messages` table size | |
| `search_vector` GIN index size | |

### Query latency

Each query is run cold (after `DISCARD ALL` and a cache drop where possible)
and warm, ten times, recording p50 and p95 rather than a single favourable run.

| Query shape | Cold p50 | Cold p95 | Warm p50 | Warm p95 |
| --- | --- | --- | --- | --- |
| Selective term (few hits) | | | | |
| Broad term (many hits) | | | | |
| Two-term phrase | | | | |
| Sender filter only | | | | |
| Date range only | | | | |
| Label + date + attachment | | | | |
| Duplicate-collapsing query | | | | |
| Facets | | | | |
| Deep cursor page (page 50) | | | | |

### Plans

`EXPLAIN (ANALYZE, BUFFERS)` for each shape above, confirming the GIN index is
used for text queries and B-tree indexes for filter-only queries. A sequential
scan on `email_messages` in any row is a failure, not a curiosity.

### Ingestion and growth

| Field | Value |
| --- | --- |
| Parse + persist throughput (messages/sec) | |
| Index growth per 10,000 messages | |
| Autovacuum behaviour during ingestion | |
| Peak database disk use | |

## Thresholds

These decide the outcome; they are set before the run so a disappointing result
cannot be rationalised afterwards.

- **Acceptable:** warm p95 under 300 ms for every query shape, cold p95 under
  1.5 s. PostgreSQL stays, no further action.
- **Tune PostgreSQL:** warm p95 between 300 ms and 1 s. Revisit `work_mem`,
  `shared_buffers`, GIN `fastupdate`, and whether the body needs its own
  index before considering a different engine.
- **Reconsider the engine:** warm p95 above 1 s after tuning, or index size
  exceeding half the database volume. Only then is a dedicated search service
  justified, and the measurement in this file is the argument for it.

## Cleanup

The generator writes into a dedicated benchmark database. It must never be
pointed at the operational archive, and the run leaves no synthetic rows
behind:

```bash
psql -c 'DROP DATABASE strife_email_benchmark'
```
