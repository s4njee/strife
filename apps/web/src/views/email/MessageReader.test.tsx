import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@solidjs/testing-library'
import userEvent from '@testing-library/user-event'
import axe from 'axe-core'
import { MessageReader } from './MessageReader'
import { ThemeProvider } from '../../theme/ThemeProvider'
import type { EmailMessage } from '../../api/types'

function message(overrides: Partial<EmailMessage> = {}): EmailMessage {
  return {
    node_id: 'n1',
    status: 'completed',
    parser_version: '0.11.5',
    message_id: '<a@example.test>',
    in_reply_to: null,
    references: [],
    subject: 'Quarterly reconciliation',
    sent_at: '2024-03-11T09:14:00Z',
    received_at: null,
    body_text: 'Line one.\nLine two.',
    body_html: '<p>Line one.</p>',
    blocked_remote_count: 0,
    blocked_hosts: [],
    preview_text: 'Line one.',
    thread_group_id: null,
    duplicate_group_id: null,
    provider_thread_id: null,
    labels: [],
    addresses: [
      { role: 'to', display_name: null, address: 'owner@example.test' },
      { role: 'from', display_name: 'Ada', address: 'ada@example.test' },
    ],
    attachments: [],
    warnings: [],
    raw_headers: null,
    ...overrides,
  }
}

function renderReader(
  props: Partial<Parameters<typeof MessageReader>[0]> = {},
) {
  return render(() => (
    <ThemeProvider>
      <MessageReader
        message={props.message ?? message()}
        remoteRevealed={props.remoteRevealed ?? false}
        onRevealRemote={props.onRevealRemote ?? (() => {})}
        onClose={props.onClose ?? (() => {})}
      />
    </ThemeProvider>
  ))
}

describe('MessageReader', () => {
  it('orders correspondents by role rather than by storage order', () => {
    renderReader()
    const roles = Array.from(document.querySelectorAll('dt')).map(
      (term) => term.textContent,
    )
    // From must precede To even though the fixture stores To first.
    expect(roles.indexOf('From')).toBeLessThan(roles.indexOf('To'))
  })

  it('renders the body in a frame that cannot run scripts', () => {
    renderReader()
    const frame = document.querySelector('iframe')
    expect(frame).not.toBeNull()
    const sandbox = frame?.getAttribute('sandbox') ?? ''
    // allow-scripts alongside allow-same-origin is the classic sandbox escape.
    expect(sandbox).not.toContain('allow-scripts')
    expect(sandbox).toContain('allow-same-origin')
    expect(frame?.getAttribute('srcdoc')).toContain("script-src 'none'")
  })

  it('never assigns message HTML into the application DOM', () => {
    renderReader({
      message: message({ body_html: '<p id="leaked">escaped</p>' }),
    })
    // The body exists only inside the frame's srcdoc, never as real elements
    // in Strife's own document.
    expect(document.getElementById('leaked')).toBeNull()
  })

  it('offers plain text that keeps its line breaks', async () => {
    const user = userEvent.setup()
    renderReader()
    await user.click(screen.getByRole('button', { name: 'Show plain text' }))
    const pre = document.querySelector('.email-reader__text')
    expect(pre?.textContent).toBe('Line one.\nLine two.')
    expect(document.querySelector('iframe')).toBeNull()
  })

  it('withholds remote images until asked and explains why', async () => {
    const user = userEvent.setup()
    const onRevealRemote = vi.fn()
    renderReader({
      message: message({
        blocked_remote_count: 2,
        blocked_hosts: ['tracker.test'],
      }),
      onRevealRemote,
    })
    expect(screen.getByText(/tracker\.test/)).toBeInTheDocument()
    expect(
      screen.getByText(/tells the sender the message was opened/),
    ).toBeInTheDocument()
    await user.click(screen.getByRole('button', { name: 'Show remote images' }))
    expect(onRevealRemote).toHaveBeenCalledOnce()
  })

  it('says nothing about remote images when there are none', () => {
    renderReader()
    expect(
      screen.queryByRole('button', { name: 'Show remote images' }),
    ).toBeNull()
  })

  it('exposes attachment size as a machine-readable value', () => {
    renderReader({
      message: message({
        attachments: [
          {
            part_path: '2',
            filename: 'report.pdf',
            media_type: 'application/pdf',
            disposition: 'attachment',
            content_id: null,
            decoded_size: 184320,
            is_inline: false,
            is_message: false,
            extraction_status: 'pending',
          },
        ],
      }),
    })
    const value = document.querySelector('data')
    expect(value).toHaveAttribute('value', '184320')
    expect(value?.textContent).toBe('184 KB')
  })

  it('offers no affordance to edit the archived message', () => {
    renderReader()
    const labels = screen
      .getAllByRole('button')
      .map((button) => button.textContent?.toLowerCase() ?? '')
    expect(
      labels.some((label) => /edit|reply|forward|delete/.test(label)),
    ).toBe(false)
  })

  it('expands message details on request', async () => {
    const user = userEvent.setup()
    renderReader()
    const toggle = screen.getByRole('button', { name: /show message details/i })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    await user.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('<a@example.test>')).toBeInTheDocument()
  })

  it('has no detectable accessibility violations', async () => {
    const { container } = renderReader({
      message: message({
        labels: ['Work'],
        warnings: ['a parser note'],
        blocked_remote_count: 1,
        blocked_hosts: ['tracker.test'],
      }),
    })
    const outcome = await axe.run(container, {
      // The message frame holds the sender's own markup, which Strife cannot
      // fix and jsdom cannot traverse. The reader's chrome around it is what
      // this check is for.
      iframes: false,
      // Contrast cannot be judged in jsdom, which has no layout or real CSS.
      rules: { 'color-contrast': { enabled: false } },
    })
    expect(outcome.violations.map((violation) => violation.id)).toEqual([])
  })
})
