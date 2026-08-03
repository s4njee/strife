# Epic 17 — Email Decisions, Schema & Queue Foundation


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
- [x] The ADR adopts the shared campaign, priority, admission-control, migration, and Orion rollout contract in [`backfill.md`](../backfill.md).
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

- [x] The shared `0017_backfill_campaigns` migration described by [`backfill.md`](../backfill.md) lands first; a reversible `0018_email_messages.{up,down}.sql` migration then adds email storage without rewriting existing node or job rows.
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
