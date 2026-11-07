# Consensus 控制面基线（ADR-018 6.4 量化证据 / ADR-019 现状）

日期：2026-11-08  
环境：本地开发机（与 `lightweight-baseline-2026-11-08-post-v040.md` 同期记录方式）；mock proposer（零延迟 ack），测量 `ClusterConsensus::propose` 路径开销（含 async/timeout/quorum 计数逻辑）。

## 命令

```bash
env -u RUSTUP_TOOLCHAIN cargo test -p sisyphus-bridge --no-default-features \
  --lib bench_consensus_5_nodes -- --nocapture
```

## 结果

- 用例：`consensus::tests::bench_consensus_5_nodes`
- 拓扑：5 节点（1 leader + 4 peers），全部 ack，`propose_timeout=1s`
- 迭代：1000 次 `propose("k", json!(i))`
- 观测：`1000 ops in 4.628215072s (4.628215ms/op, 216 ops/sec)`（单次运行快照）

## 结论（6.4）

- 当前 `propose` 为**控制面路径**，生产默认未配置 proposer；ADR-019 安全修复后，无 proposer 的 leader `propose` 返回明确错误（不再虚假 `Ok(())`）。
- 本基线用于量化记录，**未触发**需要代码优化的证据；未进行推测性优化（符合 ADR-018 6.4）。
- 若未来启用 ADR-019 控制面（满足准入条件），应在真实网络/故障注入下重建延迟、超时与多数派行为基线。

## 6.5 状态

- ADR-019 已起草（准入条件/门禁）。**未实现**满足准入条件的生产 proposer、N=3 分区/leader 变更/混沌验证矩阵与回滚开关——因未指定触发的强一致业务需求；禁止跳过门禁启用。
