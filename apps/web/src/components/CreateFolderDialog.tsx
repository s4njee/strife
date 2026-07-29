import { createSignal, onCleanup, onMount, Show } from 'solid-js'
import { ApiClientError } from '../api/client'
import './CreateFolderDialog.css'

interface CreateFolderDialogProps {
  onCreate: (name: string) => Promise<void>
  onClose: () => void
}

export function CreateFolderDialog(props: CreateFolderDialogProps) {
  const [name, setName] = createSignal('')
  const [error, setError] = createSignal<string>()
  const [submitting, setSubmitting] = createSignal(false)
  let input: HTMLInputElement | undefined
  let form: HTMLFormElement | undefined

  onMount(() => {
    input?.focus()
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !submitting()) props.onClose()
    }
    document.addEventListener('keydown', closeOnEscape)
    onCleanup(() => document.removeEventListener('keydown', closeOnEscape))
  })

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    if (!name()) return

    setSubmitting(true)
    setError(undefined)
    try {
      await props.onCreate(name())
      props.onClose()
    } catch (cause) {
      if (cause instanceof ApiClientError && cause.status === 409) {
        setError('A folder with this name already exists')
      } else {
        setError('The folder could not be created. Please try again.')
      }
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div class="dialog-backdrop" role="presentation">
      <section
        class="dialog-card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-folder-title"
      >
        <header>
          <p>New item</p>
          <h2 id="create-folder-title">Create folder</h2>
        </header>
        <form ref={form} onSubmit={submit}>
          <label for="create-folder-name">Folder name</label>
          <input
            ref={input}
            id="create-folder-name"
            value={name()}
            required
            autocomplete="off"
            aria-invalid={Boolean(error())}
            aria-describedby={error() ? 'create-folder-error' : undefined}
            onInput={(event) => {
              setName(event.currentTarget.value)
              setError(undefined)
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                form?.requestSubmit()
              }
            }}
          />
          <Show when={error()}>
            <p id="create-folder-error" class="dialog-card__error" role="alert">
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
            <button type="submit" class="is-primary" disabled={submitting()}>
              {submitting() ? 'Creating…' : 'Create'}
            </button>
          </div>
        </form>
      </section>
    </div>
  )
}
