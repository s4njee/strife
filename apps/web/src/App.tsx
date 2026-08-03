import type { RouteSectionProps } from '@solidjs/router'
import { Sidebar } from './components/Sidebar'
import { StorageWarning } from './components/StorageWarning'
import { ToastProvider } from './components/Toast'
import { UploadProgressPanel } from './components/UploadProgressPanel'
import { UploadProvider } from './uploads/UploadContext'
import './App.css'

function App(props: RouteSectionProps) {
  return (
    <UploadProvider>
      <ToastProvider>
        <div class="app-shell">
          <a class="skip-link" href="#workspace-content">
            Skip to files
          </a>
          <Sidebar />
          <main id="workspace-content" class="workspace" tabIndex={-1}>
            <StorageWarning />
            {props.children}
          </main>
          <UploadProgressPanel />
        </div>
      </ToastProvider>
    </UploadProvider>
  )
}

export default App
