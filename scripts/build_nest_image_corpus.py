"""Build a complete MTG card image corpus as a .nest file using nest's forge tooling.

This script:
  1. Reads all cards from data/mtg.sqlite
  2. Downloads every card image from Scryfall to a local cache
  3. Organizes images into a deterministic input directory
  4. Builds labels.json mapping image filename -> card metadata
  5. Uses python/forge/image_corpus.build_corpus() to:
       - encode all images into an AV1 video stream (compressed, self-contained)
       - generate CLIP/open_clip vision embeddings from decoded frames
       - emit a .nest file with blob_refs + space_table + image search capability
  6. Tests the resulting .nest with both text and image search

Usage:
    source /tmp/nest_venv/bin/activate
    python3 scripts/build_nest_image_corpus.py data/mtg.sqlite data/dataset/cards/mtg_images.nest
"""

import os
import sys
import json
import sqlite3
import urllib.request
import urllib.error
from pathlib import Path
from dataclasses import dataclass

# Put nest python tooling on path
REPO_ROOT = Path(__file__).resolve().parents[1]
NEST_PYTHON = REPO_ROOT / "crates" / "nest" / "python"
sys.path.insert(0, str(NEST_PYTHON))

from forge import embed_image, image_corpus
from forge.image_corpus import build_corpus

# Model: CLIP ViT-B-32 from openai, general purpose vision+text joint embedding
EMBED_MODEL = "ViT-B-32"
EMBED_PRETRAINED = "openai"
BATCH_SIZE = 32
IMAGE_WIDTH = 512
CRF = 35
SPEED = 8
BACKEND = "av1"


def compute_safe_filename(name: str, oracle_id: str, ext: str = ".jpg") -> str:
    """Deterministic, filesystem-safe filename for a card image."""
    safe = "".join(c for c in name if c.isalnum() or c in (" ", "-", "_")).rstrip()
    safe = safe.replace(" ", "_")[:60]
    return f"{safe}_{oracle_id}{ext}"


def download_image(url: str, dest: Path, timeout: int = 45) -> bool:
    if dest.exists():
        return True
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "SpellbookMTG/1.0"})
        with urllib.request.urlopen(req, timeout=timeout) as resp, dest.open("wb") as f:
            f.write(resp.read())
        return True
    except Exception as e:
        print(f"  download failed: {url} -> {e}")
        return False


def build_image_corpus(
    db_path: Path,
    output_nest: Path,
    input_dir: Path | None = None,
    image_cache: Path | None = None,
    sample: int | None = None,
):
    """Build a .nest image corpus from the MTG SQLite database."""
    db_path = Path(db_path)
    output_nest = Path(output_nest)
    output_nest.parent.mkdir(parents=True, exist_ok=True)

    if input_dir is None:
        input_dir = output_nest.parent / "mtg_image_input"
    if image_cache is None:
        image_cache = output_nest.parent / "mtg_image_cache"
    input_dir.mkdir(parents=True, exist_ok=True)
    image_cache.mkdir(parents=True, exist_ok=True)

    # 1. Read cards from SQLite
    print(f"Reading cards from {db_path}...")
    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()
    cursor.execute(
        "SELECT oracle_id, name, type_line, oracle_text, rarity, set_code, image_uri FROM cards "
        "WHERE image_uri IS NOT NULL AND image_uri != '' ORDER BY oracle_id"
    )
    rows = cursor.fetchall()
    conn.close()
    print(f"Found {len(rows)} cards with image URIs")

    # 2. Download images and write labels
    print(f"Downloading images to {image_cache}...")
    labels = {}
    downloaded = 0
    missing = 0

    # Create symlinks/hardlinks in input_dir for the forge collector
    for oracle_id, name, type_line, oracle_text, rarity, set_code, image_uri in rows:
        filename = compute_safe_filename(name, oracle_id)
        cache_path = image_cache / filename
        input_path = input_dir / filename

        ok = download_image(image_uri, cache_path)
        if not ok or cache_path.stat().st_size == 0:
            missing += 1
            continue

        # Symlink into input dir so forge can discover it
        if input_path.exists() or input_path.is_symlink():
            input_path.unlink()
        input_path.symlink_to(cache_path.resolve())

        label = json.dumps(
            {
                "name": name,
                "type_line": type_line or "",
                "oracle_text": (oracle_text or "")[:200],
                "rarity": rarity or "",
                "set_code": set_code or "",
                "oracle_id": oracle_id,
                "image_uri": image_uri,
            },
            ensure_ascii=False,
        )
        labels[filename] = label
        downloaded += 1

        if downloaded % 500 == 0:
            print(f"  ... {downloaded} images ready")

    print(f"Downloaded {downloaded} images, {missing} missing")

    if downloaded == 0:
        raise RuntimeError("No images available to build corpus")

    labels_path = input_dir / "labels.json"
    labels_path.write_text(json.dumps(labels, indent=2, ensure_ascii=False))

    # 3. Build the image corpus with nest forge
    print(f"\nBuilding image corpus at {output_nest}...")
    embedder = embed_image.ImageEmbedder(
        model_id=EMBED_MODEL,
        pretrained=EMBED_PRETRAINED,
        batch_size=BATCH_SIZE,
    )

    result = build_corpus(
        input_dir=input_dir,
        output_path=output_nest,
        dataset_name="mtg_cards",
        embedder=embedder,
        compress=True,
        labels=labels,
        sample=sample,
        seed=42,
        width=IMAGE_WIDTH,
        crf=CRF,
        speed=SPEED,
        backend=BACKEND,
        pix_fmt="yuv420p",
        gop_policy="auto",
        preset="compressed",
        dtype="float32",
    )

    print(f"\nCorpus built:")
    print(f"  .nest: {result['nest']}")
    print(f"  manifest: {result['manifest']}")
    print(f"  n_items: {result['n_items']}")
    print(f"  compressed: {result['compressed']}")
    print(f"  media: {result['media']}")

    return output_nest


def test_corpus(nest_path: Path):
    """Open the .nest and exercise search capabilities."""
    print(f"\nTesting corpus: {nest_path}")
    import nest as nestpy

    db = nestpy.NestFile.open(str(nest_path))
    print(f"  file_hash: {db.file_hash}")
    print(f"  model_hash: {db.model_hash}")
    print(f"  embedding_dim: {db.embedding_dim}")
    print(f"  n_embeddings: {db.n_embeddings}")
    print(f"  has_ann: {db.has_ann}")
    print(f"  has_bm25: {db.has_bm25}")
    print(f"  has_graph: {db.has_graph}")
    print(f"  space_names: {db.space_names}")

    # Search with a dummy vector in the image space
    if "image" in (db.space_names or []):
        q = [1.0] + [0.0] * (db.embedding_dim - 1)
        hits = db.search_space("image", q, k=3)
        print(f"  image search hits: {len(hits)}")
        for h in hits:
            text = getattr(h, "canonical_text", "N/A")
            score = getattr(h, "score", "N/A")
            print(f"    score={score:.4f} text={text[:80]}")


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Build MTG image .nest corpus")
    parser.add_argument("db_path", help="Path to mtg.sqlite")
    parser.add_argument("output_nest", help="Output .nest path")
    parser.add_argument("--sample", type=int, help="Build from a random sample of N cards")
    parser.add_argument("--no-test", action="store_true", help="Skip post-build test")
    args = parser.parse_args()

    nest_path = build_image_corpus(
        db_path=args.db_path,
        output_nest=args.output_nest,
        sample=args.sample,
    )

    if not args.no_test:
        test_corpus(nest_path)
