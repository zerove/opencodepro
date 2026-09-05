# ClusterConsensus Benchmark (v0.2.0)

Generated: 2026-09-05

**Hardware:** Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz, 19.5 GiB RAM
**Rust:** nightly (via rust-override)
**Test framework:** tokio::test + std::time::Instant（无 criterion）

## 5 节点 propose 延迟（TODO 8.3.3）

测试：`bench_consensus_5_nodes`（`crates/sisyphus-bridge/src/consensus.rs`）

| 配置 | 节点数 | peers | propose 次数 | 总耗时 | 单次 | throughput |
|------|------|-------|-------------|--------|------|-----------|
| mock zero-delay ack | 5 | 4 | 1000 | 4.62s | 4.62ms | 217 ops/s |

### 算法分析

- 4 次并发 peer propose（sequentially iterated, future 化后 join）
- 每次 propose:
  - 1× `leader_id.read().await` (RwLock read)
  - 1× `peers.read().await` for snapshot
  - N× `proposer.send_proposal().await` (mock 零延迟返回)
  - 1× `proposer.read().await` 获取 mock 句柄
- 4.62ms / 4 peers ≈ 1.15ms per peer（mock 零延迟理论应该 < 10µs）

### 实际瓶颈（推测）

**tokio RwLock await overhead**：每次 `.read().await` 即使是 read lock 也要：
- 获取锁 + 进入 async context
- 创建 Future
- 调度 + 唤醒

**1000 次 propose 总共 ~5000 次锁操作** → 4.62s，平均 0.92ms/锁。这与实测 1.15ms/per-op 吻合。

### 优化方向（未实施，留待 v0.3+）

1. **批处理 propose**：合并多次 propose 减少锁次数
2. **缓存 leader + peers snapshot**：避免每次读锁
3. **parking_lot::RwLock**：替换 tokio::sync::RwLock（同步锁在 async drop 时不需 await）
4. **真实的 gRPC proposer**：当前是 mock，真实场景会有 ~ms 级网络延迟，sync overhead占比下降

### 集成到 CI

`cargo test -p sisyphus-bridge --lib consensus::tests::bench_consensus_5_nodes -- --nocapture` 跑出基线。
对 future PR：单次 propose > 10ms 即 fail（当前 mock 远低于该阈值）。

### 算法复杂度

- 领导者选举: O(N)（min on HashSet）
- propose: O(N) peer 串行遍历（可并行化为 O(1)）
- 节点增删: O(N) leader 重新选举

### 安全性

- 仅 leader 可 propose（单写者）
- quorum = majority ack（容错 (N-1)/2）
- propose_timeout 默认 1s（避免永久阻塞）

## 用法

```rust
use sisyphus_bridge::consensus::{ClusterConsensus, PeerProposer};

// 自定义 proposer（生产用 gRPC client）
struct GrpcProposer { client: tonic::Client }
#[async_trait]
impl PeerProposer for GrpcProposer {
    async fn send_proposal(&self, peer: &str, key: &str, value: &Value) -> bool {
        // 真实 gRPC 调用
        self.client.commit(peer, key, value).await.is_ok()
    }
}

let consensus = ClusterConsensus::with_proposer(
    "node-a".into(),
    vec!["node-b".into(), "node-c".into()],
    Some(Arc::new(GrpcProposer { client })),
    Duration::from_secs(1),
);
```