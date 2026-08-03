import { createMemo, createSignal, For, Show } from 'solid-js'
import type { EmailMessage } from '../../api/types'
import { downloadFileUrl } from '../../api/client'
import { useTheme } from '../../theme/ThemeProvider'
import type { Theme } from '../../theme/ThemeProvider'
import {
  formatBytes,
  formatDateTime,
  ROLE_LABELS,
  ROLE_ORDER,
  subjectOf,
} from './format'

/**
 * Builds the document loaded into the reader's frame.
 *
 * The body has already been sanitized server-side. This wrapper is the second
 * of three independent defences, and it is the one that holds even if the first
 * is wrong:
 *
 * - the frame is sandboxed *without* `allow-scripts`, so no script can execute
 *   whatever survived sanitizing;
 * - the CSP names `script-src 'none'` and `default-src 'none'`, so nothing loads
 *   or runs even if the sandbox attribute were dropped by a future edit;
 * - `allow-same-origin` is present only so that inline `cid:` parts — which are
 *   same-origin authenticated Strife URLs — can load. It is safe here precisely
 *   because `allow-scripts` is absent; the two together would be the classic
 *   sandbox escape, and they must never both appear.
 *
 * `base target="_blank"` sends link clicks to a new tab rather than replacing
 * the frame, and the sanitizer has already stamped `rel="noopener noreferrer"`
 * so the opened page gets no handle on Strife's window.
 */
function frameDocument(
  html: string,
  allowRemote: boolean,
  theme: Theme,
): string {
  const imgSrc = allowRemote ? "'self' data: https: http:" : "'self' data:"
  // The frame is a separate document, so it cannot inherit Strife's custom
  // properties. It is given the resolved theme explicitly rather than left to
  // `prefers-color-scheme`, which would show a light message inside a dark app
  // whenever the user's chosen theme differs from the operating system's.
  const dark = theme === 'dark'
  const text = dark ? '#e6e6e6' : '#1a1a1a'
  const link = dark ? '#7aa2f7' : '#2563eb'
  return `<!doctype html>
<html><head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src ${imgSrc}; style-src 'unsafe-inline'; script-src 'none'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'">
<base target="_blank">
<style>
  html { color-scheme: ${dark ? 'dark' : 'light'}; }
  body {
    margin: 0; padding: 12px;
    font: 14px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif;
    color: ${text}; background: transparent;
    overflow-wrap: anywhere;
  }
  img { max-width: 100%; height: auto; }
  img:not([src]) {
    display: inline-block; min-width: 80px; padding: 4px 8px;
    border: 1px dashed currentColor; border-radius: 4px;
    font-size: 12px; opacity: 0.7;
  }
  table { max-width: 100%; border-collapse: collapse; }
  blockquote {
    margin: 0 0 0 8px; padding-left: 10px;
    border-left: 2px solid rgba(127,127,127,0.4);
  }
  a { color: ${link}; }
</style>
</head><body>${html}</body></html>`
}

/** Tallest the message frame may grow before it scrolls internally. */
const MAX_FRAME_HEIGHT = 4000

/**
 * Prepares the frame once its document exists.
 *
 * Reading the frame's document is possible because the sandbox includes
 * `allow-same-origin`; it stays safe because the sandbox withholds
 * `allow-scripts`, so nothing *inside* the frame can act on that access. This
 * code runs in Strife's own context, not the message's.
 *
 * Two things happen here:
 *
 * - the frame is sized to its content, so a two-line message is not framed by a
 *   screenful of empty box;
 * - every link is given a tooltip naming its real destination. Link text in
 *   archived mail is frequently misleading — "click here", or a display URL
 *   that differs from the target — and the sanitizer has already discarded any
 *   `title` the sender wrote, so this is the only description shown.
 */
function prepareFrame(event: { currentTarget: HTMLIFrameElement }) {
  const frame = event.currentTarget
  const document = frame.contentDocument
  if (!document) return

  for (const link of document.querySelectorAll('a[href]')) {
    const href = link.getAttribute('href')
    if (href) link.setAttribute('title', href)
  }

  const height = document.body?.scrollHeight
  if (height) {
    frame.style.height = `${Math.min(height + 24, MAX_FRAME_HEIGHT)}px`
  }
}

export function MessageReader(props: {
  message: EmailMessage
  /** Requests a reload with remote images permitted. */
  onRevealRemote: () => void
  remoteRevealed: boolean
  onClose: () => void
  /** Receives the close button so the view can restore focus predictably. */
  ref?: (element: HTMLElement) => void
}) {
  const { theme } = useTheme()
  const [showHeaders, setShowHeaders] = createSignal(false)
  const [showPlainText, setShowPlainText] = createSignal(false)
  const [copied, setCopied] = createSignal(false)

  const orderedAddresses = createMemo(() =>
    ROLE_ORDER.flatMap((role) =>
      props.message.addresses.filter((address) => address.role === role),
    ),
  )
  const hasHtml = () => props.message.body_html !== null
  const showingHtml = () => hasHtml() && !showPlainText()

  const copyBody = async () => {
    try {
      await navigator.clipboard.writeText(props.message.body_text)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      setCopied(false)
    }
  }

  return (
    <article class="email-reader" aria-labelledby="email-reader-subject">
      <header class="email-reader__header">
        <div class="email-reader__title">
          <h2 id="email-reader-subject">{subjectOf(props.message.subject)}</h2>
          <button
            type="button"
            class="email-reader__close"
            ref={props.ref}
            onClick={() => props.onClose()}
          >
            Close
          </button>
        </div>

        <dl class="email-reader__addresses">
          <For each={orderedAddresses()}>
            {(address) => (
              <div>
                <dt>{ROLE_LABELS[address.role] ?? address.role}</dt>
                <dd>
                  <Show
                    when={address.display_name}
                    fallback={<span>{address.address}</span>}
                  >
                    {(name) => (
                      <span>
                        {name()} <small>&lt;{address.address}&gt;</small>
                      </span>
                    )}
                  </Show>
                </dd>
              </div>
            )}
          </For>
          <Show when={props.message.sent_at}>
            {(sent) => (
              <div>
                <dt>Sent</dt>
                <dd>
                  <time dateTime={sent()}>{formatDateTime(sent())}</time>
                </dd>
              </div>
            )}
          </Show>
          <Show when={props.message.received_at}>
            {(received) => (
              <div>
                <dt>Received</dt>
                <dd>
                  <time dateTime={received()}>
                    {formatDateTime(received())}
                  </time>
                </dd>
              </div>
            )}
          </Show>
        </dl>

        <Show when={props.message.labels.length > 0}>
          <ul class="email-reader__labels" aria-label="Gmail labels">
            <For each={props.message.labels}>
              {(label) => <li class="email-chip">{label}</li>}
            </For>
          </ul>
        </Show>
      </header>

      <Show when={props.message.warnings.length > 0}>
        <ul class="email-reader__warnings" aria-label="Parser warnings">
          <For each={props.message.warnings}>{(note) => <li>{note}</li>}</For>
        </ul>
      </Show>

      <Show when={props.message.blocked_remote_count > 0}>
        <div class="email-reader__remote" role="note">
          <p>
            This message contains {props.message.blocked_remote_count} remote
            image
            {props.message.blocked_remote_count === 1 ? '' : 's'}
            <Show when={props.message.blocked_hosts.length > 0}>
              {' from '}
              {props.message.blocked_hosts.join(', ')}
            </Show>
            . Loading them tells the sender the message was opened.
          </p>
          <Show
            when={!props.remoteRevealed}
            fallback={
              <p class="email-reader__remote-on">Remote images shown.</p>
            }
          >
            <button type="button" onClick={() => props.onRevealRemote()}>
              Show remote images
            </button>
          </Show>
        </div>
      </Show>

      <div class="email-reader__toolbar folder-toolbar">
        <Show when={hasHtml()}>
          <button
            type="button"
            aria-pressed={showPlainText()}
            onClick={() => setShowPlainText((value) => !value)}
          >
            {showPlainText() ? 'Show formatted' : 'Show plain text'}
          </button>
        </Show>
        <button type="button" onClick={() => void copyBody()}>
          {copied() ? 'Copied' : 'Copy text'}
        </button>
        <a
          class="email-reader__download"
          href={downloadFileUrl(props.message.node_id)}
          download=""
        >
          Download original
        </a>
      </div>

      <Show
        when={showingHtml()}
        fallback={
          // Plain text keeps its own line breaks and stays selectable; it is
          // never routed through the HTML renderer.
          <pre class="email-reader__text">{props.message.body_text}</pre>
        }
      >
        <iframe
          class="email-reader__frame"
          title={`Message body: ${subjectOf(props.message.subject)}`}
          sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox"
          referrerPolicy="no-referrer"
          onLoad={prepareFrame}
          srcdoc={frameDocument(
            props.message.body_html ?? '',
            props.remoteRevealed,
            theme(),
          )}
        />
      </Show>

      <Show when={props.message.attachments.length > 0}>
        <section aria-labelledby="email-reader-attachments">
          <h3 id="email-reader-attachments">
            Attachments ({props.message.attachments.length})
          </h3>
          <ul class="email-reader__attachments">
            <For each={props.message.attachments}>
              {(attachment) => (
                <li>
                  <span class="email-reader__attachment-name">
                    {attachment.filename ?? `part ${attachment.part_path}`}
                  </span>
                  <small>{attachment.media_type}</small>
                  <Show when={attachment.decoded_size !== null}>
                    {/* `data` carries the exact byte count while the text stays
                        localized and readable. */}
                    <data value={String(attachment.decoded_size)}>
                      <small>{formatBytes(attachment.decoded_size)}</small>
                    </data>
                  </Show>
                  <Show when={attachment.is_inline}>
                    <small class="email-chip">inline</small>
                  </Show>
                </li>
              )}
            </For>
          </ul>
          <p class="email-reader__note">
            Attachment downloads arrive with attachment materialization.
          </p>
        </section>
      </Show>

      <section>
        <button
          type="button"
          class="email-reader__headers-toggle"
          aria-expanded={showHeaders()}
          onClick={() => setShowHeaders((value) => !value)}
        >
          {showHeaders() ? 'Hide' : 'Show'} message details
        </button>
        <Show when={showHeaders()}>
          <dl class="email-reader__headers">
            <div>
              <dt>Message ID</dt>
              <dd>{props.message.message_id ?? '(none recorded)'}</dd>
            </div>
            <Show when={props.message.in_reply_to}>
              {(value) => (
                <div>
                  <dt>In reply to</dt>
                  <dd>{value()}</dd>
                </div>
              )}
            </Show>
            <Show when={props.message.references.length > 0}>
              <div>
                <dt>References</dt>
                <dd>{props.message.references.join(', ')}</dd>
              </div>
            </Show>
            <div>
              <dt>Parser version</dt>
              <dd>{props.message.parser_version}</dd>
            </div>
            <div>
              <dt>Extraction status</dt>
              <dd>{props.message.status}</dd>
            </div>
          </dl>
        </Show>
      </section>
    </article>
  )
}
