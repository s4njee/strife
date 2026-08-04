/* @refresh reload */
import { render } from 'solid-js/web'
import { Route, Router } from '@solidjs/router'
import './styles/tokens.css'
import './index.css'
import App from './App.tsx'
import { ImportStatusView } from './views/ImportStatusView'
import { ErrorsView } from './views/ErrorsView'
import { OcrStatusView } from './views/OcrStatusView'
import { OcrDocumentsView } from './views/OcrDocumentsView'
import { ConsoleView } from './views/ConsoleView'
import { EmailView } from './views/EmailView'
import { ThemeProvider } from './theme/ThemeProvider'
import {
  FavoritesView,
  FolderView,
  RootFolderView,
  TrashView,
} from './views/WorkspaceView'

const root = document.getElementById('root')
const routerBase =
  import.meta.env.BASE_URL === '/'
    ? undefined
    : import.meta.env.BASE_URL.replace(/\/$/, '')

render(
  () => (
    <ThemeProvider>
      <Router base={routerBase} root={App}>
        <Route path="/" component={RootFolderView} />
        <Route path="/folder/:id" component={FolderView} />
        <Route path="/favorites" component={FavoritesView} />
        <Route path="/imports" component={ImportStatusView} />
        <Route path="/console" component={ConsoleView} />
        <Route path="/email" component={EmailView} />
        <Route path="/ocr" component={OcrStatusView} />
        <Route path="/ocr/documents" component={OcrDocumentsView} />
        <Route path="/errors" component={ErrorsView} />
        <Route path="/trash" component={TrashView} />
      </Router>
    </ThemeProvider>
  ),
  root!,
)
