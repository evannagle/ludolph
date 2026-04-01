"""Teach tools — generate audience-aware explanations from Lu's knowledge.

Takes a topic, retrieves relevant chunks from the embedding store,
and generates an explanation tailored to the specified audience.

Audience modes:
- people: Plain language, analogies, no jargon
- coders: Technical with code examples and patterns
- robots: Structured data (JSON), machine-readable
- Custom: Any audience description ("5th graders", "my manager", etc.)
"""

import json
import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

# Audience-specific system prompts
AUDIENCE_PROMPTS = {
    "people": (
        "You are explaining this topic to a general audience. "
        "Use plain language, helpful analogies, and avoid jargon. "
        "Structure your explanation with clear sections. "
        "Assume the reader is intelligent but not familiar with technical details."
    ),
    "coders": (
        "You are explaining this topic to software developers. "
        "Include relevant code examples, architectural patterns, and technical details. "
        "Reference specific tools, libraries, and APIs where appropriate. "
        "Be precise and concise — developers value clarity over friendliness."
    ),
    "robots": (
        "Output a structured JSON document capturing this topic's key concepts. "
        "Include: title, summary, key_concepts (array of {name, description}), "
        "relationships (array of {from, to, type}), and metadata. "
        "No prose — pure structured data. Output valid JSON only."
    ),
}

DEFAULT_AUDIENCE = (
    "You are explaining this topic clearly and thoroughly. "
    "Adapt your language to be accessible while maintaining accuracy."
)


def _build_teach_prompt(topic: str, audience: str, chunks: list[dict]) -> list[dict]:
    """Build the LLM messages for a teach request."""
    # Get audience-specific instructions
    audience_lower = audience.lower()
    if audience_lower in AUDIENCE_PROMPTS:
        audience_instructions = AUDIENCE_PROMPTS[audience_lower]
    else:
        audience_instructions = (
            f"You are explaining this topic for: {audience}. "
            f"Adapt your language, depth, and examples specifically for this audience."
        )

    # Format context from retrieved chunks
    context_parts = []
    for chunk in chunks:
        source = chunk.get("source", "unknown")
        heading = " > ".join(chunk.get("heading_path", [])) if chunk.get("heading_path") else ""
        content = chunk.get("content", "")
        ns = chunk.get("namespace", "vault")

        header = f"[{source}]"
        if heading:
            header += f" {heading}"
        if ns != "vault":
            header += f" ({ns})"

        context_parts.append(f"{header}\n{content}")

    context = "\n\n---\n\n".join(context_parts)

    system = (
        f"{audience_instructions}\n\n"
        "Below is source material from the user's knowledge base. "
        "Synthesize this into a coherent explanation of the topic. "
        "Do not just summarize each source — weave them together into a "
        "unified explanation. Cite sources when making specific claims.\n\n"
        "If the source material doesn't cover the topic well, say so honestly "
        "and explain what you can based on what's available."
    )

    user_msg = f"Topic: {topic}\n\nSource material:\n\n{context}"

    return [
        {"role": "system", "content": system},
        {"role": "user", "content": user_msg},
    ]


def _teach(args: dict[str, Any]) -> dict:
    """Generate an audience-aware explanation of a topic."""
    from tools.embeddings import get_store

    topic = args.get("topic", "").strip()
    if not topic:
        return {"content": "", "error": "Topic is required"}

    audience = args.get("audience", "people")
    limit = args.get("limit", 10)
    namespace = args.get("namespace")

    # Retrieve relevant chunks
    store = get_store()
    chunks = store.search(topic, namespace=namespace, limit=limit)

    if not chunks:
        return {
            "content": "",
            "error": (
                f"No relevant content found for '{topic}'. "
                "Try running rebuild_semantic_index first, or use learn_file/learn_url "
                "to add relevant content."
            ),
        }

    # Build prompt and call LLM
    messages = _build_teach_prompt(topic, audience, chunks)

    try:
        from llm import chat as llm_chat
        result = llm_chat(
            model="claude-sonnet-4-20250514",
            messages=messages,
        )

        content = result.get("content", "")
        if not content:
            # Try extracting from choices format
            choices = result.get("choices", [])
            if choices:
                content = choices[0].get("message", {}).get("content", "")

        if not content:
            return {"content": "", "error": "LLM returned empty response"}

        # Add metadata footer
        sources = list({c["source"] for c in chunks})
        footer = f"\n\n[Sources: {', '.join(sources[:5])}]"
        if len(sources) > 5:
            footer += f" (+{len(sources) - 5} more)"

        return {"content": content + footer}

    except ImportError:
        return {"content": "", "error": "LLM not available in this context"}
    except Exception as e:
        return {"content": "", "error": f"Failed to generate explanation: {e}"}


def _teach_export(args: dict[str, Any]) -> dict:
    """Export a topic as a .ludo package (JSON with privacy tiers)."""
    from tools.embeddings import get_store

    topic = args.get("topic", "").strip()
    if not topic:
        return {"content": "", "error": "Topic is required"}

    tier = args.get("tier", 2)
    if tier not in (1, 2, 3):
        return {"content": "", "error": "Tier must be 1, 2, or 3"}

    namespace = args.get("namespace")
    limit = args.get("limit", 20)

    store = get_store()
    chunks = store.search(topic, namespace=namespace, limit=limit)

    if not chunks:
        return {"content": "", "error": f"No content found for '{topic}'"}

    # Build .ludo package based on privacy tier
    package = {
        "format": "ludo",
        "version": "1.0",
        "topic": topic,
        "tier": tier,
        "sources": list({c["source"] for c in chunks}),
        "chunk_count": len(chunks),
    }

    if tier >= 2:
        # Tier 2: structure (headings, metadata, no full text)
        package["structure"] = [
            {
                "source": c["source"],
                "heading_path": c.get("heading_path", []),
                "char_count": c.get("char_count", 0),
                "namespace": c.get("namespace", "vault"),
            }
            for c in chunks
        ]

    if tier >= 3:
        # Tier 3: full text
        package["chunks"] = [
            {
                "source": c["source"],
                "heading_path": c.get("heading_path", []),
                "content": c.get("content", ""),
                "namespace": c.get("namespace", "vault"),
            }
            for c in chunks
        ]

    output = json.dumps(package, indent=2)
    return {"content": f"Generated .ludo package (tier {tier}, {len(chunks)} chunks):\n\n{output}"}


# --- Tool Definitions ---

TOOLS = [
    {
        "name": "teach",
        "description": (
            "Generate an audience-aware explanation of a topic using Lu's knowledge base. "
            "Retrieves relevant content from vault and learned sources, then synthesizes "
            "an explanation tailored to the specified audience."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "The topic to explain",
                },
                "audience": {
                    "type": "string",
                    "description": (
                        "Target audience: 'people' (plain language), 'coders' (technical), "
                        "'robots' (structured JSON), or any custom description"
                    ),
                },
                "namespace": {
                    "type": "string",
                    "description": "Limit to a specific namespace (vault, learned/files, learned/urls). Omit to search all.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum source chunks to retrieve (default 10)",
                },
            },
            "required": ["topic"],
        },
    },
    {
        "name": "teach_export",
        "description": (
            "Export a topic as a .ludo package with privacy tiers. "
            "Tier 1: embeddings only (metadata). "
            "Tier 2: embeddings + structure (headings, no text). "
            "Tier 3: full text included."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "The topic to export",
                },
                "tier": {
                    "type": "integer",
                    "description": "Privacy tier: 1 (metadata only), 2 (structure), 3 (full text). Default 2.",
                    "enum": [1, 2, 3],
                },
                "namespace": {
                    "type": "string",
                    "description": "Limit to a specific namespace. Omit to include all.",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum chunks to include (default 20)",
                },
            },
            "required": ["topic"],
        },
    },
]

HANDLERS = {
    "teach": _teach,
    "teach_export": _teach_export,
}
