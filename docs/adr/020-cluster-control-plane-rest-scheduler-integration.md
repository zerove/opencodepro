# ADR-020: Cluster control plane — REST 写入 / scheduler lease integration 设计

状态：**Accepted**（2026-11-10 定夺：REST 写入保持 **否** / 调度器租约接入保持 **B = 不接入**；设计预留 `SubmitControlLease` 全链路）

日期：2026-11-08
关联：`docs/adr/019-cluster-consensus-control-plane.md`（控制面门禁与 v1 租约语义）

## 背景

阶段 6.5（v1 租约控制面）已落地：
- A = gRPC/内部 + 只读状态（`ProposeControl`/`SubmitControlLease`/`GetControlLease`）；
- B = 不接入调度器（`cluster.control_plane.enabled=false` 默认）。

期间明确决策：**REST 写入 = 否**；**调度器接入 = B（不接入）**。

## 决策点

1. **REST 写入**：是否新增/开放写端点（如 `POST /api/cluster/control/leases`），由 REST 层
   调用控制面写入？
   - 现状：`GET /api/cluster/control/leases/:key` 只读；REST 写入=否。
   - 若启用：必须复用 `SubmitControlLease`（多数派 ack + fencing + `request_id` 幂等），
     鉴权复用 `mtls_subject_allowlist`；OpenAPI 新增写端点；受控环境。

2. **调度器租约接入（B 的延续/细化）**：是否让调度器通过 gRPC `SubmitControlLease` 全链路
   申请/续期/释放租约（`lease.acquire/renew/release`）？
   - 当前：B=不接入（`cluster.control_plane.enabled=false`）。
   - 若要接入：需将调度器生命周期与租约 TTL/fencing 对齐；超时/不确定=错误，不判定成功。

## 建议方向（更新：均已定夺，见「决策」）

- REST 写入：**保持否**（已在阶段 6.5 决策；写端点一旦开放需补齐鉴权/审计/OpenAPI/契约测试，
  属于另一次受门禁影响的变更）。
- 调度器接入：**保持 B=否** 但在设计上预留 `SubmitControlLease` 全链路（acquire/renew/release）
  能力（已由 `ControlPlaneCoordinator::propose_lease` 提供），供后续阶段启用。

## 决策（2026-11-10，阶段负责人 + 用户定夺）

- **REST 写入**：保持 **否**（阶段 6.5 决策延续；未新增/开放写端点，只读
  `GET /api/cluster/control/leases/:key` 保留）。
- **调度器租约接入**：保持 **B = 不接入**（`cluster.control_plane.enabled=false` 默认），
  但保留 `SubmitControlLease` 全链路（`lease.acquire` / `lease.renew` / `lease.release`，
  多数派 ack + fencing + `request_id` 幂等）作为后续阶段的预留接口。

## 未决问题

1. 若未来启用 REST 写入：是否保留「仅受控环境 + mTLS allowlist」边界，或另行 ADR？
2. 若未来启用调度器接入：leader 变更后调度器如何感知（需 member-change/leader 故障模型）？
3. 与既有任务调度（`Task`/`Mission`）的租约键空间设计（`key` 命名：`lease:task/...`）。

## 关联

- ADR-019（控制面门禁/租约/鉴权）、ADR-018（经验聚合/反熵）。
- 阶段 6.5 决策记录：`docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md`。

## 状态

**Accepted**（2026-11-10 定夺：REST 写入保持 **否** / 调度器租约接入保持 **B = 不接入**；设计预留 `SubmitControlLease` 全链路）。

- REST 写入：保持 **否**（阶段 6.5 决策延续；未新增/开放写端点，只读 `GET /api/cluster/control/leases/:key` 保留）。
- 调度器接入：保持 **B = 不接入**（生产默认仍关闭），但保留 `SubmitControlLease` 全链路
  （`lease.acquire/renew/release`）作为后续阶段预留接口。

未进入实现阶段：该 ADR 只定夺边界，未开放 REST 写端点 / 调度器接入。
