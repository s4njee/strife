import {
  ApiClientError,
  createFolder,
  createUploadSession,
  finalizeUpload,
  getFolderChildren,
  uploadFileChunk,
} from '../api/client'
import type { FolderItem, UploadByteRange } from '../api/types'

const configuredChunkSize = Number(import.meta.env.VITE_UPLOAD_CHUNK_SIZE_BYTES)
export const DEFAULT_CHUNK_SIZE =
  Number.isSafeInteger(configuredChunkSize) && configuredChunkSize > 0
    ? configuredChunkSize
    : 1024 * 1024
const MAX_CONCURRENT_UPLOADS = 3

export interface UploadCandidate {
  file: File
  relativePath: string
}

export interface FolderUploadResult {
  path: string
  node?: FolderItem
  error?: string
}

export interface UploadEvents {
  onSession?: (
    sessionId: string,
    candidate: UploadCandidate,
    parentId: string,
  ) => void | Promise<void>
  onProgress?: (
    sessionId: string,
    receivedBytes: number,
    totalBytes: number,
  ) => void
  onComplete?: (sessionId: string, node: FolderItem) => void | Promise<void>
  onError?: (sessionId: string | undefined, path: string, error: string) => void
  signal?: (sessionId: string) => AbortSignal | undefined
}

export async function uploadFolderFiles(
  files: File[],
  targetFolderId: string,
  chunkSize = DEFAULT_CHUNK_SIZE,
  events?: UploadEvents,
): Promise<FolderUploadResult[]> {
  return uploadFiles(
    files.map((file) => ({
      file,
      relativePath: file.webkitRelativePath || file.name,
    })),
    targetFolderId,
    chunkSize,
    events,
  )
}

export async function uploadFiles(
  candidates: UploadCandidate[],
  targetFolderId: string,
  chunkSize = DEFAULT_CHUNK_SIZE,
  events?: UploadEvents,
): Promise<FolderUploadResult[]> {
  const folderCache = new Map<string, string>()
  const results = new Array<FolderUploadResult>(candidates.length)
  let nextIndex = 0
  const uploadNext = async () => {
    while (nextIndex < candidates.length) {
      const index = nextIndex
      nextIndex += 1
      const candidate = candidates[index]
      results[index] = await uploadCandidate(
        candidate,
        targetFolderId,
        folderCache,
        chunkSize,
        events,
      )
    }
  }
  await Promise.all(
    Array.from(
      { length: Math.min(MAX_CONCURRENT_UPLOADS, candidates.length) },
      uploadNext,
    ),
  )
  return results
}

async function uploadCandidate(
  candidate: UploadCandidate,
  targetFolderId: string,
  folderCache: Map<string, string>,
  chunkSize: number,
  events?: UploadEvents,
): Promise<FolderUploadResult> {
  const { file, relativePath: path } = candidate
  let sessionId: string | undefined
  try {
    const segments = path.split('/').filter(Boolean)
    segments.pop()
    const parentId = await ensureFolderPath(
      targetFolderId,
      segments,
      folderCache,
    )
    const session = await createUploadSession(parentId, file)
    sessionId = session.session_id
    await events?.onSession?.(sessionId, candidate, parentId)
    const node = await resumeFileUpload(file, sessionId, [], chunkSize, events)
    return { path, node }
  } catch (error) {
    const message = uploadErrorMessage(error)
    events?.onError?.(sessionId, path, message)
    return { path, error: message }
  }
}

async function ensureFolderPath(
  rootId: string,
  segments: string[],
  cache: Map<string, string>,
): Promise<string> {
  let parentId = rootId
  for (const segment of segments) {
    const cacheKey = `${parentId}\0${segment}`
    const cached = cache.get(cacheKey)
    if (cached) {
      parentId = cached
      continue
    }
    const existing = await findFolder(parentId, segment)
    if (existing) {
      cache.set(cacheKey, existing.id)
      parentId = existing.id
      continue
    }
    try {
      const created = await createFolder(parentId, segment)
      cache.set(cacheKey, created.id)
      parentId = created.id
    } catch (error) {
      if (error instanceof ApiClientError && error.status === 409) {
        const raced = await findFolder(parentId, segment)
        if (raced) {
          cache.set(cacheKey, raced.id)
          parentId = raced.id
          continue
        }
      }
      throw error
    }
  }
  return parentId
}

async function findFolder(
  parentId: string,
  name: string,
): Promise<FolderItem | undefined> {
  const children = await getFolderChildren(parentId)
  return children.items.find(
    (item) => item.kind === 'folder' && item.name === name,
  )
}

export async function resumeFileUpload(
  file: File,
  sessionId: string,
  receivedRanges: UploadByteRange[],
  chunkSize: number,
  events?: UploadEvents,
): Promise<FolderItem> {
  let receivedBytes = receivedRanges.reduce(
    (total, range) => total + range.end - range.start + 1,
    0,
  )
  for (const missing of missingRanges(file.size, receivedRanges)) {
    for (let start = missing.start; start <= missing.end; start += chunkSize) {
      const endExclusive = Math.min(start + chunkSize, missing.end + 1)
      const bytes = file.slice(start, endExclusive)
      await uploadFileChunk(
        sessionId,
        bytes,
        start,
        file.size,
        events?.signal?.(sessionId),
      )
      receivedBytes += bytes.size
      events?.onProgress?.(sessionId, receivedBytes, file.size)
    }
  }
  const node = await finalizeUpload(sessionId, events?.signal?.(sessionId))
  await events?.onComplete?.(sessionId, node)
  return node
}

export function missingRanges(
  totalBytes: number,
  receivedRanges: UploadByteRange[],
): UploadByteRange[] {
  if (totalBytes === 0) return []
  const sorted = [...receivedRanges].sort(
    (left, right) => left.start - right.start,
  )
  const missing: UploadByteRange[] = []
  let cursor = 0
  for (const range of sorted) {
    if (range.start > cursor)
      missing.push({ start: cursor, end: range.start - 1 })
    cursor = Math.max(cursor, range.end + 1)
  }
  if (cursor < totalBytes) missing.push({ start: cursor, end: totalBytes - 1 })
  return missing
}

function uploadErrorMessage(error: unknown): string {
  if (error instanceof ApiClientError && error.status === 409) {
    return 'A file or folder with this name already exists'
  }
  return error instanceof Error ? error.message : 'Upload failed'
}
