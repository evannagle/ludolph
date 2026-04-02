"""Persistent embedding store for chunk-level semantic search.

Stores vector embeddings in SQLite, keyed by chunk source + hash for
incremental updates. When a file's hash changes (detected by the Rust
chunker), only that file's embeddings are regenerated.

Reads chunk JSON files produced by the Rust indexer at
  vault/.ludolph/index/chunks/

This replaces the old semantic_index.json approach with:
- Persistent SQLite storage (survives restarts)
- Incremental updates (only re-embeds changed chunks)
- Chunk-level granularity (not whole-file)
- Namespace support for learned content (files, URLs)
"""

import json
import logging
import sqlite3
import struct
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

DB_PATH = Path.home() / ".ludolph" / "embeddings.db"

# Embedding dimension for all-MiniLM-L6-v2
EMBEDDING_DIM = 384

# Lazy-loaded model
_model = None


def _get_model():
    """Lazy load the sentence transformer model."""
    global _model
    if _model is None:
        try:
            from sentence_transformers import SentenceTransformer
            _model = SentenceTransformer("all-MiniLM-L6-v2")
            logger.info("Loaded embedding model: all-MiniLM-L6-v2")
        except ImportError:
            logger.warning("sentence-transformers not installed.")
            return None
    return _model


def _encode_vector(vec: list[float]) -> bytes:
    """Pack float list into compact binary (4 bytes per float)."""
    return struct.pack(f"{len(vec)}f", *vec)


def _decode_vector(data: bytes) -> list[float]:
    """Unpack binary back to float list."""
    n = len(data) // 4
    return list(struct.unpack(f"{n}f", data))


def _cosine_similarity(a: list[float], b: list[float]) -> float:
    """Compute cosine similarity between two vectors."""
    dot = sum(x * y for x, y in zip(a, b))
    norm_a = sum(x * x for x in a) ** 0.5
    norm_b = sum(x * x for x in b) ** 0.5
    if norm_a == 0 or norm_b == 0:
        return 0.0
    return dot / (norm_a * norm_b)


class EmbeddingStore:
    """SQLite-backed embedding store with incremental updates."""

    def __init__(self, db_path: Path = DB_PATH):
        db_path.parent.mkdir(parents=True, exist_ok=True)
        self.conn = sqlite3.connect(str(db_path), check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self) -> None:
        self.conn.executescript("""
            CREATE TABLE IF NOT EXISTS embeddings (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                namespace TEXT NOT NULL DEFAULT 'vault',
                source TEXT NOT NULL,
                source_hash TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                heading_path TEXT,
                content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                char_count INTEGER,
                position INTEGER,
                created_at TEXT NOT NULL,
                UNIQUE(namespace, source, chunk_id)
            );

            CREATE INDEX IF NOT EXISTS idx_emb_namespace
                ON embeddings(namespace);
            CREATE INDEX IF NOT EXISTS idx_emb_source
                ON embeddings(namespace, source);
            CREATE INDEX IF NOT EXISTS idx_emb_hash
                ON embeddings(namespace, source, source_hash);
        """)

    def sync_from_chunks(self, chunks_dir: Path, namespace: str = "vault") -> dict:
        """Sync embeddings with chunk JSON files from the Rust indexer.

        Reads all chunk files, compares hashes, and only re-embeds files
        whose hash has changed.

        Returns:
            Stats dict with counts of added, skipped, removed files.
        """
        model = _get_model()
        if model is None:
            return {"error": "sentence-transformers not installed"}

        if not chunks_dir.exists():
            return {"error": f"Chunks directory not found: {chunks_dir}"}

        # Collect all chunk files
        chunk_files = {}
        for json_file in chunks_dir.rglob("*.json"):
            try:
                with open(json_file) as f:
                    data = json.load(f)
                source = data.get("source", "")
                if source:
                    chunk_files[source] = data
            except (json.JSONDecodeError, OSError) as e:
                logger.warning("Failed to read chunk file %s: %s", json_file, e)

        # Get current hashes from DB
        existing = {}
        for row in self.conn.execute(
            "SELECT DISTINCT source, source_hash FROM embeddings WHERE namespace = ?",
            (namespace,),
        ).fetchall():
            existing[row["source"]] = row["source_hash"]

        added = 0
        skipped = 0
        removed = 0

        # Process each chunk file
        for source, data in chunk_files.items():
            source_hash = data.get("source_hash", "")

            if source in existing and existing[source] == source_hash:
                skipped += 1
                continue

            # Hash changed or new file — re-embed all chunks
            self.conn.execute(
                "DELETE FROM embeddings WHERE namespace = ? AND source = ?",
                (namespace, source),
            )

            chunks = data.get("chunks", [])
            if not chunks:
                continue

            # Batch encode all chunks for this file
            texts = [c.get("content", "") for c in chunks]
            vectors = model.encode(texts)
            now = datetime.now(timezone.utc).isoformat()

            for chunk, vector in zip(chunks, vectors):
                heading_path = json.dumps(chunk.get("heading_path", []))
                self.conn.execute(
                    """INSERT INTO embeddings
                       (namespace, source, source_hash, chunk_id, heading_path,
                        content, embedding, char_count, position, created_at)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    (
                        namespace,
                        source,
                        source_hash,
                        chunk.get("id", ""),
                        heading_path,
                        chunk.get("content", ""),
                        _encode_vector(vector.tolist()),
                        chunk.get("char_count", 0),
                        chunk.get("position", 0),
                        now,
                    ),
                )

            added += 1

        # Remove embeddings for files no longer in chunks
        for source in existing:
            if source not in chunk_files:
                self.conn.execute(
                    "DELETE FROM embeddings WHERE namespace = ? AND source = ?",
                    (namespace, source),
                )
                removed += 1

        self.conn.commit()

        return {"added": added, "skipped": skipped, "removed": removed}

    def add_content(
        self,
        namespace: str,
        source: str,
        content: str,
        source_hash: str = "",
        chunk_size: int = 1000,
    ) -> dict:
        """Add arbitrary content to the embedding store.

        Splits content into chunks, embeds, and stores. Used by lu learn
        for files and URLs that aren't part of the vault index.

        Returns:
            Stats dict with chunk count.
        """
        model = _get_model()
        if model is None:
            return {"error": "sentence-transformers not installed"}

        # Simple paragraph-based chunking for non-vault content
        paragraphs = content.split("\n\n")
        chunks = []
        current = ""
        for para in paragraphs:
            if len(current) + len(para) > chunk_size and current:
                chunks.append(current.strip())
                current = para
            else:
                current = current + "\n\n" + para if current else para
        if current.strip():
            chunks.append(current.strip())

        if not chunks:
            return {"chunks": 0}

        # Remove existing embeddings for this source
        self.conn.execute(
            "DELETE FROM embeddings WHERE namespace = ? AND source = ?",
            (namespace, source),
        )

        # Batch encode
        vectors = model.encode(chunks)
        now = datetime.now(timezone.utc).isoformat()
        source_hash = source_hash or now

        for i, (chunk_text, vector) in enumerate(zip(chunks, vectors)):
            self.conn.execute(
                """INSERT INTO embeddings
                   (namespace, source, source_hash, chunk_id, heading_path,
                    content, embedding, char_count, position, created_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    namespace,
                    source,
                    source_hash,
                    f"{Path(source).stem}-{i}",
                    "[]",
                    chunk_text,
                    _encode_vector(vector.tolist()),
                    len(chunk_text),
                    i,
                    now,
                ),
            )

        self.conn.commit()
        return {"chunks": len(chunks)}

    def search(
        self,
        query: str,
        namespace: str | None = None,
        limit: int = 10,
        recency_weight: float = 0.0,
    ) -> list[dict]:
        """Semantic search across stored embeddings.

        Returns top matches ranked by cosine similarity with optional
        temporal decay weighting.
        """
        model = _get_model()
        if model is None:
            return []

        query_vec = model.encode(query).tolist()

        # Fetch all embeddings (brute-force for vault-scale data)
        if namespace:
            rows = self.conn.execute(
                "SELECT * FROM embeddings WHERE namespace = ?", (namespace,)
            ).fetchall()
        else:
            rows = self.conn.execute("SELECT * FROM embeddings").fetchall()

        results = []
        for row in rows:
            emb_vec = _decode_vector(row["embedding"])
            sim = _cosine_similarity(query_vec, emb_vec)

            # Optional temporal decay
            if recency_weight > 0:
                try:
                    created = datetime.fromisoformat(row["created_at"])
                    age_days = (datetime.now(timezone.utc) - created).days
                    temporal = 0.5 ** (age_days / 30)
                    final_score = (1 - recency_weight) * sim + recency_weight * temporal
                except (ValueError, TypeError):
                    final_score = sim
            else:
                final_score = sim

            results.append({
                "source": row["source"],
                "chunk_id": row["chunk_id"],
                "heading_path": json.loads(row["heading_path"]) if row["heading_path"] else [],
                "content": row["content"][:300],
                "score": round(final_score, 4),
                "similarity": round(sim, 4),
                "namespace": row["namespace"],
                "char_count": row["char_count"],
            })

        results.sort(key=lambda x: x["score"], reverse=True)
        return results[:limit]

    def remove_source(self, namespace: str, source: str) -> int:
        """Remove all embeddings for a source. Returns count removed."""
        cursor = self.conn.execute(
            "DELETE FROM embeddings WHERE namespace = ? AND source = ?",
            (namespace, source),
        )
        self.conn.commit()
        return cursor.rowcount

    def remove_namespace(self, namespace: str) -> int:
        """Remove all embeddings in a namespace. Returns count removed."""
        cursor = self.conn.execute(
            "DELETE FROM embeddings WHERE namespace = ?", (namespace,)
        )
        self.conn.commit()
        return cursor.rowcount

    def get_topics(self, namespace: str = "vault", limit: int = 20) -> list[str]:
        """Extract distinct topic areas from heading paths.

        Analyzes the first heading in each chunk's heading_path to find
        the most common top-level topics across the store.
        """
        rows = self.conn.execute(
            "SELECT heading_path FROM embeddings WHERE namespace = ? AND heading_path != '[]'",
            (namespace,),
        ).fetchall()

        topic_counts: dict[str, int] = {}
        for row in rows:
            try:
                headings = json.loads(row["heading_path"])
                if headings:
                    # Use the first heading as the topic
                    topic = headings[0]
                    topic_counts[topic] = topic_counts.get(topic, 0) + 1
            except (json.JSONDecodeError, IndexError):
                continue

        # Sort by frequency, return top N
        sorted_topics = sorted(topic_counts.items(), key=lambda x: x[1], reverse=True)
        return [topic for topic, _ in sorted_topics[:limit]]

    def stats(self, namespace: str | None = None) -> dict:
        """Get index statistics."""
        if namespace:
            row = self.conn.execute(
                """SELECT COUNT(*) as chunks, COUNT(DISTINCT source) as sources
                   FROM embeddings WHERE namespace = ?""",
                (namespace,),
            ).fetchone()
        else:
            row = self.conn.execute(
                "SELECT COUNT(*) as chunks, COUNT(DISTINCT source) as sources FROM embeddings"
            ).fetchone()

        namespaces = [
            r[0] for r in self.conn.execute(
                "SELECT DISTINCT namespace FROM embeddings"
            ).fetchall()
        ]

        return {
            "chunks": row["chunks"],
            "sources": row["sources"],
            "namespaces": namespaces,
        }


# Module-level store
_store = EmbeddingStore()


def get_store() -> EmbeddingStore:
    """Public accessor for the embedding store."""
    return _store
