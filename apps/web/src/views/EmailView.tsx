import { useLocation, useNavigate } from '@solidjs/router'
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
  untrack,
} from 'solid-js'
import {
  getEmailFacets,
  getEmailMessage,
  getEmailStatus,
  reprocessEmail,
  searchEmail,
} from '../api/client'
import type {
  EmailFacets,
  EmailMessage,
  EmailSearchCriteria,
  EmailEvent,
  EmailSearchHit,
  EmailStatus,
} from '../api/types'
import { MessageReader } from './email/MessageReader'
import { ProcessingConsole } from './email/ProcessingConsole'
import { ResultList } from './email/ResultList'
import {
  criteriaFromSearch,
  criteriaToSearch,
  EMPTY_CRITERIA,
  hasAnyCriteria,
  isSearchable,
} from './email/criteria'
import './EmailView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

/** Long enough that typing a word does not fire a query per keystroke. */
const DEBOUNCE_MS = 300
const PAGE_SIZE = 25
/** Facet lists are capped server-side; this bounds what is rendered at once. */
const VISIBLE_FACETS = 12

type Phase = 'idle' | 'loading' | 'ready' | 'error'

export function EmailView() {
  const location = useLocation()
  const navigate = useNavigate()

  const criteria = createMemo(() => criteriaFromSearch(location.search))
  const openMessageId = createMemo(() =>
    new URLSearchParams(location.search).get('message'),
  )

  // The text input is uncontrolled with respect to the URL while typing: the URL
  // is only rewritten once typing settles, so history does not fill with a
  // separate entry per keystroke. The initial read is untracked because it seeds
  // the field once; the effect below is what keeps it in step afterwards.
  const [draft, setDraft] = createSignal(untrack(() => criteria().q))
  const [results, setResults] = createSignal<EmailSearchHit[]>([])
  const [phase, setPhase] = createSignal<Phase>('idle')
  const [error, setError] = createSignal<string>()
  const [cursor, setCursor] = createSignal<string | null>(null)
  const [pageStack, setPageStack] = createSignal<(string | null)[]>([])
  const [nextCursor, setNextCursor] = createSignal<string | null>(null)
  const [message, setMessage] = createSignal<EmailMessage>()
  const [messageError, setMessageError] = createSignal<string>()
  const [revealRemote, setRevealRemote] = createSignal(false)
  const [facetFilter, setFacetFilter] = createSignal('')
  const [announcement, setAnnouncement] = createSignal('')

  let closeButton: HTMLElement | undefined
  let lastFocused: HTMLElement | undefined

  const [facets] = createResource<EmailFacets>(async () => {
    if (staticPreview) return sampleFacets
    try {
      return await getEmailFacets()
    } catch {
      return { labels: [], correspondents: [], years: [] }
    }
  })

  // Distinguishes "this archive has nothing indexed" from "this search matched
  // nothing". Telling a user to broaden their terms when no mail has been
  // parsed at all sends them looking for a problem in the wrong place.
  //
  // Seeded by a fetch and thereafter updated by the event stream, so the counts
  // and the console never disagree about what the archive is doing.
  const [archive, setArchive] = createSignal<EmailStatus | undefined>(
    staticPreview ? sampleStatus : undefined,
  )
  const [entries, setEntries] = createSignal<EmailEvent[]>(
    staticPreview ? sampleEvents : [],
  )
  const [connection, setConnection] = createSignal<
    'connecting' | 'live' | 'reconnecting'
  >(staticPreview ? 'live' : 'connecting')
  const [consoleBusy, setConsoleBusy] = createSignal(false)

  createEffect(() => {
    if (staticPreview) return
    void getEmailStatus()
      .then(setArchive)
      .catch(() => setArchive(undefined))
  })

  // One stream feeds both the counts and the activity log. The console is
  // bounded at 200 entries because a ten-year backfill would otherwise grow the
  // DOM without limit.
  createEffect(() => {
    if (staticPreview) return
    const events = new EventSource('/api/email/events')
    events.onopen = () => setConnection('live')
    events.onerror = () => setConnection('reconnecting')
    events.addEventListener('entry', (event) => {
      const parsed = parseJson<EmailEvent>((event as MessageEvent<string>).data)
      if (parsed) setEntries((items) => [parsed, ...items.slice(0, 199)])
    })
    events.addEventListener('status', (event) => {
      const parsed = parseJson<EmailStatus>(
        (event as MessageEvent<string>).data,
      )
      if (parsed) setArchive(parsed)
    })
    onCleanup(() => events.close())
  })

  const retry = async (scope: 'node' | 'failed', nodeId?: string) => {
    if (
      scope === 'failed' &&
      !window.confirm(
        'Requeue failed messages? This runs in bounded batches, not all at once.',
      )
    )
      return
    setConsoleBusy(true)
    try {
      const enqueued = staticPreview
        ? 1
        : await reprocessEmail({ scope, nodeId, limit: 50 })
      setAnnouncement(
        `${enqueued} message${enqueued === 1 ? '' : 's'} requeued.`,
      )
    } catch (cause) {
      setAnnouncement(
        cause instanceof Error ? cause.message : 'Reprocessing failed.',
      )
    } finally {
      setConsoleBusy(false)
    }
  }
  const nothingIndexed = () => {
    const counts = archive()?.counts
    return counts !== undefined && counts.indexed === 0
  }

  /** Rewrites the URL, which is the single source of truth for search state. */
  const apply = (
    next: EmailSearchCriteria,
    options?: { keepMessage?: boolean; replace?: boolean },
  ) => {
    const openMessage = options?.keepMessage ? openMessageId() : null
    setPageStack([])
    setCursor(null)
    navigate(`/email${criteriaToSearch(next, openMessage)}`, {
      replace: options?.replace ?? false,
      scroll: false,
    })
  }

  // Debounce only the free-text field. Structured controls apply immediately,
  // because a click is already a deliberate act.
  createEffect(() => {
    const text = draft()
    if (text === criteria().q) return
    const timer = setTimeout(
      () => apply({ ...criteria(), q: text }, { replace: true }),
      DEBOUNCE_MS,
    )
    onCleanup(() => clearTimeout(timer))
  })

  // Keep the input in step when the URL changes underneath it — the back button
  // and a bookmarked search both land here.
  createEffect(() => {
    const fromUrl = criteria().q
    if (fromUrl !== draft()) setDraft(fromUrl)
  })

  // Every criteria or page change starts a fresh request and abandons the one in
  // flight, so a slow earlier response cannot overwrite a newer result set.
  createEffect(() => {
    const active = criteria()
    const page = cursor()
    if (!isSearchable(active)) {
      setResults([])
      setNextCursor(null)
      setPhase('idle')
      return
    }
    if (staticPreview) {
      setResults(sampleResults)
      setNextCursor(null)
      setPhase('ready')
      return
    }

    const controller = new AbortController()
    setPhase('loading')
    setError(undefined)
    void (async () => {
      try {
        const response = await searchEmail(active, page, controller.signal)
        setResults(response.results)
        setNextCursor(response.next_cursor)
        setPhase('ready')
        setAnnouncement(
          response.results.length === 0
            ? 'No messages matched.'
            : `${response.results.length} message${
                response.results.length === 1 ? '' : 's'
              } shown.`,
        )
      } catch (cause) {
        if (controller.signal.aborted) return
        setPhase('error')
        setError(
          !navigator.onLine
            ? 'You appear to be offline. Reconnect and retry.'
            : cause instanceof Error
              ? cause.message
              : 'Search failed.',
        )
      }
    })()
    onCleanup(() => controller.abort())
  })

  // Loading the open message is driven by the URL too, so a link to a message
  // reopens the reader with the search that found it intact.
  createEffect(() => {
    const nodeId = openMessageId()
    const allowRemote = revealRemote()
    if (!nodeId) {
      setMessage(undefined)
      setMessageError(undefined)
      return
    }
    if (staticPreview) {
      setMessage(sampleMessage)
      return
    }
    const controller = new AbortController()
    void (async () => {
      try {
        const loaded = await getEmailMessage(
          nodeId,
          false,
          controller.signal,
          allowRemote,
        )
        setMessage(loaded)
        setMessageError(undefined)
      } catch (cause) {
        if (controller.signal.aborted) return
        setMessage(undefined)
        setMessageError(
          cause instanceof Error ? cause.message : 'Message failed to load.',
        )
      }
    })()
    onCleanup(() => controller.abort())
  })

  const openMessage = (nodeId: string) => {
    lastFocused = document.activeElement as HTMLElement | undefined
    setRevealRemote(false)
    navigate(`/email${criteriaToSearch(criteria(), nodeId)}`, { scroll: false })
  }

  const closeMessage = () => {
    // Closing restores the search exactly as it was, including scroll position;
    // only the message parameter is dropped.
    navigate(`/email${criteriaToSearch(criteria(), null)}`, { scroll: false })
    lastFocused?.focus()
  }

  // Escape closes the reader without touching search state.
  createEffect(() => {
    if (!openMessageId()) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeMessage()
    }
    window.addEventListener('keydown', onKey)
    onCleanup(() => window.removeEventListener('keydown', onKey))
  })

  // Focus moves into the reader when it opens so keyboard users are not left
  // behind in the list.
  createEffect(() => {
    if (message()) closeButton?.focus()
  })

  // Thread and duplicate exploration are searches, not a separate mode: they
  // navigate to a URL like any other, so back returns to the previous search
  // and the view has no extra state to keep consistent.
  const showThread = (threadId: string) => {
    navigate(`/email?thread_id=${threadId}`, { scroll: false })
  }
  const showDuplicates = (groupId: string) => {
    navigate(`/email?duplicate_group=${groupId}&include_duplicates=true`, {
      scroll: false,
    })
  }

  const toggleValue = (list: string[], value: string) =>
    list.includes(value)
      ? list.filter((entry) => entry !== value)
      : [...list, value]

  const visibleLabels = createMemo(() => {
    const needle = facetFilter().trim().toLowerCase()
    const all = facets()?.labels ?? []
    const matching = needle
      ? all.filter((facet) => facet.value.toLowerCase().includes(needle))
      : all
    return matching.slice(0, VISIBLE_FACETS)
  })

  const visibleCorrespondents = createMemo(() => {
    const needle = facetFilter().trim().toLowerCase()
    const all = facets()?.correspondents ?? []
    const matching = needle
      ? all.filter((facet) => facet.value.toLowerCase().includes(needle))
      : all
    return matching.slice(0, VISIBLE_FACETS)
  })

  const goNext = () => {
    const next = nextCursor()
    if (!next) return
    setPageStack((stack) => [...stack, cursor()])
    setCursor(next)
  }

  const goPrevious = () => {
    const stack = pageStack()
    if (stack.length === 0) return
    setCursor(stack[stack.length - 1])
    setPageStack(stack.slice(0, -1))
  }

  return (
    <section class="workspace-view email-view">
      <header>
        <p class="workspace-view__eyebrow">Archive</p>
        <h1>Email</h1>
        <p class="workspace-view__description">
          Search a decade of archived mail by text, correspondent, label, and
          date.
        </p>
      </header>

      <search class="email-search">
        <div class="email-search__primary">
          <label class="sr-only" for="email-query">
            Search message text
          </label>
          <input
            id="email-query"
            type="search"
            placeholder="Search subjects, bodies, senders, labels…"
            value={draft()}
            onInput={(event) => setDraft(event.currentTarget.value)}
          />
          <Show when={hasAnyCriteria(criteria())}>
            <button type="button" onClick={() => apply({ ...EMPTY_CRITERIA })}>
              Clear all
            </button>
          </Show>
        </div>

        <div class="email-search__filters">
          <div class="email-filter">
            <label for="email-participant">Participant</label>
            {/* Matches any role, unlike the correspondent chips below, which
                narrow to the sender. Committed on Enter so a half-typed
                address does not trigger a search. */}
            <input
              id="email-participant"
              type="text"
              placeholder="anyone@example.test"
              onKeyDown={(event) => {
                if (event.key !== 'Enter') return
                event.preventDefault()
                const value = event.currentTarget.value.trim()
                if (!value || criteria().participant.includes(value)) return
                event.currentTarget.value = ''
                apply({
                  ...criteria(),
                  participant: [...criteria().participant, value],
                })
              }}
            />
          </div>
          <div class="email-filter">
            <label for="email-after">Sent after</label>
            <input
              id="email-after"
              type="date"
              value={criteria().after}
              onChange={(event) =>
                apply({ ...criteria(), after: event.currentTarget.value })
              }
            />
          </div>
          <div class="email-filter">
            <label for="email-before">Sent before</label>
            <input
              id="email-before"
              type="date"
              value={criteria().before}
              onChange={(event) =>
                apply({ ...criteria(), before: event.currentTarget.value })
              }
            />
          </div>
          <div class="email-filter">
            <label for="email-attachment">Attachments</label>
            <select
              id="email-attachment"
              value={
                criteria().hasAttachment === null
                  ? 'any'
                  : String(criteria().hasAttachment)
              }
              onChange={(event) => {
                const raw = event.currentTarget.value
                apply({
                  ...criteria(),
                  hasAttachment: raw === 'any' ? null : raw === 'true',
                })
              }}
            >
              <option value="any">Any</option>
              <option value="true">With attachments</option>
              <option value="false">Without attachments</option>
            </select>
          </div>
          <div class="email-filter email-filter--checks">
            <label>
              <input
                type="checkbox"
                checked={criteria().includeTrashed}
                onChange={(event) =>
                  apply({
                    ...criteria(),
                    includeTrashed: event.currentTarget.checked,
                  })
                }
              />
              Include trashed
            </label>
            <label>
              <input
                type="checkbox"
                checked={criteria().includeDuplicates}
                onChange={(event) =>
                  apply({
                    ...criteria(),
                    includeDuplicates: event.currentTarget.checked,
                  })
                }
              />
              Show duplicates
            </label>
          </div>
        </div>

        <div class="email-search__facets">
          <label class="sr-only" for="email-facet-filter">
            Filter label and correspondent options
          </label>
          <input
            id="email-facet-filter"
            type="search"
            placeholder="Filter labels and people…"
            value={facetFilter()}
            onInput={(event) => setFacetFilter(event.currentTarget.value)}
          />
          {/* Active participants have no facet list to un-press, so they get
              their own removable chips. */}
          <Show when={criteria().participant.length > 0}>
            <fieldset>
              <legend>Participants</legend>
              <For each={criteria().participant}>
                {(value) => (
                  <button
                    type="button"
                    class="email-chip is-toggle"
                    aria-pressed={true}
                    aria-label={`Remove participant ${value}`}
                    onClick={() =>
                      apply({
                        ...criteria(),
                        participant: toggleValue(criteria().participant, value),
                      })
                    }
                  >
                    {value} <small>×</small>
                  </button>
                )}
              </For>
            </fieldset>
          </Show>
          <fieldset>
            <legend>Labels</legend>
            <For each={visibleLabels()}>
              {(facet) => (
                <button
                  type="button"
                  class="email-chip is-toggle"
                  aria-pressed={criteria().label.includes(facet.value)}
                  onClick={() =>
                    apply({
                      ...criteria(),
                      label: toggleValue(criteria().label, facet.value),
                    })
                  }
                >
                  {facet.value} <small>{facet.count}</small>
                </button>
              )}
            </For>
          </fieldset>
          <fieldset>
            <legend>Correspondents</legend>
            <For each={visibleCorrespondents()}>
              {(facet) => (
                <button
                  type="button"
                  class="email-chip is-toggle"
                  aria-pressed={criteria().from.includes(facet.value)}
                  onClick={() =>
                    apply({
                      ...criteria(),
                      from: toggleValue(criteria().from, facet.value),
                    })
                  }
                >
                  {facet.value} <small>{facet.count}</small>
                </button>
              )}
            </For>
          </fieldset>
        </div>
      </search>

      {/* Restrained: announces the settled result count, not every event. */}
      <p class="sr-only" role="status" aria-live="polite">
        {announcement()}
      </p>

      {/* Below the search so the page still opens on what most visits want —
          finding a message — with backfill progress a scroll away. */}
      <ProcessingConsole
        status={archive()}
        entries={entries()}
        connection={connection()}
        busy={consoleBusy()}
        onRetryOne={(nodeId) => void retry('node', nodeId)}
        onRetryFailed={() => void retry('failed')}
      />

      <div class="email-layout" classList={{ 'is-reading': !!message() }}>
        <section
          class="email-layout__list"
          aria-labelledby="email-results-title"
        >
          <h2 id="email-results-title" class="sr-only">
            Search results
          </h2>

          <Show when={nothingIndexed()}>
            <p class="email-empty">
              No mail has been indexed yet. Upload or import <code>.eml</code>{' '}
              files, or run an email backfill, and messages will appear here.
            </p>
          </Show>
          <Show when={!nothingIndexed()}>
            <Show when={phase() === 'idle'}>
              <p class="email-empty">
                Enter a search term or choose a filter to search the archive.
              </p>
            </Show>
            <Show when={phase() === 'loading'}>
              <p class="email-empty">Searching…</p>
            </Show>
            <Show when={phase() === 'error'}>
              <div class="email-empty is-error" role="alert">
                <p>{error()}</p>
                <button type="button" onClick={() => setCursor(cursor())}>
                  Retry
                </button>
              </div>
            </Show>
            <Show when={phase() === 'ready' && results().length === 0}>
              <p class="email-empty">
                No messages matched. Try fewer filters or a broader term.
              </p>
              {/* Messages that failed to parse are not in the index at all, so
                  a search cannot surface them. Saying so here keeps a parse
                  backlog from reading as a search that found nothing. */}
              <Show when={(archive()?.counts.failed ?? 0) > 0}>
                <p class="email-empty">
                  {archive()?.counts.failed} message
                  {archive()?.counts.failed === 1 ? '' : 's'} failed to parse
                  and
                  {archive()?.counts.failed === 1 ? ' is' : ' are'} not
                  searchable.
                </p>
              </Show>
            </Show>
          </Show>

          <Show when={phase() === 'ready' && results().length > 0}>
            <ResultList
              results={results()}
              selected={openMessageId()}
              onSelect={openMessage}
            />
            <div class="email-pager">
              <button
                type="button"
                disabled={pageStack().length === 0}
                onClick={goPrevious}
              >
                Previous
              </button>
              <span>
                {results().length} of up to {PAGE_SIZE} on this page
              </span>
              <button type="button" disabled={!nextCursor()} onClick={goNext}>
                Next
              </button>
            </div>
          </Show>
        </section>

        <Show when={openMessageId()}>
          <section class="email-layout__reader" aria-label="Message">
            <Show when={messageError()}>
              {(problem) => (
                <div class="email-empty is-error" role="alert">
                  <p>{problem()}</p>
                  <button type="button" onClick={closeMessage}>
                    Back to results
                  </button>
                </div>
              )}
            </Show>
            <Show when={message()}>
              {(loaded) => (
                <MessageReader
                  message={loaded()}
                  remoteRevealed={revealRemote()}
                  onRevealRemote={() => setRevealRemote(true)}
                  onClose={closeMessage}
                  onShowThread={showThread}
                  onShowDuplicates={showDuplicates}
                  onFilterLabel={(label) =>
                    apply({
                      ...criteria(),
                      label: toggleValue(criteria().label, label),
                    })
                  }
                  ref={(element) => (closeButton = element)}
                />
              )}
            </Show>
          </section>
        </Show>
      </div>
    </section>
  )
}

/** SSE payloads are untrusted text until parsed; a malformed frame is dropped. */
function parseJson<T>(value: string): T | undefined {
  try {
    return JSON.parse(value) as T
  } catch {
    return undefined
  }
}

const sampleStatus: EmailStatus = {
  counts: {
    foreground_pending: 3,
    foreground_running: 1,
    backfill_pending: 128,
    backfill_running: 1,
    completed: 48211,
    failed: 12,
    skipped: 340,
    unsupported: 88,
    remaining: 133,
    indexed: 48211,
    candidates: 48651,
    attachments_pending: 940,
    attachments_completed: 12044,
    attachments_failed: 6,
  },
  completed_per_hour: 4200,
  eta_seconds: 114,
  parser_version: '0.11.5',
  attachment_extractor_version: '1',
  campaigns: [
    {
      id: '00000000-0000-0000-0000-0000000000c1',
      state: 'running',
      snapshot_before: '2026-08-03T09:00:00Z',
      cursor_created_at: '2024-02-11T04:12:00Z',
      cursor_node_id: '00000000-0000-0000-0000-0000000000a7',
      batch_size: 100,
      max_queued: 500,
      max_running: 1,
      resource_class: 'heavy_cpu',
      foreground_fairness: 20,
      candidate_count: 691402,
      enqueued_count: 1200,
      completed_count: 1142,
      failed_count: 3,
      skipped_count: 55,
      last_error: null,
    },
  ],
}

const sampleEvents: EmailEvent[] = [
  {
    id: 3,
    node_id: 'preview-1',
    name: 'q3-recon.eml',
    state: 'completed',
    subject: 'Quarterly reconciliation figures',
    attachment_count: 2,
    duration_ms: 42,
    origin: 'backfill',
    campaign_id: '00000000-0000-0000-0000-0000000000c1',
    warning: null,
    created_at: '2026-08-03T09:42:15Z',
  },
  {
    id: 2,
    node_id: 'preview-9',
    name: 'broken.eml',
    state: 'failed',
    subject: null,
    attachment_count: null,
    duration_ms: null,
    origin: 'backfill',
    campaign_id: '00000000-0000-0000-0000-0000000000c1',
    warning:
      'email source size limit exceeded: 91000000 bytes is greater than 67108864',
    created_at: '2026-08-03T09:42:09Z',
  },
  {
    id: 1,
    node_id: 'preview-8',
    name: 'notes.txt',
    state: 'unsupported',
    subject: null,
    attachment_count: null,
    duration_ms: null,
    origin: 'foreground',
    campaign_id: null,
    warning: 'email parsing does not support MIME type text/plain',
    created_at: '2026-08-03T09:41:58Z',
  },
]

const sampleFacets: EmailFacets = {
  labels: [
    { value: 'Inbox', count: 48211 },
    { value: 'Work', count: 21044 },
    { value: 'Receipts', count: 9310 },
    { value: 'Travel', count: 4102 },
  ],
  correspondents: [
    { value: 'ada@example.test', count: 1841 },
    { value: 'billing@vendor.test', count: 902 },
  ],
  years: [
    { value: '2024', count: 4820 },
    { value: '2023', count: 5117 },
  ],
}

const sampleResults: EmailSearchHit[] = [
  {
    node_id: 'preview-1',
    subject: 'Quarterly reconciliation figures',
    sent_at: '2024-03-11T09:14:00Z',
    snippet:
      'The [[reconciliation]] figures are attached; the variance is under a point.',
    attachment_count: 2,
    duplicate_count: 1,
    thread_count: 4,
    score: 0.82,
    from_address: 'ada@example.test',
    from_display_name: 'Ada Lovelace',
    labels: ['Work'],
    match_sources: ['subject', 'body'],
    matched_attachment: null,
    matched_attachment_page: null,
  },
  {
    node_id: 'preview-2',
    subject: null,
    sent_at: null,
    snippet: 'Sent without a subject or a parsable date.',
    attachment_count: 0,
    duplicate_count: 3,
    thread_count: 1,
    score: 0.41,
    from_address: null,
    from_display_name: null,
    labels: ['Archive'],
    match_sources: ['attachment_content'],
    matched_attachment: 'contract.pdf',
    matched_attachment_page: 4,
  },
]

const sampleMessage: EmailMessage = {
  node_id: 'preview-1',
  status: 'completed',
  parser_version: '0.11.5',
  message_id: '<q3-recon@example.test>',
  in_reply_to: null,
  references: [],
  subject: 'Quarterly reconciliation figures',
  sent_at: '2024-03-11T09:14:00Z',
  received_at: '2024-03-11T09:14:22Z',
  body_text:
    'The reconciliation figures are attached.\n\nVariance is under a point.\n\n— Ada',
  body_html: '<p>The reconciliation figures are attached.</p>',
  blocked_remote_count: 2,
  blocked_hosts: ['tracker.example.test'],
  preview_text: 'The reconciliation figures are attached.',
  thread_group_id: '00000000-0000-0000-0000-0000000000a1',
  thread_reason: 'references',
  thread_conflict: false,
  duplicate_group_id: '00000000-0000-0000-0000-0000000000b2',
  duplicate_reason: 'message_id',
  provider_thread_id: null,
  labels: ['Work'],
  addresses: [
    {
      role: 'from',
      display_name: 'Ada Lovelace',
      address: 'ada@example.test',
    },
    { role: 'to', display_name: null, address: 'owner@example.test' },
  ],
  attachments: [
    {
      part_path: '2',
      filename: 'q3-recon.pdf',
      media_type: 'application/pdf',
      disposition: 'attachment',
      content_id: null,
      decoded_size: 184320,
      is_inline: false,
      is_message: false,
      extraction_status: 'pending',
    },
  ],
  warnings: [],
  raw_headers: null,
}
