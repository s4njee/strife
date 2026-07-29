import { A, useLocation } from '@solidjs/router'
import { For } from 'solid-js'
import { useTheme } from '../theme/ThemeProvider'
import './Sidebar.css'

const navigation = [
  { href: '/', label: 'All Files', icon: 'grid', end: true },
  { href: '/favorites', label: 'Favorites', icon: 'star', end: false },
  { href: '/imports', label: 'Imports', icon: 'inbox', end: false },
  { href: '/trash', label: 'Trash', icon: 'trash', end: false },
] as const

export function Sidebar() {
  const location = useLocation()
  const { theme, toggleTheme } = useTheme()

  return (
    <aside class="sidebar">
      <div class="sidebar__brand">
        <span class="sidebar__brand-mark" aria-hidden="true">
          S
        </span>
        <span>
          <strong>Strife</strong>
          <small>Home drive</small>
        </span>
      </div>

      <nav class="sidebar__nav" aria-label="Primary navigation">
        <For each={navigation}>
          {(item) => (
            <A
              href={item.href}
              end={item.end}
              class="sidebar__nav-link"
              activeClass="is-active"
              aria-current={
                location.pathname === item.href ||
                (item.href === '/' && location.pathname.startsWith('/folder/'))
                  ? 'page'
                  : undefined
              }
            >
              <SidebarIcon name={item.icon} />
              <span>{item.label}</span>
            </A>
          )}
        </For>
      </nav>

      <div class="sidebar__spacer" />

      <section class="storage-summary" aria-labelledby="storage-summary-title">
        <div class="storage-summary__heading">
          <strong id="storage-summary-title">Storage</strong>
          <span>24%</span>
        </div>
        <div
          class="storage-summary__meter"
          role="progressbar"
          aria-valuenow="24"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-label="Storage used"
        >
          <span />
        </div>
        <p>1.2 TB of 5 TB used</p>
      </section>

      <button class="sidebar__theme-toggle" type="button" onClick={toggleTheme}>
        <SidebarIcon name={theme() === 'dark' ? 'sun' : 'moon'} />
        Use {theme() === 'dark' ? 'light' : 'dark'} theme
      </button>
    </aside>
  )
}

type IconName = 'grid' | 'star' | 'inbox' | 'trash' | 'sun' | 'moon'

function SidebarIcon(props: { name: IconName }) {
  const paths: Record<IconName, string> = {
    grid: 'M4 4h5v5H4zM15 4h5v5h-5zM4 15h5v5H4zM15 15h5v5h-5z',
    star: 'm12 3 2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z',
    inbox: 'M4 5h16v14H4zM4 14h4l2 2h4l2-2h4',
    trash: 'M5 7h14M9 7V4h6v3m2 0-1 13H8L7 7m3 4v5m4-5v5',
    sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8m0-5v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4',
    moon: 'M20 15.2A8.5 8.5 0 0 1 8.8 4 8.5 8.5 0 1 0 20 15.2',
  }

  return (
    <svg class="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d={paths[props.name]} />
    </svg>
  )
}
