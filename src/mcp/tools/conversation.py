"""
Conversation state management tools.

Helps Lu track multi-topic conversations with explicit scoping,
pacing, and progress tracking.
"""

import json
from datetime import datetime, timezone, timedelta
from pathlib import Path

from security import get_vault_path

# Conversation state lives in .lu/conversations/
CONV_DIR = ".lu/conversations"


def _get_state_path(conversation_id: str) -> Path:
    """Get path to conversation state file."""
    vault = get_vault_path()
    conv_dir = vault / CONV_DIR
    conv_dir.mkdir(parents=True, exist_ok=True)
    return conv_dir / f"{conversation_id}.json"


def _load_state(conversation_id: str) -> dict:
    """Load conversation state, creating if needed."""
    path = _get_state_path(conversation_id)
    if path.exists():
        return json.loads(path.read_text())
    return {
        "id": conversation_id,
        "created": datetime.now(timezone.utc).isoformat(),
        "topics": [],
        "resolved": [],
        "current": None,
        "notes": [],
    }


def _save_state(conversation_id: str, state: dict) -> None:
    """Save conversation state."""
    path = _get_state_path(conversation_id)
    state["updated"] = datetime.now(timezone.utc).isoformat()
    path.write_text(json.dumps(state, indent=2))


def expire_stale_topics(conversation_id: str, max_age_hours: int = 24) -> int:
    """
    Move topics older than max_age_hours to 'stale' status.

    Args:
        conversation_id: Conversation identifier
        max_age_hours: Hours after which topics become stale (default 24)

    Returns:
        Number of topics moved to stale
    """
    path = _get_state_path(conversation_id)
    if not path.exists():
        return 0

    try:
        state = json.loads(path.read_text())
    except Exception:
        return 0

    updated_str = state.get("updated")
    if not updated_str:
        return 0

    try:
        updated = datetime.fromisoformat(updated_str.replace("Z", "+00:00"))
    except Exception:
        return 0

    cutoff = datetime.now(timezone.utc) - timedelta(hours=max_age_hours)

    if updated >= cutoff:
        return 0

    topics = state.get("topics", [])
    if not topics:
        return 0

    stale = state.get("stale", [])
    stale.extend(topics)

    state["stale"] = stale
    state["topics"] = []
    state["current"] = None
    state["updated"] = datetime.now(timezone.utc).isoformat()

    path.write_text(json.dumps(state, indent=2))

    return len(topics)


def conversation_scope(
    conversation_id: str,
    action: str,
    topics: list[str] | None = None,
    topic: str | None = None,
    note: str | None = None,
) -> str:
    """
    Manage conversation scope and topic tracking.

    Use this to explicitly track what topics are on the table
    in a multi-threaded conversation.

    Args:
        conversation_id: Unique ID for this conversation (e.g., telegram user ID)
        action: One of:
            - "add": Add topics to discuss
            - "resolve": Mark a topic as addressed
            - "focus": Set the current focus topic
            - "list": Show open and resolved topics
            - "note": Add a note about the conversation
            - "clear": Reset conversation state
        topics: List of topics to add (for "add" action)
        topic: Single topic (for "resolve" or "focus" actions)
        note: Note to add (for "note" action)

    Returns:
        Status message with current conversation state
    """
    state = _load_state(conversation_id)

    if action == "add":
        if not topics:
            return "Error: 'topics' required for add action"
        for t in topics:
            if t not in state["topics"] and t not in state["resolved"]:
                state["topics"].append(t)
        _save_state(conversation_id, state)
        return f"Added {len(topics)} topic(s). Open: {state['topics']}"

    elif action == "resolve":
        if not topic:
            return "Error: 'topic' required for resolve action"
        if topic in state["topics"]:
            state["topics"].remove(topic)
            state["resolved"].append(topic)
            if state["current"] == topic:
                state["current"] = state["topics"][0] if state["topics"] else None
            _save_state(conversation_id, state)
            return f"Resolved: {topic}. Remaining: {state['topics']}"
        return f"Topic not found in open topics: {topic}"

    elif action == "focus":
        if not topic:
            return "Error: 'topic' required for focus action"
        if topic in state["topics"]:
            state["current"] = topic
            _save_state(conversation_id, state)
            return f"Now focusing on: {topic}"
        return f"Topic not in open topics: {topic}"

    elif action == "list":
        open_topics = state["topics"]
        resolved = state["resolved"]
        current = state["current"]

        lines = []
        if current:
            lines.append(f"Current focus: {current}")
        if open_topics:
            lines.append(f"Open topics ({len(open_topics)}): {', '.join(open_topics)}")
        if resolved:
            lines.append(f"Resolved ({len(resolved)}): {', '.join(resolved)}")
        if not lines:
            lines.append("No topics tracked yet.")

        return "\n".join(lines)

    elif action == "note":
        if not note:
            return "Error: 'note' required for note action"
        state["notes"].append({
            "time": datetime.now(timezone.utc).isoformat(),
            "note": note,
        })
        _save_state(conversation_id, state)
        return f"Note added. Total notes: {len(state['notes'])}"

    elif action == "clear":
        state = {
            "id": conversation_id,
            "created": datetime.now(timezone.utc).isoformat(),
            "topics": [],
            "resolved": [],
            "current": None,
            "notes": [],
        }
        _save_state(conversation_id, state)
        return "Conversation state cleared."

    else:
        return f"Unknown action: {action}. Use: add, resolve, focus, list, note, clear"


# Tool definition for registration
TOOLS = [
    {
        "name": "conversation_scope",
        "description": """Track topics in a multi-threaded conversation.

Use this when a conversation has multiple topics to address:
1. Call with action="add" to register topics
2. Call with action="focus" to set current topic
3. Call with action="resolve" when a topic is addressed
4. Call with action="list" to see progress

This helps maintain conversational pacing - address one topic at a time
while keeping track of what's still open.""",
        "inputSchema": {
            "type": "object",
            "properties": {
                "conversation_id": {
                    "type": "string",
                    "description": "Unique conversation identifier (e.g., user ID)",
                },
                "action": {
                    "type": "string",
                    "enum": ["add", "resolve", "focus", "list", "note", "clear"],
                    "description": "Action to perform",
                },
                "topics": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Topics to add (for 'add' action)",
                },
                "topic": {
                    "type": "string",
                    "description": "Single topic (for 'resolve' or 'focus' actions)",
                },
                "note": {
                    "type": "string",
                    "description": "Note to add (for 'note' action)",
                },
            },
            "required": ["conversation_id", "action"],
        },
    },
]


def _handle_conversation_scope(arguments: dict) -> dict:
    """Handler for conversation_scope tool."""
    result = conversation_scope(
        conversation_id=arguments.get("conversation_id", "default"),
        action=arguments.get("action", "list"),
        topics=arguments.get("topics"),
        topic=arguments.get("topic"),
        note=arguments.get("note"),
    )
    return {"content": result}


HANDLERS = {
    "conversation_scope": _handle_conversation_scope,
}
