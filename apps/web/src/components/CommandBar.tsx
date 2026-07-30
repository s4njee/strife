import { useLocation, useNavigate } from '@solidjs/router'
import { createMemo, createSignal, Show } from 'solid-js'
import {
  createFolder,
  getFolderAncestors,
  getFolderChildren,
  getTrash,
  moveFolders,
  renameFolder,
  restoreNode,
  trashNodes,
} from '../api/client'
import type { FolderItem } from '../api/types'
import { parseCommand, splitPath, tokenize } from '../commands/parse'
import { loadCommandHistory, pushCommandHistory } from '../commands/history'
import './CommandBar.css'

const ROOT_ID = '00000000-0000-0000-0000-000000000001'

export function CommandBar() {
  const location = useLocation()
  const navigate = useNavigate()
  const [value, setValue] = createSignal('')
  const [error, setError] = createSignal<string>()
  const [output, setOutput] = createSignal<string>()
  const [pendingRm, setPendingRm] = createSignal<string>()
  const [history, setHistory] = createSignal(loadCommandHistory())
  const [historyIndex, setHistoryIndex] = createSignal(-1)

  const currentFolderId = createMemo(() => {
    const match = location.pathname.match(/\/folder\/([^/]+)/)
    if (match) return match[1]
    if (location.pathname === '/' || location.pathname.endsWith('/')) {
      return ROOT_ID
    }
    return ROOT_ID
  })

  const run = async () => {
    const line = value().trim()
    if (!line) return

    if (pendingRm()) {
      if (line.toLowerCase() === 'y' || line.toLowerCase() === 'yes') {
        const target = pendingRm()!
        setPendingRm(undefined)
        setValue('')
        await executeRm(target, true)
        return
      }
      setPendingRm(undefined)
      setError('rm cancelled')
      setValue('')
      return
    }

    setHistory(pushCommandHistory(line))
    setHistoryIndex(-1)
    const parsed = parseCommand(line)
    if ('error' in parsed) {
      setError(parsed.error)
      setOutput(undefined)
      return
    }

    try {
      setError(undefined)
      setOutput(undefined)
      switch (parsed.cmd) {
        case 'pwd': {
          const path = await pathForFolder(currentFolderId())
          setOutput(path)
          break
        }
        case 'ls': {
          const folderId = parsed.path
            ? await resolvePath(parsed.path, currentFolderId())
            : currentFolderId()
          const children = await getFolderChildren(folderId)
          setOutput(
            children.items
              .map((item) => `${item.kind === 'folder' ? 'd' : '-'}  ${item.name}`)
              .join('\n') || '(empty)',
          )
          break
        }
        case 'cd': {
          const folderId = await resolvePath(parsed.path, currentFolderId(), {
            foldersOnly: true,
          })
          navigate(folderId === ROOT_ID ? '/' : `/folder/${folderId}`)
          setOutput(undefined)
          break
        }
        case 'mkdir': {
          if (parsed.folderName.includes('/')) {
            throw new Error('mkdir accepts a single name segment')
          }
          await createFolder(currentFolderId(), parsed.folderName)
          setOutput(`created ${parsed.folderName}`)
          break
        }
        case 'mv': {
          await runMv(parsed.source, parsed.dest, currentFolderId())
          setOutput('moved')
          break
        }
        case 'rm': {
          if (!parsed.force) {
            setPendingRm(parsed.target)
            setError(`Confirm rm ${parsed.target}? Type y and Enter, or anything else to cancel.`)
            setValue('')
            return
          }
          await executeRm(parsed.target, true)
          break
        }
        case 'restore': {
          await runRestore(parsed.target)
          setOutput(`restored ${parsed.target}`)
          break
        }
        case 'open': {
          const resolved = await resolvePathEntry(parsed.target, currentFolderId())
          if (resolved.kind === 'folder') {
            navigate(
              resolved.id === ROOT_ID ? '/' : `/folder/${resolved.id}`,
            )
          } else {
            window.open(`/api/files/${resolved.id}/download`, '_blank')
          }
          setOutput(`opened ${resolved.name}`)
          break
        }
      }
      setValue('')
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Command failed')
    }
  }

  const executeRm = async (target: string, _force: boolean) => {
    const entry = await resolvePathEntry(target, currentFolderId())
    await trashNodes([entry.id])
    setOutput(`moved ${entry.name} to trash`)
    setValue('')
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      void run()
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      const list = history()
      if (list.length === 0) return
      const next = Math.min(historyIndex() + 1, list.length - 1)
      setHistoryIndex(next)
      setValue(list[next] ?? '')
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      const next = historyIndex() - 1
      if (next < 0) {
        setHistoryIndex(-1)
        setValue('')
        return
      }
      setHistoryIndex(next)
      setValue(history()[next] ?? '')
      return
    }
    if (event.key === 'Tab') {
      event.preventDefault()
      void autocomplete()
    }
  }

  const autocomplete = async () => {
    const tokens = tokenize(value())
    if (tokens.length === 0) return
    const last = tokens[tokens.length - 1] ?? ''
    const slash = last.lastIndexOf('/')
    const prefix = slash >= 0 ? last.slice(slash + 1) : last
    const dirPart = slash >= 0 ? last.slice(0, slash + 1) : ''
    try {
      const parentId =
        dirPart.length > 0
          ? await resolvePath(dirPart === '/' ? '/' : dirPart.replace(/\/$/, '') || '/', currentFolderId(), {
              foldersOnly: true,
            })
          : currentFolderId()
      const children = await getFolderChildren(parentId)
      const matches = children.items.filter((item) =>
        item.name.toLowerCase().startsWith(prefix.toLowerCase()),
      )
      if (matches.length === 0) return
      const common = longestCommonPrefix(matches.map((item) => item.name))
      const completion =
        dirPart +
        common +
        (matches.length === 1 && matches[0].kind === 'folder' ? '/' : '')
      tokens[tokens.length - 1] = completion
      setValue(tokens.join(' ') + (matches.length === 1 ? ' ' : ''))
    } catch {
      // ignore autocomplete failures
    }
  }

  return (
    <div class="command-bar">
      <label class="command-bar__label" for="command-bar-input">
        Command
      </label>
      <div class="command-bar__row">
        <span class="command-bar__prompt" aria-hidden="true">
          $
        </span>
        <input
          id="command-bar-input"
          class="command-bar__input"
          type="text"
          autocomplete="off"
          spellcheck={false}
          placeholder='pwd · ls · cd · mkdir · mv · rm · restore · open'
          value={value()}
          onInput={(event) => setValue(event.currentTarget.value)}
          onKeyDown={onKeyDown}
        />
      </div>
      <Show when={error()}>
        <p class="command-bar__error" role="alert">
          {error()}
        </p>
      </Show>
      <Show when={output()}>
        <pre class="command-bar__output">{output()}</pre>
      </Show>
    </div>
  )
}

async function pathForFolder(folderId: string): Promise<string> {
  if (folderId === ROOT_ID) return '/'
  const ancestors = await getFolderAncestors(folderId)
  const names = ancestors
    .filter((item) => item.id !== ROOT_ID)
    .map((item) => item.name)
  return `/${names.join('/')}`
}

async function resolvePath(
  path: string,
  cwd: string,
  options: { foldersOnly?: boolean } = {},
): Promise<string> {
  const entry = await resolvePathEntry(path, cwd)
  if (options.foldersOnly && entry.kind !== 'folder') {
    throw new Error(`Not a folder: ${path}`)
  }
  return entry.id
}

async function resolvePathEntry(
  path: string,
  cwd: string,
): Promise<FolderItem & { id: string }> {
  let current = path.startsWith('/') ? ROOT_ID : cwd
  const segments = splitPath(path)
  if (segments.length === 0) {
    return {
      id: current,
      name: current === ROOT_ID ? 'root' : current,
      kind: 'folder',
      size_bytes: null,
      created_at: '',
      updated_at: '',
    }
  }

  let last: FolderItem | undefined
  for (const segment of segments) {
    if (segment === '..') {
      if (current === ROOT_ID) continue
      const ancestors = await getFolderAncestors(current)
      current = ancestors.at(-2)?.id ?? ROOT_ID
      last = undefined
      continue
    }
    const children = await getFolderChildren(current)
    const match = children.items.find((item) => item.name === segment)
    if (!match) throw new Error(`No such path: ${path}`)
    current = match.id
    last = match
  }
  if (!last) {
    return {
      id: current,
      name: 'root',
      kind: 'folder',
      size_bytes: null,
      created_at: '',
      updated_at: '',
    }
  }
  return last
}

async function runMv(source: string, dest: string, cwd: string) {
  const sourceEntry = await resolvePathEntry(source, cwd)
  // If dest ends with / or resolves to a folder, move into it; else rename.
  try {
    const destFolder = await resolvePath(dest, cwd, { foldersOnly: true })
    if (sourceEntry.kind === 'folder') {
      await moveFolders([sourceEntry.id], destFolder)
    } else {
      throw new Error('Moving files via mv is not supported yet; use folders')
    }
  } catch {
    // Treat dest as a new name in the current (or parent) directory.
    const destSegments = splitPath(dest)
    const newName = destSegments.at(-1)
    if (!newName) throw new Error(`Invalid destination: ${dest}`)
    if (sourceEntry.kind === 'folder') {
      if (destSegments.length === 1 && !dest.startsWith('/')) {
        await renameFolder(sourceEntry.id, newName)
      } else {
        const parentPath = dest.startsWith('/')
          ? `/${destSegments.slice(0, -1).join('/')}`
          : destSegments.slice(0, -1).join('/')
        const parentId =
          parentPath === '' || parentPath === '/'
            ? dest.startsWith('/')
              ? ROOT_ID
              : cwd
            : await resolvePath(parentPath || '.', cwd, { foldersOnly: true })
        await moveFolders([sourceEntry.id], parentId)
        if (newName !== sourceEntry.name) {
          await renameFolder(sourceEntry.id, newName)
        }
      }
    } else {
      throw new Error('Renaming files via command bar is not supported yet')
    }
  }
}

async function runRestore(target: string) {
  const trash = await getTrash()
  const match = trash.items.find(
    (item) => item.name === target || item.node_id === target,
  )
  if (!match) throw new Error(`No such trashed item: ${target}`)
  await restoreNode(match.node_id)
}

function longestCommonPrefix(values: string[]): string {
  if (values.length === 0) return ''
  let prefix = values[0]
  for (const value of values.slice(1)) {
    let i = 0
    while (i < prefix.length && i < value.length && prefix[i] === value[i]) {
      i += 1
    }
    prefix = prefix.slice(0, i)
  }
  return prefix
}

