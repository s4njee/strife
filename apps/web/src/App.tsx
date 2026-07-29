import { createResource, Show } from 'solid-js'
import { ApiClientError, getReadiness } from './api/client'
import { ThemePreview } from './components/ThemePreview'
import { useTheme } from './theme/ThemeProvider'
import './App.css'

function App() {
  const [readiness, { refetch }] = createResource(() => getReadiness())
  const { theme, toggleTheme } = useTheme()

  const errorMessage = () => {
    const error = readiness.error
    return error instanceof ApiClientError
      ? error.message
      : 'Strife could not check the API connection.'
  }

  return (
    <main class="app-shell">
      <div class="brand-mark" aria-hidden="true">
        S
      </div>
      <p class="eyebrow">Strife</p>
      <h1>Foundation online.</h1>
      <p class="summary">
        The SolidJS frontend is connected to the Axum readiness API.
      </p>

      <button class="theme-toggle" type="button" onClick={toggleTheme}>
        Use {theme() === 'dark' ? 'light' : 'dark'} theme
      </button>

      <section class="connection-card" aria-live="polite">
        <Show
          when={!readiness.loading}
          fallback={<Status label="Connecting" tone="pending" />}
        >
          <Show
            when={!readiness.error}
            fallback={
              <>
                <Status label="API unreachable" tone="error" />
                <p>{errorMessage()}</p>
              </>
            }
          >
            <Show
              when={readiness()?.ready}
              fallback={
                <>
                  <Status label="API degraded" tone="error" />
                  <p>One or more API dependencies need attention.</p>
                </>
              }
            >
              <Status label="API connected" tone="success" />
              <p>PostgreSQL, storage, and Apache Tika are ready.</p>
            </Show>
          </Show>
        </Show>

        <button
          type="button"
          onClick={() => void refetch()}
          disabled={readiness.loading}
        >
          Check again
        </button>
      </section>

      <Show when={import.meta.env.DEV}>
        <ThemePreview />
      </Show>
    </main>
  )
}

interface StatusProps {
  label: string
  tone: 'pending' | 'success' | 'error'
}

function Status(props: StatusProps) {
  return <strong class={`status status--${props.tone}`}>{props.label}</strong>
}

export default App
