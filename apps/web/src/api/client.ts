import type {
  ApiReadiness,
  BackfillCampaign,
  CreatedUploadSession,
  DependencyStatus,
  EmailFacets,
  EmailMessage,
  EmailSearchCriteria,
  EmailSearchResponse,
  EmailStatus,
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
  MetadataEvent,
  MetadataStatus,
  MoveFolderConflict,
  MoveFoldersResponse,
  PreviewJobStatus,
  ReadinessResponse,
  UploadSession,
  FileText,
  OcrPreflight,
  OcrStatus,
  TextSearchResponse,
} from './types'

export async function getMetadataStatus(
  signal?: AbortSignal,
): Promise<MetadataStatus> {
  const response = await fetch('/api/metadata/status', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Metadata status failed (${response.status}).`)
  return (await response.json()) as MetadataStatus
}

export async function getRecentMetadataEvents(
  limit = 50,
  signal?: AbortSignal,
): Promise<MetadataEvent[]> {
  const query = new URLSearchParams({ limit: String(limit) })
  const response = await fetch(`/api/metadata/recent?${query}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Metadata activity failed (${response.status}).`)
  return (await response.json()) as MetadataEvent[]
}

export async function getEmailStatus(
  signal?: AbortSignal,
): Promise<EmailStatus> {
  const response = await fetch('/api/email/status', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Email status failed (${response.status}).`)
  return (await response.json()) as EmailStatus
}

/**
 * Builds the search query string.
 *
 * Repeatable criteria are appended as repeated keys rather than joined, because
 * the API parses the raw query string specifically to preserve them.
 */
export function emailSearchParams(
  criteria: EmailSearchCriteria,
  cursor?: string | null,
): URLSearchParams {
  const params = new URLSearchParams()
  if (criteria.q.trim()) params.set('q', criteria.q.trim())
  for (const value of criteria.from) params.append('from', value)
  for (const value of criteria.participant) params.append('participant', value)
  for (const value of criteria.label) params.append('label', value)
  if (criteria.after)
    params.set('after', new Date(criteria.after).toISOString())
  if (criteria.before)
    params.set('before', new Date(criteria.before).toISOString())
  if (criteria.hasAttachment !== null)
    params.set('has_attachment', String(criteria.hasAttachment))
  if (criteria.includeTrashed) params.set('include_trashed', 'true')
  if (criteria.includeDuplicates) params.set('include_duplicates', 'true')
  if (criteria.threadId) params.set('thread_id', criteria.threadId)
  if (criteria.duplicateGroup)
    params.set('duplicate_group', criteria.duplicateGroup)
  if (cursor) params.set('cursor', cursor)
  return params
}

export async function searchEmail(
  criteria: EmailSearchCriteria,
  cursor?: string | null,
  signal?: AbortSignal,
): Promise<EmailSearchResponse> {
  const params = emailSearchParams(criteria, cursor)
  const response = await fetch(`/api/email/search?${params}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Email search failed (${response.status}).`)
  return (await response.json()) as EmailSearchResponse
}

/** Requeues extraction for one message or a bounded batch. */
export async function reprocessEmail(options: {
  scope: 'node' | 'failed' | 'missing' | 'version'
  nodeId?: string
  limit?: number
}): Promise<number> {
  const response = await fetch('/api/email/reprocess', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      scope: options.scope,
      node_id: options.nodeId,
      limit: options.limit,
    }),
  })
  if (!response.ok)
    throw new ApiClientError(`Email reprocessing failed (${response.status}).`)
  return ((await response.json()) as { enqueued: number }).enqueued
}

/** Authenticated URL for one attachment of one message. */
export function emailAttachmentUrl(
  nodeId: string,
  partPath: string,
  inline = false,
): string {
  const query = inline ? '?inline=true' : ''
  return `/api/email/messages/${nodeId}/parts/${encodeURIComponent(partPath)}${query}`
}

export async function getEmailFacets(
  signal?: AbortSignal,
): Promise<EmailFacets> {
  const response = await fetch('/api/email/facets', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Email facets failed (${response.status}).`)
  return (await response.json()) as EmailFacets
}

/**
 * Loads one message.
 *
 * `allowRemoteImages` is a request parameter rather than a client-side toggle:
 * remote URLs are stripped server-side, so revealing them means asking for a
 * different response, and a message that is merely opened never causes a
 * tracking pixel to fire.
 */
export async function getEmailMessage(
  nodeId: string,
  includeRawHeaders = false,
  signal?: AbortSignal,
  allowRemoteImages = false,
): Promise<EmailMessage> {
  const params = new URLSearchParams()
  if (includeRawHeaders) params.set('include_raw_headers', 'true')
  if (allowRemoteImages) params.set('allow_remote_images', 'true')
  const query = params.toString() ? `?${params}` : ''
  const response = await fetch(`/api/email/messages/${nodeId}${query}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (response.status === 404)
    throw new ApiClientError('That message is no longer in the archive.')
  if (!response.ok)
    throw new ApiClientError(`Message failed to load (${response.status}).`)
  return (await response.json()) as EmailMessage
}

export async function getOcrStatus(signal?: AbortSignal): Promise<OcrStatus> {
  const response = await fetch('/api/ocr/status', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`OCR status failed (${response.status}).`)
  return (await response.json()) as OcrStatus
}

export async function reprocessOcr(options: {
  scope: 'node' | 'failed' | 'version'
  nodeId?: string
}): Promise<number> {
  const params = new URLSearchParams({ extractor: 'ocr', scope: options.scope })
  if (options.nodeId) params.set('node_id', options.nodeId)
  const response = await fetch(`/api/admin/reprocess?${params.toString()}`, {
    method: 'POST',
    headers: { Accept: 'application/json' },
  })
  if (!response.ok)
    throw new ApiClientError(`OCR reprocessing failed (${response.status}).`)
  const body = (await response.json()) as { enqueued?: unknown }
  if (typeof body.enqueued !== 'number')
    throw new ApiClientError('OCR reprocessing response was invalid.')
  return body.enqueued
}

export async function getOcrPreflight(
  signal?: AbortSignal,
): Promise<OcrPreflight> {
  const response = await fetch('/api/ocr/preflight', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`OCR preflight failed (${response.status}).`)
  return (await response.json()) as OcrPreflight
}

export async function listBackfills(
  signal?: AbortSignal,
): Promise<BackfillCampaign[]> {
  const response = await fetch('/api/backfills', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Campaign list failed (${response.status}).`)
  return (await response.json()) as BackfillCampaign[]
}

/**
 * Creates a draft campaign and immediately prepares it into `paused`.
 *
 * Creation never starts historical work: the caller must explicitly resume the
 * returned campaign. `snapshot_before` and `candidate_count` come from a
 * reviewed preflight so the campaign cannot silently enlarge itself later.
 */
export async function createOcrCampaign(preflight: {
  candidateCount: number
  snapshotBefore: string
  batchSize: number
  maxQueued: number
  maxRunning: number
}): Promise<BackfillCampaign> {
  const created = await fetch('/api/backfills', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify({
      kind: 'ocr',
      candidate_definition: { version: 1, source: 'ocr-preflight' },
      batch_size: preflight.batchSize,
      max_queued: preflight.maxQueued,
      max_running: preflight.maxRunning,
      resource_class: 'heavy_cpu',
    }),
  })
  if (!created.ok)
    throw new ApiClientError(`Campaign creation failed (${created.status}).`)
  const draft = (await created.json()) as BackfillCampaign

  const prepared = await fetch(`/api/backfills/${draft.id}/prepare`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify({
      candidate_count: preflight.candidateCount,
      snapshot_before: preflight.snapshotBefore,
      reason: 'prepared from OCR preflight',
    }),
  })
  if (!prepared.ok)
    throw new ApiClientError(`Campaign prepare failed (${prepared.status}).`)
  return (await prepared.json()) as BackfillCampaign
}

export async function actOnBackfill(
  id: string,
  action: 'resume' | 'pause' | 'drain' | 'complete' | 'cancel',
  reason?: string,
): Promise<BackfillCampaign> {
  const response = await fetch(`/api/backfills/${id}/actions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
    body: JSON.stringify({ action, reason }),
  })
  if (!response.ok)
    throw new ApiClientError(`Campaign ${action} failed (${response.status}).`)
  return (await response.json()) as BackfillCampaign
}

export async function getFileText(
  fileId: string,
  afterPage?: number,
  signal?: AbortSignal,
): Promise<FileText> {
  const params = new URLSearchParams()
  if (afterPage) params.set('after_page', afterPage.toString())
  const suffix = params.size > 0 ? `?${params.toString()}` : ''
  const response = await fetch(`/api/files/${fileId}/text${suffix}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Document text failed (${response.status}).`)
  return (await response.json()) as FileText
}

export async function getAllFileText(fileId: string): Promise<FileText> {
  let result = await getFileText(fileId)
  const pages = [...result.pages]
  while (result.next_page !== null) {
    result = await getFileText(fileId, result.next_page)
    pages.push(...result.pages)
  }
  return { ...result, pages }
}

export async function searchDocumentText(
  query: string,
  signal?: AbortSignal,
): Promise<TextSearchResponse> {
  const params = new URLSearchParams({ q: query })
  const response = await fetch(`/api/search?${params.toString()}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok)
    throw new ApiClientError(`Text search failed (${response.status}).`)
  return (await response.json()) as TextSearchResponse
}

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
    throw new ApiClientError(`Could not favorite item (${response.status}).`, {
      status: response.status,
    })
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
    throw new ApiClientError(
      `Could not move items to trash (${response.status}).`,
      {
        status: response.status,
      },
    )
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

export interface StorageUsage {
  total_bytes: number
  used_bytes: number
  available_bytes: number
  originals_bytes: number
  artifacts_bytes: number
  trash_bytes: number
  usage_percent: number
}

export async function getStorageUsage(
  signal?: AbortSignal,
): Promise<StorageUsage> {
  const response = await fetch('/api/storage/usage', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) {
    throw new ApiClientError(`Storage usage failed (${response.status}).`)
  }
  const body: unknown = await response.json()
  if (
    !isRecord(body) ||
    typeof body.total_bytes !== 'number' ||
    typeof body.used_bytes !== 'number' ||
    typeof body.usage_percent !== 'number'
  ) {
    throw new ApiClientError('The storage usage response was invalid.')
  }
  return body as unknown as StorageUsage
}

export async function getActiveJobCount(signal?: AbortSignal): Promise<number> {
  const response = await fetch('/api/jobs?state=pending,leased&count=true', {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) return 0
  const body: unknown = await response.json()
  if (!isRecord(body) || typeof body.count !== 'number') return 0
  return body.count
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
    typeof value.job_id === 'string' &&
    ['pending', 'leased'].includes(String(value.status))
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
