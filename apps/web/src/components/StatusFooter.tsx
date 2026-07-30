import { createResource, onCleanup, Show } from 'solid-js'
import { getActiveJobCount } from '../api/client'
import './StatusFooter.css'

interface StatusFooterProps {
  itemCount: number
  selectedCount: number
}

export function StatusFooter(props: StatusFooterProps) {
  const [processing, { refetch }] = createResource(() => getActiveJobCount())

  const interval = window.setInterval(() => void refetch(), 15_000)
  onCleanup(() => window.clearInterval(interval))

  return (
    <footer class="status-footer">
      <span>
        {props.itemCount} {props.itemCount === 1 ? 'item' : 'items'}
      </span>
      <Show when={props.selectedCount > 0}>
        <span>{props.selectedCount} selected</span>
      </Show>
      <Show when={(processing() ?? 0) > 0}>
        <span>
          {processing()} {processing() === 1 ? 'file' : 'files'} processing
        </span>
      </Show>
    </footer>
  )
}
