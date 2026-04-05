"""Vault index tools — search pre-chunked content and view vault structure."""

import json
import re
from datetime import UTC, datetime
from pathlib import Path
from zoneinfo import ZoneInfo

from security import get_vault_path

# Cap vault walks so freshness checks never become expensive on huge vaults.
_MAX_FRESHNESS_SCAN = 20000

TOOLS = [
    {
        "name": "current_time",
        "description": (
            "Get the current date and time in UTC and the user's local timezone. "
            "Use this when you need to know what time it is, calculate relative times "
            "(e.g., '5 minutes from now'), or set up schedules."
        ),
        "input_schema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
    {
        "name": "search_index",
        "description": (
            "Search the vault index for matching chunks. Returns relevant sections "
            "with source files and heading context. More targeted than full-text "
            "search — finds pre-chunked sections of notes with optional AI-generated "
            "summaries. Response begins with the index's last-updated timestamp and "
            "age so you can cite freshness; call vault_map for staleness details."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (text or regex pattern)",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10)",
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "vault_map",
        "description": (
            "Get a high-level overview of the vault: structure, folder breakdown, "
            "index status, and statistics. Use this to understand the vault's layout "
            "before diving into specific files. Response includes index freshness — "
            "when the index was last rebuilt and how many vault files have been "
            "modified since — so you can tell the user how trustworthy search "
            "results are."
        ),
        "input_schema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
]


def _get_index_dir() -> Path:
    """Return the vault index directory."""
    return get_vault_path() / ".ludolph" / "index"


def _get_chunks_dir() -> Path:
    """Return the chunks directory."""
    return _get_index_dir() / "chunks"


def _parse_iso(ts: str) -> datetime | None:
    """Parse an ISO-8601 timestamp, tolerating a trailing 'Z'."""
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except (ValueError, TypeError):
        return None


def _format_age(ts: str) -> str:
    """Return a short human age like '3h ago' for an ISO timestamp."""
    dt = _parse_iso(ts)
    if dt is None:
        return "unknown"
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    delta = datetime.now(UTC) - dt
    seconds = int(delta.total_seconds())
    if seconds < 0:
        return "just now"
    if seconds < 60:
        return f"{seconds}s ago"
    if seconds < 3600:
        return f"{seconds // 60}m ago"
    if seconds < 86400:
        return f"{seconds // 3600}h ago"
    return f"{seconds // 86400}d ago"


def _read_manifest() -> dict | None:
    """Load the index manifest, returning None if missing or malformed."""
    manifest_path = _get_index_dir() / "manifest.json"
    if not manifest_path.exists():
        return None
    try:
        return json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def _iter_vault_markdown() -> list[Path]:
    """Walk the vault for .md files, skipping dot-dirs and node_modules."""
    vault = get_vault_path()
    results: list[Path] = []
    for path in vault.rglob("*.md"):
        try:
            rel_parts = path.relative_to(vault).parts
        except ValueError:
            continue
        skip = False
        for part in rel_parts[:-1]:
            if part == "node_modules" or (part.startswith(".") and len(part) > 1):
                skip = True
                break
        if skip:
            continue
        results.append(path)
        if len(results) >= _MAX_FRESHNESS_SCAN:
            break
    return results


def _compute_freshness(manifest: dict) -> dict:
    """Compute staleness by comparing vault mtimes to manifest.last_indexed.

    Returns a dict with:
      last_indexed: ISO timestamp (or "unknown")
      last_indexed_age: human-readable age (e.g. "3h ago")
      stale_file_count: vault files modified since last_indexed
      total_vault_files: total markdown files in vault
      total_indexed_files: file_count from manifest
      scan_truncated: True if the vault walk hit the scan cap
    """
    last_indexed = manifest.get("last_indexed", "unknown")
    indexed_dt = _parse_iso(last_indexed)
    result = {
        "last_indexed": last_indexed,
        "last_indexed_age": _format_age(last_indexed),
        "stale_file_count": None,
        "total_vault_files": None,
        "total_indexed_files": manifest.get("file_count", 0),
        "scan_truncated": False,
    }
    if indexed_dt is None:
        return result

    if indexed_dt.tzinfo is None:
        indexed_dt = indexed_dt.replace(tzinfo=UTC)
    indexed_ts = indexed_dt.timestamp()

    stale = 0
    total = 0
    files = _iter_vault_markdown()
    for path in files:
        try:
            mtime = path.stat().st_mtime
        except OSError:
            continue
        total += 1
        if mtime > indexed_ts:
            stale += 1

    result["stale_file_count"] = stale
    result["total_vault_files"] = total
    result["scan_truncated"] = len(files) >= _MAX_FRESHNESS_SCAN
    return result


def _search_index(args: dict) -> dict:
    """Search pre-chunked vault content with ranking."""
    query = args.get("query", "")
    if not query:
        return {"content": "", "error": "Query required"}

    max_results = args.get("max_results", 10)
    chunks_dir = _get_chunks_dir()

    if not chunks_dir.exists():
        return {
            "content": (
                "Index not found. Run `lu index` to build it.\n"
                "If index is at Quick tier, upgrade with `lu index --tier standard`."
            ),
            "error": None,
        }

    # Build regex, fall back to literal
    try:
        pattern = re.compile(query, re.IGNORECASE)
    except re.error:
        pattern = re.compile(re.escape(query), re.IGNORECASE)

    scored = []

    for json_path in chunks_dir.rglob("*.json"):
        try:
            chunk_file = json.loads(json_path.read_text(encoding="utf-8"))
        except Exception:
            continue

        source = chunk_file.get("source", "")

        for chunk in chunk_file.get("chunks", []):
            score = 0.0
            content = chunk.get("content", "")
            summary = chunk.get("summary")
            char_count = chunk.get("char_count", len(content))
            position = chunk.get("position", 1)

            # Summary match scores higher
            if summary and pattern.search(summary):
                score += 2.0

            # Content match
            if pattern.search(content):
                score += 1.0

            if score > 0:
                # Signal density bonus
                density = 1.0 / max(char_count / 100.0, 1.0)
                score += density * 0.5

                # Position 0 boost
                if position == 0:
                    score += 0.3

                scored.append(
                    {
                        "source": source,
                        "heading_path": chunk.get("heading_path", []),
                        "content": content,
                        "summary": summary,
                        "score": score,
                    }
                )

    # Include a freshness header so Lu can cite index age with every search.
    manifest = _read_manifest()
    freshness_header = ""
    if manifest is not None:
        last_indexed = manifest.get("last_indexed", "unknown")
        freshness_header = (
            f"Index last updated: {last_indexed} ({_format_age(last_indexed)}). "
            "Call vault_map for staleness details.\n"
        )

    if not scored:
        return {
            "content": f"{freshness_header}No matches found for '{query}' in the index.",
            "error": None,
        }

    scored.sort(key=lambda x: x["score"], reverse=True)
    scored = scored[:max_results]

    lines = [freshness_header + f"Found {len(scored)} matches for '{query}':\n"]
    for item in scored:
        heading = ""
        if item["heading_path"]:
            heading = f" > {' > '.join(item['heading_path'])}"
        lines.append(f"--- {item['source']}{heading} ---")
        if item["summary"]:
            lines.append(f"Summary: {item['summary']}")
        preview = item["content"][:300]
        if len(item["content"]) > 300:
            preview += "..."
        lines.append(preview)
        lines.append("")

    return {"content": "\n".join(lines), "error": None}


def _vault_map(args: dict) -> dict:
    """Return vault index overview."""
    index_dir = _get_index_dir()
    manifest_path = index_dir / "manifest.json"

    if not manifest_path.exists():
        return {
            "content": (
                "No vault index found. Run `lu index` to build one.\n"
                "Available tiers:\n"
                "- quick: file map only (free, seconds)\n"
                "- standard: chunked index (free, minutes)\n"
                "- deep: chunked + AI summaries (costs API tokens, hours)"
            ),
            "error": None,
        }

    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except Exception as e:
        return {"content": "", "error": f"Failed to read manifest: {e}"}

    freshness = _compute_freshness(manifest)

    lines = [
        f"Vault: {manifest.get('vault_path', 'unknown')}",
        f"Index tier: {manifest.get('tier', 'unknown')}",
        f"Indexed files: {freshness['total_indexed_files']}",
        f"Chunks: {manifest.get('chunk_count', 0)}",
        f"Last indexed: {freshness['last_indexed']} ({freshness['last_indexed_age']})",
    ]

    if freshness["stale_file_count"] is not None:
        stale = freshness["stale_file_count"]
        total_vault = freshness["total_vault_files"]
        indexed = freshness["total_indexed_files"]
        delta = total_vault - indexed
        if delta > 0:
            delta_note = f"; vault has {delta} more .md file(s) than the index"
        elif delta < 0:
            delta_note = f"; index has {-delta} file(s) no longer in the vault"
        else:
            delta_note = ""
        lines.append(
            f"Index freshness: {stale} of {total_vault} vault file(s) "
            f"modified since last index{delta_note}"
        )
        if freshness["scan_truncated"]:
            lines.append(f"(freshness scan truncated at {_MAX_FRESHNESS_SCAN} files)")

    folders = manifest.get("folders", {})
    if folders:
        lines.append("\nFolders:")
        sorted_folders = sorted(
            folders.items(), key=lambda x: x[1].get("file_count", 0), reverse=True
        )
        for folder, stats in sorted_folders:
            lines.append(
                f"  {folder}: {stats.get('file_count', 0)} files, {stats.get('chunk_count', 0)} chunks"
            )

    return {"content": "\n".join(lines), "error": None}


def _current_time(args: dict) -> dict:
    """Return current time in UTC and detected local timezone."""
    utc_now = datetime.now(UTC)

    # Try to detect local timezone from system
    local_tz_name = "UTC"
    try:
        # macOS
        link = Path("/etc/localtime").resolve()
        path_str = str(link)
        if "/zoneinfo/" in path_str:
            local_tz_name = path_str.split("/zoneinfo/", 1)[1]
    except Exception:
        pass

    if local_tz_name == "UTC":
        try:
            # Linux
            tz_file = Path("/etc/timezone")
            if tz_file.exists():
                local_tz_name = tz_file.read_text().strip()
        except Exception:
            pass

    try:
        local_tz = ZoneInfo(local_tz_name)
        local_now = utc_now.astimezone(local_tz)
    except Exception:
        local_now = utc_now
        local_tz_name = "UTC"

    lines = [
        f"UTC: {utc_now.strftime('%Y-%m-%d %H:%M:%S')}",
        f"Local ({local_tz_name}): {local_now.strftime('%Y-%m-%d %H:%M:%S')}",
        f"Day: {local_now.strftime('%A')}",
        f"UTC offset: {local_now.strftime('%z')}",
    ]

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "current_time": _current_time,
    "search_index": _search_index,
    "vault_map": _vault_map,
}
