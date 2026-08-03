# Strife — Email Archive Search Scrum Epics & Stories

> Derived from the requirement to search roughly ten years of Gmail-exported `.eml` files already held by Strife. Epic numbering continues from [`ocr.md`](ocr.md), which ends at Epic 16. Stories are ordered by dependency within each epic. Point estimates use a Fibonacci scale (1, 2, 3, 5, 8, 13). Acceptance criteria are written so a mid-level developer can implement and self-verify without ambiguity.

**Planning decisions:** Each `.eml` remains an immutable Strife file and canonical original · parsed email data is regenerable and linked to the file's node · email gets a dedicated structured schema rather than being forced into page-oriented OCR tables · MIME is detected from bytes as `message/rfc822`, not trusted from the extension · extraction is automatic for files finalized after the feature deploys · historical files are processed only through an explicit, paused-by-default backfill campaign · deployment never enqueues the existing library wholesale · PostgreSQL weighted full-text search is the initial search engine · subject and correspondents rank above body text · the complete normalized body is indexed initially · plain text is preferred and HTML-only bodies are converted to text · rendered HTML is sanitized, scripts are forbidden, and remote resources are blocked by default · attachments, Gmail labels, raw headers, and thread relationships are preserved when present · duplicate detection never deletes originals · email is read-only and reparsable · search excludes trash by default · a dedicated Email tab provides structured filters, results, message reading, status, campaign controls, and bounded reprocessing · email parsing, OCR, and attachment extraction share the admission-control strategy in [`backfill.md`](backfill.md).

---

## Epic 17 — Email Decisions, Schema & Queue Foundation

**Goal:** Email has a recorded architectural boundary, durable structured storage, a first-class job type, and representative regression fixtures before parsing behavior becomes a compatibility contract.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 17.1 — Record the Email Archive Decision

As a developer, I want the email architecture recorded so that parsing, storage, search, deduplication, and security decisions are not rediscovered during implementation. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `docs/decisions/0009-email-archive-search.md` follows the context, decision, alternatives, consequences, and date shape used by ADR 0008.
- [ ] The ADR records that original `.eml` files remain immutable nodes and parsed rows are disposable, regenerable projections.
- [ ] The ADR chooses a dedicated email schema instead of overloading `document_text_pages`, explaining that email requires structured sender, recipient, date, thread, label, and attachment fields rather than page semantics.
- [ ] The ADR chooses PostgreSQL weighted full-text search initially and requires a measured production-scale benchmark before considering OpenSearch, Elasticsearch, or another service.
- [ ] The ADR records the initial body policy: index the complete normalized body, preserve both plain and HTML representations when available, and defer quote/signature removal until ranking is measured.
- [ ] The ADR records non-destructive duplicate handling: retain every original node, assign duplicate groups, and collapse duplicates in search by default with an explicit way to reveal every copy.
- [ ] The ADR records the safe-rendering policy: sanitize HTML, execute no active content, make links explicit, and block remote images/resources unless the user deliberately reveals them.
- [ ] The ADR records that deploying parser support does not start historical processing: new files may enqueue foreground jobs immediately, while existing files require an explicitly started backfill campaign.
- [ ] The ADR adopts the shared campaign, priority, admission-control, migration, and Orion rollout contract in [`backfill.md`](backfill.md).
- [ ] `README.md` links the ADR and this email implementation plan.

---

### Story 17.2 — Structured Email Schema

As a developer, I want durable email tables linked to file nodes so that parsing results are queryable, replaceable, and removed with their originals. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The shared `0017_backfill_campaigns` migration described by [`backfill.md`](backfill.md) lands first; a reversible `0018_email_messages.{up,down}.sql` migration then adds email storage without rewriting existing node or job rows.
- [ ] `email_extraction_status` uses `pending`, `completed`, `failed`, `skipped`, and `unsupported` states consistent with existing extractor status vocabulary.
- [ ] `email_messages` uses `node_id` as its primary key and an `ON DELETE CASCADE` foreign key to `nodes`.
- [ ] `email_messages` stores parser status/version, RFC `Message-ID`, `In-Reply-To`, ordered `References`, subject, sent and received timestamps, normalized plain body, optional sanitized-source HTML, preview text, attachment count, warnings, duration, and created/updated timestamps.
- [ ] Address data preserves display name, normalized address, address role (`from`, `sender`, `reply_to`, `to`, `cc`, `bcc`), and stable order without flattening all recipients into one string.
- [ ] Raw headers are preserved in a queryable representation without discarding repeated headers such as `Received`.
- [ ] `email_attachments` stores MIME part identity, parent email node, filename, media type, disposition, content ID, transfer encoding, decoded size, checksum, inline status, and extraction status.
- [ ] Gmail-specific metadata is optional: labels and provider thread IDs are stored when headers contain them, but ordinary RFC email does not require Gmail headers.
- [ ] Duplicate-group and thread-group identifiers are nullable and indexed; neither is a uniqueness constraint on the original node.
- [ ] Database APIs atomically replace a message, its addresses, headers, labels, and attachment manifest so reparsing cannot produce mixed parser versions.
- [ ] PostgreSQL integration tests cover insertion, atomic replacement, repeated headers, address order, cascades, and constraints.

---

### Story 17.3 — Email Extraction Job Type

As a developer, I want email parsing to use Strife's durable job queue so that a large archive survives restarts and individual malformed messages can be retried. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] A reversible `0019_email_job_type.{up,down}.sql` migration adds `email_extraction` to `job_type` and verifies the existing active-job uniqueness rule applies per node.
- [ ] Email jobs carry `origin`, an optional campaign ID, and a resource class supplied by the shared backfill foundation. `origin` uses all three values shipped in `0017_backfill_campaigns` — `foreground`, `repair`, and `backfill` — not just the first and last: Story 18.6's reprocess scopes are `repair` work, and Story 22.2's priority ordering requires `repair` as a distinct middle tier.
- [ ] The Rust `JobType` enum, API serialization, claim order, and worker dispatch all recognize `email_extraction`.
- [ ] Email extraction is claimed after metadata and preview work but before OCR, because parsing MIME is cheaper than OCR and unlocks the Email tab quickly.
- [ ] Claiming prefers foreground work regardless of job-family order; historical email cannot hide new uploads, imports, metadata, previews, repairs, or deletions behind its backlog.
- [ ] Email jobs have a documented lease duration and retry count appropriate for bounded MIME parsing.
- [ ] Existing `GET /api/jobs` endpoints return email jobs without changing their response shape.
- [ ] Tests cover enqueue uniqueness, lease renewal, retry with `last_error`, completion, and clean handling when the node disappears.

---

### Story 17.4 — Representative Email Fixture Corpus

As a developer, I want committed, synthetic email fixtures covering real MIME edge cases so that parser upgrades cannot silently change the archive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Fixtures contain synthetic identities and content only; no personal mailbox data, live addresses, authentication headers, or secrets are committed.
- [ ] The corpus covers plain text, HTML-only, multipart alternative, mixed text and attachments, inline `cid:` images, nested `message/rfc822`, quoted-printable, base64, UTF-8, a legacy charset, RFC 2047 encoded headers, folded headers, repeated recipients, missing `Message-ID`, malformed dates, and malformed MIME boundaries.
- [ ] At least one fixture carries Gmail label/thread headers and one deliberately does not.
- [ ] At least one duplicate pair has the same `Message-ID`; another has no `Message-ID` but identical canonical content.
- [ ] Expected normalized fields are stored beside fixtures in a reviewable form so failures show semantic differences rather than opaque snapshots.
- [ ] Fixture sizes remain small enough for ordinary tests; separately generated large-message fixtures are used for limit tests.

---

## Epic 18 — MIME Extraction & Durable Email Projection

**Goal:** Every RFC email is parsed safely and deterministically into structured, replaceable database records while malformed messages remain visible and diagnosable.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 18.1 — Native RFC/MIME Adapter

As a developer, I want an email-aware parsing adapter so that Strife correctly handles MIME structure and RFC headers without depending on Tika's document abstraction. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] A parser adapter is added under `crates/media/src` and exported by the crate.
- [ ] The selected Rust parser is actively maintained, has no network behavior, accepts bounded byte input, and passes the committed fixture corpus; the dependency and selection rationale are recorded in ADR 0009.
- [ ] The adapter returns a typed result containing normalized headers, addresses, body alternatives, labels, thread hints, attachment descriptors, warnings, and parser version.
- [ ] MIME media types, charsets, transfer encodings, dispositions, filenames, content IDs, and nested part paths are retained.
- [ ] Header names are compared case-insensitively while original values and repeated-header order remain available.
- [ ] MIME detection verifies `message/rfc822` from content bytes; extension and upload-provided MIME are treated only as hints.
- [ ] Parsing performs no DNS resolution, link fetching, remote-image loading, or attachment execution.
- [ ] Unit tests assert every committed fixture's normalized semantic result.

---

### Story 18.2 — Body Selection & Text Normalization

As a user, I want readable, searchable message text regardless of whether a sender supplied plain text or HTML. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] For `multipart/alternative`, the normalized searchable body prefers a usable `text/plain` alternative while retaining the HTML alternative for safe rendering.
- [ ] HTML-only messages are converted to plain text with paragraph, list, table-cell, and line-break boundaries preserved well enough for snippets.
- [ ] HTML conversion removes scripts, styles, comments, hidden content, tracking markup, and URLs from resource attributes without fetching anything.
- [ ] Character-set decoding supports UTF-8 and the legacy charset fixture, replaces invalid sequences deterministically, and records a warning when decoding is lossy.
- [ ] Unicode is normalized consistently before hashing and indexing; line endings become `\n`.
- [ ] The initial implementation indexes quoted replies and signatures exactly as normalized, matching ADR 0009 rather than applying heuristic content deletion.
- [ ] Body and preview text have independent configurable byte limits so an enormous HTML alternative cannot exhaust memory or dominate result payloads.
- [ ] Tests cover plain preference, HTML-only conversion, whitespace behavior, lossy decoding, empty bodies, and limit warnings.

---

### Story 18.3 — Header, Address & Date Normalization

As a user, I want searches and message details to use reliable correspondents and dates despite variations in RFC formatting. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Mailbox and group address syntax is parsed into display names and normalized addresses without discarding the original header.
- [ ] Address normalization lowercases the domain and preserves the local part; Gmail-specific dot or plus-address rewriting is not performed.
- [ ] Internationalized display names and domains remain displayable and searchable.
- [ ] `Date` is parsed with its original timezone offset; invalid or absent values remain null with a warning rather than using ingestion time silently.
- [ ] A defensible received timestamp is derived from trace headers only when parsing succeeds, and its provenance is documented.
- [ ] `Message-ID`, `In-Reply-To`, and `References` are normalized for comparison while their raw values remain available.
- [ ] Subject normalization decodes encoded words and derives a separate thread-comparison subject without changing the displayed subject.
- [ ] Tests cover address groups, quoted names, duplicates across roles, encoded subjects, timezone offsets, invalid dates, and missing headers.

---

### Story 18.4 — Attachment Manifest Extraction

As a user, I want attachment metadata preserved during message parsing so that search and message details accurately describe each email. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Every non-body MIME part is represented in `email_attachments`, including unnamed and inline parts.
- [ ] RFC 2231/5987-style encoded filenames and split parameters are decoded safely.
- [ ] Filenames are display values only and are never used directly as filesystem paths.
- [ ] Decoded size and SHA-256 are computed while streaming with configurable per-part and per-message decoded-byte ceilings.
- [ ] Inline parts preserve content IDs so sanitized message HTML can resolve them through authenticated Strife endpoints later.
- [ ] Nested `message/rfc822` parts are identified distinctly from ordinary binary attachments.
- [ ] A malformed attachment records a part-level warning without discarding an otherwise readable message.
- [ ] Tests cover duplicate filenames, no filename, inline images, nested messages, encoded filenames, and size-limit behavior.

---

### Story 18.5 — Email Job Handler

As a system, I want a worker handler that turns managed `.eml` originals into atomic structured records so that extraction is durable and idempotent. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] The handler mirrors established metadata/OCR handlers: lifecycle check, managed-original copy, byte MIME detection, bounded parse, atomic persistence, cleanup, and structured logging.
- [ ] Active RFC email records `completed`; non-email files record `unsupported`; trashed files are skipped; missing or permanently deleted nodes fail cleanly without orphan rows.
- [ ] Message, addresses, headers, labels, and attachment manifest are replaced in one transaction.
- [ ] Reprocessing the same node with the same parser version produces the same normalized records and no duplicates.
- [ ] Parser errors preserve the underlying cause in warnings and the job's `last_error`; error text exposed through APIs is sanitized.
- [ ] Duration, source size, decoded body bytes, attachment count, warning count, and peak process memory where measurable are logged.
- [ ] Temporary originals and decoded parts are removed on success, error, cancellation, and panic.
- [ ] Integration tests cover the complete fixture corpus, corrupt input, trash, deletion during processing, retry after lease expiry, and atomic rollback on a forced persistence error.

---

### Story 18.6 — Foreground Enqueue, Campaign Backfill & Reprocessing API

As a user, I want newly imported `.eml` files parsed automatically and the historical archive processed only through an explicit bounded campaign. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Direct-upload and watched-folder finalization create equivalent foreground email-extraction jobs for files finalized after deployment.
- [ ] Deployment, migrations, API startup, worker startup, and ordinary recovery never scan the historical library or create historical email jobs implicitly.
- [ ] Candidate selection uses detected or strongly indicated RFC email MIME and remains recoverable through a read-only preflight and explicit campaign when finalization lacks reliable MIME information.
- [ ] Historical processing uses the campaign scheduler in [`backfill.md`](backfill.md), starting paused and refilling only when active queued/running work falls below its low-water mark.
- [ ] Initial campaign defaults are a 100-node batch, at most 500 queued jobs, and one running email backfill job; every value is configurable and recorded on the campaign.
- [ ] Only one historical heavy-processing campaign is active on Orion initially, so email, OCR, and attachment backfills cannot run together.
- [ ] `POST /api/admin/reprocess` accepts email scopes for one node, failed messages, missing records, and parser-version mismatch.
- [ ] Nodes with active email jobs are excluded before the batch limit is applied, preventing a batch from reporting zero while eligible work remains later in the result set.
- [ ] Duplicate requests are no-ops and the response returns the number actually enqueued.
- [ ] Pausing a campaign stops refilling immediately while allowing leased work to finish; resuming continues from the durable cursor without rescanning completed nodes.
- [ ] Tests cover upload, watched-folder import, inert deployment/startup, explicit campaign start, pause/resume, low-water refill, all reprocess scopes, bounded batches, and active-job suppression.

---

## Epic 19 — Email Full-Text Search & Query API

**Goal:** Subject, correspondents, body text, labels, and attachment names become fast, relevant, filterable, and safely highlighted across the archive.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 19.1 — Weighted PostgreSQL Email Index

As a developer, I want an email-specific full-text index so that relevance reflects how people search mail rather than treating every token equally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Staged, reversible `0020_email_search.{up,down}.sql` schema changes add the email search vector without a table-rewriting startup migration; historical vector population happens in bounded batches.
- [ ] The large GIN index is built through the separately executed operational migration path in [`backfill.md`](backfill.md), using concurrent/index-safe behavior rather than blocking API startup on a full archive build.
- [ ] Subject and primary correspondents receive weight A, recipients and labels weight B, attachment filenames weight B, and normalized body weight C.
- [ ] English stemming is used for prose while a non-stemming configuration preserves addresses, IDs, labels, and filenames.
- [ ] Existing email rows are indexed by the migration or an explicit transactional backfill; only indexing future inserts is insufficient.
- [ ] Index maintenance remains automatic when any contributing field, address, label, or attachment filename changes.
- [ ] Search ranking uses cover density and includes a deterministic date/id tie-breaker.
- [ ] Indexes support sent-date, sender-address, recipient-address, attachment presence, label, status, duplicate group, and thread group filters.
- [ ] Tests prove that a subject match outranks the same body-only match and that address tokens are not mangled by stemming.

---

### Story 19.2 — Email Search API

As a user, I want a dedicated search endpoint returning email-shaped results so that the frontend does not reconstruct messages from generic file matches. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `GET /api/email/search` accepts `q`, cursor, and bounded limit parameters and returns subject, correspondents, sent date, highlighted snippet, attachment count, labels, thread/duplicate counts, node ID, and score.
- [ ] PostgreSQL generates snippets from normalized body text with markers that the frontend can parse without injecting HTML.
- [ ] Empty or whitespace-only `q` is allowed only when at least one structured filter is present; an entirely unconstrained query returns the unified `400` error body.
- [ ] Trashed nodes are excluded by default and included only through an explicit parameter.
- [ ] Duplicate groups collapse to one result by default, choosing a deterministic active representative; `include_duplicates=true` returns individual originals.
- [ ] Cursor pagination is stable across equal scores using score, sent date, and node ID rather than offsets.
- [ ] Internal failures are logged with context and mapped to the shared error response without leaking SQL or message contents.
- [ ] Integration tests cover hit, miss, snippet, ranking, cursor pagination, trash, collapsed duplicates, and individual duplicates.

---

### Story 19.3 — Structured Mail Filters

As a user, I want sender, recipient, date, attachment, label, and status filters so that broad ten-year searches can be narrowed precisely. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The API supports repeatable `from`, `to`, `cc`, `bcc`, and `participant` filters with exact normalized-address matching.
- [ ] `after` and `before` use documented inclusive/exclusive semantics and reject invalid or reversed ranges.
- [ ] `has_attachment`, label, extraction status, thread ID, duplicate group, and MIME-type filters are supported.
- [ ] Multiple values within one field and filters across fields have explicitly documented OR/AND semantics.
- [ ] Display-name substring search is separate from exact address filtering.
- [ ] Filter-only queries remain indexed and cursor-paginated; they do not materialize the entire archive in application memory.
- [ ] Query parsing rejects unknown fields and excessive repeated parameters with a unified `400` response.
- [ ] Tests cover every filter independently, representative combinations, Unicode names, case behavior, and invalid input.

---

### Story 19.4 — Message & Facet APIs

As a user, I want complete message details and useful filter counts so that search results can be explored without downloading raw MIME. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `GET /api/email/messages/:node_id` returns structured headers, ordered addresses, labels, body alternatives, warnings, attachment manifest, thread hints, and duplicate information.
- [ ] Raw headers require an explicit query parameter and remain bounded; the default response includes only normalized fields.
- [ ] `GET /api/email/facets` returns bounded counts for labels, years, top correspondents, attachment presence, and extraction states using aggregate SQL.
- [ ] Facets respect active/trash scope and the currently supplied structured filters where practical.
- [ ] Long address/header/label lists are paginated or capped with a documented continuation mechanism.
- [ ] Missing projections distinguish not processed, pending, failed, unsupported, and absent node states.
- [ ] API tests cover response ordering, repeated headers, state mapping, facet counts, bounds, and node lifecycle behavior.

---

### Story 19.5 — Archive-Scale Search Benchmark

As an operator, I want search measured against a corpus resembling the real Gmail archive so that PostgreSQL remains an evidence-based choice. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `docs/benchmarks/email-search.md` records hardware, PostgreSQL version/configuration, corpus sizes, body-byte distribution, label/address cardinality, and index size.
- [ ] A synthetic benchmark covers at least 100,000 messages or the real archive count, whichever is greater, without copying personal message content into the repository.
- [ ] Measurements include cold and warm text queries, selective and broad terms, sender/date/filter-only queries, duplicate collapsing, facets, and deep cursor pages.
- [ ] `EXPLAIN (ANALYZE, BUFFERS)` confirms expected GIN/B-tree index use and records planning/execution percentiles rather than one favorable run.
- [ ] Ingestion throughput, index-growth rate, vacuum behavior, and peak database disk use are recorded.
- [ ] Explicit thresholds define when to tune PostgreSQL and when a dedicated search service should be reconsidered.
- [ ] The benchmark transaction or cleanup procedure leaves no synthetic rows in the operational archive.

---

## Epic 20 — Email Navigation, Search & Reader UI

**Goal:** A dedicated Email tab lets users search, filter, inspect, and safely read the archive using Strife's established visual and accessibility language.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 20.1 — Email Sidebar Navigation & Status Badge

As a user, I want an Email entry in the sidebar so that the archive is a first-class Strife surface. **Estimated: 2 points.**

**Acceptance Criteria:**

- [ ] An Email navigation item is placed near OCR and Imports, with `/email` registered in the Solid router.
- [ ] The item uses the existing active treatment and icon system rather than introducing a separate navigation style.
- [ ] A badge shows pending plus running email jobs and is omitted at zero.
- [ ] The pending count is updated from the email status stream without refreshing the page.
- [ ] Backfill counts are visually distinct from foreground processing so a paused historical campaign does not make new mail appear stuck.
- [ ] Static preview mode renders the entry and deterministic sample count without contacting the backend.

---

### Story 20.2 — Email Search & Filter Interface

As a user, I want a responsive mail-search interface so that I can find messages by text and structured fields without memorizing URL parameters. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The Email page has one primary full-text input plus controls for sender/participant, date range, labels, attachment presence, trash, and duplicates.
- [ ] Search input is debounced and prior requests are cancelled when criteria change.
- [ ] Filter state is reflected in the URL so searches can be bookmarked and browser navigation works.
- [ ] A clear-all action resets every criterion predictably.
- [ ] Loading, no indexed mail, no matches, parse failure, offline, and retry states are distinct.
- [ ] Facet options remain bounded and searchable when the archive contains many correspondents or labels.
- [ ] Results load through cursor pagination or virtualization without an ever-growing unbounded DOM.
- [ ] Static preview mode includes representative results and active filters.

---

### Story 20.3 — Email Result List

As a user, I want results that look and behave like messages so that I can evaluate matches without opening every file. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Each result shows sender, subject, sent date, safe highlighted snippet, attachment indicator/count, relevant labels, and duplicate/thread count when greater than one.
- [ ] Missing subjects, senders, or dates have useful accessible fallbacks rather than blank cells.
- [ ] Unread semantics are not invented because a static export does not reliably preserve current Gmail read state.
- [ ] Search highlights are constructed as text/`mark` nodes from server markers and never assigned through `innerHTML`.
- [ ] Selection, hover, focus, and multi-line truncation follow existing Strife tokens and work in light and dark themes.
- [ ] Results are keyboard navigable with a visible focus position and announced result count.
- [ ] Selecting a result opens its message reader without losing the search URL or scroll position.

---

### Story 20.4 — Safe Message Reader

As a user, I want to read plain or formatted email safely so that archived messages are useful without executing hostile historical content. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] The reader shows subject, ordered correspondents, sent/received date, labels, attachment list, warnings, and expandable normalized headers.
- [ ] Plain text remains selectable and preserves meaningful line breaks.
- [ ] HTML is sanitized server-side or with a pinned, tested sanitizer configuration and rendered in an isolated boundary that cannot affect the Strife application DOM.
- [ ] Scripts, forms, event handlers, embedded objects, CSS imports, automatic redirects, and active SVG content are removed or inert.
- [ ] Remote images, fonts, stylesheets, frames, and media are blocked by default; revealing remote resources requires an explicit warning and action.
- [ ] Links display their destination, use safe schemes only, and cannot give the opened page access to Strife's window context.
- [ ] Inline `cid:` references resolve only to authenticated attachment endpoints belonging to the same message.
- [ ] The reader offers plain-text fallback, copy controls, and download-original without an edit affordance.
- [ ] Security tests use deliberately hostile HTML fixtures covering every blocked capability.

---

### Story 20.5 — Responsive & Accessible Email Experience

As a user using a keyboard, screen reader, or narrow display, I want the email interface to remain fully operable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Search, filters, result list, reader, headers, and attachments have correct labels, landmarks, names, and focus order.
- [ ] Opening/closing the reader moves and restores focus predictably; Escape behavior does not discard search state.
- [ ] Result and processing updates use restrained live-region announcements without reading every streamed event during a large backfill.
- [ ] Desktop supports a useful list/reader layout while narrow widths use a single-pane navigation model without horizontal page scrolling.
- [ ] Dates and attachment sizes have machine-readable values and localized display values.
- [ ] Automated accessibility checks and keyboard-focused component tests cover the primary search-to-reader flow.
- [ ] `eslint --max-warnings 0`, Prettier, TypeScript build, and static-preview build pass.

---

## Epic 21 — Attachment Search, Threads & Gmail Context

**Goal:** Attachments and conversation context become searchable and navigable without compromising originals, security, or resource limits.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 21.1 — Bounded Attachment Materialization

As a system, I want attachment bytes decoded into managed, regenerable artifacts so that they can be downloaded, previewed, and processed without reparsing the entire message each time. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Attachment artifacts use deterministic storage keys derived from email node and MIME part identity and never trust attachment filenames as paths.
- [ ] Materialization streams transfer decoding and hashing rather than holding the full part in memory.
- [ ] Per-part, per-message, total-artifact, compression/nesting, and timeout limits are configurable with documented provisional defaults.
- [ ] Partial output is deleted on failure and reruns replace artifacts idempotently.
- [ ] Artifact rows retain source message, part path, checksum, size, media type, and parser version.
- [ ] Nested `message/rfc822` attachments have an explicit maximum recursion depth and are not silently imported as top-level user files.
- [ ] Tests cover binary, inline, duplicate-name, nested-message, oversized, malformed-transfer, cancellation, and rerun cases.

---

### Story 21.2 — Attachment Content Extraction & Search

As a user, I want text inside supported attachments included in email search so that a message can be found by the document it carried. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] Supported document and image attachments reuse the existing Tika and OCR adapters instead of introducing duplicate extractors.
- [ ] Attachment text is stored with attachment identity, extractor source/version, status, warnings, page number, confidence where applicable, and bounded text bytes.
- [ ] The email search vector includes attachment filenames at weight B and extracted attachment text at a lower documented weight than message body.
- [ ] Search results identify whether the match came from subject, headers, body, attachment filename, or attachment content.
- [ ] Attachment matches report attachment name and page number when available and open the relevant attachment preview.
- [ ] Unsupported and failed attachments do not fail the containing message's completed extraction state.
- [ ] Reprocessing can target one attachment, failed attachments, missing text, or extractor-version mismatches in bounded batches.
- [ ] Tests cover text PDF, scanned PDF, office document, image, unsupported binary, mixed-success message, ranking, snippets, and version reprocessing.

---

### Story 21.3 — Secure Attachment Download & Preview

As a user, I want to inspect archived attachments without exposing Strife to unsafe inline content. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Authenticated endpoints stream attachment artifacts with range support and sanitized `Content-Disposition` filenames.
- [ ] The safe-inline allowlist matches Strife's file preview policy; HTML, SVG, executable, script, and unknown types download as attachments rather than render in the application origin.
- [ ] Preview generation reuses existing artifact pipelines and never executes macros or embedded active content.
- [ ] Inline `cid:` images use same-message authorization and cannot reference arbitrary storage keys.
- [ ] Missing/regenerating artifacts return explicit states and can enqueue bounded rematerialization.
- [ ] Tests cover traversal filenames, header injection, MIME spoofing, ranges, inline authorization, deleted source messages, and unsafe types.

---

### Story 21.4 — Thread Reconstruction, Labels & Duplicate Exploration

As a user, I want conversation and Gmail context reconstructed where evidence exists so that a decade of related mail is easier to navigate. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Provider thread IDs are authoritative only when present and internally consistent.
- [ ] Standards-based threading uses normalized `Message-ID`, `In-Reply-To`, and `References`; normalized subject is a documented fallback, not the primary key.
- [ ] Missing parents and cycles are handled deterministically without dropping messages.
- [ ] Thread ordering uses sent date with stable fallbacks and exposes messages missing reliable dates.
- [ ] Gmail labels are preserved as imported facts; Strife does not claim they remain synchronized with Gmail.
- [ ] Duplicate grouping uses normalized `Message-ID` plus canonical-content hash fallback and records the grouping reason.
- [ ] The UI can expand a thread, reveal collapsed duplicates, navigate to each original node, and filter by labels.
- [ ] Tests cover reply chains, forks, missing parents, cycles, subject-only fallbacks, conflicting provider IDs, duplicate groups, and label Unicode.

---

## Epic 22 — Backfill Operations, Security & Production Readiness

**Goal:** The ten-year archive can be indexed safely, observed in real time, resumed after interruption, upgraded, and validated at production scale.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 22.1 — Email Status API & SSE Backfill Console

As a user, I want live extraction progress so that a long initial archive backfill is observable without page refreshes or polling. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] `GET /api/email/status` reports candidate, pending, running, completed, failed, skipped, unsupported, remaining, attachment-processing, parser version, and indexed-message counts using aggregate SQL.
- [ ] Status separates foreground jobs from each historical campaign and reports campaign state, durable cursor, configured batch/queue/running limits, throughput, and estimated completion time.
- [ ] `GET /api/email/events` streams `entry` and `status` server-sent events with the same keep-alive, cursor-resume, and disconnect-resource guarantees as the import/OCR streams.
- [ ] Events include node, filename, extracted subject when safe, state, attachment count, duration, and concise warning; they never include body text or sensitive raw headers.
- [ ] The Email page includes a bounded newest-first processing console and connection state.
- [ ] Failed messages support per-file retry and confirmed bounded bulk retry.
- [ ] Authorized campaign controls start, pause, resume, and cancel; start requires a completed preflight and explicit confirmation of candidate count and resource policy.
- [ ] Repeated connection/disconnection tests prove database connections are released.
- [ ] Empty, mixed, active-backfill, all-complete, and parser-version-mismatch status tests use isolated PostgreSQL databases.

---

### Story 22.2 — Parser Resource Limits & Failure Isolation

As an operator, I want hostile or pathological email bounded so that one message cannot stop the archive backfill. **Estimated: 5 points.**

**Acceptance Criteria:**

- [ ] Configurable limits cover source bytes, MIME parts, header bytes/count, nesting depth, decoded body bytes, decoded attachment bytes, attachments per message, total decoded bytes, parser time, and stored warnings.
- [ ] Defaults are documented as provisional and profiled on Orion before being treated as final.
- [ ] Limit failures name the exact limit, persist a safe warning, and become terminal when retry cannot change the outcome.
- [ ] Parser work is isolated consistently with other extractors; any remaining memory/process isolation gap is recorded in `docs/known-limitations.md`.
- [ ] Worker concurrency for email and attachment extraction is separately configurable, and OCR/email/attachment backfills acquire the shared `HEAVY_CPU` admission permit before claiming work.
- [ ] `OCR_CONCURRENCY`, `EMAIL_PARSE_CONCURRENCY`, `ATTACHMENT_EXTRACTION_CONCURRENCY`, and `HEAVY_CPU_CONCURRENCY` default to 1 on Orion; the shared permit is enforced across worker processes through the renewable `worker_resource_leases` slots created by `0017_backfill_campaigns`, not through an advisory lock held for the duration of a parse.
- [ ] Email body parsing starts under `heavy_cpu` admission and moves to `extractor` only after the 10,000-message canary in Story 22.5 records safe resource behavior, per the resource-class table in [`backfill.md`](backfill.md); attachment extraction and every Tesseract path stay `heavy_cpu` regardless. The promotion is a recorded, reversible configuration change with a named owner, not an undocumented code edit.
- [ ] Foreground jobs use higher priority than repair jobs, which use higher priority than historical jobs; a documented fairness budget lets backfill progress without starving new work.
- [ ] Backfill worker containers have explicit CPU and memory ceilings in Compose; application concurrency is not treated as a substitute for a container CPU quota.
- [ ] Structured logs contain message/node/job identifiers and measurements but no body, subject, address, raw header, or attachment content.
- [ ] Synthetic tests cover every limit, cancellation, worker restart, lease expiry, malformed recursive MIME, and cleanup.

---

### Story 22.3 — Email Privacy & Rendering Threat Model

As an operator, I want archived email treated as hostile sensitive content so that search and rendering do not create a new exfiltration surface. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] `docs/security/email-threat-model.md` covers malicious MIME, parser vulnerabilities, decompression expansion, HTML/script injection, CSS leaks, tracking pixels, remote resources, unsafe URLs, attachment MIME spoofing, header injection, sensitive logs, and search-snippet leakage.
- [ ] Worker/container network policy prevents email parsing and attachment extraction from making outbound requests where deployment capabilities allow it.
- [ ] HTML sanitizer configuration is pinned and regression-tested against the threat-model fixture set.
- [ ] API responses set appropriate content types and defensive headers; raw email and unsafe attachments never render inline in the Strife application origin.
- [ ] Search and event logs redact query/body/address values by default while retaining operational correlation IDs.
- [ ] Security dependency updates trigger the relevant parser, sanitizer, MIME, and attachment regression suites.

---

### Story 22.4 — Versioning, Repair & Operational Controls

As an operator, I want parser/index versions and repair tools exposed so that upgrades and interrupted backfills are manageable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [ ] Current parser, sanitizer, normalization, attachment-extractor, and search-index versions are recorded independently where their changes require different reprocessing.
- [ ] An admin status response reports version distributions and counts requiring reprocessing.
- [ ] Bounded repair commands detect missing projections, orphan artifacts, attachment manifest/artifact mismatches, stale active jobs, and index-version mismatches without mutating state in dry-run mode.
- [ ] Mutating repair/reprocess actions require explicit scope and batch limits and report exactly what changed.
- [ ] Campaign repair detects cursor/count drift and active-job mismatches, and it cannot convert a paused campaign to running as a side effect.
- [ ] Restarting workers during backfill resumes leases and does not duplicate messages, addresses, labels, attachments, or events.
- [ ] Runbooks defer to [`backfill.md`](backfill.md) and document initial preflight, foreground-only deployment, canary stages, pause/resume, parser upgrade, index rebuild, failure triage, and image rollback.

---

### Story 22.5 — End-to-End Archive Validation

As a maintainer, I want production-shaped validation from import through search and reading so that the feature is trustworthy before indexing the only historical archive. **Estimated: 8 points.**

**Acceptance Criteria:**

- [ ] An end-to-end test imports a directory tree of synthetic `.eml` files through the watched-folder path and verifies automatic extraction, status events, weighted search, filters, snippets, reader data, attachments, duplicate collapse, threads, trash exclusion, and permanent-delete cascades.
- [ ] Direct upload of the same fixture produces equivalent parsed and searchable results.
- [ ] Restart tests interrupt parsing, attachment materialization, attachment text extraction, and index backfill, then verify idempotent recovery.
- [ ] A preflight command scans the real archive read-only and reports file count, total bytes, MIME confidence, size percentiles, malformed candidates, duplicate estimates, and projected database/artifact disk use without exposing message content.
- [ ] The rollout follows [`backfill.md`](backfill.md): additive migration and foreground-only deploy first; then email canaries of 100, 1,000, and 10,000; then the full email body campaign; ordinary OCR only after email body indexing; attachment text and attachment OCR last.
- [ ] Each canary records throughput, p50/p95 duration, CPU, memory, temperature, I/O wait, database/index growth, failures, and estimated completion time before the next stage is authorized.
- [ ] The 10,000-message canary explicitly decides the email resource-class promotion from Story 22.2 — promote to `extractor` or hold at `heavy_cpu` — and records the measurement the decision rests on.
- [ ] PostgreSQL and managed-storage backup requirements are documented before full backfill; parsed projections may be rebuilt, but canonical `.eml` originals must be included in backup and restore drills.
- [ ] ARM64 Orion validation records throughput, CPU, memory, database growth, attachment growth, and the final resource-limit adjustments.
- [ ] Rust formatting, zero-warning Clippy, full workspace tests, frontend formatting/lint/build, migration rollback checks, and worker-image build all pass.

---

## Summary

| Epic                                                      | Milestone | Stories | Estimated Points |
| --------------------------------------------------------- | --------- | ------- | ---------------- |
| 17 — Email Decisions, Schema & Queue Foundation           | M17       | 4       | 16               |
| 18 — MIME Extraction & Durable Email Projection           | M18       | 6       | 31               |
| 19 — Email Full-Text Search & Query API                   | M19       | 5       | 23               |
| 20 — Email Navigation, Search & Reader UI                 | M20       | 5       | 20               |
| 21 — Attachment Search, Threads & Gmail Context           | M21       | 4       | 23               |
| 22 — Backfill Operations, Security & Production Readiness | M22       | 5       | 24               |
| **Email Total**                                           |           | **29**  | **137**          |

> [!TIP]
> At the roughly 30-points-per-sprint planning velocity used by [`scrum.md`](../scrum.md), the complete plan is approximately **5 sprints**. A useful first release can stop after Stories 17.1–20.5: structured extraction, weighted body/header search, filters, and a safe reader. Epic 21 adds attachment content and richer archive context; Epic 22 is required before an unattended full-history production backfill.

> [!IMPORTANT]
> Suggested order: **17.1 and 17.2 first**, because parser output and schema form the compatibility contract. Build **17.4 before 18.1** so parser selection is evidence-based. Complete 18.5 before indexing. Stories 19.1–19.4 and 20.1–20.3 can then overlap. Do not render HTML before 20.4's security boundary exists, and do not launch the full ten-year backfill before Stories 22.1–22.5 are complete.

> [!NOTE]
> Shipping the feature and starting the historical backfill are deliberately separate operations. Production deployment uses foreground processing only with every campaign paused. The operator follows [`backfill.md`](backfill.md) after health checks, backup validation, and a read-only archive preflight.

> [!WARNING]
> Four choices remain intentionally measurement-driven: whether quoted reply/signature stripping improves ranking, the final parser and attachment resource limits on Orion, the corpus size at which PostgreSQL stops meeting latency targets, and whether attachment artifacts should be backed up or regenerated. The stories above require measurements and explicit documentation before changing the initial decisions.
