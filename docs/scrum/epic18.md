# Epic 18 — MIME Extraction & Durable Email Projection


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
- [x] Historical processing uses the campaign scheduler in [`backfill.md`](../backfill.md), starting paused and refilling only when active queued/running work falls below its low-water mark.
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
