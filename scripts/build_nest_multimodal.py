"""Build a complete multimodal .nest from MTG cards: text + vision embeddings.

This script uses the sovereign nest python bridge directly (no arbitrary limits):
  1. Reads every card from data/mtg.sqlite
  2. Downloads every card image available from Scryfall
  3. Generates L2-normalized text embeddings for every card (all-MiniLM-L6-v2, 384-dim)
  4. Generates L2-normalized vision embeddings for every card with an image
     (open_clip CLIP ViT-B-32, 512-dim)
  5. Emits a single .nest file with:
       - text chunks + text embeddings in the canonical embeddings section
       - a multimodal "image" space (0x15 + band) for the vision embeddings
       - HNSW vector index for fast approximate search
       - BM25 text index for keyword search
       - optional chunk-to-chunk graph adjacency
  6. Validates the file and runs sample searches.

Usage:
    source /tmp/nest_venv/bin/activate
    NEST_ALLOW_DOWNLOAD=1 python3 scripts/build_nest_multimodal.py \
        data/mtg.sqlite data/dataset/cards/corpus.nest

Environment:
    NEST_ALLOW_DOWNLOAD=1  - required on first run so open_clip can fetch CLIP weights.
"""

from __future__ import annotations

import hashlib
import json
import os
import sqlite3
import sys
import urllib.request
from pathlib import Path
from typing import List, Optional

import numpy as np

REPO_ROOT = Path(__file__).resolve().parents[1]
NEST_PYTHON = REPO_ROOT / "crates" / "nest" / "python"
sys.path.insert(0, str(NEST_PYTHON))

import _nest  # noqa: E402
from sentence_transformers import SentenceTransformer  # noqa: E402
from forge.embed_image import ImageEmbedder  # noqa: E402

TEXT_MODEL = "all-MiniLM-L6-v2"
TEXT_DIM = 384
VISION_MODEL = "ViT-B-32"
VISION_PRETRAINED = "openai"
VISION_DIM = 512
CHUNKER_VERSION = "mtg-multimodal-v1"


def compute_model_hash_text(model_name: str) -> str:
    return "sha256:" + hashlib.sha256(model_name.encode("utf-8")).hexdigest()


def safe_name(name: str) -> str:
    return "".join(c if c.isalnum() or c in " -_" else "_" for c in name).strip()


def extract_image_uuid(url: str) -> Optional[str]:
    """Extract the scryfall image UUID from an image_uri."""
    import re
    m = re.search(r"/([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\.jpg", url)
    return m.group(1) if m else None


def find_cached_image(url: str, caches: List[Path]) -> Optional[Path]:
    """Look for the image in any of the local caches by UUID."""
    uuid = extract_image_uuid(url)
    if not uuid:
        return None
    for cache in caches:
        if not cache.exists():
            continue
        # Cache files are named like <uuid>.jpg?<timestamp>
        for f in cache.glob("*.jpg*"):
            stem = f.name.split("?")[0].replace(".jpg", "").replace(".jpeg", "")
            if stem.lower() == uuid.lower():
                return f
    return None


def download_image(url: str, dest: Path, timeout: int = 45, retries: int = 3) -> bool:
    if dest.exists() and dest.stat().st_size > 0:
        return True
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "SpellbookMTG/1.0"})
            with urllib.request.urlopen(req, timeout=timeout) as resp, dest.open("wb") as f:
                f.write(resp.read())
            return dest.stat().st_size > 0
        except Exception as e:
            wait = 2 ** attempt
            print(f"  download attempt {attempt + 1}/{retries} failed {url}: {e} (retrying in {wait}s)")
            import time
            time.sleep(wait)
    return False


def load_cards(db_path: Path):
    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()
    cursor.execute(
        "SELECT oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri "
        "FROM cards ORDER BY oracle_id"
    )
    rows = cursor.fetchall()
    conn.close()
    return rows


def build_nest(
    db_path: Path,
    output_path: Path,
    image_cache: Path,
    max_images: Optional[int] = None,
    offline: bool = False,
):
    """Build the multimodal .nest file."""
    output_path.parent.mkdir(parents=True, exist_ok=True)
    image_cache.mkdir(parents=True, exist_ok=True)

    # Local caches to reuse already-downloaded images
    local_caches = [
        REPO_ROOT / "nest_cache_images",
        Path("nest_cache_images"),
        image_cache,
    ]

    print("=== Step 1: Load cards ===")
    rows = load_cards(db_path)
    print(f"Loaded {len(rows)} cards")

    # Limit if requested (for dev/testing); default None = all
    if max_images is not None and max_images > 0:
        rows = rows[:max_images]
        print(f"Limited to first {len(rows)} cards")

    print("\n=== Step 2: Collect images (cache + download) ===")
    image_paths: List[Optional[Path]] = []
    downloaded = 0
    cached = 0
    missing = 0
    for i, row in enumerate(rows):
        oracle_id, name, _, _, _, _, _, image_uri = row
        if image_uri:
            ext = Path(image_uri.split("?")[0]).suffix or ".jpg"
            filename = f"{safe_name(name)[:50]}_{oracle_id}{ext}"
            dest = image_cache / filename

            # Try local caches first
            cached_src = find_cached_image(image_uri, local_caches)
            if cached_src is not None and cached_src.exists():
                if not dest.exists():
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    if cached_src.resolve() != dest.resolve():
                        import shutil
                        shutil.copy2(cached_src, dest)
                image_paths.append(dest)
                cached += 1
            elif offline:
                image_paths.append(None)
                missing += 1
            elif download_image(image_uri, dest):
                image_paths.append(dest)
                downloaded += 1
            else:
                image_paths.append(None)
                missing += 1
        else:
            image_paths.append(None)
            missing += 1
        if (i + 1) % 500 == 0:
            print(f"  ... {i + 1}/{len(rows)} checked, {cached} cached, {downloaded} downloaded, {missing} missing")
    print(f"Images: {cached} cached, {downloaded} downloaded, {missing} missing")

    print("\n=== Step 3: Build text chunks and text embeddings ===")
    texts: List[str] = []
    chunks: List[dict] = []
    for row in rows:
        oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri = row
        parts = [f"Name: {name}"]
        if type_line:
            parts.append(f"Type: {type_line}")
        if mana_cost:
            parts.append(f"Mana: {mana_cost}")
        if oracle_text:
            ot = oracle_text[:500] if len(oracle_text) > 500 else oracle_text
            parts.append(f"Text: {ot}")
        if rarity:
            parts.append(f"Rarity: {rarity}")
        if set_code:
            parts.append(f"Set: {set_code}")
        canonical = " | ".join(parts)
        texts.append(canonical)
        chunks.append(
            {
                "canonical_text": canonical,
                "source_uri": f"oracle_id:{oracle_id}",
                "byte_start": 0,
                "byte_end": 0,
            }
        )

    print(f"Embedding {len(texts)} texts with {TEXT_MODEL}...")
    text_embedder = SentenceTransformer(TEXT_MODEL)
    text_vectors = text_embedder.encode(texts, normalize_embeddings=True, convert_to_numpy=True)
    text_hash = compute_model_hash_text(TEXT_MODEL)
    for chunk, vec in zip(chunks, text_vectors):
        chunk["embedding"] = vec.tolist()
    print(f"Text embeddings shape: {text_vectors.shape}")

    print("\n=== Step 4: Build vision embeddings ===")
    vision_embedder = ImageEmbedder(model_id=VISION_MODEL, pretrained=VISION_PRETRAINED, batch_size=32)
    print(f"Vision model: {VISION_MODEL}/{VISION_PRETRAINED}, dim={vision_embedder.dim}, hash={vision_embedder.model_hash}")

    vision_vectors: List[List[float]] = []
    valid_paths = [p for p in image_paths if p is not None]
    if valid_paths:
        print(f"Embedding {len(valid_paths)} images...")
        raw = vision_embedder.embed_paths([str(p) for p in valid_paths])
        # Map back to per-chunk list, zero-padding missing images
        path_to_vec = {str(p): raw[i].tolist() for i, p in enumerate(valid_paths)}
        for p in image_paths:
            if p is None or str(p) not in path_to_vec:
                vision_vectors.append([0.0] * VISION_DIM)
            else:
                vision_vectors.append(path_to_vec[str(p)])
    else:
        vision_vectors = [[0.0] * VISION_DIM for _ in chunks]
    print(f"Vision embeddings: {len(vision_vectors)} rows")

    print("\n=== Step 5: Emit .nest with multimodal spaces + HNSW + BM25 ===")
    if output_path.exists():
        output_path.unlink()

    spaces = [
        {
            "name": "image",
            "model_hash": vision_embedder.model_hash,
            "dtype": "float32",
            "vectors": vision_vectors,
        }
    ]

    _nest.build(
        output_path=str(output_path),
        embedding_model=TEXT_MODEL,
        embedding_dim=TEXT_DIM,
        chunker_version=CHUNKER_VERSION,
        model_hash=text_hash,
        chunks=chunks,
        title="MTG Cards Multimodal Corpus",
        version="1.0.0",
        description="Complete MTG card corpus with text and vision embeddings",
        license="",
        reproducible=True,
        preset="compressed",
        dtype="float32",
        with_hnsw=True,
        with_bm25=True,
        with_graph=False,
        hnsw_m=16,
        hnsw_ef_construction=400,
        hnsw_seed=42,
        spaces=spaces,
    )

    print(f"Built: {output_path}")

    print("\n=== Step 6: Validate and inspect ===")
    db = _nest.NestFile.open(str(output_path))
    db.validate()
    print(f"Validation PASSED")
    print(f"  file_hash: {db.file_hash}")
    print(f"  model_hash: {db.model_hash}")
    print(f"  embedding_dim: {db.embedding_dim}")
    print(f"  n_embeddings: {db.n_embeddings}")
    print(f"  has_ann (HNSW): {db.has_ann}")
    print(f"  has_bm25: {db.has_bm25}")
    print(f"  has_graph: {db.has_graph}")
    print(f"  space_names: {db.space_names}")

    print("\n=== Step 7: Sample searches ===")
    print("Text search (dummy vector):")
    hits = db.search([1.0] + [0.0] * (TEXT_DIM - 1), k=3)
    for h in hits:
        print(f"  score={h.score:.4f} chunk={h.chunk_id[:40]}... src={h.source_uri}")

    if "image" in (db.space_names or []):
        print("\nImage space search (dummy vector):")
        hits = db.search_space("image", [1.0] + [0.0] * (VISION_DIM - 1), k=3)
        for h in hits:
            print(f"  score={h.score:.4f} chunk={h.chunk_id[:40]}... src={h.source_uri}")

    return output_path


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Build multimodal MTG .nest")
    parser.add_argument("db_path", help="Path to mtg.sqlite")
    parser.add_argument("output_path", help="Output .nest path")
    parser.add_argument("--image-cache", default="nest_cache_images", help="Image cache directory")
    parser.add_argument("--max-images", type=int, default=0, help="Limit for testing (0 = all)")
    parser.add_argument("--offline", action="store_true", help="Use only local cached images, do not download")
    args = parser.parse_args()

    max_images = args.max_images if args.max_images > 0 else None

    build_nest(
        db_path=Path(args.db_path),
        output_path=Path(args.output_path),
        image_cache=Path(args.image_cache),
        max_images=max_images,
        offline=args.offline,
    )
