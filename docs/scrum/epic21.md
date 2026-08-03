# Epic 21 — Attachment Search, Threads & Gmail Context


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
