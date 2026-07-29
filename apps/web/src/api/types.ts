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

export type FolderItemKind = 'folder' | 'file'

export interface FolderItem {
  id: string
  name: string
  kind: FolderItemKind
  size_bytes: number | null
  created_at: string
  updated_at: string
}

export interface FolderChildrenResponse {
  items: FolderItem[]
  next_cursor: string | null
}
