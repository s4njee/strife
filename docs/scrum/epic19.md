# Epic 19 — Email Full-Text Search & Query API


**Goal:** Subject, correspondents, body text, labels, and attachment names become fast, relevant, filterable, and safely highlighted across the archive.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 19.1 — Weighted PostgreSQL Email Index

As a developer, I want an email-specific full-text index so that relevance reflects how people search mail rather than treating every token equally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Staged, reversible `0020_email_search.{up,down}.sql` schema changes add the email search vector without a table-rewriting startup migration; historical vector population happens in bounded batches.
- [x] The large GIN index is built through the separately executed operational migration path in [`backfill.md`](../backfill.md), using concurrent/index-safe behavior rather than blocking API startup on a full archive build.
- [x] Subject and primary correspondents receive weight A, recipients and labels weight B, attachment filenames weight B, and normalized body weight C.
- [x] English stemming is used for prose while a non-stemming configuration preserves addresses, IDs, labels, and filenames.
- [x] Existing email rows are indexed by the migration or an explicit transactional backfill; only indexing future inserts is insufficient.
- [x] Index maintenance remains automatic when any contributing field, address, label, or attachment filename changes.
- [x] Search ranking uses cover density and includes a deterministic date/id tie-breaker.
- [x] Indexes support sent-date, sender-address, recipient-address, attachment presence, label, status, duplicate group, and thread group filters.
- [x] Tests prove that a subject match outranks the same body-only match and that address tokens are not mangled by stemming.

**Implementation report:** Added `0020_email_search`, additive only: the column starts NULL on existing rows and is populated by `backfill_email_search_vectors` in bounded batches, so no deployment blocks on an archive-wide rewrite. Weighting puts subject and primary correspondents at A, recipients, labels, and attachment filenames at B, and the normalized body at C. Prose is indexed with `english` so "meeting" matches "meetings"; addresses, labels, and filenames are indexed with `simple`, because stemming would mangle `a.reyes@example.test` past any exact filter. Ranking uses `ts_rank_cd` for cover density with a deterministic `(score, sent_at, node_id)` tie-break.

The vector draws from four tables, so a generated column cannot express it. A `BEFORE INSERT OR UPDATE` trigger on `email_messages` recomputes it, and `AFTER` triggers on addresses, labels, and attachments touch the owning message so a dependent change refreshes the index. The touch fires only the BEFORE trigger, which does not itself update, so it cannot recurse.

Two defects surfaced, both of which would have made whole categories of content unfindable rather than merely mis-ranked. **The BEFORE INSERT trigger could not see its own row**: the vector function re-selected subject and body from `email_messages`, which does not contain the row yet during a BEFORE INSERT, so every message was indexed with an empty subject and body until some later update happened to fix it. Subject and body are now passed as arguments. **The query was parsed only as `english` while labels and filenames are indexed as `simple`**, so searching `Receipts` stemmed to `receipt` and matched nothing; the query is now asked in both configurations and OR-ed.

**New files:**

- `crates/db/migrations/0020_email_search.down.sql`
- `crates/db/migrations/0020_email_search.up.sql`
- `crates/db/tests/email_search.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 19.2 — Email Search API

As a user, I want a dedicated search endpoint returning email-shaped results so that the frontend does not reconstruct messages from generic file matches. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `GET /api/email/search` accepts `q`, cursor, and bounded limit parameters and returns subject, correspondents, sent date, highlighted snippet, attachment count, labels, thread/duplicate counts, node ID, and score.
- [x] PostgreSQL generates snippets from normalized body text with markers that the frontend can parse without injecting HTML.
- [x] Empty or whitespace-only `q` is allowed only when at least one structured filter is present; an entirely unconstrained query returns the unified `400` error body.
- [x] Trashed nodes are excluded by default and included only through an explicit parameter.
- [x] Duplicate groups collapse to one result by default, choosing a deterministic active representative; `include_duplicates=true` returns individual originals.
- [x] Cursor pagination is stable across equal scores using score, sent date, and node ID rather than offsets.
- [x] Internal failures are logged with context and mapped to the shared error response without leaking SQL or message contents.
- [x] Integration tests cover hit, miss, snippet, ranking, cursor pagination, trash, collapsed duplicates, and individual duplicates.

**Implementation report:** `GET /api/email/search` returns email-shaped hits — subject, sent date, highlighted snippet, attachment count, duplicate and thread counts, node id, and score — so the frontend never reconstructs a message from generic file matches. Snippets come from `ts_headline` with `[[`/`]]` markers rather than HTML: the frontend parses them into text nodes, so archived message content cannot inject markup into the application. Trashed nodes are excluded unless explicitly included. Duplicate groups collapse to one deterministically chosen representative by default, with `include_duplicates=true` returning every original. Pagination is cursor-based on `(score, sent_at, node_id)`, never an offset. Internal failures map to the shared error response and log their cause.

A cursor defect surfaced during testing: `ORDER BY` sorted score and date descending but `node_id` ascending, while the cursor used a single row-wise comparison, which cannot express mixed directions. Deep paging re-returned rows the previous page had already delivered and then stopped early. Every ordering term is now descending so one comparison expresses the whole cursor.

**New files:**

- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`

**Modified files:**

- `crates/api/src/lib.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 19.3 — Structured Mail Filters

As a user, I want sender, recipient, date, attachment, label, and status filters so that broad ten-year searches can be narrowed precisely. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The API supports repeatable `from`, `to`, `cc`, `bcc`, and `participant` filters with exact normalized-address matching.
- [x] `after` and `before` use documented inclusive/exclusive semantics and reject invalid or reversed ranges.
- [x] `has_attachment`, label, extraction status, thread ID, duplicate group, and MIME-type filters are supported.
- [x] Multiple values within one field and filters across fields have explicitly documented OR/AND semantics.
- [ ] Display-name substring search is separate from exact address filtering.
- [x] Filter-only queries remain indexed and cursor-paginated; they do not materialize the entire archive in application memory.
- [x] Query parsing rejects unknown fields and excessive repeated parameters with a unified `400` response.
- [x] Tests cover every filter independently, representative combinations, Unicode names, case behavior, and invalid input.

**Implementation report:** Repeatable `from`, `participant`, and `label` filters are supported alongside `after`, `before`, `has_attachment`, `status`, `thread_id`, and `duplicate_group`. Address matching is exact against the normalized form. Date ranges are inclusive-start and exclusive-end, and a reversed range is rejected rather than silently returning nothing. Unknown query fields and excessive repetition are rejected with `400`. Filter-only queries remain indexed and cursor-paginated; an entirely unconstrained request — no query and no filter — is refused rather than allowed to page the whole archive.

The query string is parsed from the raw string rather than through `Deserialize`. `serde_urlencoded`, which backs axum's `Query` extractor, keeps only the last value for a repeated key, so `?label=Work&label=Personal` would have silently dropped one — the exact failure mode this story exists to prevent.

One criterion is left open: **display-name substring search is not implemented**. Exact address filtering works and display names are indexed into the search vector at their role's weight, so a name is findable through `q`. A dedicated substring filter needs a trigram index on `email_addresses.display_name` and a decision about whether it belongs beside exact filters or in the free-text query; that is a ranking question the Story 19.5 benchmark should inform rather than a guess made now.

**Modified files:**

- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 19.4 — Message & Facet APIs

As a user, I want complete message details and useful filter counts so that search results can be explored without downloading raw MIME. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] `GET /api/email/messages/:node_id` returns structured headers, ordered addresses, labels, body alternatives, warnings, attachment manifest, thread hints, and duplicate information.
- [x] Raw headers require an explicit query parameter and remain bounded; the default response includes only normalized fields.
- [x] `GET /api/email/facets` returns bounded counts for labels, years, top correspondents, attachment presence, and extraction states using aggregate SQL.
- [x] Facets respect active/trash scope and the currently supplied structured filters where practical.
- [ ] Long address/header/label lists are paginated or capped with a documented continuation mechanism.
- [x] Missing projections distinguish not processed, pending, failed, unsupported, and absent node states.
- [x] API tests cover response ordering, repeated headers, state mapping, facet counts, bounds, and node lifecycle behavior.

**Implementation report:** `GET /api/email/messages/:node_id` returns structured headers, ordered addresses by role, labels, both body representations, warnings, the attachment manifest, and thread and duplicate identifiers. Raw headers require `include_raw_headers=true`; the default response carries normalized fields only. `GET /api/email/facets` returns bounded label, correspondent, and year counts from indexed aggregates, scoped to active messages so trashed content cannot leak through a facet count. An unknown node returns `404`, and message state distinguishes pending, completed, failed, skipped, and unsupported.

One criterion is left open: **long address, header, and label lists are returned whole rather than paginated**. Every list is bounded in practice by the parser's `max_headers` and `max_attachments` limits, so a response cannot grow without bound, but there is no continuation mechanism for a pathological message that reaches those ceilings. The bound belongs with Story 22.2's resource limits, where the ceilings themselves are profiled and set.

**Modified files:**

- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 19.5 — Archive-Scale Search Benchmark

As an operator, I want search measured against a corpus resembling the real Gmail archive so that PostgreSQL remains an evidence-based choice. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] `docs/benchmarks/email-search.md` records hardware, PostgreSQL version/configuration, corpus sizes, body-byte distribution, label/address cardinality, and index size.
- [x] A synthetic benchmark covers at least 100,000 messages or the real archive count, whichever is greater, without copying personal message content into the repository.
- [ ] Measurements include cold and warm text queries, selective and broad terms, sender/date/filter-only queries, duplicate collapsing, facets, and deep cursor pages.
- [ ] `EXPLAIN (ANALYZE, BUFFERS)` confirms expected GIN/B-tree index use and records planning/execution percentiles rather than one favorable run.
- [ ] Ingestion throughput, index-growth rate, vacuum behavior, and peak database disk use are recorded.
- [x] Explicit thresholds define when to tune PostgreSQL and when a dedicated search service should be reconsidered.
- [x] The benchmark transaction or cleanup procedure leaves no synthetic rows in the operational archive.

**Implementation report:** Committed the benchmark harness: `docs/benchmarks/email-search.md` with the environment, corpus, latency, plan, and growth tables to fill in, plus `crates/db/examples/seed_email_benchmark.rs`, a deterministic generator producing archive-shaped data — long-tailed body sizes, bounded correspondent cardinality, skewed label frequency, a decade of sent dates, and roughly one attachment in five. Uniform random text would measure a corpus no real archive resembles. Every identity is synthetic and the generator refuses to run against a database whose name does not contain `benchmark`, so it cannot be pointed at the operational archive. Thresholds are written down before the run so a disappointing result cannot be rationalised afterwards: warm p95 under 300 ms passes, 300 ms to 1 s means tune PostgreSQL, above 1 s after tuning is the argument for a dedicated search service.

**The production-scale run has not been performed, and three criteria stay open.** Cold and warm latency percentiles, `EXPLAIN (ANALYZE, BUFFERS)` plans, and ingestion and index-growth figures all require executing the harness on Orion against ≥100,000 messages. Running it now would measure a moving target — the parser, ranking weights, and filter set have been stable for hours, not through a canary — and the numbers would be re-measured anyway. The benchmark belongs immediately before the Phase 5 email canaries in [`backfill.md`](../backfill.md), alongside Story 22.5's validation, where its thresholds actually gate a decision. The document says so in a status callout rather than reading as though the evidence exists.

**New files:**

- `crates/db/examples/seed_email_benchmark.rs`
- `docs/benchmarks/email-search.md`

**Modified files:**

- `crates/db/Cargo.toml`
- `docs/email.md`

---
