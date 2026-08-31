"""Build a .nest file from the MTG card SQLite database with multimodal text+image embeddings."""

import os
import sys
import sqlite3
import hashlib
from pathlib import Path
from dataclasses import dataclass, field
from typing import List, Optional, Dict

try:
    import _nest
except ImportError:
    import nest as _nest

TEXT_EMBEDDING_MODEL = "all-MiniLM-L6-v2"
IMAGE_EMBEDDING_MODEL = "clip-ViT-B-32"
EMBEDDING_DIM_TEXT = 384
EMBEDDING_DIM_IMAGE = 512
CHUNKER_VERSION = "1.0.0"
MAX_IMAGES_TO_EMBED = 200



@dataclass(frozen=True)
class ChunkSpec:
    canonical_text: str
    source_uri: str
    byte_start: int
    byte_end: int

    def chunk_id(self, chunker_version: str) -> str:
        import _nest
        return _nest.chunk_id(
            self.canonical_text,
            self.source_uri,
            self.byte_start,
            self.byte_end,
            chunker_version,
        )


@dataclass
class NestBuildConfig:
    output_path: str
    embedding_model_text=TEXT_EMBEDDING_MODEL
    embedding_model_image=IMAGE_EMBEDDING_MODEL
    embedding_dim_text=EMBEDDING_DIM_TEXT
    embedding_dim_image=EMBEDDING_DIM_IMAGE
    chunker_version=CHUNKER_VERSION
    model_hash_text=""
    model_hash_image=""
    title=None
    version=None
    description=None
    license=None
    reproducible=True
    preset="exact"
    text_encoding=None
    dtype=None
    mrl_dim=None
    with_hnsw=None
    with_bm25=None
    with_graph=False
    graph_top_m=8
    hnsw_m=16
    hnsw_ef_construction=400
    hnsw_seed=42
    download_images=True
    image_cache_dir="nest_cache_images"
    max_images_to_embed=MAX_IMAGES_TO_EMBED


def compute_model_hash(model_name):
    """Fingerprint the actual model files (nest's model_hash convention),
    so the search-time embedder gate can verify the corpus. Falls back to
    hashing the name only if the local snapshot cannot be resolved — such a
    corpus will be REJECTED by `nest search-text` / nestui."""
    fp_dir = str(Path(__file__).resolve().parent.parent / "crates" / "nest" / "python")
    if fp_dir not in sys.path:
        sys.path.insert(0, fp_dir)
    try:
        from model_fingerprint import (
            compute_model_fingerprint,
            fingerprint_to_model_hash,
            hf_cache_snapshot,
        )

        snap = hf_cache_snapshot(model_name)
        if snap is None and "/" not in model_name:
            # bare names resolve from the sentence-transformers org
            snap = hf_cache_snapshot(f"sentence-transformers/{model_name}")
        if snap is not None:
            fp = compute_model_fingerprint(str(snap), model_id=model_name)
            return fingerprint_to_model_hash(fp)
        print(f"Warning: no local snapshot for {model_name}; "
              "falling back to a NAME hash — search gates will reject this corpus")
    except Exception as e:
        print(f"Warning: model fingerprint failed for {model_name} ({e}); "
              "falling back to a NAME hash — search gates will reject this corpus")
    return "sha256:" + hashlib.sha256(model_name.encode()).hexdigest()


def download_card_image(image_uri, cache_dir):
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
        req = urllib.request.Request(url, headers={"User-Agent": "SpellbookMTG/1.0"})
        with urllib.request.urlopen(req, timeout=30) as response, open(cache_path, "wb") as out_file:
            out_file.write(response.read())
        return cache_path
    except Exception as e:
        print("Warning: Failed to download " + image_uri + ": " + str(e))
        return cache_path


def embed_texts(texts):
    from sentence_transformers import SentenceTransformer
    model = SentenceTransformer(TEXT_EMBEDDING_MODEL)
    vectors = model.encode(texts, normalize_embeddings=True, convert_to_numpy=True)
    return [list(v) for v in vectors]


def embed_images_limited(image_paths, max_to_embed=MAX_IMAGES_TO_EMBED):
    from sentence_transformers import SentenceTransformer
    from PIL import Image
    model = SentenceTransformer(IMAGE_EMBEDDING_MODEL)
    effective = min(len(image_paths), max_to_embed)
    print("Embedding " + str(effective) + " / " + str(max_to_embed) + " images with CLIP...")
    embeddings = []
    valid_count = 0
    for i, img_path in enumerate(image_paths[:max_to_embed]):
        try:
            img = Image.open(img_path).convert("RGB")
            embedding = model.encode(img, convert_to_numpy=True)
            emb_list = list(embedding) if hasattr(embedding, '__iter__') else [float(x) for x in embedding]
            n = sum(x * x for x in emb_list) ** 0.5
            if n > 0:
                emb_list = [x / n for x in emb_list]
            embeddings.append(emb_list[:EMBEDDING_DIM_IMAGE])
            valid_count += 1
        except Exception as e:
            print("Warning: Failed to embed image " + img_path + ": " + str(e))
            embeddings.append([0.0] * EMBEDDING_DIM_IMAGE)
    while len(embeddings) < max_to_embed:
        embeddings.append([0.0] * EMBEDDING_DIM_IMAGE)
    return embeddings, valid_count


def build_nest(db_path, output_path, config=None):
    if config is None:
        config = NestBuildConfig(output_path=str(output_path))

    if not config.model_hash_text:
        config.model_hash_text = compute_model_hash(config.embedding_model_text)
    if not config.model_hash_image:
        config.model_hash_image = compute_model_hash(config.embedding_model_image)

    print("Text model hash: " + config.model_hash_text)
    print("Image model hash: " + config.model_hash_image)

    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()
    cursor.execute(
        "SELECT oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri FROM cards"
    )
    cards = cursor.fetchall()
    conn.close()

    print("Read " + str(len(cards)) + " cards from mtg.sqlite")

    specs = []
    texts = []
    image_uris = []

    for card in cards:
        oracle_id, name, type_line, oracle_text, mana_cost, rarity, set_code, image_uri = card
        if not name:
            continue
        text_parts = ["Name: " + name]
        if type_line:
            text_parts.append("Type: " + type_line)
        if oracle_text:
            ot = oracle_text[:500] if len(oracle_text) > 500 else oracle_text
            text_parts.append("Text: " + ot)
        canonical = " | ".join(text_parts)
        source = "oracle_id:" + oracle_id if oracle_id else "unknown"
        spec = ChunkSpec(
            canonical_text=canonical,
            source_uri=source,
            byte_start=0,
            byte_end=0,
        )
        specs.append(spec)
        texts.append(canonical)
        image_uris.append(image_uri if image_uri else "")

    print("Created " + str(len(specs)) + " chunks")

    print("Generating text embeddings using " + TEXT_EMBEDDING_MODEL + "...")
    text_embeddings = embed_texts(texts)

    image_cache_dir = config.image_cache_dir
    os.makedirs(image_cache_dir, exist_ok=True)

    image_download_indices = []
    image_download_paths = []

    for i, uri in enumerate(image_uris):
        if uri and config.download_images and len(image_download_paths) < config.max_images_to_embed:
            path = download_card_image(uri, image_cache_dir)
            image_download_indices.append(i)
            image_download_paths.append(path)
        else:
            image_download_indices.append(-1)

    print("Downloading " + str(len(image_download_paths)) + " card images (out of " + str(len(specs)) + " chunks)...")

    if image_download_paths:
        image_embeddings_subset, valid_count = embed_images_limited(image_download_paths, config.max_images_to_embed)
    else:
        image_embeddings_subset = []
        valid_count = 0

    chunk_to_subset = {}
    for idx, orig_idx in enumerate(image_download_indices):
        if orig_idx >= 0:
            chunk_to_subset[orig_idx] = idx

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
            )
        )

    spaces = []
    spaces.append(
        dict(
            name="image",
            model_hash=config.model_hash_image,
            dtype="float32",
            vectors=full_image_vectors,
        )
    )

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

    print("Built .nest file: " + output_path_str)

    db = _nest.NestFile.open(output_path_str)
    db.validate()
    print("Validation PASSED")

    return output_path_str


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Build .nest from MTG card database")
    parser.add_argument("db_path", help="Path to mtg.sqlite database")
    parser.add_argument("output_path", help="Path to output .nest file")
    parser.add_argument("--no-images", action="store_true", help="Skip downloading images")
    parser.add_argument("--image-cache", default="nest_cache_images", help="Image cache directory")
    parser.add_argument("--max-images", type=int, default=200, help="Max images to embed")
    args = parser.parse_args()

    config = NestBuildConfig(output_path=args.output_path)
    config.download_images = not args.no_images
    config.image_cache_dir = args.image_cache
    config.max_images_to_embed = args.max_images if args.max_images > 0 else 10000

    result = build_nest(args.db_path, args.output_path, config)
    print("\nSuccess! .nest file created at: " + result)
