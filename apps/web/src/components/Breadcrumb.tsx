import { A } from '@solidjs/router'
import { createSignal, For, Show } from 'solid-js'
import type { FolderAncestor } from '../api/types'
import './Breadcrumb.css'

interface BreadcrumbProps {
  items: FolderAncestor[]
}

type BreadcrumbPart = FolderAncestor | 'ellipsis'

export function Breadcrumb(props: BreadcrumbProps) {
  const [expanded, setExpanded] = createSignal(false)

  const visibleItems = (): BreadcrumbPart[] => {
    if (expanded() || props.items.length <= 5) return props.items
    return [props.items[0], 'ellipsis', ...props.items.slice(-3)]
  }

  return (
    <nav class="breadcrumb" aria-label="Folder path">
      <ol>
        <For each={visibleItems()}>
          {(item, index) => (
            <li>
              <Show
                when={item !== 'ellipsis'}
                fallback={
                  <button
                    class="breadcrumb__ellipsis"
                    type="button"
                    title="Show full folder path"
                    aria-label="Show full folder path"
                    onClick={() => setExpanded(true)}
                  >
                    …
                  </button>
                }
              >
                <Show
                  when={index() < visibleItems().length - 1}
                  fallback={
                    <span aria-current="page">{displayName(item)}</span>
                  }
                >
                  <A href={folderHref(item)}>{displayName(item)}</A>
                </Show>
              </Show>
            </li>
          )}
        </For>
      </ol>
    </nav>
  )
}

function displayName(item: BreadcrumbPart): string {
  if (item === 'ellipsis') return '…'
  return item.name === 'root' ? 'All Files' : item.name
}

function folderHref(item: BreadcrumbPart): string {
  if (item === 'ellipsis' || item.name === 'root') return '/'
  return `/folder/${item.id}`
}
