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
})
