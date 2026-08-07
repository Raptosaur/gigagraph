#!/usr/bin/env python3
"""Converts a Model2Vec static-embedding model into the compact artifact
embedded in this crate (`potion-base-2M.bin`, loaded by ../embed.rs).

Provenance of the committed artifact:
    model:     minishlab/potion-base-2M  (https://huggingface.co/minishlab/potion-base-2M)
    license:   MIT (per the model card's `license: mit` front matter)
    files:     model.safetensors (29528 x 64 f32), tokenizer.json (WordPiece,
               BertNormalizer lowercase, "##" continuation prefix)
    distilled from baai/bge-base-en-v1.5 by the MinishLab Tokenlearn pipeline.

Usage:
    python3 distill.py <dir-with-model.safetensors-and-tokenizer.json> <out.bin>

Output format (all little-endian), version tag "GGE1":
    magic   4 bytes  b"GGE1"
    vocab   u32      number of tokens (rows)
    dims    u32      embedding dimensions
    scales  vocab * f32     per-token dequantization scale
    q       vocab * dims * i8   quantized vectors, row-major (row i * scale i -> f32)
    tokens  vocab * (u16 len + utf8 bytes), in row order

Quantization is int8 with a per-token scale (scale = max|x| / 127). The script
prints a quality report: max/mean cosine error of dequantized vs original rows.
Pure stdlib on purpose — no numpy/safetensors dependency needed to reproduce.
"""

import array
import json
import math
import struct
import sys


def main():
    src_dir, out_path = sys.argv[1], sys.argv[2]

    # ---- safetensors: 8-byte LE header length, JSON header, raw tensor data.
    with open(f"{src_dir}/model.safetensors", "rb") as f:
        hlen = struct.unpack("<Q", f.read(8))[0]
        header = json.loads(f.read(hlen))
        info = header["embeddings"]
        assert info["dtype"] == "F32", info
        vocab_n, dims = info["shape"]
        start, end = info["data_offsets"]
        f.seek(8 + hlen + start)
        data = array.array("f")
        data.frombytes(f.read(end - start))
    assert len(data) == vocab_n * dims
    if sys.byteorder == "big":
        data.byteswap()

    # ---- tokenizer vocab, ordered by id (must align with embedding rows).
    tok = json.load(open(f"{src_dir}/tokenizer.json"))
    vocab = tok["model"]["vocab"]
    assert len(vocab) == vocab_n, (len(vocab), vocab_n)
    tokens = [t for t, _ in sorted(vocab.items(), key=lambda kv: kv[1])]

    # ---- int8 quantization, per-token scale.
    scales = array.array("f", [0.0] * vocab_n)
    q = array.array("b", [0] * (vocab_n * dims))
    for i in range(vocab_n):
        row = data[i * dims:(i + 1) * dims]
        m = max(abs(x) for x in row)
        if m == 0.0:
            continue
        s = m / 127.0
        scales[i] = s
        for j, x in enumerate(row):
            q[i * dims + j] = max(-127, min(127, round(x / s)))

    # ---- quality: cosine(original, dequantized) per row.
    worst, total = 1.0, 0.0
    for i in range(vocab_n):
        row = data[i * dims:(i + 1) * dims]
        deq = [q[i * dims + j] * scales[i] for j in range(dims)]
        na = math.sqrt(sum(x * x for x in row))
        nb = math.sqrt(sum(x * x for x in deq))
        if na == 0 or nb == 0:
            continue
        c = sum(a * b for a, b in zip(row, deq)) / (na * nb)
        worst = min(worst, c)
        total += c
    print(f"rows={vocab_n} dims={dims} worst_cos={worst:.6f} mean_cos={total / vocab_n:.6f}")

    if sys.byteorder == "big":
        scales.byteswap()
    with open(out_path, "wb") as f:
        f.write(b"GGE1")
        f.write(struct.pack("<II", vocab_n, dims))
        f.write(scales.tobytes())
        f.write(q.tobytes())
        for t in tokens:
            b = t.encode("utf-8")
            f.write(struct.pack("<H", len(b)))
            f.write(b)
    import os
    print(f"wrote {out_path}: {os.path.getsize(out_path)} bytes")


if __name__ == "__main__":
    main()
