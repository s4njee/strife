import type {
  ApiReadiness,
  DependencyStatus,
  FolderAncestor,
  FolderChildrenResponse,
  FolderItem,
  ReadinessResponse,
} from './types'

export class ApiClientError extends Error {
  constructor(message: string, options?: ErrorOptions) {
    super(message, options)
    this.name = 'ApiClientError'
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
