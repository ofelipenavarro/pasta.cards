"""Build a .nest file from the MTG card SQLite database.

Usage:
    python build_nest.py /path/to/mtg.sqlite /output/path/corpus.nest

The script:
  1. Reads all cards from the oracle_cards table in mtg.sqlite
  2. Creates text chunks from card name, type line, and oracle text
  3. Generates L2-normalized embeddings using a SentenceTransformer model
  4. Calls nest.build() to emit a deterministic .nest file
  5. Validates the result with nest.validate()
"""

import os
import sys
import sqlite3
import json
import hashlib
from pathlib import Path
from dataclasses import dataclass, field
from typing import List, Optional

# ---- nest Python integration ----
# The nest Python module is installed via maturin from the hoffresearch/nest repo.
# It provides _nest.build() and _nest.NestFile.open() for .nest file lifecycle.
try:
    import _nest
except ImportError:
    import nest as _nest  # fallback for dev setups


# ---- chunker ----
@dataclass(frozen=True)
class ChunkSpec:
    canonical_text: str
    source_uri: str
    byte_start: int
    byte_end: int

    def chunk_id(self, chunker_version: str) -> str:
        return _nest.chunk_id(
            self.canonical_text,
            self.source_uri,
            self.byte_start,
            self.byte_end,
            chunker_version,
        )


def chunk_text(text: str, source_uri: str, *, max_chars: int = 512, overlap: int = 0) -> List[ChunkSpec]:
    """Greedy character-window chunker. Splits on a hard character budget,
    with optional overlap. Returns chunks whose byte spans index into the
    UTF-8 encoding of `text`, so the spans round-trip through `nest cite`.
    """
    if max_chars <= 0:
        raise ValueError("max_chars must be > 0")
    if overlap < 0 or overlap >= max_chars:
        raise ValueError("overlap must be >= 0 and < max_chars")

    encoded = text.encode("utf-8")
    chunks: List[ChunkSpec] = []
    char_idx = 0
    text_len = len(text)
    while char_idx < text_len:
        end = min(char_idx + max_chars, text_len)
        piece = text[char_idx:end]
        prefix_bytes = len(text[:char_idx].encode("utf-8"))
        piece_bytes = len(piece.encode("utf-8"))
        chunks.append(
            ChunkSpec(
                canonical_text=piece,
                source_uri=source_uri,
                byte_start=prefix_bytes,
                byte_end=prefix_bytes + piece_bytes,
            )
        )
        if end == text_len:
            break
        char_idx = end - overlap
    for c in chunks:
        assert c.byte_end <= len(encoded), "byte span overshoots source"
    return chunks


# ---- embedding model ----
EMBEDDING_MODEL = "all-MiniLM-L6-v2"  # 384-dim, widely available, offline-capable
EMBEDDING_DIM = 384
CHUNKER_VERSION = "1.0.0"


def embed_texts(texts: List[str], model_name: str = EMBEDDING_MODEL) -> List[List[float]]:
    """Embed a list of texts using sentence-transformers.

    Requires the model to be available locally (downloaded once).
    Vectors are L2-normalized so cosine = dot product.
    """
    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer(model_name)
    vectors = model.encode(
        texts, normalize_embeddings=True, convert_to_numpy=True
    )
    return [list(v) for v in vectors]


# ---- pipeline ----
@dataclass
class NestBuildConfig:
    output_path: str
    embedding_model: str = EMBEDDING_MODEL
    embedding_dim: int = EMBEDDING_DIM
    chunker_version: str = CHUNKER_VERSION
    model_hash: str = ""
    title: Optional[str] = None
    version: Optional[str] = None
    description: Optional[str] = None
    license: Optional[str] = None
    reproducible: bool = True
    preset: str = "exact"
    text_encoding: Optional[str] = None  # "raw" | "zstd"
    dtype: Optional[str] = None  # "float32" | "float16" | "int8" | "int4"
    mrl_dim: Optional[int] = None
    with_hnsw: Optional[bool] = None
    with_bm25: Optional[bool] = None
    with_graph: bool = False
    graph_top_m: int = 8
    hnsw_m: int = 16
    hnsw_ef_construction: int = 400
    hnsw_seed: int = 42


def compute_model_hash(model_name: str = EMBEDDING_MODEL) -> str:
    """Compute the sha256 model hash fingerprint for nest manifest."""
    return "sha256:" + hashlib.sha256(model_name.encode()).hexdigest()


def build_nest(db_path: Path, output_path: Path, config: NestBuildConfig = None) -> str:
    """Read mtg.sqlite, create chunks+embeddings, build .nest file.

    Args:
        db_path: Path to mtg.sqlite database
        output_path: Where to write the .nest file
        config: NestBuildConfig with all pipeline settings

    Returns:
        Path to the built .nest file
    """
    if config is None:
        config = NestBuildConfig(output_path=str(output_path))

    # Compute model hash if not set
    if not config.model_hash:
        config.model_hash = compute_model_hash(config.embedding_model)
    print(f"Model hash: {config.model_hash}")

    # 1. Read all cards from SQLite
    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()

    cursor.execute(
        "SELECT oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code FROM cards"
    )
    cards = cursor.fetchall()
    conn.close()

    print(f"Read {len(cards)} cards from mtg.sqlite")

    # 2. Create chunks from card data
    specs: List[ChunkSpec] = []
    texts: List[str] = []

    for card in cards:
        oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code = card
        if not name:
            continue

        # Build canonical text from card data
        text_parts = [f"Name: {name}"]
        if type_line:
            text_parts.append(f"Type: {type_line}")
        if oracle_text:
            ot = oracle_text[:500] if len(oracle_text) > 500 else oracle_text
            text_parts.append(f"Text: {ot}")

        canonical = " | ".join(text_parts)

        source = f"oracle_id:{oracle_id}" if oracle_id else "unknown"

        spec = ChunkSpec(
            canonical_text=canonical,
            source_uri=source,
            byte_start=0,
            byte_end=0,
        )
        specs.append(spec)
        texts.append(canonical)

    print(f"Created {len(specs)} chunks")

    # 3. Generate embeddings
    print(f"Generating embeddings using {config.embedding_model}...")
    embeddings = embed_texts(texts, config.embedding_model)

    # 4. Build chunks dict for nest.build()
    chunks = []
    for spec, emb in zip(specs, embeddings, strict=False):
        chunks.append(
            dict(
                canonical_text=spec.canonical_text,
                source_uri=spec.source_uri,
                byte_start=spec.byte_start,
                byte_end=spec.byte_end,
                embedding=emb,
            )
        )

    # 5. Emit .nest file
    output_path_str = str(output_path)
    if os.path.exists(output_path_str):
        os.unlink(output_path_str)

    _nest.build(
        output_path=output_path_str,
        embedding_model=config.embedding_model,
        embedding_dim=config.embedding_dim,
        chunker_version=config.chunker_version,
        model_hash=config.model_hash,
        chunks=chunks,
        title=config.title,
        version=config.version,
        description=config.description,
        license=config.license,
        reproducible=config.reproducible,
        preset=config.preset,
        text_encoding=config.text_encoding,
        dtype=config.dtype,
        mrl_dim=config.mrl_dim,
        with_hnsw=config.with_hnsw,
        with_bm25=config.with_bm25,
        with_graph=config.with_graph,
        graph_top_m=config.graph_top_m,
        hnsw_m=config.hnsw_m,
        hnsw_ef_construction=config.hnsw_ef_construction,
        hnsw_seed=config.hnsw_seed,
    )

    print(f"Built .nest file: {output_path_str}")

    # 6. Validate using NestFile
    db = _nest.NestFile.open(output_path_str)
    db.validate()
    print("Validation PASSED")

    return output_path_str


# ---- CLI ----
if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python build_nest.py <mtg.sqlite_path> <output_nest_path>")
        sys.exit(1)

    db_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])

    if not db_path.exists():
        print(f"Error: database not found at {db_path}")
        sys.exit(1)

    # Activate nest Python bindings
    try:
        import _nest
    except ImportError:
        import nest as _nest

    result = build_nest(db_path, output_path)
    print(f"\nSuccess! .nest file created at: {result}")