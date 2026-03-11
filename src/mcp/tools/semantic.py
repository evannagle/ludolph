"""Semantic memory search using embeddings.

Provides semantic similarity search for the Obsidian vault using
sentence-transformers. The model and index are lazy-loaded to avoid
import-time costs on systems where semantic search isn't needed.

Supports temporal decay to weight recent notes higher in search results.

Note: sentence-transformers is optional. If not installed, these tools
return graceful error messages rather than crashing.
"""

import json
import logging
from datetime import datetime
from pathlib import Path
from typing import Any

from security import get_vault_path

logger = logging.getLogger(__name__)

# Lazy-loaded model instance
_model = None

# Index file location
INDEX_PATH = Path.home() / ".ludolph" / "semantic_index.json"

# Default half-life for temporal decay (days)
DEFAULT_HALF_LIFE_DAYS = 30


def temporal_score(modified_at: str, half_life_days: int = DEFAULT_HALF_LIFE_DAYS) -> float:
    """Calculate temporal decay score based on file modification time.

    Uses exponential decay: score = 0.5 ^ (age_days / half_life_days)

    Args:
        modified_at: ISO 8601 timestamp string
        half_life_days: Days until score halves (default 30)

    Returns:
        Score between 0 and 1, where 1 is most recent
    """
    try:
        modified_dt = datetime.fromisoformat(modified_at)
        age_days = (datetime.now() - modified_dt).days
        return 0.5 ** (age_days / half_life_days)
    except (ValueError, TypeError):
        return 0.5  # Default to half-decay for unparseable dates


def combined_score(
    semantic_sim: float,
    temporal: float,
    recency_weight: float = 0.3,
) -> float:
    """Combine semantic similarity with temporal score.

    Args:
        semantic_sim: Cosine similarity score (0-1)
        temporal: Temporal decay score (0-1)
        recency_weight: How much to weight recency (0.0-1.0)

    Returns:
        Combined score
    """
    return (1 - recency_weight) * semantic_sim + recency_weight * temporal


def _get_model():
    """Lazy load the sentence transformer model.

    Returns the model instance, or None if sentence-transformers
    is not installed.
    """
    global _model
    if _model is None:
        try:
            from sentence_transformers import SentenceTransformer

            _model = SentenceTransformer("all-MiniLM-L6-v2")
            logger.info("Loaded semantic search model: all-MiniLM-L6-v2")
        except ImportError:
            logger.warning("sentence-transformers not installed. Semantic search disabled.")
            return None
    return _model


def _check_numpy():
    """Check if numpy is available."""
    try:
        import numpy as np  # noqa: F401

        return True
    except ImportError:
        return False


TOOLS = [
    {
        "name": "semantic_search",
        "description": "Search vault by meaning using semantic similarity. Finds notes conceptually related to your query, not just keyword matches. Supports temporal decay to boost recent notes. Requires sentence-transformers to be installed.",
        "input_schema": {
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language search query describing what you're looking for",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default 5)",
                },
                "recency_weight": {
                    "type": "number",
                    "description": "How much to weight recency vs semantic similarity (0.0-1.0, default 0.0 for pure semantic search)",
                },
            },
            "required": ["query"],
        },
    },
    {
        "name": "get_related_notes",
        "description": "Find notes semantically related to a given note. Useful for discovering connections between ideas.",
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the reference note (relative to vault root)",
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of related notes to return (default 5)",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "rebuild_semantic_index",
        "description": "Rebuild the semantic search index by scanning all markdown files in the vault. Run this after adding many new notes or if search results seem stale.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]


def _semantic_search(args: dict) -> dict[str, Any]:
    """Search vault using semantic similarity with optional temporal decay."""
    query = args.get("query", "")
    limit = args.get("limit", 5)
    recency_weight = args.get("recency_weight", 0.0)

    # Clamp recency_weight to valid range
    recency_weight = max(0.0, min(1.0, recency_weight))

    if not query:
        return {"content": "", "error": "Query is required"}

    if not _check_numpy():
        return {
            "content": "",
            "error": "numpy not available. Install sentence-transformers: pip install sentence-transformers",
        }

    import numpy as np

    model = _get_model()
    if model is None:
        return {
            "content": "",
            "error": "Semantic search not available. Install sentence-transformers: pip install sentence-transformers",
        }

    if not INDEX_PATH.exists():
        return {
            "content": "",
            "error": "Semantic index not built. Run rebuild_semantic_index first.",
        }

    try:
        with open(INDEX_PATH) as f:
            index = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        return {"content": "", "error": f"Failed to load semantic index: {e}"}

    if not index.get("documents"):
        return {"content": "", "error": "Semantic index is empty. Run rebuild_semantic_index."}

    # Encode query
    query_embedding = model.encode(query)

    # Calculate similarities with optional temporal decay
    results = []
    for entry in index["documents"]:
        embedding = np.array(entry["embedding"])
        # Cosine similarity (embeddings are normalized by default)
        semantic_sim = float(np.dot(query_embedding, embedding))

        # Apply temporal decay if recency_weight > 0 and modified_at exists
        if recency_weight > 0 and "modified_at" in entry:
            temp_score = temporal_score(entry["modified_at"])
            final_score = combined_score(semantic_sim, temp_score, recency_weight)
        else:
            final_score = semantic_sim

        results.append(
            {
                "path": entry["path"],
                "title": entry["title"],
                "score": round(final_score, 3),
                "semantic": round(semantic_sim, 3),
                "excerpt": (
                    entry["excerpt"][:200] + "..."
                    if len(entry["excerpt"]) > 200
                    else entry["excerpt"]
                ),
                "modified_at": entry.get("modified_at"),
            }
        )

    results.sort(key=lambda x: x["score"], reverse=True)
    top_results = results[:limit]

    # Format output
    if not top_results:
        return {"content": "No results found", "error": None}

    lines = [f"Found {len(top_results)} result(s):\n"]
    for r in top_results:
        score_info = f"score: {r['score']}"
        if recency_weight > 0:
            score_info += f" (semantic: {r['semantic']})"
        lines.append(f"- {r['path']} ({score_info})")
        lines.append(f"  Title: {r['title']}")
        lines.append(f"  {r['excerpt']}\n")

    return {"content": "\n".join(lines), "error": None}


def _get_related_notes(args: dict) -> dict[str, Any]:
    """Find notes related to a given note."""
    path = args.get("path", "")
    limit = args.get("limit", 5)

    if not path:
        return {"content": "", "error": "Path is required"}

    if not _check_numpy():
        return {
            "content": "",
            "error": "numpy not available. Install sentence-transformers: pip install sentence-transformers",
        }

    import numpy as np

    model = _get_model()
    if model is None:
        return {
            "content": "",
            "error": "Semantic search not available. Install sentence-transformers: pip install sentence-transformers",
        }

    if not INDEX_PATH.exists():
        return {
            "content": "",
            "error": "Semantic index not built. Run rebuild_semantic_index first.",
        }

    try:
        with open(INDEX_PATH) as f:
            index = json.load(f)
    except (json.JSONDecodeError, OSError) as e:
        return {"content": "", "error": f"Failed to load semantic index: {e}"}

    # Find the reference document
    ref_doc = None
    for entry in index["documents"]:
        if entry["path"] == path:
            ref_doc = entry
            break

    if ref_doc is None:
        return {"content": "", "error": f"Note not found in index: {path}"}

    ref_embedding = np.array(ref_doc["embedding"])

    # Calculate similarities to all other documents
    results = []
    for entry in index["documents"]:
        if entry["path"] == path:
            continue  # Skip self
        embedding = np.array(entry["embedding"])
        similarity = float(np.dot(ref_embedding, embedding))
        results.append(
            {
                "path": entry["path"],
                "title": entry["title"],
                "similarity": round(similarity, 3),
            }
        )

    results.sort(key=lambda x: x["similarity"], reverse=True)
    top_results = results[:limit]

    # Format output
    if not top_results:
        return {"content": "No related notes found", "error": None}

    lines = [f"Notes related to '{ref_doc['title']}':\n"]
    for r in top_results:
        lines.append(f"- {r['path']} (similarity: {r['similarity']})")
        lines.append(f"  Title: {r['title']}")

    return {"content": "\n".join(lines), "error": None}


def _rebuild_semantic_index(args: dict) -> dict[str, Any]:
    """Rebuild the semantic index for the vault.

    Index entries include:
    - path: relative path to file
    - title: extracted title or filename
    - excerpt: first few lines of content
    - embedding: vector embedding for semantic search
    - modified_at: file modification time for temporal decay
    """
    model = _get_model()
    if model is None:
        return {
            "content": "",
            "error": "Semantic search not available. Install sentence-transformers: pip install sentence-transformers",
        }

    vault_path = get_vault_path()
    documents = []
    skipped = 0

    for md_file in vault_path.rglob("*.md"):
        # Skip hidden files and directories
        if any(part.startswith(".") for part in md_file.parts):
            continue

        try:
            content = md_file.read_text(errors="ignore")
        except OSError as e:
            logger.warning("Could not read %s: %s", md_file, e)
            skipped += 1
            continue

        # Skip empty files
        if not content.strip():
            skipped += 1
            continue

        # Get file modification time for temporal decay
        try:
            mtime = md_file.stat().st_mtime
            modified_at = datetime.fromtimestamp(mtime).isoformat()
        except OSError:
            modified_at = datetime.now().isoformat()

        # Extract title (first heading or filename)
        title = md_file.stem
        for line in content.split("\n"):
            if line.startswith("# "):
                title = line[2:].strip()
                break

        # Get excerpt (first non-empty, non-heading lines)
        excerpt_lines = []
        for line in content.split("\n"):
            line = line.strip()
            if line and not line.startswith("#"):
                excerpt_lines.append(line)
                if len(" ".join(excerpt_lines)) > 300:
                    break
        excerpt = " ".join(excerpt_lines)[:500]

        # Compute embedding (limit content for performance)
        # Use first 2000 chars to balance quality and speed
        embedding = model.encode(content[:2000])

        documents.append(
            {
                "path": str(md_file.relative_to(vault_path)),
                "title": title,
                "excerpt": excerpt,
                "embedding": embedding.tolist(),
                "modified_at": modified_at,
            }
        )

    # Ensure index directory exists
    INDEX_PATH.parent.mkdir(parents=True, exist_ok=True)

    # Write index
    with open(INDEX_PATH, "w") as f:
        json.dump({"documents": documents}, f)

    content = f"Indexed {len(documents)} document(s)"
    if skipped:
        content += f" (skipped {skipped} unreadable/empty)"
    content += f"\nIndex saved to: {INDEX_PATH}"

    return {"content": content, "error": None}


HANDLERS = {
    "semantic_search": _semantic_search,
    "get_related_notes": _get_related_notes,
    "rebuild_semantic_index": _rebuild_semantic_index,
}
