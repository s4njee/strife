import { A } from '@solidjs/router'
import './OcrTabs.css'

export function OcrTabs() {
  return (
    <nav class="ocr-tabs" aria-label="OCR sections">
      <A href="/ocr" end class="ocr-tabs__link" activeClass="is-active">
        Status
      </A>
      <A href="/ocr/documents" class="ocr-tabs__link" activeClass="is-active">
        Documents
      </A>
    </nav>
  )
}
