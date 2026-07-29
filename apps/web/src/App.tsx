import type { RouteSectionProps } from '@solidjs/router'
import { Sidebar } from './components/Sidebar'
import './App.css'

function App(props: RouteSectionProps) {
  return (
    <div class="app-shell">
      <Sidebar />
      <main class="workspace">{props.children}</main>
    </div>
  )
}

export default App
