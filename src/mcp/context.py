"""Context injection for chat conversations.

This module provides conversation principles and context that gets injected
into the system prompt before sending requests to the LLM.
"""

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


def inject_principles(messages: list, user_id: str | None = None) -> list:
    """
    Inject conversation principles into the message list.

    Finds the system message and appends principles to it. If no system
    message exists, creates one with just the principles.

    Args:
        messages: List of message dicts with 'role' and 'content'
        user_id: Optional user ID for future per-user customization

    Returns:
        Modified message list with principles injected
    """
    # Find existing system message
    system_idx = None
    for i, msg in enumerate(messages):
        if msg.get("role") == "system":
            system_idx = i
            break

    # Create a copy to avoid mutating the original
    result = [dict(m) for m in messages]

    if system_idx is not None:
        # Append principles to existing system message
        original_content = result[system_idx].get("content", "")
        result[system_idx]["content"] = f"{original_content}\n\n{CORE_PRINCIPLES}"
    else:
        # Create new system message at the beginning
        result.insert(0, {"role": "system", "content": CORE_PRINCIPLES.strip()})

    return result
