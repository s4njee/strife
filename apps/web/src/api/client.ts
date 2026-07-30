import type {
  ApiReadiness,
  CreatedUploadSession,
  DependencyStatus,
  FavoriteItem,
  FavoritesListResponse,
  FolderAncestor,
  FolderChildrenResponse,
  FileDetails,
  FolderItem,
  ImportEntry,
  ImportScanResult,
  ImportSource,
  MediaStream,
  MoveFolderConflict,
  MoveFoldersResponse,
  PreviewJobStatus,
  ReadinessResponse,
  UploadSession,
} from './types'

export async function prepareFilePreview(
  fileId: string,
  signal?: AbortSignal,
): Promise<string> {
  const previewUrl = `/api/files/${fileId}/preview`
  let response = await fetch(previewUrl, { signal })
  if (response.ok) {
    await response.body?.cancel()
    return previewUrl
  }
  if (response.status === 404) {
    throw new ApiClientError('A preview is not available for this file.', {
      status: 404,
      code: 'preview_not_supported',
    })
  }
  if (response.status !== 202) {
    throw new ApiClientError(`Preview request failed (${response.status}).`)
  }

  const pending: unknown = await response.json()
  if (!isRecord(pending) || typeof pending.job_id !== 'string') {
    throw new ApiClientError('The preview response was invalid.')
  }
  while (!signal?.aborted) {
    await waitForPreviewPoll(signal)
    response = await fetch(`/api/jobs/${pending.job_id}`, {
      headers: { Accept: 'application/json' },
      signal,
    })
    if (!response.ok) {
      throw new ApiClientError(`Preview status failed (${response.status}).`)
    }
    const status: unknown = await response.json()
    if (!isPreviewJobStatus(status)) {
      throw new ApiClientError('The preview status response was invalid.')
    }
    if (status.status === 'completed') return previewUrl
    if (status.status === 'failed' || status.status === 'cancelled') {
      throw new ApiClientError(
        status.error ?? 'The preview could not be generated.',
      )
    }
  }
  throw new DOMException('Preview request aborted', 'AbortError')
}

function waitForPreviewPoll(signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(resolve, 750)
    signal?.addEventListener(
      'abort',
      () => {
        window.clearTimeout(timeout)
        reject(new DOMException('Preview request aborted', 'AbortError'))
      },
      { once: true },
    )
  })
}

function isPreviewJobStatus(value: unknown): value is PreviewJobStatus {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    ['pending', 'leased', 'completed', 'failed', 'cancelled'].includes(
      String(value.status),
    ) &&
    (value.error === null || typeof value.error === 'string')
  )
}

export async function getFileDetails(
  fileId: string,
  signal?: AbortSignal,
): Promise<FileDetails> {
  const response = await fetch(`/api/files/${fileId}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) {
    throw new ApiClientError(
      `File details could not be loaded (${response.status}).`,
    )
  }
  const body: unknown = await response.json()
  if (!isFileDetails(body))
    throw new ApiClientError('The file details response was invalid.')
  return body
}

export async function getFileStreams(
  fileId: string,
  signal?: AbortSignal,
): Promise<MediaStream[]> {
  const response = await fetch(`/api/files/${fileId}/streams`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) {
    throw new ApiClientError(
      `Media streams could not be loaded (${response.status}).`,
    )
  }
  const body: unknown = await response.json()
  if (!Array.isArray(body) || !body.every(isMediaStream)) {
    throw new ApiClientError('The media streams response was invalid.')
  }
  return body
}

export class ApiClientError extends Error {
  readonly status?: number
  readonly code?: string
  readonly conflicts?: MoveFolderConflict[]

  constructor(
    message: string,
    options?: ErrorOptions & {
      status?: number
      code?: string
      conflicts?: MoveFolderConflict[]
    },
  ) {
    super(message, options)
    this.name = 'ApiClientError'
    this.status = options?.status
    this.code = options?.code
    this.conflicts = options?.conflicts
  }
}

export async function getReadiness(
  signal?: AbortSignal,
): Promise<ApiReadiness> {
  let response: Response

  try {
    response = await fetch('/api/ready', {
      headers: { Accept: 'application/json' },
      signal,
    })
  } catch (error) {
    throw new ApiClientError(
      'The Strife API is unreachable. Check that the API and development services are running.',
      { cause: error },
    )
  }

  if (response.status !== 200 && response.status !== 503) {
    throw new ApiClientError(
      `The Strife API returned an unexpected status (${response.status}).`,
    )
  }

  let body: unknown
  try {
    body = await response.json()
  } catch (error) {
    throw new ApiClientError('The Strife API returned invalid JSON.', {
      cause: error,
    })
  }

  if (!isReadinessResponse(body)) {
    throw new ApiClientError(
      'The Strife API returned an invalid readiness response.',
    )
  }

  return {
    ready: response.status === 200,
    httpStatus: response.status,
    details: body,
  }
}

export async function getFolderAncestors(
  folderId: string,
  signal?: AbortSignal,
): Promise<FolderAncestor[]> {
  let response: Response

  try {
    response = await fetch(`/api/folders/${folderId}/ancestors`, {
      headers: { Accept: 'application/json' },
      signal,
    })
  } catch (error) {
    throw new ApiClientError('The folder path could not be loaded.', {
      cause: error,
    })
  }

  if (!response.ok) {
    throw new ApiClientError(
      `The folder path request failed (${response.status}).`,
    )
  }

  const body: unknown = await response.json()
  if (!Array.isArray(body) || !body.every(isFolderAncestor)) {
    throw new ApiClientError('The folder path response was invalid.')
  }

  return body
}

export interface FolderChildrenQuery {
  sort?: string
  order?: 'asc' | 'desc'
  kind?: string[]
}

export async function getFolderChildren(
  folderId: string,
  signal?: AbortSignal,
  query: FolderChildrenQuery = {},
): Promise<FolderChildrenResponse> {
  let response: Response
  const params = new URLSearchParams({ limit: '100' })
  if (query.sort) params.set('sort', query.sort)
  if (query.order) params.set('order', query.order)
  for (const kind of query.kind ?? []) params.append('kind', kind)

  try {
    response = await fetch(
      `/api/folders/${folderId}/children?${params.toString()}`,
      {
        headers: { Accept: 'application/json' },
        signal,
      },
    )
  } catch (error) {
    throw new ApiClientError('The folder contents could not be loaded.', {
      cause: error,
    })
  }

  if (!response.ok) {
    throw new ApiClientError(
      `The folder contents request failed (${response.status}).`,
    )
  }

  const body: unknown = await response.json()
  if (!isFolderChildrenResponse(body)) {
    throw new ApiClientError('The folder contents response was invalid.')
  }

  return {
    items: body.items.map((item) => ({
      ...item,
      size_bytes: item.size_bytes ?? null,
    })),
    next_cursor: body.next_cursor,
  }
}

export async function getFavorites(
  signal?: AbortSignal,
): Promise<FavoritesListResponse> {
  let response: Response
  try {
    response = await fetch('/api/favorites', {
      headers: { Accept: 'application/json' },
      signal,
    })
  } catch (error) {
    throw new ApiClientError('Favorites could not be loaded.', { cause: error })
  }
  if (!response.ok) {
    throw new ApiClientError(
      `The favorites request failed (${response.status}).`,
    )
  }
  const body: unknown = await response.json()
  if (!isFavoritesListResponse(body)) {
    throw new ApiClientError('The favorites response was invalid.')
  }
  return body
}

export async function addFavorite(nodeId: string): Promise<void> {
  const response = await fetch(`/api/nodes/${nodeId}/favorite`, {
    method: 'PUT',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) {
    throw new ApiClientError(
      `Could not favorite item (${response.status}).`,
      { status: response.status },
    )
  }
}

export async function removeFavorite(nodeId: string): Promise<void> {
  const response = await fetch(`/api/nodes/${nodeId}/favorite`, {
    method: 'DELETE',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) {
    throw new ApiClientError(
      `Could not unfavorite item (${response.status}).`,
      { status: response.status },
    )
  }
}

export async function trashNodes(nodeIds: string[]): Promise<void> {
  const response = await fetch('/api/nodes/trash', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ node_ids: nodeIds }),
  })
  if (!response.ok) {
    throw new ApiClientError(`Could not move items to trash (${response.status}).`, {
      status: response.status,
    })
  }
}

export async function restoreNode(nodeId: string): Promise<void> {
  const response = await fetch(`/api/nodes/${nodeId}/restore`, {
    method: 'POST',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok) {
    throw new ApiClientError(`Could not restore item (${response.status}).`, {
      status: response.status,
    })
  }
}

export async function permanentDeleteNode(nodeId: string): Promise<void> {
  const response = await fetch(`/api/nodes/${nodeId}/permanent`, {
    method: 'DELETE',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok && response.status !== 200 && response.status !== 202) {
    throw new ApiClientError(
      `Could not permanently delete item (${response.status}).`,
      { status: response.status },
    )
  }
}

export interface TrashListItem {
  id: string
  node_id: string
  name: string
  kind: 'folder' | 'file'
  original_parent_id: string | null
  trashed_at: string
  scheduled_purge_at: string
  created_at: string
  updated_at: string
}

export async function getTrash(
  signal?: AbortSignal,
): Promise<{ items: TrashListItem[] }> {
  const response = await fetch('/api/trash', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) {
    throw new ApiClientError(`Could not load trash (${response.status}).`)
  }
  const body: unknown = await response.json()
  if (
    !isRecord(body) ||
    !Array.isArray(body.items) ||
    !body.items.every(
      (item) =>
        isRecord(item) &&
        typeof item.node_id === 'string' &&
        typeof item.name === 'string',
    )
  ) {
    throw new ApiClientError('The trash response was invalid.')
  }
  return body as { items: TrashListItem[] }
}

export function downloadFileUrl(nodeId: string): string {
  return `/api/files/${nodeId}/download`
}

export async function createFolder(
  parentId: string,
  name: string,
): Promise<FolderItem> {
  let response: Response

  try {
    response = await fetch('/api/folders', {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ parent_id: parentId, name }),
    })
  } catch (error) {
    throw new ApiClientError('The folder could not be created.', {
      cause: error,
    })
  }

  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `The create request failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }

  const body: unknown = await response.json()
  if (!isFolderItem(body)) {
    throw new ApiClientError('The created folder response was invalid.')
  }
  return { ...body, size_bytes: body.size_bytes ?? null }
}

export async function renameFolder(
  folderId: string,
  name: string,
): Promise<FolderItem> {
  let response: Response

  try {
    response = await fetch(`/api/folders/${folderId}`, {
      method: 'PATCH',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ name }),
    })
  } catch (error) {
    throw new ApiClientError('The folder could not be renamed.', {
      cause: error,
    })
  }

  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `The rename request failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }

  const body: unknown = await response.json()
  if (!isFolderItem(body)) {
    throw new ApiClientError('The renamed folder response was invalid.')
  }
  return { ...body, size_bytes: body.size_bytes ?? null }
}

export async function moveFolders(
  folderIds: string[],
  parentId: string,
): Promise<MoveFoldersResponse> {
  let response: Response

  try {
    response = await fetch('/api/folders/move', {
      method: 'PATCH',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ folder_ids: folderIds, parent_id: parentId }),
    })
  } catch (error) {
    throw new ApiClientError('The folders could not be moved.', {
      cause: error,
    })
  }

  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `The move request failed (${response.status}).`,
      {
        status: response.status,
        code: errorBody?.code,
        conflicts: errorBody?.conflicts,
      },
    )
  }

  const body: unknown = await response.json()
  if (!isMoveFoldersResponse(body)) {
    throw new ApiClientError('The moved folders response was invalid.')
  }
  return {
    items: body.items.map((item) => ({
      ...item,
      size_bytes: item.size_bytes ?? null,
    })),
  }
}

export async function getUploadSession(
  sessionId: string,
  signal?: AbortSignal,
): Promise<UploadSession> {
  return requestUploadSessions(`/api/uploads/${sessionId}`, signal).then(
    (sessions) => sessions[0],
  )
}

export async function getActiveUploads(
  folderId: string,
  signal?: AbortSignal,
): Promise<UploadSession[]> {
  const query = new URLSearchParams({ folder_id: folderId })
  return requestUploadSessions(`/api/uploads?${query}`, signal)
}

export async function createUploadSession(
  folderId: string,
  file: File,
): Promise<CreatedUploadSession> {
  const response = await fetch('/api/uploads', {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      folder_id: folderId,
      name: file.name,
      size: file.size,
      source_created_at: null,
      source_modified_at: new Date(file.lastModified).toISOString(),
    }),
  })
  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `Upload creation failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }
  const body: unknown = await response.json()
  if (
    !isRecord(body) ||
    typeof body.session_id !== 'string' ||
    typeof body.staging_key !== 'string'
  ) {
    throw new ApiClientError('The upload creation response was invalid.')
  }
  return body as unknown as CreatedUploadSession
}

export async function uploadFileChunk(
  sessionId: string,
  bytes: Blob,
  start: number,
  total: number,
  signal?: AbortSignal,
): Promise<void> {
  const end = start + bytes.size - 1
  const response = await fetch(`/api/uploads/${sessionId}`, {
    method: 'PATCH',
    headers: { 'Content-Range': `bytes ${start}-${end}/${total}` },
    body: bytes,
    signal,
  })
  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `Upload chunk failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }
}

export async function finalizeUpload(
  sessionId: string,
  signal?: AbortSignal,
): Promise<FolderItem> {
  const response = await fetch(`/api/uploads/${sessionId}/finalize`, {
    method: 'POST',
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `Upload finalization failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }
  const body: unknown = await response.json()
  if (!isFolderItem(body)) {
    throw new ApiClientError('The finalized upload response was invalid.')
  }
  return { ...body, size_bytes: body.size_bytes ?? null }
}

export async function cancelUpload(sessionId: string): Promise<void> {
  const response = await fetch(`/api/uploads/${sessionId}`, {
    method: 'DELETE',
  })
  if (!response.ok && response.status !== 404) {
    throw new ApiClientError(
      `Upload cancellation failed (${response.status}).`,
      {
        status: response.status,
      },
    )
  }
}

export async function getImportSources(
  signal?: AbortSignal,
): Promise<ImportSource[]> {
  return requestJson('/api/import-sources', isImportSourceArray, signal)
}

export async function setImportSourceEnabled(
  sourceId: string,
  enabled: boolean,
): Promise<ImportSource> {
  return requestJson(
    `/api/import-sources/${sourceId}`,
    isImportSource,
    undefined,
    {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled }),
    },
  )
}

export async function scanImportSource(
  sourceId: string,
): Promise<ImportScanResult> {
  return requestJson(
    `/api/import-sources/${sourceId}/scan`,
    isImportScanResult,
    undefined,
    { method: 'POST' },
  )
}

export async function getImportEntries(
  sourceId: string,
  state: 'failed',
  signal?: AbortSignal,
): Promise<ImportEntry[]> {
  const query = new URLSearchParams({ state })
  return requestJson(
    `/api/import-sources/${sourceId}/entries?${query}`,
    isImportEntryArray,
    signal,
  )
}

export async function retryImportEntry(
  sourceId: string,
  entryId: string,
): Promise<ImportEntry> {
  return requestJson(
    `/api/import-sources/${sourceId}/entries/${entryId}/retry`,
    isImportEntry,
    undefined,
    { method: 'POST' },
  )
}

async function requestJson<T>(
  url: string,
  validate: (value: unknown) => value is T,
  signal?: AbortSignal,
  init?: RequestInit,
): Promise<T> {
  let response: Response
  try {
    response = await fetch(url, {
      ...init,
      headers: { Accept: 'application/json', ...init?.headers },
      signal,
    })
  } catch (error) {
    throw new ApiClientError('Import status could not be loaded.', {
      cause: error,
    })
  }
  if (!response.ok) {
    const errorBody = await readErrorBody(response)
    throw new ApiClientError(
      errorBody?.message ?? `The import request failed (${response.status}).`,
      { status: response.status, code: errorBody?.code },
    )
  }
  const body: unknown = await response.json()
  if (!validate(body)) {
    throw new ApiClientError('The import response was invalid.')
  }
  return body
}

async function requestUploadSessions(
  url: string,
  signal?: AbortSignal,
): Promise<UploadSession[]> {
  let response: Response
  try {
    response = await fetch(url, {
      headers: { Accept: 'application/json' },
      signal,
    })
  } catch (error) {
    throw new ApiClientError('Upload progress could not be loaded.', {
      cause: error,
    })
  }
  if (!response.ok) {
    throw new ApiClientError(
      `The upload progress request failed (${response.status}).`,
      { status: response.status },
    )
  }
  const body: unknown = await response.json()
  const sessions = Array.isArray(body) ? body : [body]
  if (!sessions.every(isUploadSession)) {
    throw new ApiClientError('The upload progress response was invalid.')
  }
  return sessions
}

function isReadinessResponse(value: unknown): value is ReadinessResponse {
  if (!isRecord(value)) return false

  return (
    isDependencyStatus(value.postgres) &&
    isDependencyStatus(value.storage) &&
    isDependencyStatus(value.tika) &&
    typeof value.disk_usage_percent === 'number' &&
    Number.isFinite(value.disk_usage_percent)
  )
}

function isFileDetails(value: unknown): value is FileDetails {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    typeof value.byte_size === 'number' &&
    ['processing', 'ready', 'partially_processed', 'failed'].includes(
      String(value.processing_status),
    )
  )
}

function isMediaStream(value: unknown): value is MediaStream {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.stream_index === 'number' &&
    ['video', 'audio', 'subtitle'].includes(String(value.stream_type)) &&
    typeof value.codec === 'string'
  )
}

function isDependencyStatus(value: unknown): value is DependencyStatus {
  return value === 'ok' || value === 'error'
}

function isFolderAncestor(value: unknown): value is FolderAncestor {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string'
  )
}

function isFolderChildrenResponse(
  value: unknown,
): value is FolderChildrenResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.items) &&
    value.items.every(isFolderItem) &&
    (value.next_cursor === null || typeof value.next_cursor === 'string')
  )
}

function isFolderItem(value: unknown): value is FolderItem {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    (value.kind === 'folder' || value.kind === 'file') &&
    (value.size_bytes === undefined ||
      value.size_bytes === null ||
      typeof value.size_bytes === 'number') &&
    typeof value.created_at === 'string' &&
    typeof value.updated_at === 'string' &&
    (value.is_favorite === undefined || typeof value.is_favorite === 'boolean')
  )
}

function isFavoritesListResponse(
  value: unknown,
): value is FavoritesListResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.items) &&
    value.items.every(isFavoriteItem)
  )
}

function isFavoriteItem(value: unknown): value is FavoriteItem {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    (value.kind === 'folder' || value.kind === 'file') &&
    (value.parent_id === null || typeof value.parent_id === 'string') &&
    typeof value.favorited_at === 'string' &&
    typeof value.created_at === 'string' &&
    typeof value.updated_at === 'string'
  )
}

function isMoveFoldersResponse(value: unknown): value is MoveFoldersResponse {
  return (
    isRecord(value) &&
    Array.isArray(value.items) &&
    value.items.every(isFolderItem)
  )
}

function isMoveFolderConflict(value: unknown): value is MoveFolderConflict {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    (value.reason === 'name_conflict' || value.reason === 'cycle_detected')
  )
}

function isUploadSession(value: unknown): value is UploadSession {
  return (
    isRecord(value) &&
    typeof value.session_id === 'string' &&
    ['active', 'finalizing', 'completed', 'cancelled', 'expired'].includes(
      String(value.state),
    ) &&
    typeof value.display_name === 'string' &&
    typeof value.received_bytes === 'number' &&
    (value.expected_bytes === null ||
      typeof value.expected_bytes === 'number') &&
    Array.isArray(value.received_ranges) &&
    value.received_ranges.every(
      (range) =>
        isRecord(range) &&
        typeof range.start === 'number' &&
        typeof range.end === 'number',
    ) &&
    typeof value.created_at === 'string' &&
    typeof value.expires_at === 'string'
  )
}

function isImportSourceArray(value: unknown): value is ImportSource[] {
  return Array.isArray(value) && value.every(isImportSource)
}

function isImportSource(value: unknown): value is ImportSource {
  if (!isRecord(value) || !isRecord(value.counts)) return false
  const counts = value.counts
  return (
    typeof value.id === 'string' &&
    typeof value.watch_path === 'string' &&
    typeof value.destination_folder_id === 'string' &&
    typeof value.enabled === 'boolean' &&
    (value.last_scan_at === null || typeof value.last_scan_at === 'string') &&
    ['discovered', 'stable', 'importing', 'imported', 'failed'].every(
      (key) => typeof counts[key] === 'number',
    )
  )
}

function isImportEntryArray(value: unknown): value is ImportEntry[] {
  return Array.isArray(value) && value.every(isImportEntry)
}

function isImportEntry(value: unknown): value is ImportEntry {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.source_path === 'string' &&
    typeof value.source_size === 'number' &&
    typeof value.source_modified_at === 'string' &&
    ['discovered', 'stable', 'importing', 'imported', 'failed'].includes(
      String(value.state),
    ) &&
    (value.resulting_node_id === null ||
      typeof value.resulting_node_id === 'string') &&
    (value.error_message === null || typeof value.error_message === 'string') &&
    typeof value.updated_at === 'string'
  )
}

function isImportScanResult(value: unknown): value is ImportScanResult {
  return (
    isRecord(value) &&
    [
      'discovered',
      'imported',
      'failed',
      'skipped_hidden',
      'skipped_special',
    ].every((key) => typeof value[key] === 'number')
  )
}

async function readErrorBody(response: Response): Promise<
  | {
      code?: string
      message?: string
      conflicts?: MoveFolderConflict[]
    }
  | undefined
> {
  try {
    const body: unknown = await response.json()
    if (!isRecord(body)) return undefined
    return {
      code: typeof body.code === 'string' ? body.code : undefined,
      message: typeof body.message === 'string' ? body.message : undefined,
      conflicts:
        Array.isArray(body.conflicts) &&
        body.conflicts.every(isMoveFolderConflict)
          ? body.conflicts
          : undefined,
    }
  } catch {
    return undefined
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
