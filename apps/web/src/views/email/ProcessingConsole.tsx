import { For, Show } from 'solid-js'
import type { EmailCampaign, EmailEvent, EmailStatus } from '../../api/types'

/**
 * Live extraction activity for a long archive backfill.
 *
 * Bounded and newest-first. A ten-year backfill emits hundreds of thousands of
 * events, so the list is capped by the caller and this component never grows
 * without limit — an operator watching progress needs the last screenful, not
 * the whole history.
 */

/** Rounds an ETA to a unit an operator can act on rather than false precision. */
function formatEta(seconds: number | null): string {
  if (seconds === null) return 'unknown'
  if (seconds < 90) return 'under a minute'
  const minutes = Math.round(seconds / 60)
  if (minutes < 90) return `about ${minutes} minutes`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `about ${hours} hours`
  return `about ${Math.round(hours / 24)} days`
}

function CampaignCard(props: { campaign: EmailCampaign }) {
  const done = () =>
    props.campaign.completed_count +
    props.campaign.failed_count +
    props.campaign.skipped_count
  const percent = () =>
    props.campaign.candidate_count > 0
      ? Math.min(
          100,
          Math.round((done() / props.campaign.candidate_count) * 100),
        )
      : 0

  return (
    <article class="email-campaign">
      <div class="email-campaign__heading">
        <h4>Historical backfill</h4>
        <span class={`email-campaign__state is-${props.campaign.state}`}>
          {props.campaign.state}
        </span>
      </div>
      <p class="email-campaign__progress">
        {done()} of {props.campaign.candidate_count} ({percent()}%) ·{' '}
        {props.campaign.failed_count} failed · {props.campaign.skipped_count}{' '}
        skipped
      </p>
      <p class="email-campaign__limits">
        Batch {props.campaign.batch_size} · max {props.campaign.max_queued}{' '}
        queued · {props.campaign.max_running} running ·{' '}
        {props.campaign.resource_class} · foreground priority every{' '}
        {props.campaign.foreground_fairness} claims
      </p>
      {/* The durable cursor is what makes an interrupted backfill resumable,
          so an operator deciding whether to resume needs to see it. */}
      <Show when={props.campaign.cursor_created_at}>
        {(cursor) => (
          <p class="email-campaign__limits">
            Resumes from{' '}
            <time dateTime={cursor()}>
              {new Date(cursor()).toLocaleString()}
            </time>
          </p>
        )}
      </Show>
      <Show when={props.campaign.last_error}>
        {(error) => <p class="email-console__warning">{error()}</p>}
      </Show>
    </article>
  )
}

export function ProcessingConsole(props: {
  status: EmailStatus | undefined
  entries: EmailEvent[]
  connection: 'connecting' | 'live' | 'reconnecting'
  busy: boolean
  onRetryOne: (nodeId: string) => void
  onRetryFailed: () => void
}) {
  return (
    <section class="email-console" aria-labelledby="email-console-title">
      <div class="email-console__heading">
        <h3 id="email-console-title">Processing</h3>
        <span class={`is-${props.connection}`}>
          <i aria-hidden="true" />
          {props.connection === 'live'
            ? 'Live'
            : props.connection === 'connecting'
              ? 'Connecting'
              : 'Reconnecting'}
        </span>
      </div>

      <Show when={props.status}>
        {(status) => (
          <>
            <dl class="email-console__counts">
              <Count
                label="Foreground"
                value={
                  status().counts.foreground_pending +
                  status().counts.foreground_running
                }
              />
              <Count
                label="Backfill"
                value={
                  status().counts.backfill_pending +
                  status().counts.backfill_running
                }
              />
              <Count label="Indexed" value={status().counts.indexed} />
              <Count label="Failed" value={status().counts.failed} error />
              <Count
                label="Attachments left"
                value={status().counts.attachments_pending}
              />
              <Count
                label="Attachments failed"
                value={status().counts.attachments_failed}
                error
              />
            </dl>
            <p class="email-console__throughput">
              {status().completed_per_hour} messages/hour · remaining{' '}
              {status().counts.remaining} · finishes{' '}
              {formatEta(status().eta_seconds)} · parser{' '}
              {status().parser_version} · attachments{' '}
              {status().attachment_extractor_version}
            </p>
            <For each={status().campaigns}>
              {(campaign) => <CampaignCard campaign={campaign} />}
            </For>
            <Show when={status().counts.failed > 0}>
              <div class="folder-toolbar">
                <button
                  type="button"
                  disabled={props.busy}
                  onClick={() => props.onRetryFailed()}
                >
                  Retry failed messages
                </button>
              </div>
            </Show>
          </>
        )}
      </Show>

      <div class="email-console__output" role="log" aria-live="polite">
        <Show
          when={props.entries.length > 0}
          fallback={
            <p class="email-console__empty">Waiting for extraction activity…</p>
          }
        >
          <For each={props.entries}>
            {(entry) => (
              <div class={`email-console__entry is-${entry.state}`}>
                <p>
                  <time dateTime={entry.created_at}>
                    {new Date(entry.created_at).toLocaleTimeString()}
                  </time>
                  <span>{entry.state}</span>
                  {/* Backfill activity is labelled so a busy console during a
                      historical campaign is not mistaken for new mail. */}
                  <Show when={entry.origin !== 'foreground'}>
                    <small class="email-chip">{entry.origin}</small>
                  </Show>
                  <strong>{entry.subject ?? entry.name}</strong>
                  <Show when={entry.attachment_count}>
                    <small>{entry.attachment_count} attachments</small>
                  </Show>
                  <Show when={entry.duration_ms !== null}>
                    <small>{entry.duration_ms} ms</small>
                  </Show>
                  <Show when={entry.state === 'failed' && entry.node_id}>
                    <button
                      type="button"
                      disabled={props.busy}
                      onClick={() => props.onRetryOne(entry.node_id!)}
                    >
                      Retry
                    </button>
                  </Show>
                </p>
                <Show when={entry.warning}>
                  <p class="email-console__warning">{entry.warning}</p>
                </Show>
              </div>
            )}
          </For>
        </Show>
      </div>
    </section>
  )
}

function Count(props: { label: string; value: number; error?: boolean }) {
  return (
    <div classList={{ 'is-error': props.error && props.value > 0 }}>
      <dt>{props.label}</dt>
      <dd>{props.value}</dd>
    </div>
  )
}
