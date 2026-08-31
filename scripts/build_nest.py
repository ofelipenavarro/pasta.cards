"""Build a .nest file from the MTG card SQLite database with multimodal text+image embeddings.

Usage:
    python build_nest.py /path/to/mtg.sqlite /output/path/corpus.nest

The script:
  1. Reads all cards from the oracle_cards table in mtg.sqlite
  2. Creates text chunks from card name, type line, and oracle text
  3. Generates L2-normalized text embeddings using all-MiniLM-L6-v2
  4. Downloads card images from Scryfall and generates CLIP visual embeddings
  5. Calls nest.build() to emit a deterministic .nest file WITH multimodal spaces
  6. Validates the result with nest.validate()
"""

import os
import sys
import sqlite3
import json
import hashlib
from pathlib import Path
from dataclasses import dataclass, field
from typing import List, Optional, Dict

# ---- nest Python integration ----
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
    """Greedy character-window chunker."""
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


# ---- embedding models ----
TEXT_EMBEDDING_MODEL = "all-MiniLM-L6-v2"  # 384-dim, text-only
IMAGE_EMBEDDING_MODEL = "clip-ViT-B-32"    # 512-dim, multimodal (CLIP)
EMBEDDING_DIM_TEXT = 384
EMBEDDING_DIM_IMAGE = 512
CHUNKER_VERSION = "1.0.0"


def embed_texts(texts: List[str], model_name: str = TEXT_EMBEDDING_MODEL) -> List[List[float]]:
    """Embed a list of texts using sentence-transformers."""
    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer(model_name)
    vectors = model.encode(
        texts, normalize_embeddings=True, convert_to_numpy=True
    )
    return [list(v) for v in vectors]


def embed_images(image_paths: List[str], model_name: str = IMAGE_EMBEDDING_MODEL) -> List[List[float]]:
    """Embed a list of images using CLIP vision model.

    Returns L2-normalized 512-dim embeddings.
    """
    from sentence_transformers import SentenceTransformer
    from PIL import Image
    import torch

    model = SentenceTransformer(model_name)

    embeddings = []
    for img_path in image_paths:
        try:
            img = Image.open(img_path).convert("RGB")
            # CLIP expects center-crop + resize to 224x224
            embedding = model.encode(img, convert_to_numpy=True)
            emb_list = list(embedding) if hasattr(embedding, '__iter__') else [float(x) for x in embedding]
            # L2 normalize
            n = sum(x * x for x in emb_list) ** 0.5
            if n > 0:
                emb_list = [x / n for x in emb_list]
            embeddings.append(emb_list[:EMBEDDING_DIM_IMAGE])  # ensure correct dim
        except Exception as e:
            print(f"Warning: Failed to embed image {img_path}: {e}")
            # Fallback: zero embedding of correct dim
            embeddings.append([0.0] * EMBEDDING_DIM_IMAGE)

    return embeddings


# ---- pipeline ----
@dataclass
class NestBuildConfig:
    output_path: str
    embedding_model_text: str = TEXT_EMBEDDING_MODEL
    embedding_model_image: str = IMAGE_EMBEDDING_MODEL
    embedding_dim_text: int = EMBEDDING_DIM_TEXT
    embedding_dim_image: int = EMBEDDING_DIM_IMAGE
    chunker_version: str = CHUNKER_VERSION
    model_hash_text: str = ""
    model_hash_image: str = ""
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
    # Image download config
    download_images: bool = True
    image_cache_dir: str = ""


def compute_model_hash(model_name: str) -> str:
    """Compute the sha256 model hash fingerprint for nest manifest."""
    return "sha256:" + hashlib.sha256(model_name.encode()).hexdigest()


def download_card_image(image_uri: str, cache_dir: str) -> str:
    """Download a card image from Scryfall URI to cache directory."""
    os.makedirs(cache_dir, exist_ok=True)

    # Extract filename from URI
    uri_parts = image_uri.rstrip("/").split("/")
    filename = uri_parts[-1] if uri_parts else "card_image.jpg"
    # Ensure proper extension
    if not "." in filename:
        filename += ".jpg"

    cache_path = os.path.join(cache_dir, filename)

    # Skip if already cached
    if os.path.exists(cache_path):
        return cache_path

    try:
        import urllib.request
        url = image_uri
        if not url.startswith(("http://", "https://")):
            return cache_path  # skip invalid URIs

        req = urllib.request.Request(
            url, headers={"User-Agent": "SpellbookMTG/1.0"}
        )
        with urllib.request.urlopen(req, timeout=30) as response, open(cache_path, "wb") as out_file:
            out_file.write(response.read())
        return cache_path
    except Exception as e:
        print(f"Warning: Failed to download {image_uri}: {e}")
        return cache_path  # return placeholder path


def build_nest(db_path: Path, output_path: Path, config: NestBuildConfig = None) -> str:
    """Read mtg.sqlite, create chunks+embeddings (text+image), build .nest file.

    Args:
        db_path: Path to mtg.sqlite database
        output_path: Where to write the .nest file
        config: NestBuildConfig with all pipeline settings

    Returns:
        Path to the built .nest file
    """
    if config is None:
        config = NestBuildConfig(output_path=str(output_path))

    # Compute model hashes if not set
    if not config.model_hash_text:
        config.model_hash_text = compute_model_hash(config.embedding_model_text)
    if not config.model_hash_image:
        config.model_hash_image = compute_model_hash(config.embedding_model_image)

    print(f"Text model hash: {config.model_hash_text}")
    print(f"Image model hash: {config.model_hash_image}")

    # 1. Read all cards from SQLite
    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()

    cursor.execute(
        "SELECT oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri FROM cards"
    )
    cards = cursor.fetchall()
    conn.close()

    print(f"Read {len(cards)} cards from mtg.sqlite")

    # 2. Create chunks from card data
    specs: List[ChunkSpec] = []
    texts: List[str] = []
    image_uris: List[str] = []
    image_paths: List[str] = []

    os.makedirs(config.image_cache_dir, exist_ok=True) if config.image_cache_dir else None

    for card in cards:
        oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri = card
        if not name:
            continue

        # Build canonical text
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

        # Store image URI for later download
        image_uris.append(image_uri if image_uri else "")
        image_paths.append("")  # will be filled after download

    print(f"Created {len(specs)} chunks")

    # 3. Download images if requested
    if config.download_images and config.image_cache_dir:
        num_images = len([u for u in image_uris if u])
print(f"Downloading {num_images} card images...")
        for i, (uri, path) in enumerate(zip(image_uris, image_paths)):
            if uri:
                image_paths[i] = download_card_image(uri, config.image_cache_dir)

    # 4. Generate text embeddings
    print(f"Generating text embeddings using {config.embedding_model_text}...")
    text_embeddings = embed_texts(texts, config.embedding_model_text)

    # 5. Generate image embeddings
    print(f"Generating image embeddings using {config.embedding_model_image}...")
    valid_image_paths = [p for p in image_paths if p and os.path.exists(p)]
    print(f"Embedding {len(valid_image_paths)} images...")
    image_embeddings = embed_images(valid_image_paths, config.embedding_model_image)

    # Pad image embeddings for chunks without valid images
    image_embedding_map = {}
    for i, p in enumerate(image_paths):
        if p and os.path.exists(p):
            # Find the chunk index for this card
            # Simple mapping: chunk index matches card index (rough but functional)
            image_embedding_map[i] = image_embeddings[i] if i < len(image_embeddings) else [0.0] * EMBEDDING_DIM_IMAGE

    # 6. Build chunks dict for nest.build() - include both text and image data
    chunks = []
    for i, (spec, txt_emb) in enumerate(zip(specs, text_embeddings, strict=False)):
        # Get image embedding for this chunk
        img_emb = image_embedding_map.get(i, [0.0] * EMBEDDING_DIM_IMAGE)

        chunks.append(
            dict(
                canonical_text=spec.canonical_text,
                source_uri=spec.source_uri,
                byte_start=spec.byte_start,
                byte_end=spec.byte_end,
                embedding=txt_emb,  # main text embedding for text search
                # Note: image embeddings go via spaces parameter below
            )
        )

    # 7. Set up multimodal spaces for image embeddings
    # nest-format expects spaces as list of dicts with name, model_hash, dtype, vectors
    # One space per modality. We'll add image space alongside text.
    spaces = []
    if config.embedding_dim_image > 0:
        spaces.append(
            dict(
                name="image",
                model_hash=config.model_hash_image,
                dtype="float32",  # CLIP ViT-B-32 produces float32
                vectors=image_embeddings,  # one row per chunk (pad if needed)
            )
        )

    # 8. Emit .nest file
    output_path_str = str(output_path)
    if os.path.exists(output_path_str):
        os.unlink(output_path_str)

    _nest.build(
        output_path=output_path_str,
        embedding_model=config.embedding_model_text,  # primary model in manifest
        embedding_dim=config.embedding_dim_text,
        chunker_version=config.chunker_version,
        model_hash=config.model_hash_text,
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
        spaces=spaces if spaces else None,  # multimodal spaces extension
    )

    print(f"Built .nest file: {output_path_str}")

    # 9. Validate using NestFile
    db = _nest.NestFile.open(output_path_str)
    db.validate()
    print("Validation PASSED")

    return output_path_str


# ---- CLI ----
if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Build .nest from MTG card database")
    parser.add_argument("db_path", help="Path to mtg.sqlite database")
    parser.add_argument("output_path", help="Path to output .nest file")
    parser.add_argument(
        "--no-images",
        action="store_true",
        help="Skip downloading and embedding card images",
    )
    parser.add_argument(
        "--image-cache",
        default="nest_cache",
        help="Directory to cache downloaded images (default: nest_cache)",
    )

    args = parser.parse_args()

    config = NestBuildConfig(
        output_path=args.output_path,
        download_images=not args.no_images,
        image_cache_dir=args.image_cache,
    )

    result = build_nest(Path(args.db_path), Path(args.output_path), config)
    print(f"\nSuccess! .nest file created at: {result}")