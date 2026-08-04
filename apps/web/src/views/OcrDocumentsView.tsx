import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'
import { getOcrTree } from '../api/client'
import type { FileDetails, FolderItem, OcrTreeNode } from '../api/types'
import { FileDetailsPanel } from '../components/FileDetailsPanel'
import { FileIcon } from '../components/FileIcon'
import { OcrTabs } from '../components/OcrTabs'
import { demoImage, PreviewModal } from '../components/PreviewModal'
import './OcrDocumentsView.css'

const ROOT_FOLDER_ID = '00000000-0000-0000-0000-000000000001'
const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

/// The tree carries OCR state; the preview and details panel both speak the
/// ordinary folder item shape, so translate once here rather than in each.
function toFolderItem(node: OcrTreeNode): FolderItem {
  return {
    id: node.id,
    name: node.name,
    kind: 'file',
    size_bytes: null,
    created_at: node.updated_at,
    updated_at: node.updated_at,
  }
}

export function OcrDocumentsView() {
  const [selected, setSelected] = createSignal<OcrTreeNode>()
  const [previewing, setPreviewing] = createSignal<FolderItem>()
  // Siblings of the previewed file, so the modal's next/previous controls walk
  // the folder the user opened rather than nothing at all.
  const [previewSiblings, setPreviewSiblings] = createSignal<FolderItem[]>([])
  const selectedItem = (): FolderItem | undefined => {
    const file = selected()
    return file ? toFolderItem(file) : undefined
  }
  const openPreview = (node: OcrTreeNode, siblings: FolderItem[]) => {
    setPreviewSiblings(siblings)
    setPreviewing(toFolderItem(node))
  }

  return (
    <section class="workspace-view ocr-documents-view">
      <header>
        <p class="workspace-view__eyebrow">Processing</p>
        <h1>OCR</h1>
        <p class="workspace-view__description">
          Browse OCR documents in their library folders and inspect extraction
          status.
        </p>
      </header>
      <OcrTabs />
      <div class="workspace-view__split ocr-documents-view__split">
        <div class="workspace-view__split-main ocr-tree-panel">
          <div class="ocr-tree-panel__heading">
            <div>
              <h2>OCR documents</h2>
              <p>
                Folders contain only files that have OCR state or queued work.
              </p>
            </div>
            <StatusLegend />
          </div>
          <TreeBranch
            parentId={ROOT_FOLDER_ID}
            depth={0}
            onSelect={setSelected}
            onPreview={openPreview}
            selectedId={() => selected()?.id}
            root
          />
        </div>
        <Show when={selectedItem()}>
          {(item) => (
            <FileDetailsPanel
              item={item()}
              staticDetails={staticPreview ? previewDetails(item()) : undefined}
              staticStreams={staticPreview ? [] : undefined}
              onClose={() => setSelected()}
            />
          )}
        </Show>
      </div>
      <Show when={previewing()}>
        {(item) => (
          <PreviewModal
            item={item()}
            files={previewSiblings()}
            staticDetails={staticPreview ? previewDetails(item()) : undefined}
            staticSource={staticPreview ? demoImage : undefined}
            onNavigate={setPreviewing}
            onClose={() => setPreviewing(undefined)}
          />
        )}
      </Show>
    </section>
  )
}

function TreeBranch(props: {
  parentId: string
  depth: number
  onSelect: (node: OcrTreeNode) => void
  onPreview: (node: OcrTreeNode, siblings: FolderItem[]) => void
  selectedId: () => string | undefined
  root?: boolean
}) {
  const [items, setItems] = createSignal<OcrTreeNode[]>([])
  const [nextOffset, setNextOffset] = createSignal<number | null>(0)
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string>()
  const controller = new AbortController()
  onCleanup(() => controller.abort())

  const load = async () => {
    const offset = nextOffset()
    if (offset === null || loading()) return
    setLoading(true)
    setError()
    try {
      const response = staticPreview
        ? previewTree(props.parentId, offset)
        : await getOcrTree(props.parentId, offset, controller.signal)
      setItems((current) => [...current, ...response.items])
      setNextOffset(response.next_offset)
    } catch (cause) {
      if (!controller.signal.aborted) {
        setError(
          cause instanceof Error ? cause.message : 'Documents failed to load.',
        )
      }
    } finally {
      setLoading(false)
    }
  }

  createEffect(() => {
    if (nextOffset() === 0 && !loading()) void load()
  })

  return (
    <ul
      class="ocr-tree"
      classList={{ 'ocr-tree--root': props.root }}
      role={props.root ? 'tree' : 'group'}
      aria-label={props.root ? 'OCR document folders' : undefined}
    >
      <For each={items()}>
        {(node) => (
          <TreeNode
            node={node}
            depth={props.depth}
            onSelect={props.onSelect}
            onPreview={props.onPreview}
            siblings={() =>
              items()
                .filter((candidate) => candidate.kind === 'file')
                .map(toFolderItem)
            }
            selectedId={props.selectedId}
          />
        )}
      </For>
      <Show when={loading()}>
        <li class="ocr-tree__message" role="status">
          Loading documents…
        </li>
      </Show>
      <Show when={error()}>
        {(message) => (
          <li class="ocr-tree__message is-error">
            {message()}{' '}
            <button type="button" onClick={() => void load()}>
              Retry
            </button>
          </li>
        )}
      </Show>
      <Show when={!loading() && !error() && items().length === 0}>
        <li class="ocr-tree__message">No OCR documents in this folder.</li>
      </Show>
      <Show when={!loading() && nextOffset() !== null && nextOffset() !== 0}>
        <li class="ocr-tree__more">
          <button type="button" onClick={() => void load()}>
            Load more
          </button>
        </li>
      </Show>
    </ul>
  )
}

function TreeNode(props: {
  node: OcrTreeNode
  depth: number
  onSelect: (node: OcrTreeNode) => void
  onPreview: (node: OcrTreeNode, siblings: FolderItem[]) => void
  /// Files alongside this row, so the preview can step through the folder.
  siblings: () => FolderItem[]
  selectedId: () => string | undefined
}) {
  const [open, setOpen] = createSignal(false)
  const folder = () => props.node.kind === 'folder'
  const activate = () => {
    if (folder()) {
      setOpen((value) => !value)
    } else {
      props.onPreview(props.node, props.siblings())
    }
  }
  return (
    <li
      class="ocr-tree__item"
      role="treeitem"
      aria-expanded={folder() ? open() : undefined}
      aria-selected={
        !folder() ? props.selectedId() === props.node.id : undefined
      }
    >
      <button
        type="button"
        class="ocr-tree__row"
        classList={{
          'is-folder': folder(),
          'is-selected': props.selectedId() === props.node.id,
        }}
        style={{ '--ocr-tree-depth': String(props.depth) }}
        title={folder() ? undefined : 'Double-click or press Enter to open'}
        onClick={() =>
          folder() ? setOpen((value) => !value) : props.onSelect(props.node)
        }
        onDblClick={activate}
        onKeyDown={(event) => {
          // A button fires click on both Enter and Space, so the two cannot be
          // told apart without intercepting the key. Enter opens the file —
          // matching the file table — while Space keeps its selecting default.
          if (event.key === 'Enter') {
            event.preventDefault()
            activate()
          }
        }}
      >
        <span class="ocr-tree__toggle" aria-hidden="true">
          {folder() ? (open() ? '⌄' : '›') : ''}
        </span>
        <FileIcon name={props.node.name} kind={props.node.kind} />
        <span class="ocr-tree__name">{props.node.name}</span>
        <Show when={folder()} fallback={<FileStatus node={props.node} />}>
          <FolderSummary node={props.node} />
        </Show>
      </button>
      <Show when={folder() && open()}>
        <TreeBranch
          parentId={props.node.id}
          depth={props.depth + 1}
          onSelect={props.onSelect}
          onPreview={props.onPreview}
          selectedId={props.selectedId}
        />
      </Show>
    </li>
  )
}

function FolderSummary(props: { node: OcrTreeNode }) {
  return (
    <span class="ocr-tree__summary">
      <span>{props.node.total_files.toLocaleString()} files</span>
      <Show when={props.node.running > 0}>
        <StatusPill status="running" count={props.node.running} />
      </Show>
      <Show when={props.node.failed > 0}>
        <StatusPill status="failed" count={props.node.failed} />
      </Show>
      <Show when={props.node.completed > 0}>
        <StatusPill status="completed" count={props.node.completed} />
      </Show>
    </span>
  )
}

function FileStatus(props: { node: OcrTreeNode }) {
  const status = () => props.node.status ?? 'pending'
  return (
    <span class="ocr-tree__file-meta">
      <span>
        {props.node.page_count === null
          ? 'Pages —'
          : `${props.node.page_count.toLocaleString()} pages`}
      </span>
      <span>
        {props.node.mean_confidence === null
          ? 'Confidence —'
          : `${props.node.mean_confidence.toFixed(1)}%`}
      </span>
      <StatusPill status={status()} />
    </span>
  )
}

function StatusPill(props: { status: string; count?: number }) {
  return (
    <span class={`ocr-status-pill is-${props.status}`}>
      {props.count !== undefined ? `${props.count} ` : ''}
      {props.status}
    </span>
  )
}

function StatusLegend() {
  return (
    <div class="ocr-tree__legend" aria-label="OCR status legend">
      <StatusPill status="running" />
      <StatusPill status="completed" />
      <StatusPill status="failed" />
      <StatusPill status="skipped" />
    </div>
  )
}

const previewUpdatedAt = '2026-08-04T09:00:00Z'
const previewNodes: Record<string, OcrTreeNode[]> = {
  [ROOT_FOLDER_ID]: [
    {
      id: '00000000-0000-0000-0000-000000000101',
      parent_id: ROOT_FOLDER_ID,
      name: 'Books',
      kind: 'folder',
      status: null,
      source: null,
      page_count: null,
      mean_confidence: null,
      char_count: null,
      updated_at: previewUpdatedAt,
      total_files: 1248,
      pending: 18,
      running: 2,
      completed: 1214,
      failed: 14,
      skipped: 0,
      unsupported: 0,
    },
    {
      id: '00000000-0000-0000-0000-000000000102',
      parent_id: ROOT_FOLDER_ID,
      name: 'Receipts-2025.pdf',
      kind: 'file',
      status: 'completed',
      source: 'ocr',
      page_count: 42,
      mean_confidence: 96.4,
      char_count: 18204,
      updated_at: previewUpdatedAt,
      total_files: 1,
      pending: 0,
      running: 0,
      completed: 1,
      failed: 0,
      skipped: 0,
      unsupported: 0,
    },
  ],
  '00000000-0000-0000-0000-000000000101': [
    {
      id: '00000000-0000-0000-0000-000000000103',
      parent_id: '00000000-0000-0000-0000-000000000101',
      name: 'Modern Physical Organic Chemistry.pdf',
      kind: 'file',
      status: 'completed',
      source: 'ocr',
      page_count: 1184,
      mean_confidence: 91.8,
      char_count: 2840310,
      updated_at: previewUpdatedAt,
      total_files: 1,
      pending: 0,
      running: 0,
      completed: 1,
      failed: 0,
      skipped: 0,
      unsupported: 0,
    },
  ],
}

function previewTree(parentId: string, offset: number) {
  const items = offset === 0 ? (previewNodes[parentId] ?? []) : []
  return { items, next_offset: null }
}

function previewDetails(item: FolderItem): FileDetails {
  return {
    id: item.id,
    parent_id: null,
    name: item.name,
    byte_size: 84_000_000,
    checksum_sha256: null,
    created_at: item.created_at,
    updated_at: item.updated_at,
    detected_mime: 'application/pdf',
    media_kind: 'document',
    duration_ms: null,
    width: null,
    height: null,
    capture_time: null,
    page_count: selectedPageCount(item.id),
    orientation: null,
    has_gps: null,
    gps_latitude: null,
    gps_longitude: null,
    camera_make: null,
    camera_model: null,
    document_title: null,
    document_author: null,
    document_created_at: null,
    document_modified_at: null,
    processing_status: 'ready',
  }
}

function selectedPageCount(id: string): number | null {
  for (const items of Object.values(previewNodes)) {
    const item = items.find((candidate) => candidate.id === id)
    if (item) return item.page_count
  }
  return null
}
