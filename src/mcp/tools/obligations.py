"""Live professional obligations view — client work, hitlist, deadlines, recurring."""

import re
from datetime import UTC, datetime
from pathlib import Path

from security import get_vault_path
from tools.metadata import parse_frontmatter

# Cap task scan so the tool stays cheap on large vaults.
_MAX_TASKS_SCAN = 500

# Tasks folder and exclusions.
_TASKS_DIRNAME = "tasks"

# Hitlist file at vault root.
_HITLIST_FILENAME = "Hitlist.md"

# Recurring context file.
_RECURRING_RELPATH = "+meta/contexts/Recurring.md"

# Show deadlines within this window.
_DEADLINE_WINDOW_DAYS = 14

# Recurring cadence → minimum days between completions before "due".
_RECURRING_CADENCES = {
    "daily": 1,
    "weekly": 7,
    "biweekly": 14,
    "monthly": 30,
    "quarterly": 90,
    "yearly": 365,
    "annual": 365,
    "annually": 365,
}

# Match trailing "(id)" in a task filename stem to recover id when missing from frontmatter.
_FILENAME_ID_RE = re.compile(r"\(([a-z0-9][a-z0-9-]*)\)\s*$", re.IGNORECASE)

# Match "Last completed: YYYY-MM-DD" lines in Recurring.md.
_LAST_COMPLETED_RE = re.compile(r"(?i)^\s*Last completed:\s*(\d{4}-\d{2}-\d{2})\s*$", re.MULTILINE)

TOOLS = [
    {
        "name": "obligations",
        "description": (
            "Get a live view of professional obligations: active tasks grouped by "
            "client, today's Hitlist focus (Big 3), upcoming deadlines, and "
            "recurring commitments that are due or overdue. Use this when the "
            "user asks 'what's on my plate?', 'what do I owe clients?', 'what's "
            "due?', or when planning a day/week. Cite specific task ids and "
            "recurring item names — the output is designed to be scannable. "
            "Complements vault_map's live_context (which is task-state focused) "
            "by surfacing client-facing commitments and external deadlines."
        ),
        "input_schema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
]


def _parse_date(value) -> datetime | None:
    """Parse a YYYY-MM-DD (or similar) date from a frontmatter value."""
    if value is None:
        return None
    text = str(value).strip()
    if not text:
        return None
    # Strip wiki-link brackets if present.
    text = text.strip("[]")
    match = re.search(r"(\d{4}-\d{2}-\d{2})", text)
    if not match:
        return None
    try:
        return datetime.strptime(match.group(1), "%Y-%m-%d").replace(tzinfo=UTC)
    except ValueError:
        return None


def _days_between(earlier: datetime, later: datetime) -> int:
    """Whole-day delta between two UTC datetimes (later - earlier)."""
    if earlier.tzinfo is None:
        earlier = earlier.replace(tzinfo=UTC)
    if later.tzinfo is None:
        later = later.replace(tzinfo=UTC)
    delta = later - earlier
    return delta.days


def _scan_active_tasks() -> list[dict]:
    """Scan vault/tasks/*.md for non-Done tasks, returning frontmatter dicts."""
    vault = get_vault_path()
    tasks_dir = vault / _TASKS_DIRNAME
    if not tasks_dir.is_dir():
        return []

    results: list[dict] = []
    for path in sorted(tasks_dir.iterdir()):
        if len(results) >= _MAX_TASKS_SCAN:
            break
        if path.is_dir() or path.name.startswith(".") or path.suffix.lower() != ".md":
            continue
        try:
            content = path.read_text(encoding="utf-8")
        except OSError:
            continue
        frontmatter, _ = parse_frontmatter(content)
        if not frontmatter:
            continue
        if frontmatter.get("status") == "Done":
            continue
        task_id = frontmatter.get("id")
        if not task_id:
            stem_match = _FILENAME_ID_RE.search(path.stem)
            task_id = stem_match.group(1) if stem_match else path.stem
        results.append(
            {
                "id": task_id,
                "title": frontmatter.get("title") or path.stem,
                "client": frontmatter.get("client"),
                "status": frontmatter.get("status"),
                "priority": frontmatter.get("priority"),
                "due": _parse_date(frontmatter.get("due") or frontmatter.get("deadline")),
                "path": str(path.relative_to(vault)),
            }
        )
    return results


def _group_by_client(tasks: list[dict]) -> dict[str, list[dict]]:
    """Group active tasks by client. Tasks with no client go under 'Unassigned'."""
    groups: dict[str, list[dict]] = {}
    for task in tasks:
        client = task.get("client") or "Unassigned"
        groups.setdefault(client, []).append(task)
    return groups


def _read_hitlist_big_3(vault: Path) -> list[str]:
    """Read the 'Big 3' section from Hitlist.md. Returns list of raw task lines."""
    path = vault / _HITLIST_FILENAME
    if not path.is_file():
        return []
    try:
        content = path.read_text(encoding="utf-8")
    except OSError:
        return []

    # Find the Big 3 section: everything from "## The Big 3" until the next "## ".
    section_match = re.search(
        r"^##\s+The Big 3\s*$(.*?)^##\s+",
        content,
        re.MULTILINE | re.DOTALL,
    )
    if not section_match:
        return []
    section = section_match.group(1)

    # Pull out unchecked checkbox lines.
    items: list[str] = []
    for line in section.splitlines():
        line = line.strip()
        if line.startswith("- [ ]") or line.startswith("* [ ]"):
            text = line[5:].strip()
            if text:
                items.append(text)
    return items


def _strip_wikilinks(text: str) -> str:
    """Replace [[target|label]] or [[target]] with label or target."""

    def replace(match: re.Match) -> str:
        inner = match.group(1)
        if "|" in inner:
            return inner.split("|", 1)[1]
        return inner

    return re.sub(r"\[\[([^\]]+)\]\]", replace, text)


def _parse_recurring(vault: Path, now: datetime) -> list[dict]:
    """Parse Recurring.md and return items with days_since_completed and cadence.

    Each item: {name, cadence, cadence_days, last_completed, days_since, overdue_by}
    Only items with a parseable "Last completed" date are returned.
    """
    path = vault / _RECURRING_RELPATH
    if not path.is_file():
        return []
    try:
        content = path.read_text(encoding="utf-8")
    except OSError:
        return []

    # Walk lines, tracking current H2 (cadence) and H3 (item name).
    items: list[dict] = []
    current_section = ""

    lines = content.splitlines()

    # Build a list of (line_idx, section, item_name) anchors.
    anchors: list[tuple[int, str, str]] = []
    for idx, line in enumerate(lines):
        h2 = re.match(r"^##\s+(.+?)\s*$", line)
        if h2:
            current_section = h2.group(1).strip()
            continue
        h3 = re.match(r"^###\s+(.+?)\s*$", line)
        if h3:
            anchors.append((idx, current_section, h3.group(1).strip()))

    # For each anchor, find its body span (until next anchor or EOF) and extract date.
    for i, (start, section, item_name) in enumerate(anchors):
        end = anchors[i + 1][0] if i + 1 < len(anchors) else len(lines)
        body = "\n".join(lines[start:end])
        match = _LAST_COMPLETED_RE.search(body)
        if not match:
            continue
        try:
            last_done = datetime.strptime(match.group(1), "%Y-%m-%d").replace(tzinfo=UTC)
        except ValueError:
            continue

        cadence_key = section.lower().split()[0] if section else ""
        cadence_days = _RECURRING_CADENCES.get(cadence_key)
        days_since = _days_between(last_done, now)
        overdue_by = None
        if cadence_days is not None:
            overdue_by = days_since - cadence_days

        # Strip trailing parenthetical (e.g. "Send Invoices (1st-5th)" → "Send Invoices")
        clean_name = re.sub(r"\s*\([^)]*\)\s*$", "", item_name).strip()

        items.append(
            {
                "name": clean_name,
                "cadence": section or "Unknown",
                "cadence_days": cadence_days,
                "last_completed": last_done,
                "days_since": days_since,
                "overdue_by": overdue_by,
            }
        )
    return items


def _strip_task_title_suffix(title: str, task_id: str) -> str:
    """Strip trailing ' (id)' from a task title since we print the id separately."""
    if not title or not task_id:
        return title or ""
    suffix = f" ({task_id})"
    if title.endswith(suffix):
        return title[: -len(suffix)]
    return title


def _format_client_groups(groups: dict[str, list[dict]]) -> list[str]:
    """Render clients with active task counts and ids, sorted by count desc."""
    if not groups:
        return []
    lines = ["Clients with active work:"]
    sorted_groups = sorted(
        groups.items(),
        key=lambda kv: (-len(kv[1]), kv[0].lower()),
    )
    for client, tasks in sorted_groups:
        status_counts: dict[str, int] = {}
        for task in tasks:
            status = task.get("status") or "Unknown"
            status_counts[status] = status_counts.get(status, 0) + 1
        status_str = ", ".join(
            f"{count} {status.lower()}" for status, count in sorted(status_counts.items())
        )
        ids = ", ".join(t["id"] for t in tasks)
        lines.append(f"  {client}: {len(tasks)} active ({status_str}) — {ids}")
    return lines


def _format_big_3(items: list[str]) -> list[str]:
    """Render hitlist Big 3 items."""
    if not items:
        return []
    lines = ["", "Hitlist (Big 3):"]
    for item in items:
        lines.append(f"  - {_strip_wikilinks(item)}")
    return lines


def _format_deadlines(tasks: list[dict], now: datetime) -> list[str]:
    """Render tasks with upcoming deadlines within the window."""
    upcoming = []
    for task in tasks:
        due = task.get("due")
        if not due:
            continue
        days_until = _days_between(now, due)
        if days_until > _DEADLINE_WINDOW_DAYS:
            continue
        upcoming.append((days_until, task))

    if not upcoming:
        return []

    upcoming.sort(key=lambda pair: pair[0])
    lines = ["", f"Deadlines (within {_DEADLINE_WINDOW_DAYS}d):"]
    for days_until, task in upcoming:
        title = _strip_task_title_suffix(task.get("title") or "", task.get("id") or "")
        client = task.get("client") or "-"
        if days_until < 0:
            when = f"{-days_until}d overdue"
        elif days_until == 0:
            when = "due today"
        else:
            when = f"due in {days_until}d"
        lines.append(f"  - {task['id']}: {title} [{client}] {when}")
    return lines


def _format_recurring(items: list[dict]) -> list[str]:
    """Render recurring items that are due or overdue, overdue-first."""
    surfaced = []
    for item in items:
        overdue_by = item.get("overdue_by")
        if overdue_by is None:
            continue
        if overdue_by < 0:
            continue
        surfaced.append(item)

    if not surfaced:
        return []

    surfaced.sort(key=lambda item: item["overdue_by"], reverse=True)
    lines = ["", "Recurring (due or overdue):"]
    for item in surfaced:
        overdue_by = item["overdue_by"]
        cadence = item["cadence"]
        days_since = item["days_since"]
        if overdue_by == 0:
            when = f"due now ({cadence.lower()}, {days_since}d since last)"
        else:
            when = f"overdue by {overdue_by}d ({cadence.lower()}, {days_since}d since last)"
        lines.append(f"  - {item['name']} — {when}")
    return lines


def _obligations(args: dict) -> dict:
    """Assemble the obligations view."""
    now = datetime.now(UTC)
    vault = get_vault_path()

    try:
        tasks = _scan_active_tasks()
    except OSError as e:
        return {"content": "", "error": f"Failed to scan tasks: {e}"}

    groups = _group_by_client(tasks)
    big_3 = _read_hitlist_big_3(vault)
    recurring = _parse_recurring(vault, now)

    lines: list[str] = [f"Obligations view (as of {now.strftime('%Y-%m-%d')}):", ""]
    client_lines = _format_client_groups(groups)
    deadline_lines = _format_deadlines(tasks, now)
    big_3_lines = _format_big_3(big_3)
    recurring_lines = _format_recurring(recurring)

    # Client section has no leading blank (it's first).
    lines.extend(client_lines)
    lines.extend(big_3_lines)
    lines.extend(deadline_lines)
    lines.extend(recurring_lines)

    if not (client_lines or big_3_lines or deadline_lines or recurring_lines):
        lines.append("No active obligations found.")

    return {"content": "\n".join(lines), "error": None}


HANDLERS = {
    "obligations": _obligations,
}
