import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js'
import { searchDocumentText } from '../api/client'
import type { TextSearchResponse } from '../api/types'
import { FileIcon } from './FileIcon'
import './ContentSearch.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function ContentSearch() {
  const [query, setQuery] = createSignal('')
  const [result, setResult] = createSignal<TextSearchResponse>()
  const [loading, setLoading] = createSignal(false)
  const [error, setError] = createSignal<string>()

  createEffect(() => {
    const value = query().trim()
    if (value.length < 2) {
      setResult(undefined)
      setError(undefined)
      return
    }
    const controller = new AbortController()
    const timer = window.setTimeout(async () => {
      setLoading(true)
      setError(undefined)
      try {
        setResult(
          staticPreview
            ? sampleSearch
            : await searchDocumentText(value, controller.signal),
        )
      } catch (cause) {
        if (!controller.signal.aborted) {
          setError(cause instanceof Error ? cause.message : 'Search failed.')
        }
      } finally {
        if (!controller.signal.aborted) setLoading(false)
      }
    }, 300)
    onCleanup(() => {
      window.clearTimeout(timer)
      controller.abort()
    })
  })

  const message = () => {
    if (loading()) return 'Searching extracted text…'
    if (error()) return error()
    const current = result()
    if (!current) return undefined
    if (current.items.length > 0)
      return `${current.items.length} content matches`
    return current.indexed_documents === 0
      ? 'No text has been extracted yet.'
      : 'No text matches this query.'
  }

  return (
    <div class="content-search">
      <label for="content-search-input">Search file contents</label>
      <input
        id="content-search-input"
        type="search"
        role="combobox"
        aria-expanded={(result()?.items.length ?? 0) > 0}
        aria-controls="content-search-results"
        autocomplete="off"
        placeholder="Search extracted text…"
        value={query()}
        onInput={(event) => setQuery(event.currentTarget.value)}
      />
      <Show when={message()}>
        {(value) => (
          <p class="content-search__status" aria-live="polite">
            {value()}
          </p>
        )}
      </Show>
      <Show when={(result()?.items.length ?? 0) > 0}>
        <div
          id="content-search-results"
          class="content-search__results"
          role="listbox"
        >
          <For each={result()?.items}>
            {(match) => (
              <button
                type="button"
                role="option"
                onClick={() =>
                  window.open(
                    `/api/files/${match.node_id}/preview-native#page=${match.page_number}`,
                    '_blank',
                  )
                }
              >
                <FileIcon name={match.name} kind="file" />
                <span>
                  <strong>{match.name}</strong>
                  <small>Page {match.page_number}</small>
                  <span class="content-search__snippet">
                    <Snippet value={match.snippet} />
                  </span>
                </span>
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

function Snippet(props: { value: string }) {
  const parts = () => props.value.split(/(<<strife>>|<<\/strife>>)/)
  let highlighted = false
  return (
    <For each={parts()}>
      {(part) => {
        if (part === '<<strife>>') {
          highlighted = true
          return null
        }
        if (part === '<</strife>>') {
          highlighted = false
          return null
        }
        return highlighted ? <mark>{part}</mark> : part
      }}
    </For>
  )
}

const sampleSearch: TextSearchResponse = {
  indexed_documents: 842,
  next_cursor: null,
  items: [
    {
      node_id: 'preview-search-1',
      name: 'Field Notes.pdf',
      page_number: 12,
      snippet:
        'The <<strife>>survey marker<</strife>> was recorded beside the north fence.',
      score: 0.82,
    },
    {
      node_id: 'preview-search-2',
      name: 'Receipts 2025.tiff',
      page_number: 3,
      snippet:
        'Replacement <<strife>>survey marker<</strife>> and mounting hardware.',
      score: 0.61,
    },
  ],
}
