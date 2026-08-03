#!/usr/bin/env bash
#
# Stage an iCloud export in Strife's watched inbox and run one import pass.
# The source is copied, never moved; Strife removes only the remote staging
# copy after it has durably finalized each file.
#
# Usage:
#   scripts/import-icloud.sh [--dry-run] [--stage-only] SOURCE [PREFIX]
#
# Examples:
#   scripts/import-icloud.sh \
#     "$HOME/Library/Mobile Documents/com~apple~CloudDocs" icloud-drive
#
#   osxphotos export "$HOME/icloud-photos-export" --download-missing \
#     --directory "{created.year}/{created.month}"
#   scripts/import-icloud.sh "$HOME/icloud-photos-export" icloud-photos
#
# Environment:
#   STRIFE_SSH          SSH destination (default: sanjee@orion.local)
#   STRIFE_URL          URL as seen from Orion (default: http://127.0.0.1)
#   STRIFE_IMPORT_ROOT  host inbox path (default: /srv/strife/import)
#   STRIFE_IMPORT_UID   inbox owner UID (default: 10001)
#   STRIFE_IMPORT_GID   inbox owner GID (default: 10001)
#   STRIFE_MANIFEST     local resume manifest (default: XDG state directory)
#   SCAN_POLL_SECONDS   durable scan status interval (default: 15)
#
# Optimize Mac Storage may leave .icloud stubs in CloudDocs. This script
# reports and skips them. Materialize those files first, then rerun; rsync
# will transfer only missing or changed content.

set -euo pipefail

usage() {
  sed -n '2,32s/^# \{0,1\}//p' "$0" >&2
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

source_dir=${1%/}
[ -d "$source_dir" ] || {
  echo "error: source is not a directory: $source_dir" >&2
  exit 1
}
source_dir=$(cd "$source_dir" && pwd -P)

prefix=${2:-$(basename "$source_dir")}
case "$prefix" in
  ''|.|..) echo "error: PREFIX must name one destination folder" >&2; exit 1 ;;
  *[!A-Za-z0-9._~-]*)
    echo "error: PREFIX may contain only letters, numbers, '.', '_', '~', and '-'" >&2
    exit 1
    ;;
esac

strife_ssh=${STRIFE_SSH:-sanjee@orion.local}
strife_url=${STRIFE_URL:-http://127.0.0.1}
import_root=${STRIFE_IMPORT_ROOT:-/srv/strife/import}
import_uid=${STRIFE_IMPORT_UID:-10001}
import_gid=${STRIFE_IMPORT_GID:-10001}
source_id=00000000-0000-0000-0000-000000000003
poll_seconds=${SCAN_POLL_SECONDS:-15}
remote_dir=${import_root%/}/$prefix

case "$import_root" in
  /*) ;;
  *) echo "error: STRIFE_IMPORT_ROOT must be an absolute path" >&2; exit 1 ;;
esac
case "$import_uid:$import_gid" in
  *[!0-9:]*) echo "error: STRIFE_IMPORT_UID and STRIFE_IMPORT_GID must be numeric" >&2; exit 1 ;;
esac
case "$poll_seconds" in
  *[!0-9]*|'') echo "error: SCAN_POLL_SECONDS must be numeric" >&2; exit 1 ;;
esac

for command in awk find python3 rsync shasum ssh; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "error: required command is missing: $command" >&2
    exit 1
  }
done

file_count=$(find "$source_dir" -type f ! -path '*/.*' ! -name '*.icloud' | wc -l | tr -d ' ')
stub_count=$(find "$source_dir" -type f -name '*.icloud' | wc -l | tr -d ' ')
source_hash=$(printf '%s|%s|%s' "$strife_ssh" "$source_dir" "$prefix" | shasum | cut -c1-12)
state_home=${XDG_STATE_HOME:-$HOME/.local/state}
manifest=${STRIFE_MANIFEST:-$state_home/strife/import-icloud.$source_hash.manifest}

echo "source:      $source_dir"
echo "destination: $strife_ssh:$remote_dir"
echo "files:       $file_count materialized"
echo "manifest:    $manifest"
if [ "$stub_count" -gt 0 ]; then
  echo "iCloud:      $stub_count cloud-only stub(s) will be skipped"
fi

if [ "$dry_run" -eq 1 ]; then
  echo "dry run: no remote directory was created, no data was copied, and no scan was started"
  exit 0
fi

printf -v quoted_remote_dir '%q' "$remote_dir"
# The values are deliberately expanded and shell-quoted on the client.
# shellcheck disable=SC2029
ssh "$strife_ssh" \
  "sudo -n install -d -o $import_uid -g $import_gid -m 750 $quoted_remote_dir"

mkdir -p "$(dirname "$manifest")"
touch "$manifest"
workdir=$(mktemp -d "${TMPDIR:-/tmp}/strife-icloud.XXXXXX")
trap 'rm -rf "$workdir"' EXIT

printf -v quoted_entries_url '%q' \
  "${strife_url%/}/api/import-sources/$source_id/entries?state=imported"

refresh_manifest() {
  # The server is authoritative, so resume works even if the local manifest
  # was deleted or this script is run from a different Mac.
  # shellcheck disable=SC2029
  ssh "$strife_ssh" "curl -fsS $quoted_entries_url" >"$workdir/imported.json" || {
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

refresh_manifest

: >"$workdir/all-files"
while IFS= read -r -d '' file; do
  relative=${file#"$source_dir"/}
  case "$relative" in
    *$'\n'*) echo "error: filenames containing newlines are unsupported: $relative" >&2; exit 1 ;;
  esac
  printf '%s\n' "$relative" >>"$workdir/all-files"
done < <(find "$source_dir" -type f ! -path '*/.*' ! -name '*.icloud' -print0)

awk -v manifest_file="$manifest" \
  'FILENAME == manifest_file { imported[$0] = 1; next } !($0 in imported)' \
  "$manifest" "$workdir/all-files" >"$workdir/pending-files"
pending_count=$(wc -l <"$workdir/pending-files" | tr -d ' ')
echo "pending:     $pending_count file(s)"

if [ "$pending_count" -eq 0 ]; then
  echo "done: all materialized files are already imported"
  exit 0
fi

# --delay-updates keeps completed files hidden until the end of the transfer.
# Strife also ignores the hidden partial directory if rsync is interrupted.
rsync \
  --archive \
  --no-owner \
  --no-group \
  --partial \
  --partial-dir=.strife-partial \
  --delay-updates \
  --human-readable \
  --progress \
  --files-from="$workdir/pending-files" \
  --rsync-path='sudo -n rsync' \
  "$source_dir/" "$strife_ssh:$remote_dir/"

# rsync runs as root remotely so it can write to the protected ZFS inbox.
# Hand the narrowly scoped staged tree back to Strife's container UID.
# shellcheck disable=SC2029
ssh "$strife_ssh" \
  "sudo -n chown -R $import_uid:$import_gid $quoted_remote_dir && sudo -n chmod 750 $quoted_remote_dir"

if [ "$stage_only" -eq 1 ]; then
  echo "staged: run a scan from Strife's Imports page when ready"
  exit 0
fi

printf -v quoted_scan_url '%q' \
  "${strife_url%/}/api/import-sources/$source_id/scan"
# shellcheck disable=SC2029
scan_response=$(ssh "$strife_ssh" "curl -fsS -X POST $quoted_scan_url") || {
  echo "error: files were staged, but the Strife import scan failed to start" >&2
  echo "       retry from the Imports page or rerun with --stage-only" >&2
  exit 1
}

echo "scan:        $scan_response"
scan_job_id=$(printf '%s' "$scan_response" | python3 -c '
import json, sys
value = json.load(sys.stdin)
job_id = value.get("job_id")
if not isinstance(job_id, str):
    raise SystemExit("scan response did not contain a job_id")
print(job_id)
')
printf -v quoted_job_url '%q' "${strife_url%/}/api/jobs/$scan_job_id"
echo "scanning:    durable job $scan_job_id will continue if this shell disconnects"
while :; do
  # shellcheck disable=SC2029
  job_response=$(ssh "$strife_ssh" "curl -fsS $quoted_job_url") || {
    echo "error: could not read durable scan job status" >&2
    exit 1
  }
  job_state=$(printf '%s' "$job_response" | python3 -c '
import json, sys
print(json.load(sys.stdin).get("status", "unknown"))
')
  case "$job_state" in
    completed) break ;;
    failed|cancelled)
      echo "error: durable scan job $job_state: $job_response" >&2
      exit 1
      ;;
    pending|leased) sleep "$poll_seconds" ;;
    *) echo "error: unknown durable scan job state: $job_state" >&2; exit 1 ;;
  esac
done
echo "scan:        durable job $scan_job_id completed"
refresh_manifest

echo "done: iCloud source staged and imported into $prefix/"
