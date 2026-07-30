import { useNavigate } from '@solidjs/router'
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from 'solid-js'
import type { FolderItem } from '../api/types'
import { ContextMenu, type ContextMenuAction } from './ContextMenu'
import './FileTable.css'

export type FileTableSortColumn =
  | 'name'
  | 'kind'
  | 'size'
  | 'updated_at'
  | 'created_at'

interface FileTableProps {
  items: FolderItem[]
  loading: boolean
  error?: string
  onRetry: () => void
  sortColumn?: FileTableSortColumn
  sortOrder?: 'asc' | 'desc'
  onSort?: (column: FileTableSortColumn) => void
  onRename?: (item: FolderItem) => void
  onMove?: (items: FolderItem[]) => void
  onTrash?: (items: FolderItem[]) => void
  onDetails?: (item: FolderItem) => void
  onPreview?: (item: FolderItem) => void
  onToggleFavorite?: (item: FolderItem) => void
  onFavoriteSelected?: (items: FolderItem[], favorite: boolean) => void
  onDownload?: (item: FolderItem) => void
  onSelectionChange?: (items: FolderItem[]) => void
  /** When true, shows trash-oriented actions instead of normal toolbar. */
  trashMode?: boolean
  onRestore?: (items: FolderItem[]) => void
  onPermanentDelete?: (items: FolderItem[]) => void
}

interface ContextMenuState {
  x: number
  y: number
  items: FolderItem[]
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
  const [selectedIds, setSelectedIds] = createSignal<Set<string>>(new Set())
  const [anchorId, setAnchorId] = createSignal<string>()
  const [contextMenu, setContextMenu] = createSignal<ContextMenuState>()
  let selectAllCheckbox: HTMLInputElement | undefined
  const sortedItems = createMemo(() => {
    // Server already sorts; keep a stable client order for static preview.
    if (props.sortColumn) return props.items
    return [...props.items].sort((left, right) => {
      if (left.kind !== right.kind) return left.kind === 'folder' ? -1 : 1
      return left.name.localeCompare(right.name)
    })
  })

  const sortIndicator = (column: FileTableSortColumn) => {
    if (props.sortColumn !== column) return ''
    return props.sortOrder === 'desc' ? ' ↓' : ' ↑'
  }

  const headerButton = (column: FileTableSortColumn, label: string) => (
    <button
      type="button"
      class="file-table__sort"
      onClick={() => props.onSort?.(column)}
    >
      {label}
      {sortIndicator(column)}
    </button>
  )

  const clearSelection = () => {
    setSelectedIds(new Set<string>())
    setAnchorId(undefined)
  }

  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Delete' && event.key !== 'Backspace') return
      const target = event.target as HTMLElement | null
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)
      ) {
        return
      }
      const selected = selectedItems()
      if (selected.length === 0) return
      event.preventDefault()
      if (props.trashMode) {
        if (
          window.confirm(
            `Permanently delete ${selected.length} item${selected.length === 1 ? '' : 's'}? This cannot be undone.`,
          )
        ) {
          props.onPermanentDelete?.(selected)
          clearSelection()
        }
        return
      }
      if (
        window.confirm(
          `Move ${selected.length} item${selected.length === 1 ? '' : 's'} to trash?`,
        )
      ) {
        props.onTrash?.(selected)
        clearSelection()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    onCleanup(() => window.removeEventListener('keydown', onKeyDown))
  })
  const selectedCount = () => selectedIds().size
  const selectedItems = () =>
    sortedItems().filter((item) => selectedIds().has(item.id))
  const allSelected = () =>
    sortedItems().length > 0 && selectedCount() === sortedItems().length

  createEffect(() => {
    const visibleIds = new Set(sortedItems().map((item) => item.id))
    setSelectedIds(
      (current) => new Set([...current].filter((id) => visibleIds.has(id))),
    )
  })

  createEffect(() => props.onSelectionChange?.(selectedItems()))

  createEffect(() => {
    if (selectAllCheckbox) {
      selectAllCheckbox.indeterminate = selectedCount() > 0 && !allSelected()
    }
  })

  const openFolder = (item: FolderItem) => {
    if (item.kind === 'folder') navigate(`/folder/${item.id}`)
  }

  const selectRow = (item: FolderItem, event: MouseEvent) => {
    if (event.shiftKey && anchorId()) {
      selectRange(item.id)
      return
    }

    if (event.metaKey || event.ctrlKey) {
      toggleItem(item.id)
      setAnchorId(item.id)
      return
    }

    setSelectedIds(new Set([item.id]))
    setAnchorId(item.id)
  }

  const toggleItem = (itemId: string) => {
    setSelectedIds((current) => {
      const next = new Set(current)
      if (next.has(itemId)) next.delete(itemId)
      else next.add(itemId)
      return next
    })
  }

  const selectRange = (itemId: string) => {
    const items = sortedItems()
    const anchorIndex = items.findIndex((item) => item.id === anchorId())
    const itemIndex = items.findIndex((item) => item.id === itemId)
    if (anchorIndex < 0 || itemIndex < 0) {
      setSelectedIds(new Set([itemId]))
      setAnchorId(itemId)
      return
    }

    const start = Math.min(anchorIndex, itemIndex)
    const end = Math.max(anchorIndex, itemIndex)
    setSelectedIds(new Set(items.slice(start, end + 1).map((item) => item.id)))
  }

  const toggleAll = () => {
    if (allSelected()) {
      setSelectedIds(new Set<string>())
      setAnchorId(undefined)
    } else {
      const items = sortedItems()
      setSelectedIds(new Set(items.map((item) => item.id)))
      setAnchorId(items[0]?.id)
    }
  }

  const openContextMenu = (item: FolderItem, event: MouseEvent) => {
    event.preventDefault()
    let itemIds = selectedIds()
    if (!itemIds.has(item.id)) {
      itemIds = new Set([item.id])
      setSelectedIds(itemIds)
      setAnchorId(item.id)
    }

    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: sortedItems().filter((candidate) => itemIds.has(candidate.id)),
    })
  }

  const runContextAction = (action: ContextMenuAction) => {
    const state = contextMenu()
    if (!state) return

    if (action === 'open' && state.items.length === 1) {
      openFolder(state.items[0])
    } else if (action === 'rename' && state.items.length === 1) {
      props.onRename?.(state.items[0])
    } else if (action === 'move') {
      props.onMove?.(state.items)
    } else if (action === 'trash') {
      props.onTrash?.(state.items)
    }
    setContextMenu(undefined)
  }

  return (
    <div class="file-table-wrap">
      <Show when={selectedCount() > 0}>
        <div class="file-table-selection" role="status">
          <span>
            {selectedCount()} {selectedCount() === 1 ? 'item' : 'items'}{' '}
            selected
          </span>
          <div class="file-table-selection__actions">
            <Show when={props.trashMode}>
              <button
                type="button"
                onClick={() => {
                  props.onRestore?.(selectedItems())
                  clearSelection()
                }}
              >
                Restore
              </button>
              <button
                type="button"
                class="is-danger"
                onClick={() => {
                  if (
                    window.confirm(
                      `This will permanently delete ${selectedCount()} item${selectedCount() === 1 ? '' : 's'}. This action cannot be undone.`,
                    )
                  ) {
                    props.onPermanentDelete?.(selectedItems())
                    clearSelection()
                  }
                }}
              >
                Delete Permanently
              </button>
            </Show>
            <Show when={!props.trashMode}>
              <button
                type="button"
                onClick={() => props.onMove?.(selectedItems())}
              >
                Move to…
              </button>
              <button
                type="button"
                onClick={() => {
                  props.onTrash?.(selectedItems())
                  clearSelection()
                }}
              >
                Move to Trash
              </button>
              <button
                type="button"
                onClick={() => {
                  const selected = selectedItems()
                  const allFavorited = selected.every((item) => item.is_favorite)
                  props.onFavoriteSelected?.(selected, !allFavorited)
                }}
              >
                {selectedItems().every((item) => item.is_favorite)
                  ? 'Unfavorite'
                  : 'Favorite'}
              </button>
              <Show
                when={
                  selectedItems().length === 1 &&
                  selectedItems()[0]?.kind === 'file'
                }
              >
                <button
                  type="button"
                  onClick={() => props.onDownload?.(selectedItems()[0])}
                >
                  Download
                </button>
                <button
                  type="button"
                  onClick={() => props.onPreview?.(selectedItems()[0])}
                >
                  Preview
                </button>
                <button
                  type="button"
                  onClick={() => props.onDetails?.(selectedItems()[0])}
                >
                  Details
                </button>
              </Show>
            </Show>
          </div>
        </div>
      </Show>
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
                      ref={selectAllCheckbox}
                      type="checkbox"
                      aria-label="Select all visible items"
                      checked={allSelected()}
                      onClick={(event) => {
                        event.stopPropagation()
                        toggleAll()
                      }}
                    />
                  </th>
                  <th class="file-table__icon">
                    <span class="sr-only">Type</span>
                  </th>
                  <th class="file-table__favorite">
                    <span class="sr-only">Favorite</span>
                  </th>
                  <th>{headerButton('name', 'Name')}</th>
                  <th>{headerButton('kind', 'Kind')}</th>
                  <th>{headerButton('size', 'Size')}</th>
                  <th>{headerButton('updated_at', 'Date Modified')}</th>
                </tr>
              </thead>
              <tbody>
                <For each={sortedItems()}>
                  {(item) => (
                    <tr
                      classList={{
                        'file-table__row': true,
                        'is-selected': selectedIds().has(item.id),
                      }}
                      aria-selected={selectedIds().has(item.id)}
                      onClick={(event) => selectRow(item, event)}
                      onContextMenu={(event) => openContextMenu(item, event)}
                      onDblClick={() =>
                        item.kind === 'file'
                          ? props.onPreview?.(item)
                          : openFolder(item)
                      }
                      data-kind={item.kind}
                    >
                      <td class="file-table__check">
                        <input
                          type="checkbox"
                          aria-label={`Select ${item.name}`}
                          checked={selectedIds().has(item.id)}
                          onClick={(event) => {
                            event.stopPropagation()
                            toggleItem(item.id)
                            setAnchorId(item.id)
                          }}
                        />
                      </td>
                      <td class="file-table__icon">
                        <NodeIcon kind={item.kind} />
                      </td>
                      <td class="file-table__favorite">
                        <button
                          type="button"
                          classList={{
                            'file-table__star': true,
                            'is-active': Boolean(item.is_favorite),
                          }}
                          aria-label={
                            item.is_favorite
                              ? `Remove ${item.name} from favorites`
                              : `Add ${item.name} to favorites`
                          }
                          aria-pressed={Boolean(item.is_favorite)}
                          onClick={(event) => {
                            event.stopPropagation()
                            props.onToggleFavorite?.(item)
                          }}
                        >
                          {item.is_favorite ? '★' : '☆'}
                        </button>
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
      <Show when={contextMenu()}>
        {(state) => (
          <ContextMenu
            x={state().x}
            y={state().y}
            items={state().items}
            onAction={runContextAction}
            onClose={() => setContextMenu(undefined)}
          />
        )}
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
