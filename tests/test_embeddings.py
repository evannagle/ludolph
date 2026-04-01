"""Tests for the embedding store.

These tests use mock embeddings (random vectors) to avoid requiring
sentence-transformers in the test environment.
"""

import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

# Add parent to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent / "src" / "mcp"))

from tools.embeddings import (
    EmbeddingStore,
    _cosine_similarity,
    _decode_vector,
    _encode_vector,
)


class TestVectorEncoding(unittest.TestCase):
    """Tests for vector binary encoding/decoding."""

    def test_roundtrip(self):
        """Encoding then decoding produces the same vector."""
        vec = [0.1, 0.2, 0.3, -0.5, 1.0]
        encoded = _encode_vector(vec)
        decoded = _decode_vector(encoded)
        for a, b in zip(vec, decoded):
            self.assertAlmostEqual(a, b, places=5)

    def test_compact_binary(self):
        """Binary encoding is 4 bytes per float."""
        vec = [1.0] * 384  # all-MiniLM-L6-v2 dimension
        encoded = _encode_vector(vec)
        self.assertEqual(len(encoded), 384 * 4)


class TestCosineSimilarity(unittest.TestCase):
    """Tests for cosine similarity."""

    def test_identical_vectors(self):
        """Identical vectors have similarity 1.0."""
        vec = [0.5, 0.3, 0.8]
        self.assertAlmostEqual(_cosine_similarity(vec, vec), 1.0, places=5)

    def test_orthogonal_vectors(self):
        """Orthogonal vectors have similarity 0.0."""
        self.assertAlmostEqual(_cosine_similarity([1, 0], [0, 1]), 0.0, places=5)

    def test_opposite_vectors(self):
        """Opposite vectors have similarity -1.0."""
        self.assertAlmostEqual(_cosine_similarity([1, 0], [-1, 0]), -1.0, places=5)

    def test_zero_vector(self):
        """Zero vector returns 0.0."""
        self.assertEqual(_cosine_similarity([0, 0], [1, 1]), 0.0)


class TestEmbeddingStore(unittest.TestCase):
    """Tests for the SQLite embedding store."""

    def setUp(self):
        self.tmpdir = tempfile.mkdtemp()
        self.db_path = Path(self.tmpdir) / "test_embeddings.db"
        self.store = EmbeddingStore(self.db_path)

    def test_add_content_and_search(self):
        """Added content should be searchable."""
        vec = [0.5] * 384

        # Create a mock that mimics numpy array behavior
        class FakeArray:
            def __init__(self, data):
                self._data = data

            def tolist(self):
                return self._data

            def __iter__(self):
                return iter([FakeArray(self._data)])

            def __len__(self):
                return 1

        mock_model = MagicMock()
        mock_model.encode = MagicMock(side_effect=[
            [FakeArray(vec)],  # add_content: batch of 1 chunk
            FakeArray(vec),    # search: query vector
        ])

        with patch("tools.embeddings._get_model", return_value=mock_model):
            result = self.store.add_content(
                namespace="test",
                source="test.md",
                content="Some test content for embedding",
                source_hash="abc123",
            )
            self.assertIn("chunks", result)
            self.assertGreater(result["chunks"], 0)

            results = self.store.search("test content", namespace="test", limit=5)
            self.assertGreater(len(results), 0)

    def test_remove_source(self):
        """Removing a source deletes its embeddings."""
        # Manually insert
        vec = _encode_vector([0.1] * 384)
        self.store.conn.execute(
            """INSERT INTO embeddings
               (namespace, source, source_hash, chunk_id, heading_path,
                content, embedding, char_count, position, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            ("test", "file.md", "hash1", "file-0", "[]", "content", vec, 7, 0, "2024-01-01"),
        )
        self.store.conn.commit()

        count = self.store.remove_source("test", "file.md")
        self.assertEqual(count, 1)

        # Verify removed
        stats = self.store.stats(namespace="test")
        self.assertEqual(stats["chunks"], 0)

    def test_stats(self):
        """Stats returns correct counts."""
        vec = _encode_vector([0.1] * 384)
        for i in range(3):
            self.store.conn.execute(
                """INSERT INTO embeddings
                   (namespace, source, source_hash, chunk_id, heading_path,
                    content, embedding, char_count, position, created_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                ("vault", f"file{i}.md", f"hash{i}", f"file{i}-0", "[]",
                 f"content {i}", vec, 10, 0, "2024-01-01"),
            )
        self.store.conn.commit()

        stats = self.store.stats(namespace="vault")
        self.assertEqual(stats["chunks"], 3)
        self.assertEqual(stats["sources"], 3)

    def test_sync_from_chunks(self):
        """Sync reads chunk JSON files and creates embeddings."""
        # Create a temporary chunks directory with a test chunk file
        chunks_dir = Path(self.tmpdir) / "chunks"
        chunks_dir.mkdir()

        chunk_data = {
            "source": "notes/test.md",
            "source_hash": "abc123",
            "indexed_at": "2024-01-01",
            "tier": "standard",
            "frontmatter": {},
            "chunks": [
                {
                    "id": "test-0",
                    "heading_path": ["Title"],
                    "content": "Test chunk content",
                    "char_count": 18,
                    "position": 0,
                }
            ],
        }

        with open(chunks_dir / "test.json", "w") as f:
            json.dump(chunk_data, f)

        mock_model = MagicMock()
        mock_model.encode = MagicMock(
            return_value=[MagicMock(tolist=lambda: [0.5] * 384)]
        )

        with patch("tools.embeddings._get_model", return_value=mock_model):
            result = self.store.sync_from_chunks(chunks_dir, namespace="vault")

        self.assertEqual(result["added"], 1)
        self.assertEqual(result["skipped"], 0)

    def test_sync_skips_unchanged(self):
        """Sync skips files with matching hashes."""
        # Pre-populate with matching hash
        vec = _encode_vector([0.1] * 384)
        self.store.conn.execute(
            """INSERT INTO embeddings
               (namespace, source, source_hash, chunk_id, heading_path,
                content, embedding, char_count, position, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            ("vault", "notes/test.md", "abc123", "test-0", '["Title"]',
             "content", vec, 7, 0, "2024-01-01"),
        )
        self.store.conn.commit()

        chunks_dir = Path(self.tmpdir) / "chunks"
        chunks_dir.mkdir()
        with open(chunks_dir / "test.json", "w") as f:
            json.dump({
                "source": "notes/test.md",
                "source_hash": "abc123",  # Same hash
                "chunks": [{"id": "test-0", "heading_path": ["Title"],
                           "content": "content", "char_count": 7, "position": 0}],
            }, f)

        mock_model = MagicMock()
        with patch("tools.embeddings._get_model", return_value=mock_model):
            result = self.store.sync_from_chunks(chunks_dir, namespace="vault")

        self.assertEqual(result["skipped"], 1)
        self.assertEqual(result["added"], 0)
        mock_model.encode.assert_not_called()  # Should not re-embed

    def test_namespace_isolation(self):
        """Different namespaces are isolated."""
        vec = _encode_vector([0.1] * 384)
        for ns in ["vault", "learned/files"]:
            self.store.conn.execute(
                """INSERT INTO embeddings
                   (namespace, source, source_hash, chunk_id, heading_path,
                    content, embedding, char_count, position, created_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (ns, "file.md", "hash1", "file-0", "[]", "content", vec, 7, 0, "2024-01-01"),
            )
        self.store.conn.commit()

        vault_stats = self.store.stats(namespace="vault")
        learned_stats = self.store.stats(namespace="learned/files")
        all_stats = self.store.stats()

        self.assertEqual(vault_stats["chunks"], 1)
        self.assertEqual(learned_stats["chunks"], 1)
        self.assertEqual(all_stats["chunks"], 2)


if __name__ == "__main__":
    unittest.main()
