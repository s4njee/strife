# Strife — Deferred Questions for v2 and Later

These questions are intentionally outside v1. They are not commitments: revisit them after v1 behavior is stable and measured. Moving a question back into active scope requires updating [`README.md`](README.md) and adding it to [`questions.md`](questions.md).

## Authentication and Accounts

- Should v2 use a local username/password, passkeys, a trusted reverse proxy, or an external identity provider?
- If passwords are supported, what recovery mechanism works without depending on email infrastructure?
- How long should browser sessions remain valid?
- Should users be able to inspect and revoke individual sessions?
- Should a new device generate a visible security event?
- Should failed sign-ins use rate limiting, delay, lockout, or a combination?
- Should Strife remain single-user, or add household/general multi-user accounts?
- If multiple users are added, how should ownership, per-user quotas, shared folders, and administrator privileges work?

## Network Exposure and Deployment

- Should Strife remain LAN-only, move behind a VPN such as Tailscale, or become reachable from the public internet?
- If publicly reachable, what domain, TLS termination, and reverse proxy should be supported?
- What should be added beyond the existing LAN Compose stack (registry publishing, TLS/auth productization, multi-host)?
- When should ARM64 and x86-64 release images be built and published?
- Should native binaries be supported in addition to containers?
- When is Kubernetes support justified, and what should its storage and ingress model be?
- Should remote S3-compatible storage, replication, or tiered cold storage be supported?

## Upload and Conflict Robustness

- What additional behavior is needed for unreliable or remote connections beyond v1 resumable chunking?
- Should upload sessions support parallel chunks, adaptive chunk sizes, integrity proofs per chunk, or bandwidth controls?
- Should clients be able to resume uploads across devices?
- Should v2 offer conflict resolution instead of atomically rejecting a move with duplicate names?
- If so, should conflicts support overwrite, auto-rename, selective resolution, or file versions?
- Should content checksums ever be used for physical deduplication rather than integrity only?

## Watched-Folder Import

- Should Strife extend `IMPORT_WATCH_ROOT` into multiple configurable source-to-destination mappings instead of the current single source-to-root mapping?
- Should imports run on a schedule or continuous watcher instead of only through an explicit manual scan?
- If automatic scanning is added, what quiet period or consecutive-scan threshold proves that a producer has finished writing a file?
- Should successful imports optionally copy, archive, or retain their source instead of always moving it into managed storage?
- Should external sources outside the single import inbox be monitored for later changes or disappearance?

## Implementation Profiling Follow-ups

- Profile OCR on Orion and replace the provisional page-count, pixel-count,
  execution-time, memory, and stored-output limits with measured values.
- Revisit whether engine or language version changes should offer an automatic
  gradual reprocessing policy after the manual bounded re-trigger path has
  production usage data.

## Search and Organization

- Which fields should global search cover: filename, folder path, kind, metadata, document text, OCR text, and future tags?
- Should search default to the current folder or the entire drive?
- Should search include trash, and if so, only through an explicit filter?
- Should filename search use fuzzy matching or strict prefix/substring matching?
- Which filters are needed: kind, MIME, date, size, duration, dimensions, favorite, processing status, or GPS availability?
- Should content matches show contextual snippets and document page numbers?
- Should PostgreSQL full-text search and `pg_trgm` remain sufficient, or is a dedicated search service justified by measured scale?
- Are user-defined tags or labels needed?
- Should metadata such as title, description, tags, or capture date become editable?
- Are saved searches or smart folders useful?
- Should “Recent” mean uploaded, modified, viewed, or a combination?
- Should Strife track last-viewed activity, and how long should it retain that history?
- Should semantic/vector search augment filename and full-text search?

## Sharing

- Should public link sharing support files, folders, or both?
- Should a shared folder allow browsing, ZIP download, or uploads?
- Should shares allow preview and download as separate permissions?
- What should the default and maximum share expiry be?
- Should share passwords be available or required?
- Should public access be rate-limited, bandwidth-limited, or capped by download count?
- Should the owner see access counts, timestamps, IP addresses, or a privacy-preserving subset?
- What happens to a share when its target is moved, trashed, restored, permanently deleted, or versioned?
- Should sensitive metadata such as GPS be hidden or stripped from shared previews/downloads?
- Should share tokens be rotatable independently of share settings?

## Command Palette and Additional UI

- Which basic actions belong in the first command-palette version?
- How should the palette relate to the filesystem-like command bar?
- Should palette commands be discoverable, searchable, and keyboard-remappable?
- Should Strife add mobile and tablet layouts?
- Should a grid/gallery view be reconsidered for photo-heavy folders?
- Should a light/dark theme follow the operating system, remember a manual choice, or both?

## Media and Preview Enhancements

- Should unsupported video codecs be transcoded for browser playback?
- If so, which output codecs, containers, resolutions, and quality levels should be generated?
- Should transcoding happen at ingestion or on demand?
- Should audio waveforms or embedded cover art be generated?
- Should document attachments be extractable?
- Should archive contents be listed without extraction?
- If archives are inspected, what entry-count, recursion, and reported-uncompressed-size limits apply?
- Should generated previews have configurable cache retention or eviction?

## Security and Privacy Hardening

- Should antivirus scanning be included, and which engine should be used?
- Should files remain inaccessible until antivirus or required processing completes?
- What process or container isolation should protect ffprobe, ExifTool, Tika, OCR, RAW decoding, and office conversion?
- Should processing workers be denied network access?
- Should particularly risky formats be disabled or processed only on demand?
- Which safe file types may render inline, and which must always download as attachments?
- Is host full-disk encryption sufficient, or is application-level encryption at rest required?
- Which mutation, access, sharing, and security audit events should be retained, and for how long?
- What CSRF and origin policy is required once authentication or public exposure exists?
- Which API actions need rate limits?
- Should secrets and keys support rotation without downtime?

## Backup, Restore, and Maintenance

- Where should local backups be stored?
- Is an encrypted off-site backup required?
- What recovery point objective is acceptable—how much recent data may be lost?
- What recovery time objective is acceptable—how long may restoration take?
- How frequently should PostgreSQL and managed file storage be backed up?
- Can storage snapshots be coordinated with a consistent PostgreSQL backup?
- Should generated artifacts be backed up or rebuilt after restore?
- How should backup success, failure, and restore drills be reported?
- What is the documented bare-metal restore process?
- Should application and extractor updates be automatic, notification-based, or manual?
- How much planned downtime is acceptable for migrations and upgrades?
- Should an administration page expose disk health, queue depth, failed jobs, extractor versions, and reprocessing controls?
- Should immutable snapshots or ransomware-resistant retention be added?

## Development, Packaging, and Quality Policy

- Which package managers and minimum Rust and Node versions should a packaged release standardize?
- Should frontend/backend API types be generated from OpenAPI, generated from Rust definitions, or maintained separately?
- Which CI service should run builds and tests?
- Which sample formats require permanent regression fixtures?
- Should ordinary integration tests mock external tools while a dedicated suite runs real tools?
- What performance targets should apply to folder listing, upload finalization, metadata availability, preview generation, and future search?
- Approximately how many files should a library contain before performance is considered unacceptable?
- Should accessibility formally target WCAG 2.2 AA?
- Which developer and deployment platforms need official support?
- Should the shipped Compose/systemd deployment gain published registry images or native OS packages?

## Future Integrations and Product Directions

- Would desktop or mobile synchronization clients be valuable?
- Should WebDAV expose Strife to external file managers?
- Should watched-folder import expand into bidirectional synchronization?
- Should photo features include albums, timelines, maps, face grouping, or object recognition?
- Should automatic media tagging use local machine-learning models?
- Should an API or plugin system allow custom extractors and automations?
- Should file version history be introduced, and what retention policy should it use?
- Should Strife support immutable user snapshots?
- Should remote storage replication or cold-storage tiers be transparent to users?
