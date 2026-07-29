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

export interface MoveFolderConflict {
  id: string
  name: string
  reason: 'name_conflict' | 'cycle_detected'
}

export interface MoveFoldersResponse {
  items: FolderItem[]
}

export type UploadSessionState =
  'active' | 'finalizing' | 'completed' | 'cancelled' | 'expired'

export interface UploadByteRange {
  start: number
  end: number
}

export interface UploadSession {
  session_id: string
  state: UploadSessionState
  display_name: string
  received_bytes: number
  expected_bytes: number | null
  received_ranges: UploadByteRange[]
  created_at: string
  expires_at: string
}

export interface CreatedUploadSession {
  session_id: string
  staging_key: string
}
