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
MAX_IMAGES_TO_EMBED = 200  # practical limit: embed 200 card images max


def embed_texts(texts: List[str], model_name: str = TEXT_EMBEDDING_MODEL) -> List[List[float]]:
    """Embed a list of texts using sentence-transformers."""
    from sentence_transformers import SentenceTransformer

    model = SentenceTransformer(model_name)
    vectors = model.encode(
        texts, normalize_embeddings=True, convert_to_numpy=True
    )
    return [list(v) for v in vectors]


def embed_images_limited(image_paths: List[str], max_to_embed: int = MAX_IMAGES_TO_EMBED) -> tuple:
    """Embed up to max_to_embed images using CLIP vision model.

    Returns:
        (embeddings_list, valid_count) where embeddings_list has exactly max_to_embed entries
        (last entries are zero-padding if fewer images provided), and valid_count is how many
        were actually successfully embedded.
    """
    from sentence_transformers import SentenceTransformer
    from PIL import Image

    model = SentenceTransformer(IMAGE_EMBEDDING_MODEL)
    n_chunks = max(len(image_paths), max_to_embed)
    effective = min(len(image_paths), max_to_embed)

    print(f"Embedding {effective} / {max_to_embed} images with CLIP……

    embeddings = []
    valid_count = 0

    for i, img_path in enumerate(image_paths[:max_to_embed]):
        try:
            img = Image.open(img_path).convert("RGB")
            embedding = model.encode(img, convert_to_numpy=True)
            emb_list = list(embedding) if hasattr(embedding, '__iter__') else [float(x) for x in embedding]
            # L2 normalize
            n = sum(x * x for x in emb_list) ** 0.5
            if n > 0:
                emb_list = [x / n for x in emb_list]
            embeddings.append(emb_list[:EMBEDDING_DIM_IMAGE])
            valid_count += 1
        except Exception as e:
            print(f"Warning: Failed to embed image {img_path}: {e}")
            embeddings.append([0.0] * EMBEDDING_DIM_IMAGE)

    # Pad to exactly max_to_embed entries
    while len(embeddings) < max_to_embed:
        embeddings.append([0.0] * EMBEDDING_DIM_IMAGE)

    return embeddings, valid_count


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
    image_cache_dir: str = "nest_cache_images"
    max_images_to_embed: int = MAX_IMAGES_TO_EMBED


def compute_model_hash(model_name: str) -> str:
    """Compute the sha256 model hash fingerprint for nest manifest."""
    return "sha256:" + hashlib.sha256(model_name.encode()).hexdigest()


def download_card_image(image_uri: str, cache_dir: str) -> str:
    """Download a card image from Scryfall URI to cache directory."""
    os.makedirs(cache_dir, exist_ok=True)

    uri_parts = image_uri.rstrip("/").split("/")
    filename = uri_parts[-1] if uri_parts else "card_image.jpg"
    if not "." in filename:
        filename += ".jpg"

    cache_path = os.path.join(cache_dir, filename)

    if os.path.exists(cache_path):
        return cache_path

    try:
        import urllib.request
        url = image_uri
        if not url.startswith(("http://", "https://")):
            return cache_path

        req = urllib.request.Request(
            url, headers={"User-Agent": "SpellbookMTG/1.0"}
        )
        with urllib.request.urlopen(req, timeout=30) as response, open(cache_path, "wb") as out_file:
            out_file.write(response.read())
        return cache_path
    except Exception as e:
        print(f"Warning: Failed to download {image_uri}: {e}")
        return cache_path


def build_nest(db_path: Path, output_path: Path, config: NestBuildConfig = None) -> str:
    """Read mtg.sqlite, create chunks+embeddings (text+image), build .nest file."""

    if config is None:
        config = NestBuildConfig(output_path=str(output_path))

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

    for card in cards:
        oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri = card
        if not name:
            continue

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
        image_uris.append(image_uri if image_uri else "")

    print(f"Created {len(specs)} chunks")

    # 3. Generate text embeddings (all 38K chunks - this is fast enough)
    print(f"Generating text embeddings using {config.embedding_model_text}……")
    text_embeddings = embed_texts(texts)

    # 4. Download images for a subset and generate image embeddings
    image_cache_dir = config.image_cache_dir
    os.makedirs(image_cache_dir, exist_ok=True)

    # Only download/embed images for a limited subset (first N cards with image URIs)
    image_download_indices = []
    image_download_paths = []

    for i, uri in enumerate(image_uris):
        if uri and config.download_images and len(image_download_indices) < config.max_images_to_embed:
            path = download_card_image(uri, image_cache_dir)
            image_download_indices.append(i)
            image_download_paths.append(path)
        else:
            image_download_indices.append(-1)  # marker: no image download

    print(f"Downloading {len(image_download_paths)} card images (out of {len(specs)} chunks)……")

    # Generate image embeddings for the downloaded subset
    if image_download_paths:
        image_embeddings_subset, valid_count = embed_images_limited(image_download_paths, config.max_images_to_embed)
    else:
        image_embeddings_subset = []
        valid_count = 0

    # 5. Build the full image embeddings array: one entry per chunk
    # First chunk_idx -> subset index mapping
    chunk_to_subset = {}
    for idx, orig_idx in enumerate(image_download_indices):
        if orig_idx >= 0:
            chunk_to_subset[orig_idx] = idx

    # Build full vectors list: one per chunk, with image embeddings for the subset
    full_image_vectors = []
    for i in range(len(specs)):
        if i in chunk_to_subset:
            subset_idx = chunk_to_subset[i]
            if subset_idx < len(image_embeddings_subset):
                full_image_vectors.append(image_embeddings_subset[subset_idx])
            else:
                full_image_vectors.append([0.0] * EMBEDDING_DIM_IMAGE)
        else:
            full_image_vectors.append([0.0] * EMBEDDING_DIM_IMAGE)

    # 6. Build chunks dict for nest.build()
    chunks = []
    for i, (spec, txt_emb) in enumerate(zip(specs, text_embeddings, strict=False)):
        img_emb = full_image_vectors[i]

        chunks.append(
            dict(
                canonical_text=spec.canonical_text,
                source_uri=spec.source_uri,
                byte_start=spec.byte_start,
                byte_end=spec.byte_end,
                embedding=txt_emb,
                # Image embedding will be accessed through the "image" space in nest runtime
            )
        )

    # 7. Set up multimodal spaces for image embeddings
    spaces.append(
        dict(
            name="image",
            model_hash=config.model_hash_image,
            dtype="float32",
            vectors=full_image_vectors,  # one entry per chunk (most are zero-padded)
        )
    )

    # 8. Emit .nest file
    output_path_str = str(output_path)
    if os.path.exists(output_path_str):
        os.unlink(output_path_str)

    _nest.build(
        output_path=output_path_str,
        embedding_model=config.embedding_model_text,
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
        spaces=spaces if spaces else None,
    )

    print(f"Built .nest file: {output_path_str}")

    # 9. Validate
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
        default="nest_cache_images",
        help="Directory to cache downloaded images (default: nest_cache_images)",
    )
    parser.add_argument(
        "--max-images",
        type=int,
        default=200,
        help="Maximum number of card images to embed (default: 200, set 0 to use all with images)",
    )

    args = parser.parse_args()

    max_images = 0 if args.no_images else args.max_images

    config = NestBuildConfig(
        output_path=args.output_path,
        download_images=not args.no_images,
        image_cache_dir=args.image_cache,
        max_images_to_embed=max_images if max_images > 0 else 10000,  # large number = use all available
    )

    result = build_nest(Path(args.db_path), Path(args.output_path), config)
    print(f"\nSuccess! .nest file created at: {result}")