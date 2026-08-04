import { render, screen, waitFor } from '@solidjs/testing-library'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { getOcrTree } from '../api/client'
import type { OcrTreeNode } from '../api/types'
import { OcrDocumentsView } from './OcrDocumentsView'

vi.mock('../api/client', () => ({ getOcrTree: vi.fn() }))
vi.mock('../components/OcrTabs', () => ({
  OcrTabs: () => <nav aria-label="OCR sections">OCR tabs</nav>,
}))
vi.mock('../components/FileDetailsPanel', () => ({
  FileDetailsPanel: (props: { item: { name: string } }) => (
    <aside aria-label="File details">{props.item.name}</aside>
  ),
}))
vi.mock('../components/PreviewModal', () => ({
  demoImage: 'data:image/svg+xml,demo',
  PreviewModal: (props: {
    item: { name: string }
    files: { name: string }[]
  }) => (
    <div role="dialog" aria-label="Preview">
      {props.item.name}
      <span data-testid="preview-siblings">{props.files.length}</span>
    </div>
  ),
}))

const rootId = '00000000-0000-0000-0000-000000000001'
const folderId = '00000000-0000-0000-0000-000000000101'

function node(overrides: Partial<OcrTreeNode>): OcrTreeNode {
  return {
    id: '00000000-0000-0000-0000-000000000102',
    parent_id: rootId,
    name: 'Scanned book.pdf',
    kind: 'file',
    status: 'completed',
    source: 'ocr',
    page_count: 12,
    mean_confidence: 94.2,
    char_count: 4200,
    updated_at: '2026-08-04T09:00:00Z',
    total_files: 1,
    pending: 0,
    running: 0,
    completed: 1,
    failed: 0,
    skipped: 0,
    unsupported: 0,
    ...overrides,
  }
}

describe('OcrDocumentsView', () => {
  beforeEach(() => {
    vi.mocked(getOcrTree).mockImplementation(async (parentId) => ({
      items:
        parentId === rootId
          ? [
              node({
                id: folderId,
                name: 'Books',
                kind: 'folder',
                status: null,
                source: null,
                page_count: null,
                mean_confidence: null,
                char_count: null,
                total_files: 1,
              }),
            ]
          : [node({ parent_id: folderId })],
      next_offset: null,
    }))
  })

  it('loads the root lazily, expands folders, and opens file details', async () => {
    const user = userEvent.setup()
    render(() => <OcrDocumentsView />)

    expect(await screen.findByText('Books')).toBeInTheDocument()
    expect(getOcrTree).toHaveBeenCalledWith(rootId, 0, expect.any(AbortSignal))
    expect(screen.queryByText('Scanned book.pdf')).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: /Books/ }))
    expect(await screen.findByText('Scanned book.pdf')).toBeInTheDocument()
    expect(getOcrTree).toHaveBeenCalledWith(
      folderId,
      0,
      expect.any(AbortSignal),
    )
    expect(screen.getByText('12 pages')).toBeInTheDocument()
    expect(screen.getByText('94.2%')).toBeInTheDocument()

    await user.click(screen.getByText('Scanned book.pdf'))
    await waitFor(() =>
      expect(screen.getByLabelText('File details')).toHaveTextContent(
        'Scanned book.pdf',
      ),
    )
  })

  it('opens a preview on double click and leaves single click selecting', async () => {
    const user = userEvent.setup()
    render(() => <OcrDocumentsView />)

    await user.click(await screen.findByRole('button', { name: /Books/ }))
    const file = await screen.findByText('Scanned book.pdf')

    // A single click still opens the metadata panel, not the preview.
    await user.click(file)
    await waitFor(() =>
      expect(screen.getByLabelText('File details')).toBeInTheDocument(),
    )
    expect(screen.queryByRole('dialog', { name: 'Preview' })).toBeNull()

    await user.dblClick(file)
    const preview = await screen.findByRole('dialog', { name: 'Preview' })
    expect(preview).toHaveTextContent('Scanned book.pdf')
    // The previewed file's own folder is what the modal steps through.
    expect(screen.getByTestId('preview-siblings')).toHaveTextContent('1')
  })

  it('opens a preview on Enter, and a folder row expands instead', async () => {
    const user = userEvent.setup()
    render(() => <OcrDocumentsView />)

    // Enter on a folder expands it rather than previewing it.
    const folder = await screen.findByRole('button', { name: /Books/ })
    folder.focus()
    await user.keyboard('{Enter}')
    const file = await screen.findByText('Scanned book.pdf')
    expect(screen.queryByRole('dialog', { name: 'Preview' })).toBeNull()

    const fileRow = file.closest('button')
    expect(fileRow).not.toBeNull()
    fileRow?.focus()
    await user.keyboard('{Enter}')
    expect(
      await screen.findByRole('dialog', { name: 'Preview' }),
    ).toHaveTextContent('Scanned book.pdf')
  })
})
