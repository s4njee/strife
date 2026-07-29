# Handoff: Strife — Cloud Storage Files Browser (black theme)

## Overview
A minimalist, technical cloud-storage files browser ("Strife", Nextcloud-like). The approved direction is the **true-black workspace** (option `3a` in the design file): sidebar navigation + storage meter, terminal-style command bar, dense metadata table with multi-select, share popover, and a status footer.

## About the Design Files
The files in this bundle are **design references created in HTML** — prototypes showing intended look and behavior, not production code to copy directly. The task is to **recreate this design in the target codebase's existing environment** (React, Vue, etc.) using its established patterns and libraries. If no environment exists yet, choose the most appropriate framework and implement the design there.

`Files Browser.dc.html` contains five artboards. **Implement `#3a` (the 1440×900 true-black build).** `#2a` is the same layout in an earlier softer-dark palette; `#1a`/`#1b`/`#1c` are light/dark exploration drafts — reference only.

## Fidelity
**High-fidelity.** Colors, typography, spacing, and copy are final. Recreate pixel-perfectly using the codebase's existing component library where equivalents exist. The mockup is **static**: the interactions below are specified but not implemented in the HTML.

## Design Tokens

Accent (electric blue):
- Accent: `oklch(0.7 0.2 258)` — buttons, active states, meters, links, sort/selection indicators
- Accent soft (fills/badges): `oklch(0.7 0.2 258 / 0.16)`
- **Text/icons on accent are near-black (`#000`)**, not white — the accent is bright

Surfaces:
- App background: `#000`
- Panel / raised surface (sidebar, command bar): `#0b0b0c`
- Popover surface: `#101012`
- Input surface (search, kbd chips): `#131315`; chip/kbd fill `#212125`
- Selected row: `#17181c`; active nav item: `#1a1a1d`; folder glyph fill `#1c1c1f`

Borders:
- Structural: `#1e1e21` · row separators: `#161618` · inputs/chips: `#26262a` · popover: `#2c2c31`

Text ramp:
- Primary `#f2f2f3` · secondary `#c9c9cd` · tertiary `#9c9ca2` · muted mono `#85858c` · faint `#5f5f66` · ghost/disabled `#3c3c41`

File-type glyph colors (on black, at 15% opacity background — `col + '26'`): PDF `#d06a52`, Figma `#8d68d6`, Spreadsheet `#4da36c`, Markdown `#6e7783`, Image `#d09b4d`, Document `#4d80d6`, Archive `#9c8d4d`. Folder glyph: fill `#1c1c1f`, text `#9c9ca2`.

Typography:
- UI sans: `Helvetica, 'Inter', system-ui, sans-serif`
- Mono (metadata, sizes, dates, labels, command bar): `'JetBrains Mono', monospace` (Google Fonts, 400/500)
- Scale: 15px/600 page title · 13px/500 file names, breadcrumb · 12.5px/500 nav & buttons · 12px/400 owner, popover rows · 11–11.5px mono metadata · 10–10.5px/500 mono labels (uppercase, letter-spacing .4px) · 9px mono badges

Spacing & shape:
- Sidebar 228px · topbar 56px · table header 34px · rows 42px · footer 32px
- Horizontal page padding 20px · row grid gap 12px
- Radii: 10px canvas/popover · 8px bars/cards · 7px buttons/inputs/selection bar · 5–6px chips/nav/glyphs · 4px checkboxes/badges
- Popover shadow: `0 12px 40px rgba(0,0,0,.8)`

## Screen: Files Browser (3a)

Layout: `grid-template-columns: 228px 1fr`, canvas 1440×900.

### Sidebar (bg panel, right border structural, padding 16px 12px)
- Logo: 20px accent rounded square (radius 5) + "Strife" 13px/600, letter-spacing -0.2px
- Primary button "＋ Upload": full-width, accent bg, **black text**, padding 9px 12px, radius 7
- Nav (12.5px/500, padding 7px 10px, radius 6): All files (182), Recent (20), Starred (7), Shared (14), Trash (3). Counts right-aligned 10px mono faint. Active item bg `#1a1a1d`, text primary
- Bottom-pinned storage card (1px structural border, radius 8, padding 10px): "STORAGE / 61%" 10px mono row · 4px bar (track `#1c1c1f`, fill accent) · "122 GB of 200 GB"

### Topbar (56px, bottom border)
- Breadcrumb "Home / Workspace" — parent muted mono color, separator ghost, current primary
- Search: 260px, bg input, border inputs, radius 8, "⌕ Search files" placeholder faint, trailing `⌘K` chip (10px mono, bg `#212125`)
- Avatar: 28px circle, accent bg, **black** initials "YU" 11px/600

### Command bar (margin 14px 20px 0; bg panel, structural border, radius 8, padding 10px 14px, 12px mono)
`~/workspace` (accent) · `$` (ghost) · `ls —sort modified —all` (muted); right: "14 objects · 355 MB" (faint). Terminal-flavored breadcrumb — path segments navigate.

### Filter row (padding 12px 20px)
Chips (10px mono/500, 2px 6px, radius 4): inactive = transparent bg, muted text, 1px `#26262a` border; active ("All") = accent-soft bg, accent text, transparent border. Chips: All · Folders · Documents · Media · Archives.
Right: **selection action bar**, visible when ≥1 selected — accent-soft bg, radius 7, padding 5px 10px: "2 SELECTED" 10.5px mono accent · 1px `#2c2c31` divider · Share / Move / Star / Trash 11.5px/500 secondary.

### Table
Grid columns `28px 1fr 130px 90px 110px 90px`, gap 12px (checkbox, Name, Owner, Kind, Size, Modified+actions).
- Header 34px: 10px mono uppercase faint; "MODIFIED ↓" right-aligned marks active sort
- Checkbox 14px, radius 4, border ghost; checked = accent bg/border with black ✓
- Row 42px, bottom border row-separator. Selected: bg `#17181c` + `inset 2px 0 0 accent` left rule
- Name cell: 22px type glyph (rounded square, 8px mono code — FLD/PDF/FIG/XLS/MD/IMG/DOC/ZIP) + name 13px/500 ellipsized · optional "SHARED" badge (9px mono, accent on accent-soft) · folders show item count ("24 items") 11px faint
- Owner 12px sans tertiary · Kind 10px mono uppercase faint · Size 11px mono muted ("—" for folders)
- Trailing group right-aligned: date 11px mono muted · star (★ accent when starred, ☆ ghost) · overflow "⋯" faint

Content (14 rows, rows 4 and 8 shown selected): Design (folder, starred, 24 items) · Q3 Financials (folder, 8 items) · Client Handoffs (folder, shared, 11 items) · roadmap-2026.pdf (starred, shared, selected) · brand-system.fig · metrics.xlsx · launch-notes.md · hero-render.png (shared, selected) · contract-v3.docx · archive-2025.zip · onboarding-flow.fig · retro-notes.md (starred) · billing-export.xlsx · team-photo.png.

### Share popover (absolute; top 214px, right 36px, width 280px)
Bg `#101012`, border `#2c2c31`, radius 10, padding 14px. Header: 6px accent dot + "SHARE — roadmap-2026.pdf" 10.5px mono accent. Link row: bg `#000` inset, border inputs, radius 7 — `strife.io/s/9fA2-roadmap` 11px mono ellipsized + "COPY" chip (10px mono, bg `#212125`). Permission rows, each with a top structural border: "Anyone with link / View only" · "a.reyes@studio.co / Can edit" · "m.okafor@studio.co / Can edit" — 12px sans secondary left, 10px mono faint right.

### Status footer (32px, top border, 10px mono faint)
Left: "CONNECTED · strife-eu-2" · "SYNC OK · 14:32:07". Right: "122 GB / 200 GB" · "⌘K COMMANDS" (accent).

## Interactions & Behavior (to implement)
- Row click selects (single); checkbox toggles multi-select; selection bar appears at ≥1 selected and reflects the count
- Row hover: subtle bg lift (selected bg at ~50%); "⋯" opens a per-file menu (rename, move, download, share, trash)
- Star toggles favorite in place; Starred nav filters to starred
- Column header click toggles sort (arrow indicator); Kind chips filter the table
- Share (row action or selection bar) opens the popover anchored to the row; COPY copies the link with brief confirmation
- Double-click a folder navigates in; breadcrumb and command-bar path segments navigate up
- Upload opens a file picker; drag-drop onto the table is the natural extension (design a full-surface drop state from these tokens)
- ⌘K opens a command palette (referenced in search + footer, not designed — follow the mono/black vocabulary)
- Transitions minimal and fast: 120–150ms ease-out on hover/selection; no decorative animation

## State Management
- `files[]` (name, type, kind, size, owner, modifiedAt, starred, shared, itemCount) · `selection: Set<id>` · `sort {col, dir}` · `activeFilter` · `activeNav` · `sharePopover {fileId} | null` · `storage {used, quota}`
- Data: file listing per path · share-link CRUD · star toggle · trash/move operations

## Assets
No image assets. File-type glyphs are pure CSS (colored rounded squares with mono letter codes) — build as a small component, or swap in the codebase's icon set keeping the muted color mapping. Fonts from Google Fonts (JetBrains Mono; Inter as the Helvetica fallback).

## Files
- `Files Browser.dc.html` — all artboards; implement `#3a`. `#2a` (softer dark) and `#1a`/`#1b`/`#1c` are exploration history
- `support.js` — preview runtime for the prototype file; ignore for implementation
