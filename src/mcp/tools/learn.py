"""Learn tools — ingest external content into Lu's knowledge base.

Supports:
- lu learn <file> — ingest text/markdown files
- lu learn <url> — fetch and ingest web content
- lu learn <folder> — batch ingest all files in a directory
- lu forget <source> — remove learned content

Learned content is stored in the embedding store with namespace
separation: "vault" for vault files, "learned/files" for ingested
files, "learned/urls" for web content.
"""

import hashlib
import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


def _extract_pdf_text(path: Path) -> str:
    """Extract text from a PDF file.

    Tries pymupdf first (fastest), then falls back to basic stdlib approach.
    Returns extracted text or raises ImportError if no PDF library is available.
    """
    try:
        import fitz  # pymupdf

        doc = fitz.open(str(path))
        pages = []
        for page in doc:
            text = page.get_text()
            if text.strip():
                pages.append(text)
        doc.close()
        return "\n\n".join(pages)
    except ImportError:
        pass

    raise ImportError(
        "No PDF library available. Install pymupdf: pip install pymupdf"
    )


def _read_text_file(path: str) -> tuple[str, str]:
    """Read a text/PDF file and return (content, hash)."""
    p = Path(path).expanduser().resolve()
    if not p.exists():
        raise FileNotFoundError(f"File not found: {path}")
    if not p.is_file():
        raise ValueError(f"Not a file: {path}")

    # Handle PDFs specially
    if p.suffix.lower() == ".pdf":
        content = _extract_pdf_text(p)
    else:
        content = p.read_text(errors="ignore")

    content_hash = hashlib.sha256(content.encode()).hexdigest()[:16]
    return content, content_hash


def _fetch_url(url: str) -> tuple[str, str]:
    """Fetch URL content and convert to text. Returns (content, hash)."""
    import urllib.request
    import html.parser

    class TextExtractor(html.parser.HTMLParser):
        def __init__(self):
            super().__init__()
            self.text_parts = []
            self._skip = False
            self._skip_tags = {"script", "style", "nav", "footer", "header"}

        def handle_starttag(self, tag, attrs):
            if tag in self._skip_tags:
                self._skip = True

        def handle_endtag(self, tag):
            if tag in self._skip_tags:
                self._skip = False

        def handle_data(self, data):
            if not self._skip:
                text = data.strip()
                if text:
                    self.text_parts.append(text)

    req = urllib.request.Request(url, headers={"User-Agent": "Ludolph/1.0"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        raw = resp.read().decode("utf-8", errors="ignore")

    content_hash = hashlib.sha256(raw.encode()).hexdigest()[:16]

    # Try to extract text from HTML
    if "<html" in raw.lower()[:500]:
        extractor = TextExtractor()
        extractor.feed(raw)
        content = "\n\n".join(extractor.text_parts)
    else:
        content = raw

    return content, content_hash


# --- MCP Tool Handlers ---


def _learn_file(args: dict[str, Any]) -> dict:
    """Ingest a file into the embedding store."""
    from tools.embeddings import get_store

    path = args.get("path", "").strip()
    if not path:
        return {"content": "", "error": "File path is required"}

    try:
        content, content_hash = _read_text_file(path)
    except (FileNotFoundError, ValueError) as e:
        return {"content": "", "error": str(e)}

    if not content.strip():
        return {"content": "", "error": "File is empty"}

    store = get_store()
    source_name = Path(path).name
    result = store.add_content(
        namespace="learned/files",
        source=source_name,
        content=content,
        source_hash=content_hash,
    )

    if "error" in result:
        return {"content": "", "error": result["error"]}

    return {"content": f"Learned '{source_name}': {result['chunks']} chunks indexed."}


def _learn_url(args: dict[str, Any]) -> dict:
    """Fetch and ingest a URL into the embedding store."""
    from tools.embeddings import get_store

    url = args.get("url", "").strip()
    if not url:
        return {"content": "", "error": "URL is required"}

    try:
        content, content_hash = _fetch_url(url)
    except Exception as e:
        return {"content": "", "error": f"Failed to fetch URL: {e}"}

    if not content.strip():
        return {"content": "", "error": "No content extracted from URL"}

    store = get_store()
    result = store.add_content(
        namespace="learned/urls",
        source=url,
        content=content,
        source_hash=content_hash,
    )

    if "error" in result:
        return {"content": "", "error": result["error"]}

    return {"content": f"Learned '{url}': {result['chunks']} chunks indexed."}


def _learn_folder(args: dict[str, Any]) -> dict:
    """Ingest all text/markdown files in a folder."""
    from tools.embeddings import get_store

    folder = args.get("path", "").strip()
    if not folder:
        return {"content": "", "error": "Folder path is required"}

    p = Path(folder).expanduser().resolve()
    if not p.is_dir():
        return {"content": "", "error": f"Not a directory: {folder}"}

    store = get_store()
    total_files = 0
    total_chunks = 0
    errors = []

    extensions = {
        ".md", ".txt", ".rst", ".org", ".csv", ".json", ".yaml", ".yml", ".toml", ".pdf",
    }

    for f in sorted(p.rglob("*")):
        if not f.is_file():
            continue
        if f.suffix.lower() not in extensions:
            continue
        if any(part.startswith(".") for part in f.relative_to(p).parts):
            continue

        try:
            content, content_hash = _read_text_file(str(f))
            if not content.strip():
                continue

            result = store.add_content(
                namespace="learned/files",
                source=str(f.relative_to(p)),
                content=content,
                source_hash=content_hash,
            )

            if "error" not in result:
                total_files += 1
                total_chunks += result.get("chunks", 0)
        except Exception as e:
            errors.append(f"{f.name}: {e}")

    msg = f"Learned {total_files} file(s): {total_chunks} chunks indexed."
    if errors:
        msg += f"\n{len(errors)} error(s):\n" + "\n".join(errors[:5])

    return {"content": msg}


def _learn_github(args: dict[str, Any]) -> dict:
    """Clone a GitHub repo and ingest its code/docs into the embedding store."""
    from tools.embeddings import get_store

    repo = args.get("repo", "").strip()
    if not repo:
        return {"content": "", "error": "Repository is required (e.g., 'owner/repo')"}

    # Normalize repo format
    repo = repo.removeprefix("github:")
    repo = repo.removeprefix("https://github.com/")
    repo = repo.rstrip("/")

    if "/" not in repo:
        return {"content": "", "error": f"Invalid repo format: '{repo}'. Use 'owner/repo'."}

    import subprocess
    import tempfile

    # Clone to temp directory (shallow clone for speed)
    with tempfile.TemporaryDirectory() as tmpdir:
        clone_url = f"https://github.com/{repo}.git"
        result = subprocess.run(
            ["git", "clone", "--depth", "1", clone_url, tmpdir],
            capture_output=True,
            text=True,
            timeout=120,
        )

        if result.returncode != 0:
            return {"content": "", "error": f"Failed to clone {repo}: {result.stderr.strip()}"}

        # Ingest code files
        store = get_store()
        total_files = 0
        total_chunks = 0

        code_extensions = {
            ".rs", ".py", ".js", ".ts", ".jsx", ".tsx", ".go", ".java",
            ".rb", ".php", ".c", ".cpp", ".h", ".hpp", ".cs", ".swift",
            ".kt", ".scala", ".sh", ".bash", ".zsh", ".sql", ".r",
            ".md", ".txt", ".rst", ".toml", ".yaml", ".yml", ".json",
            ".html", ".css", ".scss", ".less", ".vue", ".svelte",
        }

        skip_dirs = {
            ".git", "node_modules", "vendor", "target", "build", "dist",
            "__pycache__", ".venv", "venv", ".tox", "coverage",
        }

        tmppath = Path(tmpdir)
        for f in sorted(tmppath.rglob("*")):
            if not f.is_file():
                continue
            if f.suffix.lower() not in code_extensions:
                continue

            rel = f.relative_to(tmppath)
            if any(part in skip_dirs for part in rel.parts):
                continue

            try:
                content = f.read_text(errors="ignore")
                if not content.strip() or len(content) > 100_000:
                    continue

                file_result = store.add_content(
                    namespace=f"learned/github/{repo}",
                    source=str(rel),
                    content=content,
                    source_hash="",
                )

                if "error" not in file_result:
                    total_files += 1
                    total_chunks += file_result.get("chunks", 0)
            except Exception:
                continue

    return {
        "content": (
            f"Learned github:{repo}: {total_files} files, {total_chunks} chunks indexed."
        )
    }


def _forget(args: dict[str, Any]) -> dict:
    """Remove learned content by source name."""
    from tools.embeddings import get_store

    source = args.get("source", "").strip()
    if not source:
        return {"content": "", "error": "Source is required (file name, URL, or 'all')"}

    store = get_store()

    if source == "all":
        count = 0
        for ns in ["learned/files", "learned/urls"]:
            count += store.remove_namespace(ns)
        # Also remove all github repos
        stats = store.stats()
        for ns in stats.get("namespaces", []):
            if ns.startswith("learned/github/"):
                count += store.remove_namespace(ns)
        return {"content": f"Forgot all learned content ({count} chunks removed)."}

    # Try all learned namespaces
    count = store.remove_source("learned/files", source)
    count += store.remove_source("learned/urls", source)

    # Check if it's a github repo reference
    repo = source.removeprefix("github:").removeprefix("https://github.com/").rstrip("/")
    if "/" in repo:
        count += store.remove_namespace(f"learned/github/{repo}")

    if count == 0:
        return {"content": "", "error": f"No learned content found for '{source}'"}

    return {"content": f"Forgot '{source}' ({count} chunks removed)."}


def _learned_status(args: dict[str, Any]) -> dict:
    """Show what Lu has learned."""
    from tools.embeddings import get_store

    store = get_store()
    stats = store.stats()

    if stats["chunks"] == 0:
        return {"content": "Nothing learned yet. Use learn_file or learn_url to ingest content."}

    lines = [f"Knowledge base: {stats['chunks']} chunks from {stats['sources']} source(s)"]
    lines.append(f"Namespaces: {', '.join(stats['namespaces'])}")

    # Per-namespace breakdown
    for ns in stats["namespaces"]:
        ns_stats = store.stats(namespace=ns)
        lines.append(f"  {ns}: {ns_stats['chunks']} chunks, {ns_stats['sources']} sources")

    return {"content": "\n".join(lines)}


# --- Tool Definitions ---

TOOLS = [
    {
        "name": "learn_file",
        "description": (
            "Ingest a file into Lu's knowledge base. "
            "Supports text, markdown, and PDF files. "
            "The content is chunked and embedded for semantic search. "
            "Use this when the user wants Lu to learn from a file."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file to learn from",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "learn_url",
        "description": (
            "Fetch a URL and ingest its content into Lu's knowledge base. "
            "HTML is converted to text, then chunked and embedded. "
            "Use this when the user wants Lu to learn from a web page."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch and learn from",
                },
            },
            "required": ["url"],
        },
    },
    {
        "name": "learn_folder",
        "description": (
            "Ingest all text/markdown files in a folder into Lu's knowledge base. "
            "Supports .md, .txt, .rst, .org, .csv, .json, .yaml, .toml files."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the folder to learn from",
                },
            },
            "required": ["path"],
        },
    },
    {
        "name": "learn_github",
        "description": (
            "Clone a GitHub repository and ingest its code and documentation. "
            "Shallow clones for speed, indexes code files and docs. "
            "Use format 'owner/repo' or 'github:owner/repo'."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "repo": {
                    "type": "string",
                    "description": "GitHub repo (e.g., 'evannagle/ludolph' or 'github:owner/repo')",
                },
            },
            "required": ["repo"],
        },
    },
    {
        "name": "forget",
        "description": (
            "Remove learned content by source name. Use 'all' to forget everything. "
            "This only affects learned content, not the vault index."
        ),
        "input_schema": {
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source to forget (file name, URL, or 'all')",
                },
            },
            "required": ["source"],
        },
    },
    {
        "name": "learned_status",
        "description": "Show what Lu has learned — chunk counts, sources, and namespaces.",
        "input_schema": {
            "type": "object",
            "properties": {},
        },
    },
]

HANDLERS = {
    "learn_file": _learn_file,
    "learn_url": _learn_url,
    "learn_folder": _learn_folder,
    "learn_github": _learn_github,
    "forget": _forget,
    "learned_status": _learned_status,
}
