import { For, Show } from 'solid-js'
import type { EmailSearchHit } from '../../api/types'
import { formatDate, senderOf, subjectOf } from './format'
import { parseSnippet } from './snippet'

/**
 * Renders a server-marked snippet.
 *
 * The runs come back as data, and each one becomes a real text or `<mark>`
 * node. Message bodies are attacker-controlled, so the snippet never touches
 * `innerHTML` — this is the whole reason the markers are parsed rather than
 * interpolated.
 */
function Snippet(props: { snippet: string }) {
  return (
    <p class="email-result__snippet">
      <For each={parseSnippet(props.snippet)}>
        {(run) => (
          <Show when={run.marked} fallback={<>{run.text}</>}>
            <mark>{run.text}</mark>
          </Show>
        )}
      </For>
    </p>
  )
}

export function ResultList(props: {
  results: EmailSearchHit[]
  selected: string | null
  onSelect: (nodeId: string) => void
}) {
  /**
   * Roving arrow-key navigation across results. Home and End jump to the ends,
   * which matters most on a long page where the mouse is the slow path.
   */
  const onKeyDown = (event: KeyboardEvent, index: number) => {
    const keys = ['ArrowDown', 'ArrowUp', 'Home', 'End']
    if (!keys.includes(event.key)) return
    event.preventDefault()
    const last = props.results.length - 1
    const next =
      event.key === 'ArrowDown'
        ? Math.min(index + 1, last)
        : event.key === 'ArrowUp'
          ? Math.max(index - 1, 0)
          : event.key === 'Home'
            ? 0
            : last
    // Resolved from the list root rather than by walking siblings, so the
    // markup between the list and a result can change without silently
    // breaking keyboard navigation.
    const current = event.currentTarget as HTMLElement
    const items = current
      .closest('.email-results')
      ?.querySelectorAll<HTMLElement>('.email-result')
    items?.[next]?.focus()
  }

  return (
    <ul class="email-results" role="list">
      <For each={props.results}>
        {(hit, index) => (
          <li>
            {/* A result is a control, not a link: activating it opens the
                reader beside the list without discarding the search URL. */}
            <div
              class="email-result"
              classList={{ 'is-selected': props.selected === hit.node_id }}
              role="button"
              tabIndex={0}
              aria-pressed={props.selected === hit.node_id}
              onClick={() => props.onSelect(hit.node_id)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault()
                  props.onSelect(hit.node_id)
                  return
                }
                onKeyDown(event, index())
              }}
            >
              <div class="email-result__line">
                <span class="email-result__sender">
                  {senderOf(hit.from_display_name, hit.from_address)}
                </span>
                <Show
                  when={hit.sent_at}
                  fallback={
                    <span class="email-result__date is-missing">
                      (no date recorded)
                    </span>
                  }
                >
                  {(sent) => (
                    <time class="email-result__date" dateTime={sent()}>
                      {formatDate(sent())}
                    </time>
                  )}
                </Show>
              </div>

              <p class="email-result__subject">{subjectOf(hit.subject)}</p>
              <Snippet snippet={hit.snippet} />

              {/* An attachment-only hit looks like a mistake without this: the
                  search term appears nowhere in the message itself. */}
              <Show when={hit.matched_attachment}>
                {(name) => (
                  <p class="email-result__provenance">
                    Found in {name()}
                    <Show when={hit.matched_attachment_page !== null}>
                      {' '}
                      (page {hit.matched_attachment_page})
                    </Show>
                  </p>
                )}
              </Show>

              <div class="email-result__meta">
                <Show when={hit.attachment_count > 0}>
                  <span class="email-chip">
                    {hit.attachment_count} attachment
                    {hit.attachment_count === 1 ? '' : 's'}
                  </span>
                </Show>
                <Show when={hit.thread_count > 1}>
                  <span class="email-chip">{hit.thread_count} in thread</span>
                </Show>
                <Show when={hit.duplicate_count > 1}>
                  <span class="email-chip">{hit.duplicate_count} copies</span>
                </Show>
                <For each={hit.labels}>
                  {(label) => <span class="email-chip is-label">{label}</span>}
                </For>
              </div>
            </div>
          </li>
        )}
      </For>
    </ul>
  )
}
