import { createSignal, For, onCleanup, onMount, Show } from 'solid-js'
import type { FolderItem } from '../api/types'
import './ContextMenu.css'

export type ContextMenuAction = 'open' | 'rename' | 'move' | 'trash'

interface ContextMenuProps {
  x: number
  y: number
  items: FolderItem[]
  onAction: (action: ContextMenuAction) => void
  onClose: () => void
}

interface MenuItem {
  action: ContextMenuAction
  label: string
  danger?: boolean
}

export function ContextMenu(props: ContextMenuProps) {
  const [position, setPosition] = createSignal({ x: 0, y: 0 })
  let menu: HTMLDivElement | undefined

  const menuItems = (): MenuItem[] => {
    const allFolders =
      props.items.length > 0 &&
      props.items.every((item) => item.kind === 'folder')
    if (!allFolders) return []

    const common: MenuItem[] = [
      { action: 'move', label: 'Move to…' },
      { action: 'trash', label: 'Move to Trash', danger: true },
    ]
    if (props.items.length === 1) {
      return [
        { action: 'open', label: 'Open' },
        { action: 'rename', label: 'Rename' },
        ...common,
      ]
    }
    return common
  }

  onMount(() => {
    if (menu) {
      const rect = menu.getBoundingClientRect()
      setPosition({
        x: Math.max(8, Math.min(props.x, window.innerWidth - rect.width - 8)),
        y: Math.max(8, Math.min(props.y, window.innerHeight - rect.height - 8)),
      })
    }

    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (menu && !menu.contains(event.target as globalThis.Node))
        props.onClose()
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') props.onClose()
    }
    document.addEventListener('pointerdown', closeOnOutsidePointer)
    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => {
      document.removeEventListener('pointerdown', closeOnOutsidePointer)
      document.removeEventListener('keydown', closeOnEscape)
    })
  })

  return (
    <div
      ref={menu}
      class="context-menu"
      role="menu"
      aria-label="Item actions"
      style={{ left: `${position().x}px`, top: `${position().y}px` }}
    >
      <Show when={props.items.length > 1}>
        <p class="context-menu__selection">
          {props.items.length} items selected
        </p>
      </Show>
      <Show
        when={menuItems().length > 0}
        fallback={
          <p class="context-menu__empty">No folder actions available</p>
        }
      >
        <For each={menuItems()}>
          {(item) => (
            <button
              type="button"
              role="menuitem"
              classList={{ 'is-danger': item.danger }}
              onClick={() => props.onAction(item.action)}
            >
              {item.label}
            </button>
          )}
        </For>
      </Show>
    </div>
  )
}
