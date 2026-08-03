#!/usr/bin/env bash
#
# Pull a Google Drive tree into Strife's watched inbox and run one import pass.
# Runs on the Strife host itself. Files are copied, never moved; nothing is
# deleted from Google Drive, and Strife removes only the local inbox copy after
# it has durably finalized each file.
#
# Usage:
#   scripts/pull-gdrive.sh [--dry-run] [--stage-only] REMOTE[:PATH] [PREFIX]
#
# Examples:
#   rclone config                          # one-time: create a "gdrive" remote
#   scripts/pull-gdrive.sh gdrive: google-drive
#   scripts/pull-gdrive.sh gdrive:Photos google-photos
#
# Environment:
#   STRIFE_URL             API base URL (default: http://127.0.0.1)
#   STRIFE_IMPORT_ROOT     host inbox path (default: /srv/strife/import)
#   STRIFE_IMPORT_UID      inbox owner UID (default: 10001)
#   STRIFE_IMPORT_GID      inbox owner GID (default: 10001)
#   STRIFE_MANIFEST        local resume manifest (default: XDG state directory)
#   RCLONE_CONFIG          rclone config file (default: XDG config directory)
#   RCLONE_EXPORT_FORMATS  Workspace export formats (default: docx,xlsx,pptx,svg)
#   RCLONE_TRANSFERS       parallel transfers (default: 4)
#   SCAN_POLL_SECONDS      import progress poll interval (default: 15)
#
# Google Workspace files (Docs, Sheets, Slides) store no bytes of their own and
# are exported on download, so they arrive with the extensions listed in
# RCLONE_EXPORT_FORMATS. Shortcuts are skipped so a target is never imported
# twice. Files Strife has already imported are skipped, so the same tree can be
# pulled repeatedly without re-downloading it.
#
# Do not start a scan from Strife's Imports page while this script is copying:
# a half-written file would be visible to that scan. This script deletes any
# leftover rclone partials before starting its own scan.

set -euo pipefail

usage() {
  sed -n '2,37s/^# \{0,1\}//p' "$0" >&2
  exit 2
}

dry_run=0
stage_only=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run) dry_run=1; shift ;;
    --stage-only) stage_only=1; shift ;;
    -h|--help) usage ;;
    --) shift; break ;;
    -*) echo "error: unknown option: $1" >&2; usage ;;
    *) break ;;
  esac
done

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage

remote=$1
case "$remote" in
  *:*) ;;
  *) echo "error: REMOTE must include a colon, such as 'gdrive:' or 'gdrive:Photos'" >&2; exit 1 ;;
esac

prefix=${2:-$(printf '%s' "${remote%%:*}")}
case "$prefix" in
  ''|.|..) echo "error: PREFIX must name one destination folder" >&2; exit 1 ;;
  *[!A-Za-z0-9._~-]*)
    echo "error: PREFIX may contain only letters, numbers, '.', '_', '~', and '-'" >&2
    exit 1
    ;;
esac

strife_url=${STRIFE_URL:-http://127.0.0.1}
import_root=${STRIFE_IMPORT_ROOT:-/srv/strife/import}
import_uid=${STRIFE_IMPORT_UID:-10001}
import_gid=${STRIFE_IMPORT_GID:-10001}
export_formats=${RCLONE_EXPORT_FORMATS:-docx,xlsx,pptx,svg}
transfers=${RCLONE_TRANSFERS:-4}
poll_seconds=${SCAN_POLL_SECONDS:-15}
config_home=${XDG_CONFIG_HOME:-$HOME/.config}
rclone_config=${RCLONE_CONFIG:-$config_home/rclone/rclone.conf}
source_id=00000000-0000-0000-0000-000000000003
dest_dir=${import_root%/}/$prefix

case "$import_root" in
  /*) ;;
  *) echo "error: STRIFE_IMPORT_ROOT must be an absolute path" >&2; exit 1 ;;
esac
case "$import_uid:$import_gid" in
  *[!0-9:]*) echo "error: STRIFE_IMPORT_UID and STRIFE_IMPORT_GID must be numeric" >&2; exit 1 ;;
esac
case "$transfers$poll_seconds" in
  *[!0-9]*) echo "error: RCLONE_TRANSFERS and SCAN_POLL_SECONDS must be numeric" >&2; exit 1 ;;
esac

for command in awk curl flock python3 rclone; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "error: required command is missing: $command" >&2
    [ "$command" = rclone ] && echo "       install it with: sudo apt install rclone" >&2
    exit 1
  }
done

[ -r "$rclone_config" ] || {
  echo "error: no rclone configuration at $rclone_config" >&2
  echo "       run 'rclone config' to create a Google Drive remote first" >&2
  exit 1
}
rclone --config "$rclone_config" listremotes 2>/dev/null \
  | grep -qxF "${remote%%:*}:" || {
  echo "error: rclone remote '${remote%%:*}:' is not configured" >&2
  echo "       configured remotes:" >&2
  rclone --config "$rclone_config" listremotes 2>/dev/null | sed 's/^/         /' >&2
  exit 1
}

state_home=${XDG_STATE_HOME:-$HOME/.local/state}
source_hash=$(printf '%s|%s' "$remote" "$prefix" | cksum | cut -d' ' -f1)
manifest=${STRIFE_MANIFEST:-$state_home/strife/pull-gdrive.$source_hash.manifest}
mkdir -p "$(dirname "$manifest")"
touch "$manifest"

# One pull at a time per source: two concurrent runs would race on the same
# inbox paths and could import a file twice.
exec 9>"$manifest.lock"
flock -n 9 || {
  echo "error: another pull for this source is already running" >&2
  exit 1
}

workdir=$(mktemp -d "${TMPDIR:-/tmp}/strife-gdrive.XXXXXX")
trap 'rm -rf "$workdir"' EXIT

rclone_flags=(
  --config "$rclone_config"
  --drive-export-formats "$export_formats"
  --drive-skip-shortcuts
  --fast-list
)

strife_counts() {
  curl -fsS --max-time 30 "${strife_url%/}/api/import-sources" \
    | python3 -c '
import json, sys
for source in json.load(sys.stdin):
    if source["id"] == sys.argv[1]:
        counts = source["counts"]
        print(counts["discovered"], counts["importing"],
              counts["imported"], counts["failed"])
        break
else:
    raise SystemExit("import source not found")
' "$source_id"
}

refresh_manifest() {
  # The server is authoritative, so resume works even if the local manifest was
  # deleted. Without this, files Strife already imported and removed from the
  # inbox would look absent and be downloaded again.
  curl -fsS --max-time 120 \
    "${strife_url%/}/api/import-sources/$source_id/entries?state=imported" \
    >"$workdir/imported.json" || {
    echo "error: could not reconcile imported paths with Strife" >&2
    exit 1
  }
  python3 -c '
import json, sys
prefix = sys.argv[1] + "/"
with open(sys.argv[2], encoding="utf-8") as source:
    for entry in json.load(source):
        path = entry.get("source_path", "")
        if path.startswith(prefix):
            relative = path[len(prefix):]
            if "\n" in relative:
                raise SystemExit("newline in imported path is unsupported")
            print(relative)
' "$prefix" "$workdir/imported.json" >"$workdir/server-imported"
  sort -u "$manifest" "$workdir/server-imported" >"$workdir/manifest.next"
  mv "$workdir/manifest.next" "$manifest"
}

echo "source:      $remote"
echo "destination: $dest_dir"
echo "manifest:    $manifest"

refresh_manifest

# One listing gives both the paths and their sizes, so the capacity check below
# costs no extra Drive API calls.
rclone "${rclone_flags[@]}" lsf --recursive --files-only \
  --format sp --separator "$(printf '\t')" "$remote" >"$workdir/listing" || {
  echo "error: could not list $remote" >&2
  exit 1
}

awk -F'\t' 'NF < 2 { next } { sub(/^[^\t]*\t/, ""); print }' \
  "$workdir/listing" >"$workdir/all-files"
if grep -q '^$' "$workdir/all-files"; then
  echo "error: the remote listing contained an empty path" >&2
  exit 1
fi

awk -v manifest_file="$manifest" \
  'FILENAME == manifest_file { imported[$0] = 1; next } !($0 in imported)' \
  "$manifest" "$workdir/all-files" >"$workdir/pending-files"

pending_count=$(wc -l <"$workdir/pending-files" | tr -d ' ')
remote_count=$(wc -l <"$workdir/all-files" | tr -d ' ')
pending_bytes=$(awk -F'\t' -v list="$workdir/pending-files" '
  BEGIN { while ((getline line < list) > 0) pending[line] = 1 }
  NF >= 2 {
    size = $1
    path = $0
    sub(/^[^\t]*\t/, "", path)
    if (path in pending) total += size
  }
  END { printf "%d", total + 0 }
' "$workdir/listing")

available_bytes=$(df -PB1 "$import_root" | awk 'NR == 2 { print $4 }')
echo "remote:      $remote_count file(s)"
echo "pending:     $pending_count file(s), $(numfmt --to=iec "$pending_bytes")B"
echo "free space:  $(numfmt --to=iec "$available_bytes")B on $import_root"

if [ "$pending_count" -eq 0 ]; then
  echo "done: every remote file has already been imported"
  exit 0
fi

if [ "$pending_bytes" -gt "$available_bytes" ]; then
  echo "error: pending transfer needs more space than $import_root has free" >&2
  exit 1
fi

if [ "$dry_run" -eq 1 ]; then
  echo "dry run: nothing was downloaded and no scan was started"
  echo "         first 20 pending files:"
  head -20 "$workdir/pending-files" | sed 's/^/           /'
  exit 0
fi

sudo -n install -d -o "$import_uid" -g "$import_gid" -m 750 "$dest_dir"

# A populated manifest means most of the tree is already imported, so naming the
# survivors explicitly is cheaper than re-walking everything. An empty manifest
# means this is a first pull, where a plain recursive copy costs far fewer API
# calls than one lookup per file.
copy_flags=(--transfers "$transfers" --progress --stats-one-line)
if [ -s "$workdir/server-imported" ]; then
  copy_flags+=(--files-from "$workdir/pending-files")
fi

sudo -n rclone "${rclone_flags[@]}" copy "${copy_flags[@]}" "$remote" "$dest_dir"

# An interrupted transfer leaves "<name>.<random>.partial" behind. Those names
# do not begin with a dot, so Strife would treat them as ordinary files and
# import a truncated copy. rclone re-fetches them on the next run.
partials=$(sudo -n find "$dest_dir" -type f -name '*.partial' -print -delete | wc -l | tr -d ' ')
[ "$partials" -gt 0 ] && echo "cleaned:     $partials incomplete download(s)"

sudo -n chown -R "$import_uid:$import_gid" "$dest_dir"
sudo -n chmod 750 "$dest_dir"

if [ "$stage_only" -eq 1 ]; then
  echo "staged: run a scan from Strife's Imports page when ready"
  exit 0
fi

read -r _ _ imported_before _ <<<"$(strife_counts)"

scan_url="${strife_url%/}/api/import-sources/$source_id/scan"
curl -fsS -X POST "$scan_url" >"$workdir/scan.json" 2>"$workdir/scan.err" || {
  echo "error: files were staged, but the durable Strife scan could not be queued" >&2
  sed 's/^/       /' "$workdir/scan.err" >&2
  exit 1
}
scan_job_id=$(python3 -c '
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
job_id = value.get("job_id")
if not isinstance(job_id, str):
    raise SystemExit("scan response did not contain a job_id")
print(job_id)
' "$workdir/scan.json")

echo "scanning:    durable job $scan_job_id queued for $pending_count file(s)"
while :; do
  curl -fsS --max-time 30 "${strife_url%/}/api/jobs/$scan_job_id" \
    >"$workdir/job.json" || {
    echo "error: could not read durable scan job status" >&2
    exit 1
  }
  read -r job_state job_error <<<"$(python3 -c '
import json, shlex, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
print(value.get("status", "unknown"), shlex.quote(value.get("error") or ""))
' "$workdir/job.json")"
  case "$job_state" in
    completed) break ;;
    failed|cancelled)
      echo "error: durable scan job $job_state: $job_error" >&2
      exit 1
      ;;
    pending|leased) ;;
    *) echo "error: unknown durable scan job state: $job_state" >&2; exit 1 ;;
  esac
  sleep "$poll_seconds"
  if counts=$(strife_counts 2>/dev/null); then
    read -r discovered importing imported failed <<<"$counts"
    printf '  imported=%s pending=%s in-flight=%s failed=%s\n' \
      "$imported" "$discovered" "$importing" "$failed"
  fi
done
echo "scan:        durable job $scan_job_id completed"
refresh_manifest

read -r _ _ imported_after failed_after <<<"$(strife_counts)"
gained=$((imported_after - imported_before))
echo "imported:    $gained of $pending_count staged file(s)"

if [ "$failed_after" -gt 0 ]; then
  echo "error: $failed_after import(s) failed; sources remain in the inbox and" >&2
  echo "       appear on Strife's Errors page" >&2
  exit 1
fi

# A scan that reports success while importing nothing means the staged tree was
# not visible to Strife - usually STRIFE_IMPORT_ROOT disagreeing with the watch
# path bind-mounted into the API container.
if [ "$gained" -eq 0 ]; then
  echo "error: the scan completed but imported nothing" >&2
  echo "       confirm $import_root is the host path mounted at /mnt/ext/watch" >&2
  exit 1
fi

if [ "$gained" -lt "$pending_count" ]; then
  echo "warning: $((pending_count - gained)) staged file(s) were not imported;" >&2
  echo "         rerun this script to retry them" >&2
  exit 1
fi

echo "done: Google Drive source staged and imported into $prefix/"
