import { createContext, createSignal, useContext, type JSX } from 'solid-js'
import { cancelUpload, getActiveUploads } from '../api/client'
import type { UploadSession } from '../api/types'
import {
  DEFAULT_CHUNK_SIZE,
  resumeFileUpload,
  uploadFiles,
  type FolderUploadResult,
  type UploadCandidate,
  type UploadEvents,
} from './folderUpload'
import {
  loadUploadFile,
  persistUploadFile,
  removeUploadFile,
} from './uploadPersistence'

export type UploadPanelState =
  'uploading' | 'needs_file' | 'completed' | 'error'

export interface UploadPanelItem {
  id: string
  folderId: string
  name: string
  path: string
  receivedBytes: number
  totalBytes: number
  startedAt: number
  state: UploadPanelState
  error?: string
}

interface UploadContextValue {
  items: () => UploadPanelItem[]
  start: (
    candidates: UploadCandidate[],
    folderId: string,
    onComplete?: () => void | Promise<void>,
  ) => Promise<FolderUploadResult[]>
  discover: (
    folderId: string,
    onComplete?: () => void | Promise<void>,
  ) => Promise<void>
  cancel: (sessionId: string) => Promise<void>
  resumeWithFile: (sessionId: string, file: File) => Promise<void>
}

const UploadContext = createContext<UploadContextValue>()

export function UploadProvider(props: { children: JSX.Element }) {
  const [items, setItems] = createSignal<UploadPanelItem[]>([])
  const controllers = new Map<string, AbortController>()
  const completionCallbacks = new Map<string, () => void | Promise<void>>()

  const updateItem = (id: string, update: Partial<UploadPanelItem>): void => {
    setItems((current) =>
      current.map((item) => (item.id === id ? { ...item, ...update } : item)),
    )
  }

  const events: UploadEvents = {
    onSession: async (sessionId, candidate, folderId) => {
      const controller = new AbortController()
      controllers.set(sessionId, controller)
      try {
        await persistUploadFile(sessionId, candidate.file)
      } catch {
        // Uploading can continue even when browser persistence is unavailable.
      }
      setItems((current) => [
        ...current.filter((item) => item.id !== sessionId),
        {
          id: sessionId,
          folderId,
          name: candidate.file.name,
          path: candidate.relativePath,
          receivedBytes: 0,
          totalBytes: candidate.file.size,
          startedAt: Date.now(),
          state: 'uploading',
        },
      ])
    },
    onProgress: (sessionId, receivedBytes, totalBytes) =>
      updateItem(sessionId, { receivedBytes, totalBytes }),
    onComplete: async (sessionId) => {
      const item = items().find((candidate) => candidate.id === sessionId)
      updateItem(sessionId, { state: 'completed' })
      controllers.delete(sessionId)
      await removeUploadFile(sessionId).catch(() => undefined)
      if (item) await completionCallbacks.get(item.folderId)?.()
    },
    onError: (sessionId, path, error) => {
      if (sessionId) {
        if (items().some((item) => item.id === sessionId)) {
          updateItem(sessionId, { state: 'error', error })
        }
        return
      }
      setItems((current) => [
        ...current,
        {
          id: `error:${crypto.randomUUID()}`,
          folderId: '',
          name: path.split('/').at(-1) ?? path,
          path,
          receivedBytes: 0,
          totalBytes: 0,
          startedAt: Date.now(),
          state: 'error',
          error,
        },
      ])
    },
    signal: (sessionId) => controllers.get(sessionId)?.signal,
  }

  const resumeSession = async (session: UploadSession, file: File) => {
    if (
      file.name !== session.display_name ||
      file.size !== session.expected_bytes
    ) {
      updateItem(session.session_id, {
        state: 'error',
        error: 'Selected file does not match this upload',
      })
      return
    }
    controllers.set(session.session_id, new AbortController())
    updateItem(session.session_id, { state: 'uploading', error: undefined })
    await persistUploadFile(session.session_id, file).catch(() => undefined)
    try {
      await resumeFileUpload(
        file,
        session.session_id,
        session.received_ranges,
        DEFAULT_CHUNK_SIZE,
        events,
      )
    } catch (error) {
      events.onError?.(
        session.session_id,
        session.display_name,
        error instanceof Error ? error.message : 'Resume failed',
      )
    }
  }

  const discover = async (
    folderId: string,
    onComplete?: () => void | Promise<void>,
  ) => {
    if (onComplete) completionCallbacks.set(folderId, onComplete)
    const sessions = await getActiveUploads(folderId)
    for (const session of sessions) {
      if (items().some((item) => item.id === session.session_id)) continue
      setItems((current) => [
        ...current,
        panelItemFromSession(session, folderId),
      ])
      const file = await loadUploadFile(session.session_id).catch(
        () => undefined,
      )
      if (file) void resumeSession(session, file)
    }
  }

  const start = (
    candidates: UploadCandidate[],
    folderId: string,
    onComplete?: () => void | Promise<void>,
  ) => {
    if (onComplete) completionCallbacks.set(folderId, onComplete)
    return uploadFiles(candidates, folderId, DEFAULT_CHUNK_SIZE, events)
  }

  const cancel = async (sessionId: string) => {
    controllers.get(sessionId)?.abort()
    controllers.delete(sessionId)
    await cancelUpload(sessionId)
    await removeUploadFile(sessionId).catch(() => undefined)
    setItems((current) => current.filter((item) => item.id !== sessionId))
  }

  const resumeWithFile = async (sessionId: string, file: File) => {
    const sessions = await Promise.all(
      [...completionCallbacks.keys()].map((folderId) =>
        getActiveUploads(folderId),
      ),
    )
    const session = sessions
      .flat()
      .find((candidate) => candidate.session_id === sessionId)
    if (session) await resumeSession(session, file)
  }

  return (
    <UploadContext.Provider
      value={{ items, start, discover, cancel, resumeWithFile }}
    >
      {props.children}
    </UploadContext.Provider>
  )
}

function panelItemFromSession(
  session: UploadSession,
  folderId: string,
): UploadPanelItem {
  return {
    id: session.session_id,
    folderId,
    name: session.display_name,
    path: session.display_name,
    receivedBytes: session.received_bytes,
    totalBytes: session.expected_bytes ?? session.received_bytes,
    startedAt: Date.parse(session.created_at),
    state: 'needs_file',
    error: 'Waiting for the original file to resume',
  }
}

export function useUploads(): UploadContextValue {
  const context = useContext(UploadContext)
  if (!context) throw new Error('useUploads must be used inside UploadProvider')
  return context
}
