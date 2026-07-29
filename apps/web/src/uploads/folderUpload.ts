import {
  ApiClientError,
  createFolder,
  createUploadSession,
  finalizeUpload,
  getFolderChildren,
  uploadFileChunk,
} from '../api/client'
import type { FolderItem } from '../api/types'

const configuredChunkSize = Number(import.meta.env.VITE_UPLOAD_CHUNK_SIZE_BYTES)
const DEFAULT_CHUNK_SIZE =
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

export async function uploadFolderFiles(
  files: File[],
  targetFolderId: string,
  chunkSize = DEFAULT_CHUNK_SIZE,
): Promise<FolderUploadResult[]> {
  return uploadFiles(
    files.map((file) => ({
      file,
      relativePath: file.webkitRelativePath || file.name,
    })),
    targetFolderId,
    chunkSize,
  )
}

export async function uploadFiles(
  candidates: UploadCandidate[],
  targetFolderId: string,
  chunkSize = DEFAULT_CHUNK_SIZE,
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
): Promise<FolderUploadResult> {
  const { file, relativePath: path } = candidate
  try {
    const segments = path.split('/').filter(Boolean)
    segments.pop()
    const parentId = await ensureFolderPath(
      targetFolderId,
      segments,
      folderCache,
    )
    const node = await uploadOneFile(file, parentId, chunkSize)
    return { path, node }
  } catch (error) {
    return { path, error: uploadErrorMessage(error) }
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

async function uploadOneFile(
  file: File,
  parentId: string,
  chunkSize: number,
): Promise<FolderItem> {
  const session = await createUploadSession(parentId, file)
  for (let start = 0; start < file.size; start += chunkSize) {
    await uploadFileChunk(
      session.session_id,
      file.slice(start, Math.min(start + chunkSize, file.size)),
      start,
      file.size,
    )
  }
  return finalizeUpload(session.session_id)
}

function uploadErrorMessage(error: unknown): string {
  if (error instanceof ApiClientError && error.status === 409) {
    return 'A file or folder with this name already exists'
  }
  return error instanceof Error ? error.message : 'Upload failed'
}
