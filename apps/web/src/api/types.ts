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
  is_favorite?: boolean
}

export interface FavoriteItem {
  id: string
  name: string
  kind: FolderItemKind
  parent_id: string | null
  favorited_at: string
  created_at: string
  updated_at: string
}

export interface FavoritesListResponse {
  items: FavoriteItem[]
}

export interface FolderChildrenResponse {
  items: FolderItem[]
  next_cursor: string | null
}

export type ProcessingStatus =
  'processing' | 'ready' | 'partially_processed' | 'failed'

export interface FileDetails {
  id: string
  parent_id: string | null
  name: string
  byte_size: number
  checksum_sha256: string | null
  created_at: string
  updated_at: string
  detected_mime: string | null
  media_kind: 'document' | 'image' | 'video' | 'audio' | 'other' | null
  duration_ms: number | null
  width: number | null
  height: number | null
  capture_time: string | null
  page_count: number | null
  orientation: number | null
  has_gps: boolean | null
  gps_latitude: number | null
  gps_longitude: number | null
  camera_make: string | null
  camera_model: string | null
  document_title: string | null
  document_author: string | null
  document_created_at: string | null
  document_modified_at: string | null
  processing_status: ProcessingStatus
}

export interface PreviewJobStatus {
  id: string
  status: 'pending' | 'leased' | 'completed' | 'failed' | 'cancelled'
  error: string | null
}

export interface MediaStream {
  id: string
  stream_index: number
  stream_type: 'video' | 'audio' | 'subtitle'
  codec: string
  width: number | null
  height: number | null
  duration_ms: number | null
  bitrate_bps: number | null
  frame_rate: string | null
  language: string | null
  created_at: string
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

export type ImportEntryState =
  'discovered' | 'stable' | 'importing' | 'imported' | 'failed'

export interface ImportCounts {
  discovered: number
  stable: number
  importing: number
  imported: number
  failed: number
}

export interface ImportSource {
  id: string
  watch_path: string
  destination_folder_id: string
  enabled: boolean
  last_scan_at: string | null
  counts: ImportCounts
}

export interface ImportEntry {
  id: string
  source_path: string
  source_size: number
  source_modified_at: string
  state: ImportEntryState
  resulting_node_id: string | null
  error_message: string | null
  updated_at: string
}

export interface ImportScanResult {
  job_id: string
  status: 'pending' | 'leased'
}

export interface OcrCounts {
  pending: number
  running: number
  completed: number
  failed: number
  skipped: number
  unsupported: number
  remaining: number
}

export interface OcrStatus {
  counts: OcrCounts
  engine_name: string | null
  engine_version: string | null
  language: string | null
}

export interface OcrEvent {
  id: number
  node_id: string | null
  name: string
  state: 'running' | 'completed' | 'failed' | 'skipped' | 'unsupported'
  page_count: number | null
  mean_confidence: number | null
  warning: string | null
  created_at: string
}

export interface DocumentTextPage {
  page_number: number
  content: string
  confidence: number | null
  width: number | null
  height: number | null
}

export interface FileText {
  status:
    | 'not_processed'
    | 'in_progress'
    | 'completed'
    | 'failed'
    | 'skipped'
    | 'skipped_embedded'
    | 'unsupported'
  source: 'embedded' | 'ocr' | null
  language: string | null
  engine_name: string | null
  engine_version: string | null
  mean_confidence: number | null
  warnings: string[]
  pages: DocumentTextPage[]
  next_page: number | null
}

export interface TextSearchMatch {
  node_id: string
  name: string
  page_number: number
  snippet: string
  score: number
}

export interface TextSearchResponse {
  items: TextSearchMatch[]
  next_cursor: string | null
  indexed_documents: number
}
