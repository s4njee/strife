import {
  createContext,
  createEffect,
  createSignal,
  onMount,
  useContext,
  type Accessor,
  type ParentProps,
} from 'solid-js'

export type Theme = 'light' | 'dark'

interface ThemeContextValue {
  theme: Accessor<Theme>
  toggleTheme: () => void
}

const STORAGE_KEY = 'strife-theme'
const ThemeContext = createContext<ThemeContextValue>()

export function ThemeProvider(props: ParentProps) {
  const [theme, setTheme] = createSignal<Theme>('dark')

  onMount(() => setTheme(readInitialTheme()))

  createEffect(() => {
    const selectedTheme = theme()
    document.documentElement.dataset.theme = selectedTheme

    try {
      localStorage.setItem(STORAGE_KEY, selectedTheme)
    } catch {
      // The theme still applies when storage is unavailable.
    }
  })

  const value: ThemeContextValue = {
    theme,
    toggleTheme: () =>
      setTheme((current) => (current === 'dark' ? 'light' : 'dark')),
  }

  return (
    <ThemeContext.Provider value={value}>
      {props.children}
    </ThemeContext.Provider>
  )
}

export function useTheme(): ThemeContextValue {
  const context = useContext(ThemeContext)
  if (!context) throw new Error('useTheme must be called inside ThemeProvider')
  return context
}

function readInitialTheme(): Theme {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored === 'light' || stored === 'dark') return stored
  } catch {
    // Fall through to the operating-system preference.
  }

  return matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}
