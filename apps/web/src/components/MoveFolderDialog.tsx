import {
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  untrack,
} from 'solid-js'
import { ApiClientError, getFolderChildren } from '../api/client'
import type {
  FolderChildrenResponse,
  FolderItem,
  MoveFolderConflict,
} from '../api/types'
import './CreateFolderDialog.css'
import './MoveFolderDialog.css'

const ROOT_FOLDER = {
  id: '00000000-0000-0000-0000-000000000001',
  name: 'All Files',
}

interface TreeFolder {
  id: string
  name: string
}

interface MoveFolderDialogProps {
  items: FolderItem[]
  currentFolderId: string
  onMove: (parentId: string) => Promise<void>
  onClose: () => void
  loadChildren?: (folderId: string) => Promise<FolderChildrenResponse>
}

export function MoveFolderDialog(props: MoveFolderDialogProps) {
  const [destinationId, setDestinationId] = createSignal<string>()
  const [error, setError] = createSignal<string>()
  const [conflicts, setConflicts] = createSignal<MoveFolderConflict[]>([])
  const [submitting, setSubmitting] = createSignal(false)
  const selectedIds = createMemo(
    () => new Set(props.items.map((item) => item.id)),
  )
  const loader = (folderId: string) =>
    props.loadChildren?.(folderId) ?? getFolderChildren(folderId)

  onMount(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !submitting()) props.onClose()
    }
    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => document.removeEventListener('keydown', closeOnEscape))
  })

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    const destination = destinationId()
    if (!destination || destination === props.currentFolderId) return

    setSubmitting(true)
    setError(undefined)
    setConflicts([])
    try {
      await props.onMove(destination)
      props.onClose()
    } catch (cause) {
      if (cause instanceof ApiClientError && cause.conflicts?.length) {
        setConflicts(cause.conflicts)
      } else {
        setError('The selected folders could not be moved. Please try again.')
      }
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div class="dialog-backdrop" role="presentation">
      <section
        class="dialog-card move-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="move-folder-title"
      >
        <header>
          <p>Folder action</p>
          <h2 id="move-folder-title">
            Move{' '}
            {props.items.length === 1
              ? props.items[0].name
              : `${props.items.length} folders`}
          </h2>
        </header>
        <form onSubmit={submit}>
          <p class="move-dialog__instruction">Choose a destination folder.</p>
          <div class="move-tree" role="tree" aria-label="Destination folders">
            <MoveTreeNode
              node={ROOT_FOLDER}
              selectedIds={selectedIds()}
              inheritedDisabled={false}
              destinationId={destinationId()}
              initialExpanded
              loadChildren={loader}
              onSelect={setDestinationId}
            />
          </div>
          <Show when={destinationId() === props.currentFolderId}>
            <p class="move-dialog__hint">These folders are already here.</p>
          </Show>
          <Show when={conflicts().length > 0}>
            <div class="dialog-card__error move-dialog__conflicts" role="alert">
              <strong>Some folders conflict with this destination:</strong>
              <ul>
                <For each={conflicts()}>
                  {(conflict) => (
                    <li>
                      {conflict.name} —{' '}
                      {conflict.reason === 'name_conflict'
                        ? 'a folder with this name already exists'
                        : 'this destination is inside the folder'}
                    </li>
                  )}
                </For>
              </ul>
            </div>
          </Show>
          <Show when={error()}>
            <p class="dialog-card__error" role="alert">
              {error()}
            </p>
          </Show>
          <div class="dialog-card__actions">
            <button
              type="button"
              onClick={() => props.onClose()}
              disabled={submitting()}
            >
              Cancel
            </button>
            <button
              type="submit"
              class="is-primary"
              disabled={
                submitting() ||
                !destinationId() ||
                destinationId() === props.currentFolderId
              }
            >
              {submitting() ? 'Moving…' : 'Move'}
            </button>
          </div>
        </form>
      </section>
    </div>
  )
}

interface MoveTreeNodeProps {
  node: TreeFolder
  selectedIds: Set<string>
  inheritedDisabled: boolean
  destinationId?: string
  initialExpanded?: boolean
  loadChildren: (folderId: string) => Promise<FolderChildrenResponse>
  onSelect: (folderId: string) => void
}

function MoveTreeNode(props: MoveTreeNodeProps) {
  const [expanded, setExpanded] = createSignal(Boolean(props.initialExpanded))
  const [children] = createResource(
    () => (expanded() ? props.node.id : undefined),
    (folderId) => untrack(() => props.loadChildren)(folderId),
  )
  const folders = () =>
    children()?.items.filter((item) => item.kind === 'folder') ?? []
  const disabled = () =>
    props.inheritedDisabled || props.selectedIds.has(props.node.id)

  return (
    <div
      classList={{
        'move-tree__node': true,
        'is-disabled': disabled(),
      }}
      role="treeitem"
      aria-expanded={expanded()}
      aria-selected={props.destinationId === props.node.id}
    >
      <div class="move-tree__row">
        <button
          type="button"
          class="move-tree__expand"
          aria-label={`${expanded() ? 'Collapse' : 'Expand'} ${props.node.name}`}
          onClick={() => setExpanded((current) => !current)}
        >
          {expanded() ? '−' : '+'}
        </button>
        <button
          type="button"
          classList={{
            'move-tree__destination': true,
            'is-selected': props.destinationId === props.node.id,
          }}
          disabled={disabled()}
          onClick={() => props.onSelect(props.node.id)}
        >
          <FolderTreeIcon />
          <span>{props.node.name}</span>
        </button>
      </div>
      <Show when={expanded()}>
        <div class="move-tree__children" role="group">
          <Show when={children.loading}>
            <p class="move-tree__status">Loading…</p>
          </Show>
          <Show when={children.error}>
            <p class="move-tree__status is-error">Couldn’t load folders</p>
          </Show>
          <For each={folders()}>
            {(folder) => (
              <MoveTreeNode
                node={folder}
                selectedIds={props.selectedIds}
                inheritedDisabled={disabled()}
                destinationId={props.destinationId}
                loadChildren={props.loadChildren}
                onSelect={props.onSelect}
              />
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

function FolderTreeIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M3 7h7l2 2h9v11H3z" />
    </svg>
  )
}
