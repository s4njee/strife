export type DependencyStatus = 'ok' | 'error'

export interface ReadinessResponse {
  postgres: DependencyStatus
  storage: DependencyStatus
  tika: DependencyStatus
  disk_usage_percent: number
}

export interface ApiReadiness {
  ready: boolean
  httpStatus: 200 | 503
  details: ReadinessResponse
}

export interface FolderAncestor {
  id: string
  name: string
}
