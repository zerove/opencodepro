# Bench Dashboard

Sisyphus 使用 [Criterion](https://github.com/bheisler/criterion.rs) 做 benchmark。每个 bench 生成详细 HTML 报告（分布、回归检测、对比图表）。

## 本地生成

```bash
# 跑全部 bench + 生成 dashboard（首次约 5-10 分钟）
./scripts/bench-dashboard.sh

# 仅跑单个 bench
./scripts/bench-dashboard.sh core_benches

# 自定义输出路径
OUTPUT_DIR=/tmp/bench ./scripts/bench-dashboard.sh
```

生成的 dashboard：

- `docs/benchmarks/dashboard/index.html` — 列出所有 bench 的入口
- `target/criterion/<group>/<name>/report/index.html` — 每个 bench 的详细报告

直接用浏览器打开 `docs/benchmarks/dashboard/index.html` 查看。

## CI 工作流

`.github/workflows/ci.yml` 的 bench 阶段：

1. 从 cache 恢复 main 分支的 baseline
2. 跑 `cargo bench -- --save-baseline current` 生成当前结果
3. 用 [critcmp](https://github.com/BurntSushi/critcmp) 与 main 比较
4. 检测 >5% 回归并 PR comment
5. 在 main 上把当前结果保存为新 baseline

详细工作流见 `.github/workflows/ci.yml`（`bench` job）。

## 关键 bench 类别

| Bench | 用途 | 关键 metric |
|-------|------|-------------|
| `dag_execution` | DAG 节点执行 | 节点数 vs 吞吐 |
| `wasm_execution` | WASM 工具调用 | fuel 消耗 |
| `mfs_storage` | 多层存储读写 | 延迟 by size |
| `v2_adaptive_depth` | 自适应深度控制器 | 收敛速度 |
| `v2_anomaly_detector` | 异常检测器 | 检测延迟 |
| `v2_branch_predictor` | 分支预测 | 准确率 |

完整列表 + 历史基线见 [`docs/benchmarks/`](https://github.com/zerove/opencodepro/tree/master/docs/benchmarks)。

## 贡献新 bench

1. 在 `crates/sisyphus-core/benches/core_benches.rs` 加 bench 函数
2. 用 `#[bench]` 或 `c.bench_function("name", |b| b.iter(|| { ... }))` 标记
3. 跑 `./scripts/bench-dashboard.sh` 验证 HTML 输出
4. 提交 PR；CI 会自动跑并与 main baseline 比较

回归 >5% 会被 `bench` job 警告（不阻塞），<5% 静默通过。