/* @refresh reload */
import { render } from 'solid-js/web'
import { Route, Router } from '@solidjs/router'
import './styles/tokens.css'
import './index.css'
import App from './App.tsx'
import { ImportStatusView } from './views/ImportStatusView'
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
        <Route path="/trash" component={TrashView} />
      </Router>
    </ThemeProvider>
  ),
  root!,
)
