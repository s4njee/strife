import type {
  ApiReadiness,
  DependencyStatus,
  FolderAncestor,
  FolderChildrenResponse,
  FolderItem,
  MoveFolderConflict,
  MoveFoldersResponse,
  ReadinessResponse,
} from './types'

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

export async function getFolderChildren(
  folderId: string,
  signal?: AbortSignal,
): Promise<FolderChildrenResponse> {
  let response: Response

  try {
    response = await fetch(`/api/folders/${folderId}/children?limit=100`, {
      headers: { Accept: 'application/json' },
      signal,
    })
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
