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

export interface MetadataCounts {
  pending: number
  running: number
  completed: number
  failed: number
  skipped: number
  cancelled: number
  remaining: number
  total: number
}

export interface MetadataStatus {
  counts: MetadataCounts
  completed_per_hour: number
  eta_seconds: number | null
}

export interface MetadataEvent {
  id: number
  job_id: string | null
  node_id: string | null
  name: string
  state:
    | 'queued'
    | 'running'
    | 'retrying'
    | 'completed'
    | 'failed'
    | 'skipped'
    | 'cancelled'
  attempt: number
  extractor_name: string | null
  duration_ms: number | null
  error: string | null
  created_at: string
}

export interface OcrPreflightFamily {
  detected_mime: string
  candidates: number
  total_bytes: number
  p50_bytes: number
  p95_bytes: number
  max_bytes: number
}

/** Read-only projection of historical OCR work. Never enqueues anything. */
export interface OcrPreflight {
  snapshot_before: string
  engine_version: string | null
  candidates: number
  already_completed: number
  already_skipped: number
  already_failed: number
  already_unsupported: number
  awaiting_metadata: number
  total_candidate_bytes: number
  families: OcrPreflightFamily[]
}

export type BackfillState =
  | 'draft'
  | 'paused'
  | 'running'
  | 'draining'
  | 'completed'
  | 'cancelled'
  | 'failed'

export interface BackfillCampaign {
  id: string
  kind: 'email' | 'ocr' | 'attachment_text' | 'attachment_ocr'
  state: BackfillState
  snapshot_before: string | null
  cursor_created_at: string | null
  cursor_node_id: string | null
  batch_size: number
  max_queued: number
  max_running: number
  resource_class: string
  foreground_fairness: number
  candidate_count: number
  enqueued_count: number
  completed_count: number
  failed_count: number
  skipped_count: number
  created_by_version: string
  last_error: string | null
  created_at: string
  updated_at: string
  started_at: string | null
  paused_at: string | null
  completed_at: string | null
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

/**
 * Queue depth is reported per origin because a paused historical campaign and a
 * stalled inbox look identical once summed.
 */
export interface EmailCounts {
  foreground_pending: number
  foreground_running: number
  backfill_pending: number
  backfill_running: number
  completed: number
  failed: number
  skipped: number
  unsupported: number
  remaining: number
  indexed: number
}

export interface EmailStatus {
  counts: EmailCounts
}

export interface EmailSearchHit {
  node_id: string
  subject: string | null
  sent_at: string | null
  /** Server-marked snippet. Matches are wrapped in `[[` and `]]`. */
  snippet: string
  attachment_count: number
  duplicate_count: number
  thread_count: number
  score: number
  /** Primary `From` address; absent when the message declared none. */
  from_address: string | null
  from_display_name: string | null
  labels: string[]
}

export interface EmailSearchResponse {
  results: EmailSearchHit[]
  next_cursor: string | null
}

export interface EmailFacetBucket {
  value: string
  count: number
}

export interface EmailFacets {
  labels: EmailFacetBucket[]
  correspondents: EmailFacetBucket[]
  years: EmailFacetBucket[]
}

export type EmailAddressRole =
  'from' | 'sender' | 'reply_to' | 'to' | 'cc' | 'bcc'

export interface EmailAddress {
  role: EmailAddressRole
  display_name: string | null
  address: string
}

export interface EmailHeader {
  name: string
  value: string
}

export interface EmailAttachment {
  part_path: string
  filename: string | null
  media_type: string
  disposition: string | null
  content_id: string | null
  decoded_size: number | null
  is_inline: boolean
  is_message: boolean
  extraction_status: string
}

export interface EmailMessage {
  node_id: string
  status: string
  parser_version: string
  message_id: string | null
  in_reply_to: string | null
  references: string[]
  subject: string | null
  sent_at: string | null
  received_at: string | null
  body_text: string
  /** Already sanitized server-side; still rendered inside a sandboxed frame. */
  body_html: string | null
  blocked_remote_count: number
  blocked_hosts: string[]
  preview_text: string
  thread_group_id: string | null
  duplicate_group_id: string | null
  provider_thread_id: string | null
  labels: string[]
  addresses: EmailAddress[]
  attachments: EmailAttachment[]
  warnings: string[]
  raw_headers: EmailHeader[] | null
}

/** Structured criteria mirrored into the URL so a search can be bookmarked. */
export interface EmailSearchCriteria {
  q: string
  from: string[]
  participant: string[]
  label: string[]
  after: string
  before: string
  hasAttachment: boolean | null
  includeTrashed: boolean
  includeDuplicates: boolean
}
