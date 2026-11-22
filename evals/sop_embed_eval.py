#!/usr/bin/env python3
"""ADR-022 实验：SOP 嵌入匹配的匹配测评脚本。

用途：对实现的编码器（§2.2 `SopEmbedder` 接口）做匹配测评（§3.2）：
- `hit@5`：每个查询的参考近邻（ground truth）在 top5 内
- `mAP@5`：近邻按相关性排序后的平均精度（±0.05 容差）

专注于「匹配」本身，不涉及检索索引/数据库（§5）。

用法：
```bash
python3 evals/sop_embed_eval.py --truth evals/ground_truth_42.json --codec name_hash --k 5
python3 evals/sop_embed_eval.py --truth evals/ground_truth_42.json --codec none
```
"""

from __future__ import annotations

import argparse
import json
import math
from collections import Counter
from pathlib import Path


def _norm(v: list[float]) -> list[float]:
    n = math.sqrt(sum(x * x for x in v))
    if n == 0:
        return [0.0] * len(v)
    return [x / n for x in v]


def cosine(a: list[float], b: list[float]) -> float:
    va, vb = _norm(a), _norm(b)
    return sum(x * y for x, y in zip(va, vb))


def fnv1a64(data: bytes) -> int:
    """FNV-1a 64-bit（与 Rust bench 一致，确定性）。"""
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def make_vectors(names: list[str], codec: str) -> dict[str, list[float]]:
    """§2.2 输出接口：name -> embedding vector（共享工具模块接入点）。"""
    if codec == "none":
        return {nm: [0.0] for nm in names}
    if codec == "name_hash":
        out: dict[str, list[float]] = {}
        for nm in names:
            h = fnv1a64(nm.lower().encode())
            vec = []
            for i in range(8):
                if 8 * i < 64:
                    vec.append(((h >> (8 * i)) & 0xFF) / 255.0)
                else:
                    vec.append(0.0)
            out[nm] = vec
        return out
    if codec == "name_hash_dense16":
        out: dict[str, list[float]] = {}
        for nm in names:
            h = fnv1a64(nm.lower().encode())
            dense = [0.0] * 128
            # Rust bench: 16 bytes -> 128 slots, low 8 bits of hash in rotation
            for i in range(16):
                if 8 * i < 64:
                    b = ((h >> (8 * i)) & 0xFF) - 127.5
                    dense[(i * 8) % 128] += b / 127.5
            out[nm] = dense
        return out
    raise ValueError(f"unknown codec: {codec}")


def ap(query_names: list[str], truth_names: list[str], vec: dict[str, list[float]], k: int) -> dict:
    """AP for one query against the "ground truth" name (the first one)."""
    q = query_names[0]
    sims = sorted(truth_names, key=lambda t: -cosine(vec[q], vec[t]))
    topk = sims[:k]
    hits = 1 if q in topk else 0
    if not hits:
        return {"hits": 0, "ap": 0.0, "topk": topk}
    # 单条 ground truth：AP = 1 / rank（rank 从 1 起）。此前误把 rank 当 AP，
    # 会让 mAP 大于 hit@k（例如 rank=2 记 2.0），与 Rust bench 口径不一致。
    rank = topk.index(q) + 1
    return {"hits": 1, "ap": 1.0 / rank, "topk": topk}


def run(truth: dict, codec: str, k: int) -> dict:
    names: list[str] = []
    for dag in truth["dags"]:
        names.extend(dag["node_names"])
    # 去重 + 排序（确保位置一致，vec/排序稳定）
    names = sorted(set(names))

    vec = make_vectors(names, codec)
    truth_names = names

    total_hit = 0
    total_ap = 0.0
    per_quality = {"a": [0, 0.0], "b": [0, 0.0]}
    by_id: dict[str, dict] = {}

    for q in truth["queries"]:
        r = ap(q["pattern_set"], truth_names, vec, k)
        rec = {"hit": r["hits"], "ap": r["ap"], "topk": r["topk"]}
        by_id[q["id"]] = rec
        total_hit += r["hits"]
        total_ap += r["ap"]
        qa = q.get("quality", "a")
        if qa in per_quality:
            per_quality[qa][0] += r["hits"]
            per_quality[qa][1] += r["ap"]

    n = max(1, len(truth["queries"]))
    cnt: dict[str, int] = Counter(x.get("quality", "a") for x in truth["queries"])
    result = {
        "codec": codec,
        "hit@k": round(total_hit / n, 4),
        "mAP@k": round(total_ap / n, 4),
        "k": k,
        "by_quality": {
            q: {
                "hit@k": round(per_quality[q][0] / max(1, cnt[q]), 4),
                "mAP@k": round(per_quality[q][1] / max(1, cnt[q]), 4),
            }
            for q in cnt
        },
        "n_queries": n,
    }

    # 双线输出：JSON（machine-readable）+ 单行摘要（human-readable）
    print(json.dumps(result, indent=2, ensure_ascii=False))
    print("summary: hit@{}={:.4f} mAP@{}={:.4f} (queries={}, k={})".format(k, result["hit@k"], k, result["mAP@k"], n, k))
    return result


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--truth", required=True, help="ground truth JSON（见 ground_truth_gen.py）")
    ap.add_argument("--codec", default="none", help="none|name_hash|name_hash_dense16")
    ap.add_argument("--k", type=int, default=5, help="hit@k / mAP@k")
    ap.add_argument("--case", action="append", help="过滤 query id（可重复，如 q-lib-case-backwards-00）")
    args = ap.parse_args()

    truth = json.loads(Path(args.truth).read_text(encoding="utf-8"))
    if args.case:
        wanted = set(args.case)
        truth["queries"] = [q for q in truth["queries"] if q["id"] in wanted]
    run(truth, args.codec, args.k)


if __name__ == "__main__":
    main()
