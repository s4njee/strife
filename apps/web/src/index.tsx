/* @refresh reload */
import { render } from 'solid-js/web'
import './styles/tokens.css'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from './theme/ThemeProvider'

const root = document.getElementById('root')

render(
  () => (
    <ThemeProvider>
      <App />
    </ThemeProvider>
  ),
  root!,
)
