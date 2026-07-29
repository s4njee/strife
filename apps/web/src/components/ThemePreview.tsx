import { For } from 'solid-js'
import './ThemePreview.css'

const swatches = [
  ['Canvas', 'var(--color-canvas)'],
  ['Surface', 'var(--color-surface)'],
  ['Raised', 'var(--color-surface-raised)'],
  ['Selected', 'var(--color-surface-selected)'],
  ['Accent', 'var(--color-accent)'],
  ['Success', 'var(--color-success)'],
  ['Error', 'var(--color-error)'],
] as const

export function ThemePreview() {
  return (
    <section class="theme-preview" aria-labelledby="theme-preview-heading">
      <div class="theme-preview__header">
        <div>
          <p>Development only</p>
          <h2 id="theme-preview-heading">Theme tokens</h2>
        </div>
        <span>Sans + mono</span>
      </div>

      <div class="theme-preview__swatches">
        <For each={swatches}>
          {([label, color]) => (
            <div class="theme-preview__swatch">
              <span style={{ background: color }} aria-hidden="true" />
              <code>{label}</code>
            </div>
          )}
        </For>
      </div>
    </section>
  )
}
