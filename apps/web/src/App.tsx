import type { RouteSectionProps } from '@solidjs/router'
import { Sidebar } from './components/Sidebar'
import { StorageWarning } from './components/StorageWarning'
import { UploadProgressPanel } from './components/UploadProgressPanel'
import { UploadProvider } from './uploads/UploadContext'
import './App.css'

function App(props: RouteSectionProps) {
  return (
    <UploadProvider>
      <div class="app-shell">
        <Sidebar />
        <main class="workspace">
          <StorageWarning />
          {props.children}
        </main>
        <UploadProgressPanel />
      </div>
    </UploadProvider>
  )
}

export default App
