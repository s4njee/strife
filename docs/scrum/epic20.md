# Epic 20 — Email Navigation, Search & Reader UI


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
