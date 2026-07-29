/* @refresh reload */
import { render } from 'solid-js/web'
import { Route, Router } from '@solidjs/router'
import './styles/tokens.css'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from './theme/ThemeProvider'
import {
  FavoritesView,
  FolderView,
  RootFolderView,
  TrashView,
} from './views/WorkspaceView'

const root = document.getElementById('root')

render(
  () => (
    <ThemeProvider>
      <Router base={import.meta.env.BASE_URL} root={App}>
        <Route path="/" component={RootFolderView} />
        <Route path="/folder/:id" component={FolderView} />
        <Route path="/favorites" component={FavoritesView} />
        <Route path="/trash" component={TrashView} />
      </Router>
    </ThemeProvider>
  ),
  root!,
)
