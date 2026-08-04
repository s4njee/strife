#!/usr/bin/env python3
"""Generate the API route-to-test ledger and reject unassigned routes."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "crates/api/src"
OUTPUT = ROOT / "docs/development/api-route-coverage.md"

# Deliberately explicit: adding a route must also name the test that owns it.
ROUTE_TESTS = {
    "POST /api/admin/reprocess": "ocr_api.rs; error_contract.rs",
    "GET /api/admin/email/versions": "email_api.rs",
    "GET /api/admin/email/repair": "email_api.rs",
    "POST /api/admin/email/repair/campaign": "email_api.rs",
    "GET /api/backfills": "backfills_api.rs",
    "POST /api/backfills": "backfills_api.rs",
    "GET /api/backfills/events": "backfills_api.rs",
    "GET /api/backfills/{id}": "backfills_api.rs",
    "GET /api/backfills/{id}/metrics": "backfills_api.rs",
    "GET /api/backfills/{id}/canary-results": "backfills_api.rs",
    "POST /api/backfills/{id}/canary-results": "backfills_api.rs",
    "POST /api/backfills/{id}/canary-stage": "backfills_api.rs",
    "POST /api/backfills/{id}/prepare": "backfills_api.rs",
    "POST /api/backfills/{id}/actions": "backfills_api.rs",
    "GET /api/email/status": "email_api.rs",
    "GET /api/email/events": "email_api.rs",
    "POST /api/email/reprocess": "email_api.rs",
    "GET /api/email/search": "email_api.rs",
    "GET /api/email/facets": "email_api.rs",
    "GET /api/email/messages/{node_id}": "email_api.rs",
    "GET /api/email/messages/{node_id}/parts/{part_path}": "email_parts_api.rs",
    "GET /api/files/{id}": "files_api.rs; error_contract.rs",
    "GET /api/files/{id}/metadata": "files_api.rs; error_contract.rs",
    "GET /api/files/{id}/streams": "files_api.rs; error_contract.rs",
    "GET /api/files/{id}/text": "ocr_api.rs; error_contract.rs",
    "GET /api/files/{id}/preview-native": "files_api.rs",
    "GET /api/files/{id}/preview": "files_api.rs; error_contract.rs",
    "GET /api/files/{id}/thumbnail": "files_api.rs; error_contract.rs",
    "GET /api/files/{id}/download": "files_api.rs; error_contract.rs",
    "POST /api/folders": "folders_api.rs",
    "PATCH /api/folders/move": "folders_api.rs",
    "PATCH /api/folders/{id}": "folders_api.rs",
    "GET /api/folders/{id}/children": "folders_api.rs",
    "GET /api/folders/{id}/ancestors": "folders_api.rs",
    "GET /health": "health_api.rs; health.rs unit tests",
    "GET /ready": "health_api.rs; health.rs unit tests",
    "GET /api/health": "health_api.rs; health.rs unit tests",
    "GET /api/ready": "health_api.rs; health.rs unit tests",
    "GET /api/import-sources": "imports_api.rs",
    "PATCH /api/import-sources/{id}": "imports_api.rs",
    "POST /api/import-sources/{id}/scan": "imports_api.rs",
    "GET /api/import-sources/{id}/entries": "imports_api.rs",
    "GET /api/import-sources/{id}/events": "imports_api.rs",
    "POST /api/import-sources/{id}/entries/{entry_id}/retry": "imports_api.rs",
    "GET /api/jobs/{id}": "jobs_api.rs; error_contract.rs",
    "GET /api/jobs": "jobs_api.rs; error_contract.rs",
    "GET /api/metadata/status": "metadata_api.rs",
    "GET /api/metadata/recent": "metadata_api.rs",
    "GET /api/metadata/events": "metadata_api.rs",
    "GET /api/trash": "nodes_api.rs",
    "GET /api/favorites": "error_contract.rs",
    "POST /api/nodes/{id}/trash": "nodes_api.rs",
    "POST /api/nodes/trash": "nodes_api.rs",
    "POST /api/nodes/{id}/restore": "nodes_api.rs",
    "DELETE /api/nodes/{id}/permanent": "nodes_api.rs; edge_cases.rs",
    "PUT /api/nodes/{id}/favorite": "nodes_api.rs",
    "DELETE /api/nodes/{id}/favorite": "nodes_api.rs",
    "GET /api/ocr/status": "ocr_api.rs",
    "GET /api/ocr/tree": "ocr_api.rs",
    "GET /api/ocr/preflight": "ocr_api.rs",
    "GET /api/ocr/events": "ocr_api.rs",
    "GET /api/search": "ocr_api.rs; error_contract.rs",
    "GET /api/storage/usage": "error_contract.rs",
    "POST /api/uploads": "uploads_api.rs; folder_upload_api.rs",
    "GET /api/uploads": "uploads_api.rs",
    "PATCH /api/uploads/{id}": "uploads_api.rs; folder_upload_api.rs",
    "GET /api/uploads/{id}": "uploads_api.rs",
    "DELETE /api/uploads/{id}": "uploads_api.rs",
    "POST /api/uploads/{id}/finalize": "uploads_api.rs; folder_upload_api.rs",
}


def registered_routes() -> dict[str, str]:
    routes: dict[str, str] = {}
    for source in sorted(SOURCE.glob("*.rs")):
        text = source.read_text()
        text = text.split("#[cfg(test)]", 1)[0]
        offset = 0
        while (start := text.find(".route(", offset)) >= 0:
            cursor = start + len(".route(")
            depth = 1
            while cursor < len(text) and depth:
                depth += (text[cursor] == "(") - (text[cursor] == ")")
                cursor += 1
            call = text[start:cursor]
            offset = cursor
            path_match = re.search(r'\.route\(\s*"([^"]+)"', call)
            if not path_match:
                continue
            path = path_match.group(1)
            methods = set(re.findall(r"\b(get|post|patch|put|delete)\s*\(", call))
            line = text.count("\n", 0, start) + 1
            for method in methods:
                routes[f"{method.upper()} {path}"] = f"{source.name}:{line}"
    return routes


def main() -> int:
    routes = registered_routes()
    missing = sorted(set(routes) - set(ROUTE_TESTS))
    stale = sorted(set(ROUTE_TESTS) - set(routes))
    if missing or stale:
        if missing:
            print("Routes missing test ownership:", *missing, sep="\n  ", file=sys.stderr)
        if stale:
            print("Stale route ownership entries:", *stale, sep="\n  ", file=sys.stderr)
        return 1

    rows = [
        "# API route coverage",
        "",
        "Generated by `scripts/api-route-coverage.py`. Do not edit by hand. The generator",
        "fails when a registered method/path has no explicit test owner.",
        "",
        "| Route | Registration | Test coverage |",
        "|---|---|---|",
    ]
    for route in sorted(routes, key=lambda value: (value.split(" ", 1)[1], value)):
        tests = ", ".join(f"`{item.strip()}`" for item in ROUTE_TESTS[route].split(";"))
        rows.append(f"| `{route}` | `{routes[route]}` | {tests} |")
    rows.extend(["", f"**Total:** {len(routes)} registered method/path pairs.", ""])
    OUTPUT.write_text("\n".join(rows))
    print(f"wrote {OUTPUT.relative_to(ROOT)} ({len(routes)} routes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
