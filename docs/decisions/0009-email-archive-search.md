# ADR 0009: Search a Gmail-Exported Email Archive as Structured Strife Files

- **Status:** Accepted
- **Date:** 2026-08-03

## Context

Strife already holds roughly ten years of Gmail-exported `.eml` files as
ordinary managed originals. They are currently opaque: the metadata pipeline
records a MIME type and size, no body text is retained, and nothing about a
message's sender, recipients, date, thread, labels, or attachments is
queryable. Finding a message means downloading candidate files and opening them
elsewhere.

Making the archive searchable touches parsing, storage, ranking, rendering, and
capacity at once. Email is also the first content type Strife will process
whose historical volume — hundreds of thousands of messages — makes an
unbounded backfill a genuine operational risk on Orion, and the first whose
content is actively hostile: archived mail contains tracking pixels, remote
resources, and scripted HTML written by third parties.

The OCR work in [ADR 0008](0008-ocr-and-document-text.md) established the
document-text contract and [`backfill.md`](../backfill.md) established the
shared campaign, admission-control, and rollout machinery. Email reuses both
rather than inventing parallel mechanisms.

## Decision

- Each `.eml` file remains an immutable Strife node and the canonical original.
  Parsed email rows are disposable, regenerable projections keyed by node, and
  are removed when their node is permanently deleted.
- Email gets a dedicated structured schema rather than being forced into the
  page-oriented `document_text` tables. Email needs ordered addresses by role,
  repeated headers, thread and label relationships, and an attachment manifest;
  page semantics do not apply to it.
- MIME is confirmed as `message/rfc822` from content bytes. The file extension
  and any upload-supplied MIME are hints only.
- Parsing uses `mail-parser` (Stalwart Labs, Apache-2.0 OR MIT), pinned at
  0.11.5 with `default-features = false` and only `full_encoding` enabled. It
  was chosen over `mailparse` and the `mailrs-*` family because it covers the
  whole surface this archive needs in one crate — MIME part trees, RFC 2047
  encoded words, RFC 2231 parameters, transfer-encoding decoding, nested
  `message/rfc822`, and legacy charset decoding — and because its dependency
  tree is `encoding_rs` plus one compile-time proc-macro, giving the adapter no
  network or filesystem surface. `full_encoding` is required: a ten-year
  archive contains latin-1 and other pre-UTF-8 bodies.
- The parser's typed single-header accessors are deliberately not used for
  address extraction. A message carrying repeated `To:` headers would lose
  every recipient but the last, so extraction walks the full header list.
- Extraction is automatic for files finalized after the feature deploys.
  Historical files are processed only through an explicitly started, initially
  paused backfill campaign. Deployment, migration, and worker startup never
  enumerate the existing library.
- PostgreSQL weighted full-text search is the initial engine. Subject and
  primary correspondents rank above recipients, labels, and attachment
  filenames, which rank above body text. A measured archive-scale benchmark is
  required before a dedicated search service is reconsidered.
- The complete normalized body is indexed initially. Quoted replies and
  signatures are indexed as written; heuristic stripping is deferred until
  ranking is measured, because removing real content is worse than ranking it
  low.
- Plain text is preferred for search and reading when a `multipart/alternative`
  message supplies it. HTML-only bodies are converted to text for indexing
  while the HTML alternative is retained for rendering.
- Rendered HTML is sanitized with a pinned configuration, executes no active
  content, shows link destinations explicitly, and blocks remote images, fonts,
  stylesheets, frames, and media until the reader deliberately reveals them.
  Inline `cid:` references resolve only to authenticated attachment endpoints
  belonging to the same message.
- Duplicate detection never deletes an original. Duplicates are grouped by
  normalized `Message-ID` with a canonical-content hash fallback, collapsed to
  one representative in search by default, and always fully revealable.
- Gmail labels and provider thread identifiers are preserved as imported facts
  when the export supplies them. Strife does not claim they remain synchronized
  with Gmail, and ordinary RFC email does not require them.
- Email is read-only and reparsable. The interface displays, copies, and
  downloads it but never edits a message or its original.
- Search excludes trashed nodes by default; including them requires an explicit
  parameter.
- Email parsing, OCR, and attachment extraction share the campaign scheduler,
  job origin and resource classes, renewable resource leases, priority and
  fairness policy, migration rules, and Orion rollout sequence defined in
  [`backfill.md`](../backfill.md).
- Email body parsing begins under the shared `heavy_cpu` admission even though
  MIME parsing is expected to be cheaper than OCR. Promotion to `extractor`
  requires the 10,000-message canary to record safe resource behavior first.
- Parser, sanitizer, normalization, attachment-extractor, and search-index
  versions are recorded independently, because a change to any one of them
  implies a different and independently schedulable reprocessing scope.

## Alternatives Considered

- Store email text in the existing `document_text` page tables
- Import each message as a folder of extracted parts and index those as files
- Index only headers and defer body search
- Adopt OpenSearch or Elasticsearch before measuring PostgreSQL
- Strip quoted replies and signatures before indexing
- Deduplicate by deleting redundant `.eml` originals
- Render original message HTML directly in the application origin
- Enqueue the historical archive automatically on deployment

## Consequences

- The archive becomes searchable by content, correspondent, date, label, and
  attachment without changing or endangering the canonical `.eml` files.
- PostgreSQL capacity planning must include normalized bodies, structured
  address and header rows, and a weighted search index sized for the whole
  archive. Parsed projections are rebuildable, but the `.eml` originals must be
  covered by backup and restore drills.
- Rendering archived mail is a security surface, not only a display concern. It
  requires a pinned sanitizer, hostile-HTML regression fixtures, and a
  documented threat model before the reader ships.
- Shipping email support and indexing the historical archive are separate
  operations. A production deployment is inert until an operator reviews a
  read-only preflight and explicitly starts a campaign.
- Because email, OCR, and attachment work share one heavy admission permit,
  only one historical campaign progresses at a time on Orion initially. This is
  deliberate: it bounds total CPU rather than balancing several backlogs.
- The initial ranking weights, body policy, and resource limits are hypotheses.
  They must be revised from measurements rather than treated as settled.
