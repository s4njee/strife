import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@solidjs/testing-library'
import userEvent from '@testing-library/user-event'
import axe from 'axe-core'
import { ResultList } from './ResultList'
import type { EmailSearchHit } from '../../api/types'

function hit(overrides: Partial<EmailSearchHit> = {}): EmailSearchHit {
  return {
    node_id: 'n1',
    subject: 'Quarterly reconciliation',
    sent_at: '2024-03-11T09:14:00Z',
    snippet: 'The [[reconciliation]] figures are attached.',
    attachment_count: 0,
    duplicate_count: 1,
    thread_count: 1,
    score: 0.5,
    from_address: 'ada@example.test',
    from_display_name: 'Ada Lovelace',
    labels: [],
    match_sources: [],
    matched_attachment: null,
    matched_attachment_page: null,
    ...overrides,
  }
}

describe('ResultList', () => {
  it('renders highlights as mark elements rather than markup', () => {
    render(() => (
      <ResultList
        results={[hit({ snippet: 'a [[term]] b <b>not bold</b>' })]}
        selected={null}
        onSelect={() => {}}
      />
    ))
    const mark = screen.getByText('term')
    expect(mark.tagName).toBe('MARK')
    // The literal markup in the body must appear as text, never as an element.
    expect(document.querySelector('.email-result__snippet b')).toBeNull()
    expect(screen.getByText(/not bold/)).toBeInTheDocument()
  })

  it('states a fallback for a missing sender, subject, and date', () => {
    render(() => (
      <ResultList
        results={[
          hit({
            subject: null,
            sent_at: null,
            from_address: null,
            from_display_name: null,
          }),
        ]}
        selected={null}
        onSelect={() => {}}
      />
    ))
    // A blank cell is indistinguishable from a rendering bug, so each gap is
    // named explicitly for anyone reading or listening.
    expect(screen.getByText('(no sender recorded)')).toBeInTheDocument()
    expect(screen.getByText('(no subject)')).toBeInTheDocument()
    expect(screen.getByText('(no date recorded)')).toBeInTheDocument()
  })

  it('opens a result with Enter and with Space', async () => {
    const user = userEvent.setup()
    const onSelect = vi.fn()
    render(() => (
      <ResultList results={[hit()]} selected={null} onSelect={onSelect} />
    ))
    const result = screen.getByRole('button')
    result.focus()
    await user.keyboard('{Enter}')
    await user.keyboard(' ')
    expect(onSelect).toHaveBeenCalledTimes(2)
    expect(onSelect).toHaveBeenCalledWith('n1')
  })

  it('moves focus between results with the arrow keys', async () => {
    const user = userEvent.setup()
    render(() => (
      <ResultList
        results={[
          hit({ node_id: 'n1' }),
          hit({ node_id: 'n2' }),
          hit({ node_id: 'n3' }),
        ]}
        selected={null}
        onSelect={() => {}}
      />
    ))
    const results = screen.getAllByRole('button')
    results[0].focus()
    await user.keyboard('{ArrowDown}')
    expect(document.activeElement).toBe(results[1])
    await user.keyboard('{End}')
    expect(document.activeElement).toBe(results[2])
    await user.keyboard('{Home}')
    expect(document.activeElement).toBe(results[0])
    // Arrowing past the first result must not wrap or lose focus.
    await user.keyboard('{ArrowUp}')
    expect(document.activeElement).toBe(results[0])
  })

  it('marks the open result as pressed', () => {
    render(() => (
      <ResultList
        results={[hit({ node_id: 'n1' }), hit({ node_id: 'n2' })]}
        selected="n2"
        onSelect={() => {}}
      />
    ))
    const results = screen.getAllByRole('button')
    expect(results[0]).toHaveAttribute('aria-pressed', 'false')
    expect(results[1]).toHaveAttribute('aria-pressed', 'true')
  })

  it('has no detectable accessibility violations', async () => {
    const { container } = render(() => (
      <ResultList
        results={[
          hit(),
          hit({ node_id: 'n2', subject: null, attachment_count: 2 }),
        ]}
        selected="n2"
        onSelect={() => {}}
      />
    ))
    const outcome = await axe.run(container, {
      // Contrast cannot be judged in jsdom, which has no layout or real CSS.
      rules: { 'color-contrast': { enabled: false } },
    })
    expect(outcome.violations.map((violation) => violation.id)).toEqual([])
  })
})
