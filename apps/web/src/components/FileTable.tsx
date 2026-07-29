import { useNavigate } from '@solidjs/router'
import { createMemo, For, Show } from 'solid-js'
import type { FolderItem } from '../api/types'
import './FileTable.css'

interface FileTableProps {
  items: FolderItem[]
  loading: boolean
  error?: string
  onRetry: () => void
}

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: 'short',
  day: 'numeric',
  hour: 'numeric',
  minute: '2-digit',
})

export function FileTable(props: FileTableProps) {
  const navigate = useNavigate()
  const sortedItems = createMemo(() =>
    [...props.items].sort((left, right) => {
      if (left.kind !== right.kind) return left.kind === 'folder' ? -1 : 1
      return left.name.localeCompare(right.name)
    }),
  )

  const openFolder = (item: FolderItem) => {
    if (item.kind === 'folder') navigate(`/folder/${item.id}`)
  }

  return (
    <div class="file-table-wrap">
      <Show when={!props.loading} fallback={<FileTableSkeleton />}>
        <Show
          when={!props.error}
          fallback={
            <div class="file-table-state" role="alert">
              <strong>Couldn’t load this folder</strong>
              <p>{props.error}</p>
              <button type="button" onClick={props.onRetry}>
                Retry
              </button>
            </div>
          }
        >
          <Show
            when={sortedItems().length > 0}
            fallback={
              <div class="file-table-state file-table-state--empty">
                <FolderEmptyIcon />
                <strong>This folder is empty</strong>
              </div>
            }
          >
            <table class="file-table">
              <thead>
                <tr>
                  <th class="file-table__check">
                    <input
                      type="checkbox"
                      aria-label="Select all visible items"
                    />
                  </th>
                  <th class="file-table__icon">
                    <span class="sr-only">Type</span>
                  </th>
                  <th>Name</th>
                  <th>Kind</th>
                  <th>Size</th>
                  <th>Date Modified</th>
                </tr>
              </thead>
              <tbody>
                <For each={sortedItems()}>
                  {(item) => (
                    <tr
                      class="file-table__row"
                      onDblClick={() => openFolder(item)}
                      data-kind={item.kind}
                    >
                      <td class="file-table__check">
                        <input
                          type="checkbox"
                          aria-label={`Select ${item.name}`}
                          onClick={(event) => event.stopPropagation()}
                        />
                      </td>
                      <td class="file-table__icon">
                        <NodeIcon kind={item.kind} />
                      </td>
                      <td class="file-table__name">{item.name}</td>
                      <td class="file-table__kind">{capitalize(item.kind)}</td>
                      <td>{formatFileSize(item.size_bytes, item.kind)}</td>
                      <td>{formatDate(item.updated_at)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </Show>
        </Show>
      </Show>
    </div>
  )
}

export function formatFileSize(
  sizeBytes: number | null,
  kind: FolderItem['kind'],
): string {
  if (kind === 'folder' || sizeBytes === null) return '—'
  if (sizeBytes < 1_000) return `${sizeBytes} B`

  const units = ['KB', 'MB', 'GB', 'TB']
  let size = sizeBytes / 1_000
  let unitIndex = 0
  while (size >= 1_000 && unitIndex < units.length - 1) {
    size /= 1_000
    unitIndex += 1
  }
  const precision = size >= 10 ? 0 : 1
  return `${size.toFixed(precision)} ${units[unitIndex]}`
}

function formatDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : dateFormatter.format(date)
}

function capitalize(value: string): string {
  return `${value.charAt(0).toUpperCase()}${value.slice(1)}`
}

function NodeIcon(props: { kind: FolderItem['kind'] }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <Show
        when={props.kind === 'folder'}
        fallback={<path d="M7 3h7l4 4v14H7zM14 3v5h5" />}
      >
        <path d="M3 7h7l2 2h9v11H3z" />
      </Show>
    </svg>
  )
}

function FolderEmptyIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3 7h7l2 2h9v11H3z" />
    </svg>
  )
}

function FileTableSkeleton() {
  return (
    <div class="file-table-skeleton" aria-label="Loading folder contents">
      <For each={[1, 2, 3, 4, 5]}>
        {() => (
          <div class="file-table-skeleton__row">
            <span />
            <span />
            <span />
            <span />
          </div>
        )}
      </For>
    </div>
  )
}
