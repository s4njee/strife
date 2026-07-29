import type { ApiReadiness, DependencyStatus, ReadinessResponse } from './types'

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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
