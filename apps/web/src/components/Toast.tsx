import {
  createContext,
  createSignal,
  For,
  onCleanup,
  useContext,
  type ParentProps,
} from 'solid-js'
import './Toast.css'

export type ToastType = 'success' | 'error' | 'info'

export interface ToastMessage {
  id: string
  type: ToastType
  message: string
}

interface ToastContextValue {
  push: (type: ToastType, message: string) => void
}

const ToastContext = createContext<ToastContextValue>()

export function ToastProvider(props: ParentProps) {
  const [toasts, setToasts] = createSignal<ToastMessage[]>([])
  const timers = new Map<string, number>()

  const dismiss = (id: string) => {
    setToasts((current) => current.filter((toast) => toast.id !== id))
    const timer = timers.get(id)
    if (timer) {
      window.clearTimeout(timer)
      timers.delete(id)
    }
  }

  const push = (type: ToastType, message: string) => {
    const id = crypto.randomUUID()
    setToasts((current) => [...current, { id, type, message }].slice(-3))
    const timer = window.setTimeout(() => dismiss(id), 5000)
    timers.set(id, timer)
  }

  onCleanup(() => {
    for (const timer of timers.values()) window.clearTimeout(timer)
  })

  return (
    <ToastContext.Provider value={{ push }}>
      {props.children}
      <div class="toast-stack" aria-live="polite">
        <For each={toasts()}>
          {(toast) => (
            <div class={`toast toast--${toast.type}`} role="status">
              <span>{toast.message}</span>
              <button
                type="button"
                class="toast__dismiss"
                aria-label="Dismiss"
                onClick={() => dismiss(toast.id)}
              >
                ×
              </button>
            </div>
          )}
        </For>
      </div>
    </ToastContext.Provider>
  )
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext)
  if (!ctx) {
    return {
      push: () => {
        /* no-op outside provider */
      },
    }
  }
  return ctx
}

export function toastSuccess(message: string): void {
  // helper for non-hook call sites is intentionally unused; prefer useToast()
  void message
}
