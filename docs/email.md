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

- [x] `docs/decisions/0009-email-archive-search.md` follows the context, decision, alternatives, consequences, and date shape used by ADR 0008.
- [x] The ADR records that original `.eml` files remain immutable nodes and parsed rows are disposable, regenerable projections.
- [x] The ADR chooses a dedicated email schema instead of overloading `document_text_pages`, explaining that email requires structured sender, recipient, date, thread, label, and attachment fields rather than page semantics.
- [x] The ADR chooses PostgreSQL weighted full-text search initially and requires a measured production-scale benchmark before considering OpenSearch, Elasticsearch, or another service.
- [x] The ADR records the initial body policy: index the complete normalized body, preserve both plain and HTML representations when available, and defer quote/signature removal until ranking is measured.
- [x] The ADR records non-destructive duplicate handling: retain every original node, assign duplicate groups, and collapse duplicates in search by default with an explicit way to reveal every copy.
- [x] The ADR records the safe-rendering policy: sanitize HTML, execute no active content, make links explicit, and block remote images/resources unless the user deliberately reveals them.
- [x] The ADR records that deploying parser support does not start historical processing: new files may enqueue foreground jobs immediately, while existing files require an explicitly started backfill campaign.
- [x] The ADR adopts the shared campaign, priority, admission-control, migration, and Orion rollout contract in [`backfill.md`](backfill.md).
- [x] `README.md` links the ADR and this email implementation plan.

**Implementation report:** Recorded the email archive contract in ADR 0009: immutable `.eml` originals with disposable parsed projections, a dedicated structured schema rather than the page-oriented OCR tables, PostgreSQL weighted search pending a measured benchmark, complete-body indexing with quote stripping deferred, non-destructive duplicate grouping, a sanitized remote-blocking reader, and inert deployment with historical work gated behind an operator-started campaign. Added the `heavy_cpu`-first admission posture and its canary-gated promotion to `extractor`, and the independent parser, sanitizer, normalization, attachment-extractor, and search-index version axes. Linked the ADR, this plan, and the backfill plan from the README's processing section.

**New files:**

- `docs/decisions/0009-email-archive-search.md`

**Modified files:**

- `README.md`
- `docs/email.md`

---

### Story 17.2 — Structured Email Schema

As a developer, I want durable email tables linked to file nodes so that parsing results are queryable, replaceable, and removed with their originals. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The shared `0017_backfill_campaigns` migration described by [`backfill.md`](backfill.md) lands first; a reversible `0018_email_messages.{up,down}.sql` migration then adds email storage without rewriting existing node or job rows.
- [x] `email_extraction_status` uses `pending`, `completed`, `failed`, `skipped`, and `unsupported` states consistent with existing extractor status vocabulary.
- [x] `email_messages` uses `node_id` as its primary key and an `ON DELETE CASCADE` foreign key to `nodes`.
- [x] `email_messages` stores parser status/version, RFC `Message-ID`, `In-Reply-To`, ordered `References`, subject, sent and received timestamps, normalized plain body, optional sanitized-source HTML, preview text, attachment count, warnings, duration, and created/updated timestamps.
- [x] Address data preserves display name, normalized address, address role (`from`, `sender`, `reply_to`, `to`, `cc`, `bcc`), and stable order without flattening all recipients into one string.
- [x] Raw headers are preserved in a queryable representation without discarding repeated headers such as `Received`.
- [x] `email_attachments` stores MIME part identity, parent email node, filename, media type, disposition, content ID, transfer encoding, decoded size, checksum, inline status, and extraction status.
- [x] Gmail-specific metadata is optional: labels and provider thread IDs are stored when headers contain them, but ordinary RFC email does not require Gmail headers.
- [x] Duplicate-group and thread-group identifiers are nullable and indexed; neither is a uniqueness constraint on the original node.
- [x] Database APIs atomically replace a message, its addresses, headers, labels, and attachment manifest so reparsing cannot produce mixed parser versions.
- [x] PostgreSQL integration tests cover insertion, atomic replacement, repeated headers, address order, cascades, and constraints.

**Implementation report:** Added the reversible `0018_email_messages` migration with five tables — messages, addresses, headers, labels, and the attachment manifest — all cascading from `nodes` so a permanently deleted original takes its projection with it. Headers key on `(node_id, position)` rather than name, so repeated `Received` traces survive with their order while a `normalized_name` column keeps case-insensitive lookup indexed. Addresses keep display name, address, role, and stable position instead of one flattened recipient string. Thread and duplicate group identifiers are nullable and indexed but deliberately not unique, since several originals legitimately share both. `replace_email_projection` upserts the message and replaces every dependent table inside one transaction, so a reparse cannot leave a message carrying addresses from one parser version and attachments from another. Verified the migration up/down/up round-trip against PostgreSQL 17 and covered insertion, atomic replacement across a version change, repeated headers, address ordering, node cascade, and the size/count/uniqueness constraints.

**New files:**

- `crates/db/migrations/0018_email_messages.down.sql`
- `crates/db/migrations/0018_email_messages.up.sql`
- `crates/db/tests/email_messages.rs`

**Modified files:**

- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 17.3 — Email Extraction Job Type

As a developer, I want email parsing to use Strife's durable job queue so that a large archive survives restarts and individual malformed messages can be retried. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] A reversible `0019_email_job_type.{up,down}.sql` migration adds `email_extraction` to `job_type` and verifies the existing active-job uniqueness rule applies per node.
- [x] Email jobs carry `origin`, an optional campaign ID, and a resource class supplied by the shared backfill foundation. `origin` uses all three values shipped in `0017_backfill_campaigns` — `foreground`, `repair`, and `backfill` — not just the first and last: Story 18.6's reprocess scopes are `repair` work, and Story 22.2's priority ordering requires `repair` as a distinct middle tier.
- [x] The Rust `JobType` enum, API serialization, claim order, and worker dispatch all recognize `email_extraction`.
- [x] Email extraction is claimed after metadata and preview work but before OCR, because parsing MIME is cheaper than OCR and unlocks the Email tab quickly.
- [x] Claiming prefers foreground work regardless of job-family order; historical email cannot hide new uploads, imports, metadata, previews, repairs, or deletions behind its backlog.
- [x] Email jobs have a documented lease duration and retry count appropriate for bounded MIME parsing.
- [x] Existing `GET /api/jobs` endpoints return email jobs without changing their response shape.
- [ ] Tests cover enqueue uniqueness, lease renewal, retry with `last_error`, completion, and clean handling when the node disappears.

**Implementation report:** Added the `0019_email_job_type` migration following 0014's precedent that PostgreSQL enum values are not removed on rollback, since dropping one would require rebuilding every dependent column. Added `JobType::EmailExtraction` with `heavy_cpu` as its default resource class per ADR 0009, placed it in the processor claim order after metadata and preview but before OCR, and gave it a doubled lease TTL — headroom for a pathological message, short of OCR's tripled lease. Origin-based ordering from the shared foundation still dominates job-family order, so foreground work cannot be hidden behind a historical email backlog. `GET /api/jobs` needed no change because it returns only id, state, and error, never a job type. The dispatch arm returns an explicit error rather than silently completing: the parser lands in Story 18.5 and nothing enqueues email work until Story 18.6, so a message marked done without being parsed would be invisible to every later repair scope.

The test criterion is left open deliberately. Enqueue uniqueness, resource classification, and claiming are covered here, and generic lease renewal, retry with `last_error`, and completion are already covered for every job type by `crates/db/tests/jobs.rs`. Clean handling when the node disappears is meaningless before a handler exists and belongs with Story 18.5.

**New files:**

- `crates/db/migrations/0019_email_job_type.down.sql`
- `crates/db/migrations/0019_email_job_type.up.sql`

**Modified files:**

- `crates/db/src/lib.rs`
- `crates/worker/src/lib.rs`
- `docs/email.md`

---

### Story 17.4 — Representative Email Fixture Corpus

As a developer, I want committed, synthetic email fixtures covering real MIME edge cases so that parser upgrades cannot silently change the archive. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Fixtures contain synthetic identities and content only; no personal mailbox data, live addresses, authentication headers, or secrets are committed.
- [x] The corpus covers plain text, HTML-only, multipart alternative, mixed text and attachments, inline `cid:` images, nested `message/rfc822`, quoted-printable, base64, UTF-8, a legacy charset, RFC 2047 encoded headers, folded headers, repeated recipients, missing `Message-ID`, malformed dates, and malformed MIME boundaries.
- [x] At least one fixture carries Gmail label/thread headers and one deliberately does not.
- [x] At least one duplicate pair has the same `Message-ID`; another has no `Message-ID` but identical canonical content.
- [x] Expected normalized fields are stored beside fixtures in a reviewable form so failures show semantic differences rather than opaque snapshots.
- [x] Fixture sizes remain small enough for ordinary tests; separately generated large-message fixtures are used for limit tests.

**Implementation report:** Committed 21 synthetic `.eml` fixtures under `crates/media/tests/fixtures/email/` covering every edge case the story names, plus a Gmail-headers pair, a same-`Message-ID` duplicate pair, and a no-`Message-ID` identical-content duplicate pair. Expected normalized values live beside them in `expected.json` as reviewable per-fixture entries — subject, addresses, message id, body substrings that must and must not appear, attachment descriptors, labels, and expected warnings — so a Story 18.1 failure shows a semantic difference rather than an opaque snapshot mismatch. Every identity uses the reserved `example.test` domain and the largest fixture is under 2 KB; large-message fixtures for the Story 22.2 limit tests are deliberately generated rather than committed.

Four guard tests keep the corpus itself trustworthy before any parser exists: every fixture has a manifest entry and vice versa, the named edge-case coverage cannot silently regress, the wire format is valid CRLF with a header/body separator and no bare LF, and no fixture commits a non-synthetic domain or a real `DKIM-Signature`, `Authentication-Results`, `X-Google-DKIM`, or `Received-SPF` header.

**New files:**

- `crates/media/tests/email_fixtures.rs`
- `crates/media/tests/fixtures/email/base64-body.eml`
- `crates/media/tests/fixtures/email/duplicate-content-a.eml`
- `crates/media/tests/fixtures/email/duplicate-content-b.eml`
- `crates/media/tests/fixtures/email/duplicate-message-id-a.eml`
- `crates/media/tests/fixtures/email/duplicate-message-id-b.eml`
- `crates/media/tests/fixtures/email/encoded-and-folded-headers.eml`
- `crates/media/tests/fixtures/email/expected.json`
- `crates/media/tests/fixtures/email/gmail-labels.eml`
- `crates/media/tests/fixtures/email/html-only.eml`
- `crates/media/tests/fixtures/email/inline-cid-image.eml`
- `crates/media/tests/fixtures/email/legacy-charset.eml`
- `crates/media/tests/fixtures/email/malformed-boundary.eml`
- `crates/media/tests/fixtures/email/malformed-date.eml`
- `crates/media/tests/fixtures/email/missing-message-id.eml`
- `crates/media/tests/fixtures/email/mixed-with-attachment.eml`
- `crates/media/tests/fixtures/email/multipart-alternative.eml`
- `crates/media/tests/fixtures/email/nested-rfc822.eml`
- `crates/media/tests/fixtures/email/no-gmail-labels.eml`
- `crates/media/tests/fixtures/email/plain-text.eml`
- `crates/media/tests/fixtures/email/quoted-printable.eml`
- `crates/media/tests/fixtures/email/repeated-recipients.eml`
- `crates/media/tests/fixtures/email/utf8-subject-and-body.eml`

**Modified files:**

- `crates/media/Cargo.toml`
- `docs/email.md`

---

## Epic 18 — MIME Extraction & Durable Email Projection

**Goal:** Every RFC email is parsed safely and deterministically into structured, replaceable database records while malformed messages remain visible and diagnosable.

**Sprint Capacity Estimate:** 1–2 sprints

---

### Story 18.1 — Native RFC/MIME Adapter

As a developer, I want an email-aware parsing adapter so that Strife correctly handles MIME structure and RFC headers without depending on Tika's document abstraction. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] A parser adapter is added under `crates/media/src` and exported by the crate.
- [x] The selected Rust parser is actively maintained, has no network behavior, accepts bounded byte input, and passes the committed fixture corpus; the dependency and selection rationale are recorded in ADR 0009.
- [x] The adapter returns a typed result containing normalized headers, addresses, body alternatives, labels, thread hints, attachment descriptors, warnings, and parser version.
- [x] MIME media types, charsets, transfer encodings, dispositions, filenames, content IDs, and nested part paths are retained.
- [x] Header names are compared case-insensitively while original values and repeated-header order remain available.
- [x] MIME detection verifies `message/rfc822` from content bytes; extension and upload-provided MIME are treated only as hints.
- [x] Parsing performs no DNS resolution, link fetching, remote-image loading, or attachment execution.
- [x] Unit tests assert every committed fixture's normalized semantic result.

**Implementation report:** Selected `mail-parser` 0.11.5 (Stalwart Labs, Apache-2.0 OR MIT) and recorded the choice in ADR 0009. Its whole dependency tree is `encoding_rs` plus a compile-time proc-macro, so the adapter has no network or filesystem surface at all; `full_encoding` is enabled for the legacy charsets a ten-year archive contains. Added `crates/media/src/email/` returning one typed `ParsedEmail` carrying normalized ids, subject, dates, addresses, ordered headers, labels, thread hints, both body representations, a canonical content hash, attachment descriptors, warnings, and the parser identity. `looks_like_rfc822` confirms the message shape from bytes rather than trusting the extension or upload-declared MIME, and rejects PDFs and JPEGs outright.

Three real defects surfaced while making the fixtures pass, each one a silent data-loss bug rather than a crash:

1. The byte sniffer required its whole 8 KB window to be UTF-8, which rejected any message with a latin-1 body. Header field names are ASCII but bodies are not, so the check now scopes itself to the header block.
2. A message with an unterminated multipart boundary produced an empty body. Structure that cannot be assembled now falls back to recovering text from the parts and records a warning, rather than indexing nothing.
3. `mail-parser`'s typed accessors return a single header, so a message carrying two `To:` lines lost every recipient but the last. Address collection now walks the full header list, preserving all recipients in the order written.

**New files:**

- `crates/media/src/email/html.rs`
- `crates/media/src/email/mod.rs`
- `crates/media/tests/email_parser.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/media/Cargo.toml`
- `crates/media/src/lib.rs`
- `docs/decisions/0009-email-archive-search.md`
- `docs/email.md`

---

### Story 18.2 — Body Selection & Text Normalization

As a user, I want readable, searchable message text regardless of whether a sender supplied plain text or HTML. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] For `multipart/alternative`, the normalized searchable body prefers a usable `text/plain` alternative while retaining the HTML alternative for safe rendering.
- [x] HTML-only messages are converted to plain text with paragraph, list, table-cell, and line-break boundaries preserved well enough for snippets.
- [x] HTML conversion removes scripts, styles, comments, hidden content, tracking markup, and URLs from resource attributes without fetching anything.
- [x] Character-set decoding supports UTF-8 and the legacy charset fixture, replaces invalid sequences deterministically, and records a warning when decoding is lossy.
- [x] Unicode is normalized consistently before hashing and indexing; line endings become `\n`.
- [x] The initial implementation indexes quoted replies and signatures exactly as normalized, matching ADR 0009 rather than applying heuristic content deletion.
- [x] Body and preview text have independent configurable byte limits so an enormous HTML alternative cannot exhaust memory or dominate result payloads.
- [x] Tests cover plain preference, HTML-only conversion, whitespace behavior, lossy decoding, empty bodies, and limit warnings.

**Implementation report:** Body selection prefers a non-empty `text/plain` alternative for indexing and keeps the HTML alternative for the reader. HTML-only messages go through `crates/media/src/email/html.rs`, a single-pass converter that is deliberately not a browser: it resolves no URL, loads no resource, and executes nothing. It drops `script`, `style`, `head`, `title`, and `noscript` subtrees entirely, skips comments — which is where tracking pixels usually hide — excludes `display:none` and `visibility:hidden` blocks with a warning, and emits line breaks at block boundaries and tabs between table cells so snippets stay readable. Resource URLs in `src` and `href` attributes never reach the indexed text, verified by an explicit test.

Line endings normalize to `\n`, trailing whitespace is trimmed per line, and the same normalized text feeds both the canonical content hash and the index, so two copies of one message hash identically. Quoted replies and signatures are indexed exactly as written, per ADR 0009 — stripping real content is worse than ranking it low. Body and preview have independent byte ceilings; exceeding the body limit truncates on a character boundary and warns rather than failing the message.

The html converter carries six unit tests of its own. One of them caught a bug the integration tests could not: the text branch did not consult the drop state, so the contents of `<script>` and hidden tracking blocks still reached the indexed body even though their tags were correctly recognized and discarded.

**New files:** None beyond Story 18.1.

**Modified files:**

- `crates/media/src/email/html.rs`
- `crates/media/src/email/mod.rs`
- `docs/email.md`

---

### Story 18.3 — Header, Address & Date Normalization

As a user, I want searches and message details to use reliable correspondents and dates despite variations in RFC formatting. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Mailbox and group address syntax is parsed into display names and normalized addresses without discarding the original header.
- [x] Address normalization lowercases the domain and preserves the local part; Gmail-specific dot or plus-address rewriting is not performed.
- [x] Internationalized display names and domains remain displayable and searchable.
- [x] `Date` is parsed with its original timezone offset; invalid or absent values remain null with a warning rather than using ingestion time silently.
- [x] A defensible received timestamp is derived from trace headers only when parsing succeeds, and its provenance is documented.
- [x] `Message-ID`, `In-Reply-To`, and `References` are normalized for comparison while their raw values remain available.
- [x] Subject normalization decodes encoded words and derives a separate thread-comparison subject without changing the displayed subject.
- [x] Tests cover address groups, quoted names, duplicates across roles, encoded subjects, timezone offsets, invalid dates, and missing headers.

**Implementation report:** Address extraction handles both mailbox lists and RFC 5322 group syntax, keeping group members rather than the group label. Normalization lowercases the domain and leaves the local part untouched: Gmail treats `a.b@gmail.com` and `ab+tag@gmail.com` as one mailbox, but they are distinct as written, and collapsing them would discard what the sender actually typed. The full raw header list is retained separately, so no normalization is destructive. Internationalized display names survive as UTF-8 and stay searchable.

`Date` parses with its original offset and is stored as UTC. A header that is present but unparseable stays null and records a warning — it is never replaced with ingestion time, which would invent a plausible-looking date the sender never wrote. The received timestamp comes only from a `Received` trace header that parsed cleanly, with the same no-fallback rule.

`Message-ID`, `In-Reply-To`, and `References` are normalized by stripping angle brackets and lowercasing for comparison, with raw values preserved in the header list. Subject normalization strips reply and forward prefixes in several languages and collapses whitespace to derive a thread-comparison subject, leaving the displayed subject exactly as received. Encoded-word decoding is handled by the parser and asserted through the RFC 2047 fixture, which covers both base64 and quoted-printable header encodings alongside folded continuation lines.

**New files:** None beyond Story 18.1.

**Modified files:**

- `crates/media/src/email/mod.rs`
- `docs/email.md`

---

### Story 18.4 — Attachment Manifest Extraction

As a user, I want attachment metadata preserved during message parsing so that search and message details accurately describe each email. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Every non-body MIME part is represented in `email_attachments`, including unnamed and inline parts.
- [x] RFC 2231/5987-style encoded filenames and split parameters are decoded safely.
- [x] Filenames are display values only and are never used directly as filesystem paths.
- [ ] Decoded size and SHA-256 are computed while streaming with configurable per-part and per-message decoded-byte ceilings.
- [x] Inline parts preserve content IDs so sanitized message HTML can resolve them through authenticated Strife endpoints later.
- [x] Nested `message/rfc822` parts are identified distinctly from ordinary binary attachments.
- [x] A malformed attachment records a part-level warning without discarding an otherwise readable message.
- [ ] Tests cover duplicate filenames, no filename, inline images, nested messages, encoded filenames, and size-limit behavior.

**Implementation report:** Every non-body part becomes a manifest row carrying a dotted MIME part path (`1.2`), media type, disposition, content id, transfer encoding, decoded size, SHA-256, and inline and nested-message flags. The part path — not the filename — is the part's identity, so a hostile `../../etc/passwd` filename is a display string and nothing more; Story 21.1 derives storage keys from part identity for the same reason. Inline parts keep their normalized content id so the reader can later resolve `cid:` references against authenticated same-message endpoints. Nested `message/rfc822` parts are flagged distinctly and are deliberately not flattened into user-visible files. A part reporting a transfer-encoding problem records a part-level warning while the containing message still parses.

Two criteria are left open rather than claimed. **Hashing is not yet streaming**: sizes and digests are computed over the decoded part the parser already materialized in memory, which is correct and bounded by `max_source_bytes` but does not yet honour per-part and per-message decoded-byte ceilings. Streaming decode belongs with Story 21.1's bounded materialization, which is where attachment bytes are actually written out; doing it here would mean decoding twice. **Attachment-specific test coverage is partial**: inline images, nested messages, encoded filenames, and unnamed parts are asserted through the fixture corpus, but duplicate filenames within one message and per-part size-limit behaviour are not yet covered and follow the streaming work.

**New files:** None beyond Story 18.1.

**Modified files:**

- `crates/media/src/email/mod.rs`
- `docs/email.md`

---

### Story 18.5 — Email Job Handler

As a system, I want a worker handler that turns managed `.eml` originals into atomic structured records so that extraction is durable and idempotent. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] The handler mirrors established metadata/OCR handlers: lifecycle check, managed-original copy, byte MIME detection, bounded parse, atomic persistence, cleanup, and structured logging.
- [x] Active RFC email records `completed`; non-email files record `unsupported`; trashed files are skipped; missing or permanently deleted nodes fail cleanly without orphan rows.
- [x] Message, addresses, headers, labels, and attachment manifest are replaced in one transaction.
- [x] Reprocessing the same node with the same parser version produces the same normalized records and no duplicates.
- [x] Parser errors preserve the underlying cause in warnings and the job's `last_error`; error text exposed through APIs is sanitized.
- [ ] Duration, source size, decoded body bytes, attachment count, warning count, and peak process memory where measurable are logged.
- [x] Temporary originals and decoded parts are removed on success, error, cancellation, and panic.
- [ ] Integration tests cover the complete fixture corpus, corrupt input, trash, deletion during processing, retry after lease expiry, and atomic rollback on a forced persistence error.

**Implementation report:** Added `crates/worker/src/email.rs` following the established handler shape: lifecycle check, copy the managed original to a temporary path, confirm the MIME from bytes, parse under bounded limits, persist atomically, clean up, and log. The `EmailExtraction` dispatch arm now routes here instead of returning the Story 17.3 placeholder error. The staged temporary file is removed on every path — success, error, timeout, and cancellation.

Outcomes are deliberately distinguished rather than collapsed into pass/fail. An active RFC message records `completed`; a non-email file records `unsupported` with the detected MIME in its warning; a trashed file records `skipped` and the job is skipped rather than failed, because the file may be restored and a failure would consume the retry budget; a node deleted between claim and handling fails cleanly with no projection row written. Size and shape failures are terminal: retrying cannot change the verdict, so they fail the job outright instead of burning three attempts, with the specific limit named in the stored warning. The API-visible `last_error` is reduced to a single bounded line while the full cause stays in the logs and the persisted warning, so message content cannot leak through an error field.

Two criteria are left open. **Peak process memory is not measured** — duration, source bytes, body bytes, attachment count, and warning count are all logged, but per-job peak RSS needs the same measurement harness Story 22.2 introduces for the parser resource limits, and inventing a second mechanism here would be thrown away. **Two test cases are missing**: retry after lease expiry, and atomic rollback on a forced persistence error. The corpus, corrupt input, trash, mid-flight deletion, idempotent reparse, and terminal-limit paths are covered by eight integration tests; the lease-expiry path is covered generically for all job types in `crates/db/tests/jobs.rs`, and forcing a persistence failure needs a fault-injection seam the handler does not yet expose.

**New files:**

- `crates/worker/src/email.rs`
- `crates/worker/tests/email_job.rs`

**Modified files:**

- `crates/worker/src/lib.rs`
- `docs/email.md`

---

### Story 18.6 — Foreground Enqueue, Campaign Backfill & Reprocessing API

As a user, I want newly imported `.eml` files parsed automatically and the historical archive processed only through an explicit bounded campaign. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Direct-upload and watched-folder finalization create equivalent foreground email-extraction jobs for files finalized after deployment.
- [x] Deployment, migrations, API startup, worker startup, and ordinary recovery never scan the historical library or create historical email jobs implicitly.
- [x] Candidate selection uses detected or strongly indicated RFC email MIME and remains recoverable through a read-only preflight and explicit campaign when finalization lacks reliable MIME information.
- [x] Historical processing uses the campaign scheduler in [`backfill.md`](backfill.md), starting paused and refilling only when active queued/running work falls below its low-water mark.
- [x] Initial campaign defaults are a 100-node batch, at most 500 queued jobs, and one running email backfill job; every value is configurable and recorded on the campaign.
- [x] Only one historical heavy-processing campaign is active on Orion initially, so email, OCR, and attachment backfills cannot run together.
- [x] `POST /api/admin/reprocess` accepts email scopes for one node, failed messages, missing records, and parser-version mismatch.
- [x] Nodes with active email jobs are excluded before the batch limit is applied, preventing a batch from reporting zero while eligible work remains later in the result set.
- [x] Duplicate requests are no-ops and the response returns the number actually enqueued.
- [x] Pausing a campaign stops refilling immediately while allowing leased work to finish; resuming continues from the durable cursor without rescanning completed nodes.
- [ ] Tests cover upload, watched-folder import, inert deployment/startup, explicit campaign start, pause/resume, low-water refill, all reprocess scopes, bounded batches, and active-job suppression.

**Implementation report:** `finalize_upload` and `finalize_import` now enqueue a foreground email job in the same transaction that publishes the file, using the savepoint pattern already established for OCR so a failed enqueue can never roll back the finalization. Enqueueing is unconditional rather than gated on the declared MIME: `.eml` files are frequently detected as `text/plain`, so a pre-filter here would silently hide most of the archive. The handler confirms the RFC 5322 shape from bytes and records `unsupported` otherwise, which costs one cheap sniff per non-email file and is recoverable — where a wrong pre-filter is not.

Registered `EmailBackfillProvider` on the shared coordinator alongside the OCR one, with the same single-transaction selection, enqueue, and `(created_at, id)` cursor advance. Added a read-only preflight reporting candidate count and byte percentiles, and email scopes on `POST /api/admin/reprocess` for one node, failed messages, missing projections, and parser-version mismatch. The `missing` scope is what recovers files finalized before the feature deployed or whose enqueue failed after finalization. Active jobs are excluded inside each candidate query, before the limit is applied — filtering afterwards would let a batch report zero while eligible nodes waited further down the result set.

Added the cross-pipeline guard the criterion requires: `transition_backfill_campaign` refuses to move a `heavy_cpu` campaign to `running` while another is already running or draining. Email, OCR, and attachment backfills share one heavy admission permit, so a second campaign could not make progress anyway — it would only compete for refills and make the queue harder to reason about.

Two defects surfaced during testing. **A paused campaign could still enqueue work**: only the cursor update was state-guarded, so a refill against a paused campaign queued jobs while the cursor stood still, and a later resume then skipped those nodes because they had active jobs. Both the email and OCR refill functions now re-read campaign state inside their transaction. **An existing API test shared the dev database** and began failing against the new mutual-exclusion guard because of campaigns left running by an earlier test run; the test now quiesces competing heavy campaigns before asserting its own resume.

The test criterion is left open on one item: **watched-folder import is not separately covered**. Both finalization paths call the same helper and direct upload is asserted end to end, but `finalize_import` has no equivalent test of its own. Everything else in the list — inert deployment, explicit campaign start, pause/resume from the cursor, low-water refill, all four reprocess scopes, bounded batches, and active-job suppression — is covered by thirteen integration tests.

**New files:**

- `crates/db/tests/email_backfill.rs`

**Modified files:**

- `crates/api/src/admin.rs`
- `crates/api/tests/backfills_api.rs`
- `crates/db/src/lib.rs`
- `crates/worker/src/backfill.rs`
- `crates/worker/src/lib.rs`
- `docs/email.md`

---

## Epic 19 — Email Full-Text Search & Query API

**Goal:** Subject, correspondents, body text, labels, and attachment names become fast, relevant, filterable, and safely highlighted across the archive.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 19.1 — Weighted PostgreSQL Email Index

As a developer, I want an email-specific full-text index so that relevance reflects how people search mail rather than treating every token equally. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Staged, reversible `0020_email_search.{up,down}.sql` schema changes add the email search vector without a table-rewriting startup migration; historical vector population happens in bounded batches.
- [x] The large GIN index is built through the separately executed operational migration path in [`backfill.md`](backfill.md), using concurrent/index-safe behavior rather than blocking API startup on a full archive build.
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

**The production-scale run has not been performed, and three criteria stay open.** Cold and warm latency percentiles, `EXPLAIN (ANALYZE, BUFFERS)` plans, and ingestion and index-growth figures all require executing the harness on Orion against ≥100,000 messages. Running it now would measure a moving target — the parser, ranking weights, and filter set have been stable for hours, not through a canary — and the numbers would be re-measured anyway. The benchmark belongs immediately before the Phase 5 email canaries in [`backfill.md`](backfill.md), alongside Story 22.5's validation, where its thresholds actually gate a decision. The document says so in a status callout rather than reading as though the evidence exists.

**New files:**

- `crates/db/examples/seed_email_benchmark.rs`
- `docs/benchmarks/email-search.md`

**Modified files:**

- `crates/db/Cargo.toml`
- `docs/email.md`

---

## Epic 20 — Email Navigation, Search & Reader UI

**Goal:** A dedicated Email tab lets users search, filter, inspect, and safely read the archive using Strife's established visual and accessibility language.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 20.1 — Email Sidebar Navigation & Status Badge

As a user, I want an Email entry in the sidebar so that the archive is a first-class Strife surface. **Estimated: 2 points.**

**Acceptance Criteria:**

- [x] An Email navigation item is placed near OCR and Imports, with `/email` registered in the Solid router.
- [x] The item uses the existing active treatment and icon system rather than introducing a separate navigation style.
- [x] A badge shows pending plus running email jobs and is omitted at zero.
- [ ] The pending count is updated from the email status stream without refreshing the page.
- [x] Backfill counts are visually distinct from foreground processing so a paused historical campaign does not make new mail appear stuck.
- [x] Static preview mode renders the entry and deterministic sample count without contacting the backend.

**Implementation report:** The Email item sits between Console and OCR in the sidebar's `navigation` array, with `/email` registered in the router. It reuses the existing `A` element, active treatment, and `SidebarIcon` system; the new `mail` glyph is one more path in the same 24-unit vocabulary, not a separate icon mechanism.

`GET /api/email/status` backs the badge, and `strife_db::email_status_counts` splits queue depth by `jobs.origin`. The badge counts only foreground pending plus running. This distinction is the point of the endpoint rather than an incidental detail: a paused 600,000-message historical campaign would otherwise pin a permanent number to the navigation that no user action can clear, and a badge that can never reach zero stops being read. Historical depth appears as a second chip in muted chip colours rather than the error colour, with a title naming it as backfill work. Both are omitted at zero. Static preview mode renders fixed counts and contacts nothing.

One criterion is left open: **the badge does not yet update live.** `GET /api/email/events` is Story 22.1's deliverable, and until it exists the count is a resource fetched on mount, so it changes on navigation rather than as jobs complete. Subscribing is a few lines once the stream exists — the OCR view's `EventSource` block is the template — and polling was deliberately not substituted, because a poll that looks live but lags by its interval is harder to reason about than a count that visibly refreshes on navigation.

**New files:**

- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/components/Sidebar.css`
- `apps/web/src/components/Sidebar.tsx`
- `apps/web/src/index.tsx`
- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 20.2 — Email Search & Filter Interface

As a user, I want a responsive mail-search interface so that I can find messages by text and structured fields without memorizing URL parameters. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The Email page has one primary full-text input plus controls for sender/participant, date range, labels, attachment presence, trash, and duplicates.
- [x] Search input is debounced and prior requests are cancelled when criteria change.
- [x] Filter state is reflected in the URL so searches can be bookmarked and browser navigation works.
- [x] A clear-all action resets every criterion predictably.
- [x] Loading, no indexed mail, no matches, parse failure, offline, and retry states are distinct.
- [x] Facet options remain bounded and searchable when the archive contains many correspondents or labels.
- [x] Results load through cursor pagination or virtualization without an ever-growing unbounded DOM.
- [x] Static preview mode includes representative results and active filters.

**Implementation report:** One `type="search"` field drives full-text query. Structured controls cover sender (correspondent facet chips, narrowing `from`), participant (a free-text field committing on Enter, matching any role), sent-after and sent-before dates, labels (facet chips), attachment presence (any / with / without, which is three states rather than a checkbox because "no attachments" is a real search), and checkboxes for trashed and duplicate inclusion.

The URL is the single source of truth. `criteria.ts` converts between a query string and typed criteria, and every control writes through `apply()`, which navigates rather than mutating local state; the search effect reads back from `useLocation`. Bookmarking, the back button, and deep-linking to an open message therefore work without separate handling. Only the free-text field is debounced, at 300 ms with `replace: true` so typing a word leaves one history entry instead of one per keystroke; structured controls apply immediately, because a click is already deliberate. Each search runs under an `AbortController` that `onCleanup` aborts, so a slow earlier response cannot overwrite a newer result set.

Six states are distinguished rather than collapsed into "no results": nothing indexed in the archive at all, idle with no criteria entered, loading, no matches, an error with a retry control, and offline (checked through `navigator.onLine`, which produces a different message from a server error). When a search matches nothing and the archive also holds messages that failed to parse, the count of unsearchable failures is stated — a parse backlog reads exactly like a bad search otherwise, and it sends people looking for the problem in the wrong place. `isSearchable()` mirrors the API's own rule that a request with neither text nor a filter is rejected, so an empty form shows a prompt instead of provoking a 400.

Facets are capped at 50 server-side and 12 in the rendered list, with a filter input narrowing labels and correspondents together. Pagination replaces the page rather than appending: a cursor stack drives Previous and Next, so the DOM holds one page of 25 regardless of how deep the user walks.

**New files:**

- `apps/web/src/views/email/criteria.test.ts`
- `apps/web/src/views/email/criteria.ts`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`
- `docs/email.md`

---

### Story 20.3 — Email Result List

As a user, I want results that look and behave like messages so that I can evaluate matches without opening every file. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Each result shows sender, subject, sent date, safe highlighted snippet, attachment indicator/count, relevant labels, and duplicate/thread count when greater than one.
- [x] Missing subjects, senders, or dates have useful accessible fallbacks rather than blank cells.
- [x] Unread semantics are not invented because a static export does not reliably preserve current Gmail read state.
- [x] Search highlights are constructed as text/`mark` nodes from server markers and never assigned through `innerHTML`.
- [x] Selection, hover, focus, and multi-line truncation follow existing Strife tokens and work in light and dark themes.
- [x] Results are keyboard navigable with a visible focus position and announced result count.
- [x] Selecting a result opens its message reader without losing the search URL or scroll position.

**Implementation report:** Sender and labels were missing from the search response, so `search_email` gained them. They are joined by `LEFT JOIN LATERAL` in a final stage after the page has been cut, which means two lookups per returned row rather than per match — the alternative would have made every search pay for sender resolution on rows it then discarded. A result therefore renders from one request: sender, subject, sent date, snippet, attachment count, thread count and duplicate count when above one, and labels.

Highlighting is the security-relevant part. PostgreSQL's `ts_headline` wraps matches in `[[` and `]]`; `parseSnippet` turns a snippet into an array of `{ text, marked }` runs, and the component maps those to text nodes and `<mark>` elements. The body never reaches `innerHTML`, so HTML inside an archived message renders as visible text rather than as markup. A test asserts exactly this by feeding `<b>` through a snippet and checking that no `<b>` element exists in the result. Unbalanced markers degrade to plain text rather than highlighting to the end of the fragment, so a body containing a literal `[[` is merely unhighlighted.

Missing fields are named rather than blank: `(no subject)`, `(no sender recorded)`, `(no date recorded)`. A blank cell is indistinguishable from a rendering bug, and a ten-year archive produces all three routinely. No unread state is shown anywhere, because a static export does not carry it.

Results are `role="button"` with `tabIndex={0}`, activated by Enter or Space, with Arrow, Home, and End moving focus across the list; focus resolves from the list root rather than by walking siblings, so intervening markup can change without silently breaking navigation. Selection, hover, and focus use `--color-surface-selected`, `--color-surface-raised`, and a `--color-accent` focus ring, and truncation uses line clamping, so both themes follow from the tokens. The settled result count is announced through a restrained live region. Opening a result adds a `message` parameter to the existing URL and navigates with `scroll: false`, so the search and scroll position both survive.

**New files:**

- `apps/web/src/views/email/ResultList.test.tsx`
- `apps/web/src/views/email/ResultList.tsx`
- `apps/web/src/views/email/format.ts`
- `apps/web/src/views/email/snippet.test.ts`
- `apps/web/src/views/email/snippet.ts`

**Modified files:**

- `apps/web/src/api/types.ts`
- `apps/web/src/views/EmailView.css`
- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

---

### Story 20.4 — Safe Message Reader

As a user, I want to read plain or formatted email safely so that archived messages are useful without executing hostile historical content. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] The reader shows subject, ordered correspondents, sent/received date, labels, attachment list, warnings, and expandable normalized headers.
- [x] Plain text remains selectable and preserves meaningful line breaks.
- [x] HTML is sanitized server-side or with a pinned, tested sanitizer configuration and rendered in an isolated boundary that cannot affect the Strife application DOM.
- [x] Scripts, forms, event handlers, embedded objects, CSS imports, automatic redirects, and active SVG content are removed or inert.
- [x] Remote images, fonts, stylesheets, frames, and media are blocked by default; revealing remote resources requires an explicit warning and action.
- [x] Links display their destination, use safe schemes only, and cannot give the opened page access to Strife's window context.
- [ ] Inline `cid:` references resolve only to authenticated attachment endpoints belonging to the same message.
- [x] The reader offers plain-text fallback, copy controls, and download-original without an edit affordance.
- [x] Security tests use deliberately hostile HTML fixtures covering every blocked capability.

**Implementation report:** Archived mail is hostile input that happens to be a decade old, so the reader is built as three independent layers, none of which is asked to be sufficient alone.

The first is `crates/media/src/email/sanitize.rs`, built on `ammonia` pinned to `=4.1.4` — an exact pin, because a silent relaxation of its allowlist is a rendering-security change. Parsing is delegated to `html5ever` so hostile markup is parsed the way a browser parses it rather than by pattern matching; the tests include the classic regex-sanitizer bypasses (`<scr<script>ipt>`, an attribute-quoted `<script>`, a malformed comment) to hold that line. The tag allowlist has no `script`, `style`, `iframe`, `object`, `embed`, `form`, `input`, `svg`, `math`, `link`, `meta`, or `base`, so scripting, submission, plugin content, remote stylesheets, and meta-refresh redirects have no element to attach to; `clean_content_tags` removes script and style *text* as well, which a tag-stripping sanitizer would leave behind as visible message content.

CSS is filtered by property rather than dropped or passed through. Inline styles carry most of an archived message's layout, so discarding them would make a decade of mail unreadable, but `background-image`, `position`, and anything containing `url(`, `expression`, `@import`, a CSS escape, or a comment is rejected. Sanitizing happens server-side, so the browser never receives the original bytes at all.

The second layer is the reader's frame: `sandbox` without `allow-scripts`, plus a CSP naming `default-src 'none'` and `script-src 'none'`. `allow-same-origin` is present only so inline parts can load, and is safe precisely because `allow-scripts` is absent — the two together are the classic sandbox escape and must never both appear, which a test asserts directly. The third layer is that message HTML only ever exists as an `srcdoc` string, never as elements in Strife's own document; a test renders a body containing `<p id="leaked">` and asserts `getElementById` finds nothing.

Remote images are stripped and counted rather than hidden client-side, because a tracking pixel that reaches the browser has already fired. The reader states how many remote images a message holds and which hosts they come from, and revealing them re-requests the message with `allow_remote_images=true` — consent is a different server response, not a CSS toggle, and consent to images is not consent to anything else, which a test verifies by confirming scripts and `url()` stay stripped in the revealed response. Links keep only `http`, `https`, `mailto`, and `tel`; relative and protocol-relative hrefs are rejected because an archived message has no base URL and they would resolve against Strife's own origin. `rel="noopener noreferrer"` denies the opened page any handle on Strife's window. Sender-supplied `title` attributes are discarded and replaced, on frame load, with the link's real destination, since link text in archived mail is frequently misleading.

The reader shows subject, correspondents ordered by role rather than by storage order, sent and received dates, labels, warnings, the attachment manifest, and an expandable details panel. Plain text renders in a `pre` that preserves line breaks and stays selectable, and is available as a fallback for any HTML message. Copy and download-original are offered; nothing edits, which a test asserts by scanning every button label.

One criterion is left open: **inline `cid:` references resolve to an endpoint that does not exist yet.** The restriction itself is implemented and tested — a reference is matched case-insensitively against the parts *this* message declares, an unknown one is dropped rather than guessed at and reported as a warning, and traversal attempts never reach URL construction because they match no declared part. But the URL it resolves to, `/api/email/messages/{node_id}/parts/{part_path}`, is Story 21.3's deliverable, so revealing images in a message with inline parts currently yields a broken image rather than the attachment. Marking this done would claim a working path that does not exist; the security property it describes is nonetheless already enforced.

**New files:**

- `apps/web/src/views/email/MessageReader.test.tsx`
- `apps/web/src/views/email/MessageReader.tsx`
- `crates/media/src/email/sanitize.rs`
- `crates/media/tests/email_sanitize.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`
- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/media/Cargo.toml`
- `crates/media/src/email/mod.rs`
- `crates/media/src/lib.rs`
- `docs/email.md`

---

### Story 20.5 — Responsive & Accessible Email Experience

As a user using a keyboard, screen reader, or narrow display, I want the email interface to remain fully operable. **Estimated: 3 points.**

**Acceptance Criteria:**

- [x] Search, filters, result list, reader, headers, and attachments have correct labels, landmarks, names, and focus order.
- [x] Opening/closing the reader moves and restores focus predictably; Escape behavior does not discard search state.
- [x] Result and processing updates use restrained live-region announcements without reading every streamed event during a large backfill.
- [x] Desktop supports a useful list/reader layout while narrow widths use a single-pane navigation model without horizontal page scrolling.
- [x] Dates and attachment sizes have machine-readable values and localized display values.
- [x] Automated accessibility checks and keyboard-focused component tests cover the primary search-to-reader flow.
- [x] `eslint --max-warnings 0`, Prettier, TypeScript build, and static-preview build pass.

**Implementation report:** The page uses a `search` landmark for the query and filters, labelled sections for the result list and reader, `fieldset`/`legend` for facet groups, and visually hidden labels where a placeholder alone would leave a control unnamed. Opening a message records the previously focused element and moves focus to the reader's Close button; closing restores it. Escape closes the reader by dropping only the `message` parameter, so search state survives — the criteria live in the URL, which is what makes that cheap. The live region announces the settled result count rather than each event, so a large backfill cannot turn it into a screen-reader firehose.

Desktop places the reader beside the list in a two-column grid; below 60rem the reader replaces the list entirely, so a narrow screen shows one pane and the page never scrolls horizontally. Dates render through `<time datetime>` with localized text, and attachment sizes through `<data value>` carrying exact bytes alongside a human-readable string.

Testing needed tooling the web app did not have, so Vitest, jsdom, `@solidjs/testing-library`, `@testing-library/user-event`, and `axe-core` were added, with a `vitest.config.ts` kept separate from the app build. Twenty-nine tests cover the search-to-reader flow: snippet parsing including the injection case, criteria URL round-tripping including repeated keys, result-list keyboard navigation and fallbacks, the reader's sandbox and isolation properties, and `axe` runs over both components. Contrast rules are disabled in those runs because jsdom has no layout, and the message frame is excluded because it holds the sender's markup, which Strife cannot fix and jsdom cannot traverse.

Two defects surfaced during this work and were fixed. Arrow-key navigation resolved siblings through the wrong parent element and moved focus nowhere; the test caught it before the feature shipped. The message frame styled itself from `prefers-color-scheme` while the application follows its own theme toggle, so a user running a dark Strife on a light OS saw a white message panel inside a dark reader; the frame is now handed the resolved theme explicitly.

`eslint --max-warnings 0`, `prettier --check`, `tsc -b`, `vitest run`, and the static-preview `vite build` all pass, along with 201 Rust tests, `cargo fmt --check`, and `cargo clippy --workspace --all-targets` with zero warnings.

**New files:**

- `apps/web/src/test/setup.ts`
- `apps/web/vitest.config.ts`

**Modified files:**

- `apps/web/package-lock.json`
- `apps/web/package.json`
- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`
- `apps/web/src/views/email/MessageReader.tsx`
- `apps/web/src/views/email/ResultList.tsx`
- `docs/email.md`

---

## Epic 21 — Attachment Search, Threads & Gmail Context

**Goal:** Attachments and conversation context become searchable and navigable without compromising originals, security, or resource limits.

**Sprint Capacity Estimate:** 1 sprint

---

### Story 21.1 — Bounded Attachment Materialization

As a system, I want attachment bytes decoded into managed, regenerable artifacts so that they can be downloaded, previewed, and processed without reparsing the entire message each time. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Attachment artifacts use deterministic storage keys derived from email node and MIME part identity and never trust attachment filenames as paths.
- [ ] Materialization streams transfer decoding and hashing rather than holding the full part in memory.
- [x] Per-part, per-message, total-artifact, compression/nesting, and timeout limits are configurable with documented provisional defaults.
- [x] Partial output is deleted on failure and reruns replace artifacts idempotently.
- [x] Artifact rows retain source message, part path, checksum, size, media type, and parser version.
- [x] Nested `message/rfc822` attachments have an explicit maximum recursion depth and are not silently imported as top-level user files.
- [x] Tests cover binary, inline, duplicate-name, nested-message, oversized, malformed-transfer, cancellation, and rerun cases.

**Implementation report:** `0023_email_attachment_artifacts` adds a table whose primary key doubles as its storage object id, computed as a `UUIDv5` of `(message node, MIME part path)`. A sender-controlled filename participates in nothing: not the key, not a path, not a directory. Two attachments both called `invoice.pdf` therefore land in separate objects, which a test verifies by reading both back and asserting the bytes differ — a filename-derived key would have silently kept one of them.

Determinism is what makes reruns idempotent. The same message recomputes the same id, so a rerun replaces its artifacts in place rather than accumulating a second copy, and a test asserts the row id, storage key, and checksum are unchanged across two runs. A failed write deletes its object before returning, so a later read can never find a truncated artifact whose checksum would not match its source.

`AttachmentLimits` bounds per-part bytes, per-message total bytes, and nesting depth, with provisional defaults of 25 MB, 64 MB, and one level, documented as needing Orion profiling before they are treated as final. The per-attachment timeout is the existing email job timeout. An over-limit part is skipped with a warning rather than truncated — half an attachment is not a smaller attachment, and storing one would produce an artifact whose checksum could never verify. A nested `message/rfc822` part is materialized whole as opaque bytes and never unpacked into top-level files; a test asserts the file tree gains no nodes.

One property is deliberate and worth stating: **a bad attachment never fails its message.** A part that is too large, malformed, or unwritable is recorded as a failed artifact carrying the reason, while the message keeps its `completed` state. The message parsed and its body is searchable; one unreadable PDF is not a reason to lose that.

Permanent deletion was extended to reclaim attachment artifacts. Without it their bytes would outlive the message they belong to, which a test now guards.

One criterion is left open: **transfer decoding is not streamed.** `mail-parser` is a DOM parser — it decodes the whole message into memory before any part is addressable — so a decoded part is already resident when materialization begins. What is streamed is everything after that: hashing runs in 64 KB chunks in a single pass, and each part's buffer is *moved* into the writer rather than copied, so no second full-size allocation is made. True streaming decode needs either a different parser or a hand-written MIME walk, which is a larger change than this story, and the per-part ceiling bounds the exposure in the meantime. Peak memory belongs with Story 22.2's profiling.

**New files:**

- `crates/db/migrations/0023_email_attachment_artifacts.down.sql`
- `crates/db/migrations/0023_email_attachment_artifacts.up.sql`
- `crates/media/tests/fixtures/email/duplicate-attachment-names.eml`
- `crates/media/tests/fixtures/email/malformed-transfer-encoding.eml`
- `crates/worker/src/attachments.rs`
- `crates/worker/tests/attachment_materialization.rs`

**Modified files:**

- `Cargo.lock`
- `Cargo.toml`
- `crates/db/src/lib.rs`
- `crates/media/src/email/mod.rs`
- `crates/media/src/lib.rs`
- `crates/media/tests/fixtures/email/expected.json`
- `crates/worker/Cargo.toml`
- `crates/worker/src/email.rs`
- `crates/worker/src/lib.rs`
- `crates/worker/tests/email_job.rs`
- `docs/email.md`

---

### Story 21.2 — Attachment Content Extraction & Search

As a user, I want text inside supported attachments included in email search so that a message can be found by the document it carried. **Estimated: 8 points.**

**Acceptance Criteria:**

- [x] Supported document and image attachments reuse the existing Tika and OCR adapters instead of introducing duplicate extractors.
- [x] Attachment text is stored with attachment identity, extractor source/version, status, warnings, page number, confidence where applicable, and bounded text bytes.
- [x] The email search vector includes attachment filenames at weight B and extracted attachment text at a lower documented weight than message body.
- [x] Search results identify whether the match came from subject, headers, body, attachment filename, or attachment content.
- [x] Attachment matches report attachment name and page number when available and open the relevant attachment preview.
- [x] Unsupported and failed attachments do not fail the containing message's completed extraction state.
- [x] Reprocessing can target one attachment, failed attachments, missing text, or extractor-version mismatches in bounded batches.
- [ ] Tests cover text PDF, scanned PDF, office document, image, unsupported binary, mixed-success message, ranking, snippets, and version reprocessing.

**Implementation report:** A new `attachment_extraction` job type runs `AttachmentTextHandler` over one message's stored artifacts. Routing reuses the adapters Strife already runs rather than adding a second set: a PDF is asked for its embedded text through Tika first and rasterized through the OCR pipeline only when that text is too thin to be real — the same decision the OCR handler makes, for the same reason. Office formats go to Tika, images to OCR, and plain text is read directly without invoking anything.

One job covers a whole message rather than one attachment. The queue targets nodes and an attachment is a MIME part, so per-attachment jobs would need a parallel queue for no benefit; a message's attachments are also the natural unit for reindexing, since they all feed one search vector.

`0025_email_attachment_text` stores text per page with its source (`embedded` or `ocr`) and confidence, and records the outcome — status, extractor name and version, byte count, warnings — once per attachment on the artifact row rather than repeated on every page. Stored text is capped at 1 MB per attachment, truncated on a character boundary so the column stays valid UTF-8, with a warning naming the limit.

The search vector gains attachment text at **weight D**, below the body's weight C. That ordering is the ranking policy: the message is what is being searched, so a term in the message outranks the same term in a file it happened to carry. A test asserts exactly this by putting one term in a body and the same term in another message's attachment, then checking the order. Filenames were already at weight B and stay there.

Match provenance is computed after the page is cut, so it costs per returned row rather than per match. Subject, sender, and filename use cheap `to_tsvector` calls over short strings; the body is tested against the **stored** vector's weight-C class through `ts_rank`, which avoids re-tokenizing a body that can be megabytes. An attachment-content match also returns the attachment's filename and page number, which the result list renders as "Found in contract.pdf (page 4)" — without it, a hit whose term appears nowhere in the message reads as a bug.

Reprocessing is bounded and targets one attachment, all failures, attachments with no text yet, or an extractor-version mismatch. Each scope resets `text_status` to pending, because the handler skips attachments already in a terminal state; a test confirms the limit is respected, since a ten-year archive cannot afford a reprocess that ignores it.

One criterion is left open: **the format-routing tests do not cover PDF, office, or image attachments.** Those routes require a running Tika server and a Tesseract binary, and a test that silently passes when a service is absent is worse than no test. What is covered without external services is the plain-text route end to end, the unsupported-binary route, a mixed-success message where one attachment extracts and another is rejected, and — through direct projection writes — ranking, provenance, snippets, replacement, and every reprocessing scope. The three service-dependent routes are exercised by the Tika and OCR adapters' own suites; wiring them into a service-backed integration run belongs with Story 22.5's canary environment, where those services are guaranteed present.

**New files:**

- `crates/db/migrations/0024_attachment_text_job_type.down.sql`
- `crates/db/migrations/0024_attachment_text_job_type.up.sql`
- `crates/db/migrations/0025_email_attachment_text.down.sql`
- `crates/db/migrations/0025_email_attachment_text.up.sql`
- `crates/db/tests/attachment_text_search.rs`
- `crates/media/tests/fixtures/email/text-and-binary-attachments.eml`
- `crates/worker/src/attachment_text.rs`
- `crates/worker/tests/attachment_text_job.rs`

**Modified files:**

- `apps/web/src/api/types.ts`
- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`
- `apps/web/src/views/email/ResultList.test.tsx`
- `apps/web/src/views/email/ResultList.tsx`
- `crates/api/src/email.rs`
- `crates/db/src/lib.rs`
- `crates/media/tests/fixtures/email/expected.json`
- `crates/worker/src/email.rs`
- `crates/worker/src/lib.rs`
- `docs/email.md`

---

### Story 21.3 — Secure Attachment Download & Preview

As a user, I want to inspect archived attachments without exposing Strife to unsafe inline content. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Authenticated endpoints stream attachment artifacts with range support and sanitized `Content-Disposition` filenames.
- [x] The safe-inline allowlist matches Strife's file preview policy; HTML, SVG, executable, script, and unknown types download as attachments rather than render in the application origin.
- [x] Preview generation reuses existing artifact pipelines and never executes macros or embedded active content.
- [x] Inline `cid:` images use same-message authorization and cannot reference arbitrary storage keys.
- [x] Missing/regenerating artifacts return explicit states and can enqueue bounded rematerialization.
- [x] Tests cover traversal filenames, header injection, MIME spoofing, ranges, inline authorization, deleted source messages, and unsafe types.

**Implementation report:** `GET /api/email/messages/{node_id}/parts/{part_path}` streams one attachment, with `Range` support reusing the parser the file download already uses. This is the same URL the sanitizer rewrites a `cid:` reference to, which closes the criterion Story 20.4 had to leave open.

Same-message authorization is structural rather than checked: the artifact lookup is scoped by `node_id`, so there is no code path from this route to another message's artifact whatever the caller supplies. A test seeds two messages that both use part path `2` and confirms each serves only its own bytes, and that a part path a message does not declare returns 404 rather than resolving to someone else's. The storage key is never accepted from the request — it is read from the artifact row.

**Building this surfaced a pre-existing vulnerability.** The criterion says the allowlist should match Strife's file preview policy, and that policy — `is_native_preview_mime` in `files.rs`, used by `/api/files/{id}/preview-native` — allowed all of `image/*` inline. That includes `image/svg+xml`, which is a document that can carry script, so an uploaded SVG would have executed in Strife's own origin as stored XSS. `nosniff` does not help, because the type is declared correctly; it simply is not safe to render. Matching that policy would have propagated the bug, so the shared function now excludes SVG, HTML, and XHTML, and both surfaces read from it. The fix applies to user file previews as well as attachments.

Filename handling has two separate concerns and both are covered. Quotes, backslashes, and control characters are neutralized so a sender-chosen name cannot close the quoted `Content-Disposition` parameter and append headers of its own — a test confirms a `\r\n Set-Cookie` filename produces no such header. Path separators are also stripped, so `../../etc/passwd` is saved as `passwd`: the value is what the browser names the downloaded file, even though it never touches a path server-side.

An inline request is treated as a request rather than an instruction: it is honoured only for allowlisted types, and even then under a `default-src 'none'; sandbox` CSP, so an allowlisted type that turns out to be something else still cannot load or run anything. An artifact whose bytes are missing or whose materialization failed returns `202` naming the state rather than a bare `404` — "this attachment failed" and "no such attachment" are different facts — and enqueues a bounded rebuild, which is possible because the `.eml` original remains canonical.

**New files:**

- `crates/api/src/email_parts.rs`
- `crates/api/tests/email_parts_api.rs`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/views/email/MessageReader.tsx`
- `crates/api/src/files.rs`
- `crates/api/src/lib.rs`
- `docs/email.md`

---

### Story 21.4 — Thread Reconstruction, Labels & Duplicate Exploration

As a user, I want conversation and Gmail context reconstructed where evidence exists so that a decade of related mail is easier to navigate. **Estimated: 5 points.**

**Acceptance Criteria:**

- [x] Provider thread IDs are authoritative only when present and internally consistent.
- [x] Standards-based threading uses normalized `Message-ID`, `In-Reply-To`, and `References`; normalized subject is a documented fallback, not the primary key.
- [x] Missing parents and cycles are handled deterministically without dropping messages.
- [ ] Thread ordering uses sent date with stable fallbacks and exposes messages missing reliable dates.
- [x] Gmail labels are preserved as imported facts; Strife does not claim they remain synchronized with Gmail.
- [x] Duplicate grouping uses normalized `Message-ID` plus canonical-content hash fallback and records the grouping reason.
- [x] The UI can expand a thread, reveal collapsed duplicates, navigate to each original node, and filter by labels.
- [x] Tests cover reply chains, forks, missing parents, cycles, subject-only fallbacks, conflicting provider IDs, duplicate groups, and label Unicode.

**Implementation report:** The `thread_group_id` and `duplicate_group_id` columns existed from Epic 17 but nothing populated them. `crates/db/src/grouping.rs` now computes both **per message, from that message's own headers, without walking a chain of other messages.** That single choice is what makes the result safe against the shapes a decade-old export actually contains:

- a reply whose parent was never exported still lands in the right thread, because the group is derived from the root named in `References` rather than from a parent row that has to exist;
- two messages referencing each other cannot loop, because nothing is traversed — the worst case is two groups instead of one, and both messages are kept;
- a message computes the same group whether it is indexed first or last, so a backfill and a live import agree, and a reparse does not scatter a thread across new ids. A test asserts stability across a reparse for exactly that reason.

Group ids are `UUIDv5` over a fixed namespace and a normalized key, so identical evidence always yields the same id with no lookup table and no coordination.

Precedence is: provider thread id, then the `References` root (falling back to `In-Reply-To`, then the message's own id), then normalized subject. Subject is last and recorded as such — unrelated messages share a subject far more often than they share a `Message-ID`, so it is a documented fallback and never the primary key. A provider thread id is trusted only when it is *internally consistent*, meaning well-formed as Gmail writes it; anything unrecognizable is ignored rather than used to merge unrelated messages, which a test checks across four malformed values. When a usable provider id disagrees with the `References` root, the provider id still wins — Gmail knows about conversation moves the headers never recorded — but the disagreement is recorded in `thread_conflict` rather than hidden.

Duplicates group by normalized `Message-ID` (case-insensitively) and fall back to the canonical content hash when a message carries no id at all. Nothing is ever deleted: search collapses duplicates by default and revealing them lists every copy, so a user can open whichever original they need. `0026_email_grouping_reasons` stores why each grouping was chosen, which lets the reader tell a user in plain language — "Grouped only by a matching subject, which can join unrelated mail" — instead of presenting inference as certainty.

Gmail labels are stored verbatim, including Unicode and slashes, and a test asserts they survive unaltered and remain filterable. They are presented as imported facts; the reader labels them "Gmail labels as imported" and Strife makes no claim they still match Gmail. In the UI, a label chip narrows the search, and "Show conversation" and "Show every copy" navigate to a thread or duplicate-group URL — they are ordinary searches, so the back button returns to the previous one and the view keeps no extra state.

Implementing this changed real behaviour, which one existing test caught: `email_api.rs` had been seeding five messages that all shared one hard-coded `Message-ID`. They now correctly collapse as duplicates, so the fixture was given a distinct id per message — its premise was only true while grouping was unimplemented.

One criterion is left open: **thread ordering is not yet a first-class concern.** A thread search returns messages in relevance-then-date order like any other search rather than in conversation order, and a message with no reliable date is not called out as such within its thread. Ordering a thread properly means ordering by sent date with a stable fallback for undated messages and surfacing which ones lack a date — that belongs with the thread *view* rather than with grouping, and there is no thread view yet, only a filtered search. Building one on top of the grouping now in place is the natural next step.

**New files:**

- `crates/db/migrations/0026_email_grouping_reasons.down.sql`
- `crates/db/migrations/0026_email_grouping_reasons.up.sql`
- `crates/db/src/grouping.rs`
- `crates/db/tests/email_grouping.rs`

**Modified files:**

- `apps/web/src/api/client.ts`
- `apps/web/src/api/types.ts`
- `apps/web/src/views/EmailView.css`
- `apps/web/src/views/EmailView.tsx`
- `apps/web/src/views/email/MessageReader.test.tsx`
- `apps/web/src/views/email/MessageReader.tsx`
- `apps/web/src/views/email/criteria.test.ts`
- `apps/web/src/views/email/criteria.ts`
- `crates/api/src/email.rs`
- `crates/api/tests/email_api.rs`
- `crates/db/src/lib.rs`
- `docs/email.md`

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
