# ADR 0007: Command Bar Commands and Parsing

- **Status:** Accepted
- **Date:** 2026-07-29

## Context

Strife's desktop UI includes a filesystem-like command bar for power users. Milestone 6 needs a fixed v1 command list and parsing rules so the bar maps cleanly onto existing folder, trash, and preview operations without becoming a general-purpose shell.

## Decision

### v1 commands

| Command | Behavior |
|---|---|
| `pwd` | Print the absolute virtual path of the current folder |
| `ls [path]` | List children of the current folder or the resolved path (name, kind) |
| `cd <path>` | Navigate to a folder (UI route change) |
| `mkdir <name>` | Create a folder in the current directory (name may be a relative path of one segment) |
| `mv <source> <dest>` | Move or rename within the virtual hierarchy |
| `rm <target>` | Move the target into trash |
| `restore <target>` | Restore a trashed item (by trash listing name or absolute path when unique) |
| `open <target>` | Open a folder (navigate) or preview/download a file |

No other commands ship in v1. Pipes, redirection, variables, subshells, wildcards, and chaining are out of scope.

### Parsing rules

- **Tokenization:** split on unquoted whitespace. Support double quotes (`"My Photos"`), single quotes (`'My Photos'`), and backslash escapes for spaces and quote characters (`My\ Photos`).
- **Paths:** absolute paths start with `/` and are rooted at the virtual root folder. Relative paths resolve from the current folder. `.` and `..` are supported. Path segments are exact display names (case-sensitive).
- **Flags:** only `rm` accepts `-f` / `--force` to skip the destructive-action confirmation. Unknown flags are errors.
- **Autocomplete:** Tab completes the current path segment by querying active children of the resolved parent folder. Ambiguous prefixes leave the longest common match; no match is a no-op.
- **History:** Up/Down cycles the last 50 successful or attempted command lines stored in `localStorage`.
- **Destructive confirmation:** `rm` without `--force` prompts once (inline confirm / second Enter). `restore` does not require confirmation.
- **Errors:** single-line, human-readable messages under the bar (e.g. `No such folder: /photos/2025`, `Name conflict: report.pdf`). No shell-style exit codes in the UI.

## Alternatives Considered

- Ship a smaller subset (`cd`, `ls`, `pwd` only) — rejected because the product plan already lists trash, mkdir, and move as first-class UI operations the bar should mirror.
- Full shell grammar (globs, pipes, scripting) — rejected as out of scope for a single-user file manager.
- Separate command palette with free-text actions — deferred to v2 per the product plan.

## Consequences

- Command-bar work (Story 6.10) implements exactly these eight commands against existing APIs.
- Path resolution is a pure virtual-hierarchy concern and does not touch storage keys or host filesystem paths.
- Confirmation UX for `rm` is required unless the force flag is present.
