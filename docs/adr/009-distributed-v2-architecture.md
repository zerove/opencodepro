# ADR-009: 分布式 v2 架构草案（peer 协议 + Experience CRDT + 共识）

**Status**: Partially Accepted (2026-09-05) — § 2 Experience CRDT 核心已实现，§ 1 peer 协议留待 v0.3+
**Deciders**: core team
**Supersedes**: N/A

## Context

Sisyphus 当前 `ClusterConsensus` 是 no-op（仅 logging），`share_experience` 是单节点发布到 registry REST API，**多节点同步缺失**。阶段 8.3 计划三项目标：

1. **peer 协议真实环境测试**（3 节点 bridge cluster 互连）
2. **experience bank 跨实例同步**（CRDT 或 last-writer-wins）
3. **共识机制 benchmark**（5 节点 latency / throughput）— **已 2026-09-05 完成**（commit 7feadf2）

本 ADR 给出 1 + 2 的设计草案，留待 v0.3+ 实现。

## Decision

### 1. Peer 协议（8.3.1）

**位置**: `crates/sisyphus-bridge/src/peer/`

**目标**: 3+ 节点 bridge 实例互相发现 + 状态同步（不依赖外部 registry）

**协议**:
- **gRPC bidirectional streaming**: `ClusterStream(stream ClusterEvent)` bidirectional RPC
- **mDNS / DNS-SD 服务发现**: 多播 `224.0.0.251:5353` UDP，节点加入即广播 `_sisyphus-bridge._tcp.local`
- **事件类型**:
  - `PeerJoined { node_id, addr }`
  - `PeerLeft { node_id }`
  - `StateProposal { key, value, vector_clock }`（跨节点状态变更）
  - `ExperienceSync { exp_key, version, payload }`

**mDNS 优势**:
- 零配置（LAN 自动发现）
- 不依赖外部服务
- TTL-based 故障检测（节点下线自动移除）

**Proto 草案** (crates/sisyphus-bridge-api/proto/cluster.proto):

```proto
service Cluster {
  rpc Join (JoinRequest) returns (JoinResponse);
  rpc Stream (stream ClusterEvent) returns (stream ClusterEvent);
}

message JoinRequest { string node_id = 1; string addr = 2; }

message ClusterEvent {
  oneof event {
    PeerJoined peer_joined = 1;
    PeerLeft peer_left = 2;
    StateProposal state_proposal = 3;
    ExperienceSync experience_sync = 4;
  }
}

message StateProposal {
  string key = 1;
  bytes value = 2;
  map<string, uint64> vector_clock = 3;
}
```

### 2. Experience CRDT（8.3.2）

**目标**: 多节点共享 experience 时，无冲突且最终一致

**算法选择**: **Last-Writer-Wins with vector clock**（LWW-Element-Set）

**理由**:
- 单字段 semantically idempotent（同 exp_key 重复 sync 无害）
- LWW 比 OR-Set 简单（无需 tombstones）
- Vector clock 提供因果序（a→b 不被颠倒）

**数据结构** (`crates/sisyphus-types/src/experience.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceTuple {
    // 现有字段...
    pub exp_key: String,
    /// Vector clock: 每个 node_id 的最近 seen version
    #[serde(default)]
    pub vector_clock: BTreeMap<String, u64>,
    /// Local version for LWW
    #[serde(default)]
    pub version: u64,
}
```

**合并规则** (sync 时):
- 同 exp_key 收到多个版本 → 取 (vector_clock, version) 字典序最大者
- 收到的 vector_clock 不小于本地 → 替换；否则丢弃（已被更新的覆盖）

**冲突处理**:
- LWW 不检测冲突，但通过 vector clock 提供"happened-before"
- 失败语义：用户看到的是最新写入者的版本，不保证因果序一致

### 3. 共识（8.3.3 — 已完成）

`ClusterConsensus` (commit 7feadf2) 提供：
- `PeerProposer` trait（可注入 mock）
- `with_proposer()` 构造方法
- `add_peer() / remove_peer() / cluster_size()`
- majority quorum 校验 + 超时
- 5 节点 bench：4.62ms / 1000 ops（mock 零延迟）

**注**: 当前 propose 走 sequential iteration peer，**真实 gRPC 实现** 待 8.3.1 peer 协议落地后接入。

## 实施路径（v0.3 候选）

### Phase A (1 周): peer 协议骨架
1. 新增 `cluster.proto` + 生成代码
2. `sisyphus-bridge/src/peer/discovery.rs` (mDNS via `mdns-sd` crate)
3. `sisyphus-bridge/src/peer/cluster_service.rs` (gRPC service impl)
4. 集成到 `bridge_service.rs`：start 启动 discovery + cluster stream

### Phase B (1 周): Experience CRDT
1. `ExperienceTuple` 加 `vector_clock` + `version` 字段
2. `sisyphus-bridge/src/peer/experience_sync.rs` 合并逻辑
3. 集成到 `share_experience_impl`：本地 store + 广播 peer
4. 测试：模拟 2 节点 + 100 个 exp 双向 sync，验证最终一致

### Phase C (3 天): 端到端集成测试
1. Docker Compose：3 节点 bridge cluster
2. 集成测试：3 节点各 capture_experience → 验证 sync
3. bench：3 节点 end-to-end sync latency

## 风险与权衡

### Risk 1: mDNS 在 K8s 内可能不工作
- K8s pod IP 多播被限制
- **缓解**: 提供静态 `peers: ["node-1:50051", "node-2:50051"]` 配置 fallback

### Risk 2: LWW 不能完全避免数据丢失
- 离线节点恢复可能错过同步
- **缓解**: 启动时强制全量 sync（每个 peer 发送本地所有 exp）

### Risk 3: 3 节点测试需要 docker-compose + 网络
- CI 不能直接跑（资源限制）
- **缓解**: 标记 `#[ignore = "requires 3-node cluster"]`，本地/CI nightly 验证

## 替代方案

### 8.3.1 peer 协议替代
- **静态配置**: `[peers] = ["node-1:50051"]` — 简单但缺灵活性
- **外部 registry**: ETCD/Consul — 增加运维依赖
- **决策**: mDNS + 静态 fallback（平衡零配置与可调试性）

### 8.3.2 CRDT 替代
- **OR-Set**: 支持删除（更复杂，需要 tombstones）
- **LWW 不带 vector clock**: 简单但无因果序
- **决策**: LWW + vector clock（sufficient for current semantics）

## References

- ClusterConsensus 当前实现: `crates/sisyphus-bridge/src/consensus.rs`
- 5 节点 bench baseline: `docs/benchmarks/consensus-v0.2.0-baseline.md`
- TODO 阶段 8.3: `TODO.md` line 224-237
- LWW-Element-Set CRDT: https://en.wikipedia.org/wiki/Conflict-free_replicated_data_type