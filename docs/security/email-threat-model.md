# Email archive threat model

Archived email is the most hostile input Strife handles. Every other source —
uploads, watched-folder imports — comes from the operator. A ten-year mailbox
comes from everyone who ever emailed them, including people who were trying
something. The messages are also old, which matters: they were written for
renderers that no longer exist, by senders who are no longer reachable, and they
have already been delivered once. Nothing in the archive can be re-fetched,
re-validated, or asked to explain itself.

This document states what Strife assumes about that content, what it does about
each threat, and what remains uncovered. It is the reference the sanitizer
fixtures and the parser limit tests are written against.

## Trust posture

Three assumptions drive every decision below.

1. **Message content is attacker-controlled.** Body, headers, filenames, MIME
   structure, and `Content-Type` are all sender-supplied. None is trusted for
   routing, storage keys, rendering decisions, or type dispatch.
2. **The `.eml` original is canonical and immutable.** Everything derived from it
   — projections, artifacts, index entries — is disposable and rebuildable. This
   is what makes it safe to reprocess aggressively after fixing a parser bug.
3. **Reading a message must not be an event the sender can observe.** Opening an
   archived message is a private act. Anything that would tell a sender their
   decade-old message was just read is a leak, not a feature.

## Threats and controls

### Malicious MIME structure

A message can declare thousands of parts, nest multipart containers dozens deep,
or truncate mid-boundary. Each part is individually cheap; the total is not.

**Controls.** `EMAIL_MAX_PARTS` is checked immediately after parsing and before
any per-part work. `EMAIL_MAX_ATTACHMENT_DEPTH` bounds how deep materialization
descends. Parsing is delegated to `mail-parser`, which is iterative over a parsed
tree rather than recursive over input, so a deeply nested message cannot exhaust
the stack. `EMAIL_FILE_TIMEOUT_SECONDS` bounds wall time regardless of shape.

**Tests.** `crates/media/tests/email_limits.rs` covers the part-count limit,
deep recursive nesting, and truncated input.

### Parser vulnerabilities

`mail-parser` is pure Rust with no `unsafe` in its parsing path, which removes
the memory-corruption class that C MIME parsers are known for. It does not
remove logic bugs.

**Controls.** Limits bound what a single message can consume even when parsing
succeeds. The worker container has a CPU quota and a memory ceiling, so a runaway
parse is killed by the container rather than taking the host down. Parser and
sanitizer versions are recorded per message, so a fix can be followed by bounded
reprocessing of exactly the affected rows.

**Gap.** Parsing runs in-process in the worker rather than in a child process
with its own rlimits. A memory-safety bug in the parser would run with the
worker's privileges. Recorded in [`known-limitations.md`](../known-limitations.md).

### Decompression and expansion

A small message can decode to a very large one — base64 expands, nested messages
contain messages, and a compressed attachment can be a zip bomb.

**Controls.** `EMAIL_MAX_SOURCE_BYTES` bounds input. `EMAIL_MAX_BODY_BYTES`
bounds the decoded body. `EMAIL_MAX_ATTACHMENT_BYTES` and
`EMAIL_MAX_TOTAL_ATTACHMENT_BYTES` bound decoded attachments per part and per
message. An over-limit part is **skipped, not truncated**: half an attachment is
not a smaller attachment, and storing one would produce an artifact whose
checksum can never verify.

**Gap.** Strife does not decompress archive attachments, so zip-bomb expansion is
out of scope today. It becomes in scope the moment archive contents are indexed.

### HTML and script injection

Archived HTML contains script tags that were live when the message was sent,
event handlers, embedded objects, and markup written for renderers with different
parsing quirks.

**Controls.** Three independent layers, none asked to be sufficient alone:

1. **Server-side sanitizing** (`crates/media/src/email/sanitize.rs`) on a pinned
   `ammonia` built on `html5ever`, so hostile markup is parsed the way a browser
   parses it rather than by pattern matching. The browser never receives the
   original bytes. Script, style, iframe, object, embed, form, input, svg, math,
   link, meta, and base have no allowlist entry, and script/style text is removed
   along with the element rather than left behind as visible text.
2. **A sandboxed frame** without `allow-scripts`. `allow-same-origin` is present
   only so inline `cid:` parts can load; the two together would be the classic
   sandbox escape and **must never both appear**.
3. **A CSP** naming `default-src 'none'` and `script-src 'none'`, so nothing runs
   even if the sandbox attribute were dropped by a future edit.

Message HTML exists only as an `srcdoc` string and is never assigned into
Strife's own DOM.

**Tests.** `crates/media/tests/email_sanitize.rs` is written from the attacker's
side: each case asserts a capability is *absent*, so output formatting may change
freely while a surviving capability fails the build. It includes the classic
regex-sanitizer bypasses. `MessageReader.test.tsx` asserts the sandbox never
gains `allow-scripts` and that message HTML never reaches the application DOM.

### CSS-based leaks

CSS can exfiltrate without script: `background-image: url(...)` fires a request,
`@import` pulls a remote stylesheet, and attribute selectors can leak content
character by character through selective image loads.

**Controls.** Inline styles are filtered **by property**, not passed through.
Dropping CSS wholesale would make a decade of table-layout mail unreadable, so
the allowlist keeps colour, spacing, and typography and rejects anything that can
fetch, position, or escape. Values containing `url(`, `image-set`, `expression`,
`@import`, a CSS escape (`\`), or a comment (`/*`) are dropped outright.
`<style>` elements are removed with their contents, so selector-based leaks have
nowhere to live.

### Tracking pixels and remote resources

A 1×1 remote image tells the sender their message was opened, and when.

**Controls.** Remote image URLs are **removed server-side**, not hidden in the
browser. This distinction is the whole control: a URL that reaches the browser
has already fired. The reader reports how many remote references a message holds
and which hosts they name, and revealing them re-requests the message with
`allow_remote_images=true` — consent is a different server response. Consent to
images is not consent to anything else; scripts and `url()` stay stripped in the
revealed response, which a test asserts. Remote fonts, stylesheets, frames, and
media have no allowlist entry at all and cannot be revealed.

### Unsafe URLs

Link text in archived mail is frequently misleading — "click here", or a display
URL that differs from the target.

**Controls.** Only `http`, `https`, `mailto`, and `tel` survive. Relative and
protocol-relative URLs are rejected, because an archived message has no base URL
and they would resolve against Strife's own origin. `rel="noopener noreferrer"`
denies the opened page any handle on Strife's window. Sender-supplied `title`
attributes are discarded and replaced on load with the link's real destination.

### Attachment MIME spoofing

A sender chooses both the filename and the declared `Content-Type`. An
executable can claim to be a PDF, and an SVG is an image type that can carry
script.

**Controls.** Attachments are served with `X-Content-Type-Options: nosniff` and a
restrictive CSP. The safe-inline allowlist explicitly excludes SVG, HTML, and
XHTML despite SVG being an `image/*` type — `nosniff` does not help there,
because the type is declared correctly and simply is not safe to render. An
inline request is honoured only for allowlisted types; everything else downloads
regardless of what the caller asked for.

> This allowlist is shared with Strife's own file preview. It was **widened from
> a pre-existing bug**: `is_native_preview_mime` previously allowed all of
> `image/*`, so an uploaded SVG would have executed script in Strife's origin.

### Header injection

Filenames reach HTTP headers. A filename containing CRLF could inject headers; a
filename containing quotes could close the `Content-Disposition` parameter and
append parameters of its own.

**Controls.** `safe_filename` neutralizes quotes, backslashes, and control
characters, and strips path separators so a name like `../../etc/passwd` is saved
as `passwd`. The value is a header value only — bytes are located by the artifact
row, never by this string.

### Sensitive data in logs and events

Logs are retained, shipped, and read by people who are not reading the mailbox.

**Controls.** Structured logs carry node, job, and campaign identifiers plus
measurements (byte counts, durations, part counts) and never body text,
addresses, or raw headers. The `email_events` table — retained indefinitely and
streamed live to the console — carries identifiers, states, and measurements. The
subject is the single exception: bounded to 120 bytes, present only for
successfully parsed messages, and already visible in search results to anyone who
can see the console. API-visible errors pass through `sanitize`, which keeps only
the first line so an underlying cause cannot drag content into a response.

### Search snippet leakage

Snippets are generated from message bodies and rendered in a list.

**Controls.** `ts_headline` markers are parsed into structured runs and rendered
as text and `<mark>` nodes. The snippet never touches `innerHTML`, so HTML inside
an archived message renders as visible text rather than as markup. Search queries
are not logged.

## Network policy

Email parsing and attachment materialization make **no outbound requests by
design**: parsing is pure, and remote resources are stripped rather than fetched.
Attachment *text* extraction is the exception — it posts to Tika, which is a
service on the internal Compose network.

The production stack runs `read_only: true`, `cap_drop: ALL`, and
`no-new-privileges`, with the worker on the internal network alongside Postgres
and Tika. A container-level egress deny rule is **not** currently applied; on a
single-host Compose deployment there is no separate egress path to block without
also blocking Tika. Recorded here so it is a decision rather than an oversight.

## Dependency updates

The following changes require the named suites to be re-run before merge:

| Change | Suites |
| --- | --- |
| `ammonia` or `html5ever` | `email_sanitize`, `MessageReader.test.tsx` |
| `mail-parser` or `encoding_rs` | `email_parser`, `email_fixtures`, `email_limits` |
| Tika image | `attachment_text_job`, OCR adapter suite |
| Tesseract | OCR adapter suite, `attachment_text_job` |
| Storage or `axum` | `email_parts_api`, `edge_cases` |

`ammonia` is pinned with `=` rather than a caret specifically so a relaxation of
its allowlist cannot arrive through a routine lockfile update.

## Review triggers

Revisit this document when any of the following change: the sanitizer allowlist,
the sandbox or CSP on the reader frame, the safe-inline MIME allowlist, what is
written to `email_events`, whether archive attachments are decompressed, or
whether attachment extraction gains a network dependency beyond Tika.
