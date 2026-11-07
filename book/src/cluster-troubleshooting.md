# 集群排障（Phase 6.3，只读观测）

本页用于排查多实例协调与经验/策略同步问题。依据 ADR-009/010/011/015/016/017/018；
观测优先，**不改变传播、合并或决策语义**。

## 1. 首先查看状态快照

```http
GET /api/cluster/status
```

关键字段（只读）：
- `enabled` / `cluster_name` / `config`：集群配置（含 `peer_ttl_secs`、`strategy_sync_interval_secs`、静态 peers）。
- `peer_manager.self_node_id|leader_id|is_leader|cluster_size`：本地 peer 管理视图；`leader_id`/`is_leader` 为字典序/trait 观测视图，**不是强一致互斥依据**（ADR-018）。
- `peer_manager.state_proposal_broadcasts`：本节点发出的 `StateProposal` 事件数（ADR-017 Phase F）。
- `peer_manager.experience_sync`（Phase 6.2b/6.3）：
  - `broadcasts`：本地产生并发出的经验同步事件数。
  - `inbound_delivered`：从远端投递到**入站总线**的事件数（不计入 `broadcasts`）。
  - `inbound_applied`：CRDT 合并后实际写入本地（`Applied`）。
  - `inbound_rejected`：拒绝总数 = `inbound_rejected_deserialize + inbound_rejected_quality + inbound_rejected_apply`。
  - `inbound_rejected_deserialize`：payload 反序列化失败。
  - `inbound_rejected_quality`：联邦质量门拒绝（ADR-016，启用 `FederatedAggregatorConfig` 时）。
  - `inbound_rejected_apply`：落库/应用错误。
  - 说明：`Kept`/`Equal`（本地更新或幂等）**不计入 rejected**，属于正常 CRDT 收敛。
- `peers[]`：`node_id`、`grpc_addr`、`joined_at`、`last_seen_secs_ago`。

单机/未启用集群时返回 200，`enabled=false`，`peers=[]`，`cluster_size=1`，计数通常为 0。

## 2. 常见症状与判读

| 症状 | 观察点 | 可能原因 / 处理 |
|---|---|---|
| `peers` 为空或 `cluster_size=1` | `enabled`、`config.static_peers`、`last_seen_secs_ago` | 集群未启用；静态 peer 不可达；`peer_ttl_secs` 过期被回收。检查网络/gRPC 地址与 `[cluster]` 配置。 |
| `broadcasts` 增长但对端无数据 | 对端 `inbound_delivered`、连通性 | 发送侧有事件但对端未收到：拨号/流建立失败、拓扑隔离。结合 peer 列表与日志。 |
| `inbound_delivered` 增长但 `applied` 不增长 | `inbound_rejected_*`、`Kept/Equal` 语义 | 关注拒绝分解：`deserialize` 查 payload/版本兼容；`quality` 查 ADR-016 联邦质量配置与经验质量；`apply` 查存储/落库错误。`Kept/Equal` 多为正常（本地已新或幂等）。 |
| `state_proposal_broadcasts` 异常高 | 节点数/拓扑、ADR-017 Phase F/G | 全连接下 fan-out 放大属预期观察项；定向转发未实现（ADR-017 Phase G 推迟，N≥16 或载荷变大再评估）。 |
| 事件延迟后短暂缺失 | 日志 `Lagged` / catchup | ADR-011：Lagged 触发 catchup/反熵补齐；计数为进程内观测，重启归零。持续 Lagged 需关注广播容量、处理速度与负载。 |
| 策略权重视图为空 | `/api/strategy/weights`、`strategy_sync_interval_secs` | `0`=只收不发；未启用集群或尚无节点上报时空视图正常（非 404）。见 REST API 与 ADR-017。 |

## 3. 相关接口与配置

- `GET /api/cluster/status`：协调/经验同步只读快照。
- `GET /api/strategy/weights`：联邦策略权重聚合视图（ADR-017）。
- `POST /api/experience/similar`：本地已收敛经验的相似查询。
- `[cluster]`：`enabled`、`node_id`、`peers`、`peer_ttl_secs`、`join_timeout_secs`、`strategy_sync_interval_secs`、`[cluster.strategy_selection]`、联邦经验聚合配置（按实际 `config.example.toml`/types 定义）。

## 4. 边界（避免误判）

- 计数器是**进程内原子计数**，进程重启归零；用于运维观测，不作为计费/审计真相源。
- 经验/策略同步为**最终一致**（CRDT + generation + 反熵）；短暂不一致不等于故障。
- 强一致 `propose/quorum` 控制面**未在生产路径启用**（ADR-018）；不要将 `leader`/`is_leader` 解释为互斥提交成功。
- 非目标（当前不做）：跨节点策略协商、定向转发（除非满足触发条件）、独立经验聚合进程。


## 5. 控制面（ADR-019，门禁关闭；A=gRPC/内部+只读）

- `cluster.control_plane.enabled` / `SISYPHUS_CLUSTER_CONTROL_PLANE_ENABLED` 默认 `false`；`GET /api/cluster/status` 的 `config.control_plane_enabled` 反映门禁状态。
- gRPC：
  - `Cluster.ProposeControl`：peer 本地 apply（proposer 调用）。门禁关闭/未装配返回 `accepted=false` + `DISABLED`/`UNAVAILABLE`，**不是**提交成功。
  - `Cluster.SubmitControlLease`：协调节点多数派提交；`ControlLeaseResponse.uncertain=true` 或超时/连接失败导致的不确定结果 **必须视为失败/未知，不得判定成功**。可用同一 `request_id` 重试（幂等），并用 `Cluster.GetControlLease` 查询当前租约状态；协调节点不缓存 `uncertain` 结果，跨节点幂等由各节点租约状态保障。
  - `Cluster.GetControlLease`：只读查询（`OK`/`NOT_FOUND`/`DISABLED`/`UNAVAILABLE`），用于不确定提交后的状态核对。
- v1 租约（`lease.acquire/renew/release`）：`request_id` 幂等、TTL、fencing generation；语义为**多数派 ack + 不确定=错误**（非 Raft 线性一致）。生产多节点 quorum/分区/混沌/连接池-TLS/成员变更集成与专用运行器证据未完成前不得生产启用。
