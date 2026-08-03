# Strife Scrum Epics Index

This directory contains individual Scrum Epic specifications split from [`scrum.md`](../../scrum.md), [`docs/ocr.md`](../ocr.md), and [`docs/email.md`](../email.md).

---

## Epics Overview

| Epic | Source | Description / Goal |
| :--- | :--- | :--- |
| **[Epic 0 — Foundations & Scaffold](epic0.md)** | [`scrum.md`](../../scrum.md) | Runnable dev environment on ARM64 (and cross-compiling for x86-64) with all services healthy and the frontend connected to the API. |
| **[Epic 1 — Persist & Browse Folders](epic1.md)** | [`scrum.md`](../../scrum.md) | Users can create, rename, move, and navigate a persistent folder hierarchy through the UI. Both themes are functional. |
| **[Epic 2 — Resumable Upload & Download](epic2.md)** | [`scrum.md`](../../scrum.md) | Files can be uploaded (resumbly, with chunking), downloaded, and range-streamed. Uploads survive page reloads and service restarts. |
| **[Epic 3 — Watched-Folder Import](epic3.md)** | [`scrum.md`](../../scrum.md) | Files placed in the fixed server-side inbox are manually discovered, validated, and moved into Strife using the same finalization pipeline as uploads. |
| **[Epic 4 — Metadata Extraction](epic4.md)** | [`scrum.md`](../../scrum.md) | Uploaded and imported files have rich metadata extracted asynchronously via durable background jobs, without blocking ingestion. |
| **[Epic 5 — On-Demand Previews](epic5.md)** | [`scrum.md`](../../scrum.md) | Supported file types can be previewed in the browser. Previews are generated on first request, cached, and served efficiently. |
| **[Epic 6 — Complete v1 File Management & UI](epic6.md)** | [`scrum.md`](../../scrum.md) | Trash, restore, permanent deletion, favorites, sorting, filters, command bar, and all remaining UI features are implemented. |
| **[Epic 7 — v1 Stabilization](epic7.md)** | [`scrum.md`](../../scrum.md) | The application is reliable, documented, and tested on the target hardware. All v1 behaviors are verified. |
| **[Epic 8 — Observability & API Error Contract](epic8.md)** | [`scrum.md`](../../scrum.md) | Every API failure is logged with its cause and returns one consistent error shape, and SQL type errors fail the build instead of reaching production. |
| **[Epic 9 — Production Deployment & Process Lifecycle](epic9.md)** | [`scrum.md`](../../scrum.md) | The production deployment that already exists in the working tree is committed, documented, and survives a restart without severing in-flight work. |
| **[Epic 10 — Queue Durability & Configuration Hygiene](epic10.md)** | [`scrum.md`](../../scrum.md) | The job queue stays fast as the library grows, and fixed paths become configuration. |
| **[Epic 11 — Frontend Test Foundation & API Contract](epic11.md)** | [`scrum.md`](../../scrum.md) | The frontend has automated tests for its riskiest logic, and API types stop being maintained by hand on both sides. |
| **[Epic 12 — Structural Maintainability](epic12.md)** | [`scrum.md`](../../scrum.md) | The files that concentrate the most change are split along the seams that v2 work will follow. |
| **[Epic 13 — OCR Decisions & Text Storage Foundation](epic13.md)** | [`docs/ocr.md`](../ocr.md) | The OCR answers in `deferred.md` become recorded decisions, and PostgreSQL gains the text tables and job type that every later story writes into. |
| **[Epic 14 — OCR Engine & Worker Pipeline](epic14.md)** | [`docs/ocr.md`](../ocr.md) | Every supported image and image-only PDF is automatically OCR'd by a bounded, isolated Tesseract process, with page text, language, and confidence persisted. |
| **[Epic 15 — Document Text Search](epic15.md)** | [`docs/ocr.md`](../ocr.md) | Stored OCR and embedded text becomes searchable across the whole drive, which is the stated reason for keeping text in PostgreSQL. |
| **[Epic 16 — OCR Status & Text UI](epic16.md)** | [`docs/ocr.md`](../ocr.md) | OCR is observable and its output readable — a sidebar entry leads to a page with counts, status, and a live console, and extracted text is visible and copyable per file. |
| **[Epic 17 — Email Decisions, Schema & Queue Foundation](epic17.md)** | [`docs/email.md`](../email.md) | Email has a recorded architectural boundary, durable structured storage, a first-class job type, and representative regression fixtures before parsing behavior becomes a compatibility contract. |
| **[Epic 18 — MIME Extraction & Durable Email Projection](epic18.md)** | [`docs/email.md`](../email.md) | Every RFC email is parsed safely and deterministically into structured, replaceable database records while malformed messages remain visible and diagnosable. |
| **[Epic 19 — Email Full-Text Search & Query API](epic19.md)** | [`docs/email.md`](../email.md) | Subject, correspondents, body text, labels, and attachment names become fast, relevant, filterable, and safely highlighted across the archive. |
| **[Epic 20 — Email Navigation, Search & Reader UI](epic20.md)** | [`docs/email.md`](../email.md) | A dedicated Email tab lets users search, filter, inspect, and safely read the archive using Strife's established visual and accessibility language. |
| **[Epic 21 — Attachment Search, Threads & Gmail Context](epic21.md)** | [`docs/email.md`](../email.md) | Attachments and conversation context become searchable and navigable without compromising originals, security, or resource limits. |
| **[Epic 22 — Backfill Operations, Security & Production Readiness](epic22.md)** | [`docs/email.md`](../email.md) | The ten-year archive can be indexed safely, observed in real time, resumed after interruption, upgraded, and validated at production scale. |
