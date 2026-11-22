#!/usr/bin/env python3
"""ADR-022 实验基础：合成 SOP DAG 生成器 + 地面真相（ground truth）。

可重复（确定性 RNG），无网络/LLM，零依赖（仅 stdlib）。输出 `evals/ground_truth_<seed>.json`。

设计详见 `docs/plans/adr022-sop-embedding-experiment.md` §2.1（DAG 合成规范、节点名风格、Query 生成）。

可选生成边界（`--dags 4 --queries 3`）：纯向前 + 确定性回退的 Library-case 兜底稳定性基线，
与 A/B 集合互斥（见 §5.3）。真实集默认：8 DAGs / 80 queries。
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ── DAG 合成规范（可重复）─────────────────────────────────────────

_NAME_STYLE_WORDY = [
    "run_phase", "run_sink", "read_context", "validate_config",
    "check_health", "read_file", "execute_sql", "store_entry",
]
_NAME_STYLE_ABBR = [
    "stg_mig", "rot_now", "lsink_v", "ex_sql", "lnk_probe", "dbg_qry", "vld_cnt",
]
_NAME_STYLE_HASHY = ["t_16a2f", "n_8f3b", "s_7d1e", "d_9c4f"]


def _node_id(name: str, dag_id: str, idx: int) -> str:
    """节点唯一 ID（名称哈希，防止不同 DAG 同名）。"""
    h = hashlib.blake2b(f"{dag_id}:{name}".encode(), digest_size=8).hexdigest()
    return f"node-{h}"


def _node_name(rng: random.Random, dag_id: str, i: int, style: str) -> str:
    if style == "wordy":
        return f"{rng.choice(_NAME_STYLE_WORDY)}_{rng.choice(['alpha', 'beta', 'gamma', 'delta'])}"
    if style == "abbr":
        base = rng.choice(_NAME_STYLE_ABBR)
        if rng.random() < 0.5:
            return f"{base}_{i:02d}"
        return f"{base}-{rng.choice(['a', 'b', 'c', 'd'])}"
    return rng.choice(_NAME_STYLE_HASHY) + f"{i:02d}"


def _node_type(rng: random.Random, i: int) -> str:
    if i < 2 and rng.random() < 0.7:
        return "resource"
    return rng.choice(["checkpoint", "action", "scanner", "evaluator"])


def _leaf_name(rng: random.Random) -> str:
    return f"{rng.choice(['handle', 'deploy', 'migrate', 'scale', 'rotate'])}_leaf_{rng.randint(100, 999)}"


def generate_dag(seed: int, dag_id: str, rng: random.Random) -> dict:
    """生成一条 DAG（§2.1：n 15-60 节点，复杂度 ~ ln(n)）。"""
    n = rng.randint(15, 60)
    names: list[str] = []
    types: list[str] = []
    seen: set[str] = set()
    style = rng.choice(["wordy", "abbr", "hashy"])

    for i in range(n):
        for _ in range(20):
            nm = _node_name(rng, dag_id, i, style)
            if nm not in seen:
                break
        seen.add(nm)
        names.append(nm)
        types.append(_node_type(rng, i))

    if rng.random() < 0.6:
        names[0] = f"{_leaf_name(rng)}_entry"
    if rng.random() < 0.6:
        names[-1] = f"{_leaf_name(rng)}_exit"

    edges: list[dict] = []
    prev = 0
    for i in range(1, n):
        p = rng.uniform(0.2, 0.8)
        if i > 1 and rng.random() < p:
            prev = max(0, i - rng.randint(1, 3))
        edges.append({
            "u": _node_id(names[prev], dag_id, prev),
            "v": _node_id(names[i], dag_id, i),
            "taxonomy": rng.choice(["new_node", "new_edge", "run_node", "run_sink_fatal", "success_sink_partial"]),
            "op_id": _node_id(names[i], dag_id, i) + ":op",
        })

    # path_entropy ≈ Shannon entropy of out-degrees（§2.1，轻量估计）
    out_degs = [2] * max(1, round(n * 0.3))
    out_degs += [1] * (n - sum(out_degs))
    pe = round(sum(-float(x / n) * math.log2(x / n) for x in out_degs if x), 4) if n else 0.0

    op_counts: dict[str, int] = {}
    for e in edges:
        op_counts[e["taxonomy"]] = op_counts.get(e["taxonomy"], 0) + 1

    return {
        "id": dag_id,
        "node_names": names,
        "node_types": types,
        "edges": edges,
        "op_counts": op_counts,
        "path_entropy": pe,
    }


def generate_backwards_library_case() -> dict:
    """Library 兜底：向前层 + 确定性回退边（§5.3 Library Case）。"""
    names = [f"lib_case_node_{i}" for i in range(10)]
    edges = [
        {"u": names[0], "v": names[1], "taxonomy": "new_node", "op_id": names[1] + ":op"},
        {"u": names[1], "v": names[2], "taxonomy": "new_node", "op_id": names[2] + ":op"},
        {"u": names[2], "v": names[3], "taxonomy": "new_node", "op_id": names[3] + ":op"},
        {"u": names[3], "v": names[4], "taxonomy": "new_node", "op_id": names[4] + ":op"},
        {"u": names[4], "v": names[0], "taxonomy": "new_edge", "op_id": names[0] + ":op"},
    ]
    return {
        "id": "lib-case-backwards",
        "node_names": names,
        "node_types": ["resource", "action", "scanner", "evaluator"],
        "edges": edges,
        "op_counts": {"new_node": 4, "new_edge": 1},
        "path_entropy": 1.0,
    }


def generate_queries(
    seed: int, n_dags: int, per_dag: int, rng: random.Random, backwards_case: bool
) -> list[dict]:
    """生成查询参考样本（§2.1）：名称相似度轮廓 + 质量标签。"""
    queries = []
    for q_id in range(n_dags):
        src_dag = f"eval_sop_{q_id:02d}"
        for _ in range(per_dag):
            name = _node_name(rng, src_dag, 0, "wordy")
            pads = [rng.choice(["near", "far", "deep"]) + str(rng.randint(0, 9)) for _ in range(rng.randint(0, 3))]
            queries.append({
                "id": f"q-{q_id:02d}-{len(queries):02d}",
                "name": name,
                "pattern_set": [name] + pads,
                "quality": rng.choice(["a", "b"]),
            })
    if backwards_case:
        # Library backwards case（§5.3）：确定性名称，10 个节点。
        # 只在显式开启时追加：否则 8-DAG A/B 子集的查询会引用语料中不存在的节点。
        lib_case = generate_backwards_library_case()
        for i, nm in enumerate(lib_case["node_names"]):
            queries.append({
                "id": f"q-lib-case-backwards-{i:02d}",
                "name": nm,
                "pattern_set": [nm],
                "quality": "a",
            })
    return queries


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--dags", type=int, default=8, help="DAG 数")
    ap.add_argument("--queries", type=int, default=10, help="查询样本数")
    ap.add_argument("--backwards-case", action="store_true", help="追加 Library backwards case")
    ap.add_argument("--out", type=str, default="", help="输出路径（默认 evals/ground_truth_<seed>.json）")
    args = ap.parse_args()

    rng = random.Random(args.seed)
    dags = [generate_dag(args.seed, f"sop-{i:03d}", rng) for i in range(args.dags)]
    if args.backwards_case:
        dags.append(generate_backwards_library_case())
    queries = generate_queries(args.seed, args.dags, args.queries, rng, args.backwards_case)

    out = args.out or str(ROOT / "evals" / f"ground_truth_{args.seed}.json")
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    payload = {"seed": args.seed, "dags": dags, "queries": queries}
    Path(out).write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print("wrote {} (dags={}, queries={})".format(out, len(dags), len(queries)))


if __name__ == "__main__":
    main()
