"""Context injection for chat conversations.

This module provides conversation principles and context that gets injected
into the system prompt before sending requests to the LLM.
"""

from pathlib import Path

from security import get_vault_path

CORE_PRINCIPLES = """
CONVERSATION PRINCIPLES:

Scoping: When a message contains multiple topics or questions, silently
note them using conversation_scope, then address one at a time. Don't
announce the structure - just naturally work through them without losing
track.

Pacing: Ask one question per message. Wait for the response before asking
the next. Acknowledge what the user said before moving on.

Ma: Not every response needs to advance an agenda. Sometimes notice
something without acting on it. Sometimes appreciate a moment before
rushing forward. Read the user's energy - if they're reflective, be
reflective. If task-focused, stay efficient.
"""

DEFAULT_PHILOSOPHY = """# Conversation Philosophy

## Scoping

When you detect 2+ topics in a message:
1. Call conversation_scope to register them
2. Address the first naturally
3. After resolving, transition: "Now about [next topic]..."
4. If user redirects, follow their lead

## Pacing

- One question per message
- Acknowledge before asking next
- Don't stack questions

## Ma

Read the room:
- User finished something big -> pause, appreciate
- User is venting -> listen, don't solve immediately
- User is task-focused -> stay efficient
- Silence is okay

## Anti-patterns

Avoid:
- Question dumps
- Rushing past emotional moments
- "Great! Awesome!" empty acknowledgments
- Forgetting topics that were raised
"""


def load_philosophy() -> str | None:
    """
    Load .lu/philosophy.md from vault, creating with defaults if missing.

    Returns:
        Philosophy content, or None on error
    """
    vault = get_vault_path()
    lu_dir = vault / ".lu"
    philosophy_file = lu_dir / "philosophy.md"

    try:
        if philosophy_file.exists():
            return philosophy_file.read_text()

        lu_dir.mkdir(parents=True, exist_ok=True)
        philosophy_file.write_text(DEFAULT_PHILOSOPHY)
        return DEFAULT_PHILOSOPHY

    except Exception:
        return None


def inject_principles(messages: list, user_id: str | None = None) -> list:
    """
    Inject conversation principles and philosophy into the system message.

    Includes:
    - Core principles (always)
    - Philosophy file content (if available)

    Args:
        messages: Original message list
        user_id: Optional user ID for loading topics (used in next task)

    Returns a new list (does not mutate input).
    """
    # Load philosophy (may be None)
    philosophy = load_philosophy()

    # Build full context
    context_parts = [CORE_PRINCIPLES.strip()]
    if philosophy:
        context_parts.append(f"\n## Philosophy Context\n\n{philosophy}")

    full_context = "\n".join(context_parts)

    result = []
    found_system = False

    for msg in messages:
        if msg.get("role") == "system" and not found_system:
            found_system = True
            result.append({
                "role": "system",
                "content": msg.get("content", "") + "\n\n" + full_context
            })
        else:
            result.append(msg)

    if not found_system:
        result.insert(0, {"role": "system", "content": full_context})

    return result
