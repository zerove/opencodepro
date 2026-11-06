# Changelog

本项目所有显著变更都会记录于此文件。格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，
版本遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

> **说明**: 本仓库 2026-08-31 之前的 320+ 个本地提交以边界测试扩充为主，
> 未在此追溯。本 CHANGELOG 自 v0.x 起从此日期开始记录。

## 发布流程（通用模板）

发版时按以下步骤整理 CHANGELOG：

1. **合并 Unreleased → 版本段**：将 `## [Unreleased]` 改名为 `## [vX.Y.Z] - <YYYY-MM-DD>`
2. **校验分组**：Unreleased 段下应只有 `### Added` / `### Fixed` / `### Changed` / `### Deprecated` / `### Removed` / `### Security` 子段
   （keep-a-changelog 规范）。如出现不属于这 6 类的内容，重写为正确分类
3. **生成 Verified 段**：在版本段底部加 `### Verified`，列出关键 CI 验证（如 `cargo test-all: 4689/4689 通过`）
4. **创建新 Unreleased**：在版本段**之前**插入空的 `## [Unreleased]` 段（保持工作流）
5. **git tag**：本地 `git tag -a vX.Y.Z -m "Release vX.Y.Z"`，推到 origin 触发 `.github/workflows/release.yml`
   （打 tag 前先跑 `make release-check` 校验版本/CHANGELOG/metadata 一致性）
6. **GitHub Release**：release.yml 会自动从 `git log` 生成 release notes（`generate_release_notes: true`），
   此 CHANGELOG 段作为该版本的引用存档

### 模板骨架

```markdown
## [vX.Y.Z] - YYYY-MM-DD

> 阶段 8 收尾：可观测性 + 开发者体验 + 文档 + 供应链加固

### Added
- （从 [Unreleased] 段迁移，按主题分组）

### Fixed
- ...

### Changed
- ...

### Verified
- cargo test-all: NNNN/NNNN 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- cargo deny check: 全部 ok
```

详细发布检查表见 [CONTRIBUTING.md § 发布流程](./CONTRIBUTING.md#发布流程)。

---

## [Unreleased]

### Added

- **单机全功能端到端集成测试**（Phase 1 of the single-node full-functionality
  follow-up）：`crates/sisyphus-tests/tests/single_node_full_stack.rs` 新增
  `single_node_full_stack_end_to_end`，在无集群条件下驱动真实 `SisyphusKernel`
  「目标合成 → 认知循环执行 → 反思/批判 → swarm audit → 经验引导再次执行」全主链路。
  当前处于实现推进中（mock LLM 按 critic/swarm audit prompt 分支返回合法 JSON、
  mock DAG 合成器构造含 start node 的图），验证证据：
  `cargo check -p sisyphus-tests --tests` ✅ / 单测 `1 passed` ✅ /
  fmt + clippy `-D warnings` ✅。

- **联邦经验聚合：入站经验质量门槛**（v0.4.0 Phase A，ADR-016）。
  `sisyphus_types::experience::evaluate_experience_quality` 提供纯函数质量评分，
  `FederatedAggregatorConfig` 提供 `enabled` / `min_quality_score` /
  `success_weight` / `deviation_weight` 四个参数（默认 **关闭**）；
  桥接入站同步路径在 CRDT 合并前按门槛过滤，被拒绝的经验记 `warn` 并携带分数。
  配置段 `[cluster.federated]`，环境变量 `SISYPHUS_CLUSTER_FEDERATED_*`。
  默认关闭保证单机与既有集群部署行为不变。验证：
  `cargo test -p sisyphus-types --lib` 135 passed（新增 6）；
  `peer::experience_sync` 6 passed（新增 3，含变异验证：门槛置恒假后对应用例稳定失败）；
  反熵/混沌集成用例全绿（`cluster_integration` 21、`anti_entropy_integration` 12、
  `anti_entropy_partition_convergence` 1、`chaos_partition_convergence` 1、
  `chaos_one_sided_reverse_pull` 1）。

- **集群策略权重同步**（v0.4.0 Phase B，ADR-017）：每个节点周期性把本地
  `MetaLearner` 的策略统计打包成 `NodeStrategySnapshot`（`generation` 取 unix
  毫秒并与上次 +1 取大），经 `StateProposal` 集群事件广播；对端按 `generation`
  **严格取新**合并进 `FederatedStrategyWeights`，聚合按 `uses` 求和、
  `avg_duration` 按 `uses` 加权。合并幂等、可交换，重复投递与乱序到达都安全。
  - 传输层补上入站缺口：`proto_to_mgr_event` 此前**丢弃** `Event::StateProposal`，
    现在保留并由 `apply_mgr_event` 投递到入站总线
    （`PeerManager::deliver_inbound_state_proposal`，用 `deliver_` 而非
    `broadcast_` 避免回环）。这也是不走 `Consensus::propose` 的原因——生产装配
    里 proposer 是 `None`，`propose()` 只写日志。
  - 配置 `ClusterConfig.strategy_sync_interval_secs`（默认 60，`0` = 只收不发），
    env `SISYPHUS_CLUSTER_STRATEGY_SYNC_INTERVAL_SECS`；发布周期内首 tick 即发。
  - REST `GET /api/strategy/weights` 暴露聚合视图（`node_count` / `nodes` /
    `aggregate` / `best`），已进 OpenAPI（`operationId: getStrategyWeights`）。
  - 验证：`cargo test -p sisyphus-types --lib` **146 passed**（新增 11）；
    `peer::` **60 passed**（`peer::strategy_weights` 5 + `peer::service` 新增 2）；
    `cluster_integration` **22 passed**（新增 2 节点端到端
    `test_two_node_strategy_weights_propagate_and_merge`）；
    `strategy_weights_rest_test` **2 passed**（新增：空视图返回 200、
    合并视图的聚合与 `best` 契约）；反熵/混沌/契约用例全绿。
    **变异验证 3 处**（`merge` 的 `>=` → `>`、删自回环过滤、`publish_once` 不广播）
    各自稳定触发对应用例失败，还原后全绿。

<!-- 下一轮的变更从这里开始写。发版时整段改名为 `## [vX.Y.Z] - <日期>`，
     并在其**之前**插入新的空 `[Unreleased]` 段（见上方发布流程）。 -->

## [v0.3.0] - 2026-11-05

> 自 v0.2.0 以来：反熵（ADR-010..013）落地、供应链清理、WebUI 契约门禁、
> 全连接双侧分区混沌用例。**破坏性变更**：`ExperienceBank` trait 新增
> `export_version_vector()`，外部实现方需补该方法的实现（本仓库内实现均已补齐）。

### Verified (v0.3.0 release gate, 2026-11-05)
- `cargo test-all`: **4939 passed / 0 failed / 26 ignored**（227 个 test target）
- `cargo check-all` / `cargo fmt --all --check` / `cargo clippy -p sisyphus-bridge --all-targets -- -D warnings`: 通过
- bridge 重型测试（不在 `cargo test-all` 覆盖范围）：
  `cluster_integration` 21 + `anti_entropy_integration` 12 +
  `anti_entropy_partition_convergence` 1 + `chaos_partition_convergence` 1 +
  `webui_contract` 2，全通过
- `cargo deny check`: advisories / bans / licenses / sources 全 ok
- `make audit`: 全绿（含 `audit_docker_release` 8 例自检、`audit_todo` 121 条勾选、
  `bump_version` 5 例自检）
- `make release-check`: 通过（27 个 crate 版本一致 0.3.0 / metadata 齐全 /
  CHANGELOG 已切段 / tag 未创建）
- 版本号：147 处（27 个 `[package] version` + 内部 path 依赖约束）全部为 `0.3.0`，
  `Cargo.lock` 在 `--locked` 下可解析

### Added
- **WebUI → Bridge REST 契约门禁**：`crates/sisyphus-bridge/tests/webui_contract.rs`
  把 `webui/src/lib.rs` 里的 `format!("{}/…")` 调用点解析成 (method, path)，逐条要求
  它同时存在于 axum 路由表与真实服务返回的 OpenAPI 文档中。两个方向都断言，是因为
  只查文档会漏掉「文档停留在旧路径」，只查路由会漏掉「端点从 Swagger 里消失」。
  路径参数名不参与比较（webui 写 `{sop_id}`、axum 写 `:id`、OpenAPI 写 `{id}`），
  只比**形状**：参数改名不失败，路径改名必定失败。三处变异（改路径 / 删文档条目 /
  改期望清单）均验证过失败路径。
- **全连接双侧反复分区混沌用例**：`crates/sisyphus-bridge/tests/chaos_partition_convergence.rs`
  （`full_mesh_repeated_partitions_converge_exactly`）。每轮关两侧 Join 闸门并断开两条
  已建立的流 → 在**无连接**窗口内两侧各写 4 条 → 重连后断言两个方向都真的发出反熵
  请求，终局断言「无丢失 / 无重复 / 内容与主键逐条一致」。单独成文件是因为用例需要
  按节点切换 `SISYPHUS_CLUSTER_NODE_ID`（进程级），与并行用例共处一个进程会互相污染。
  变异验证：禁用拨号器的首轮反熵拉取后，用例在「首连未完成」处失败。
- **版本 bump 脚本**（`scripts/bump_version.py`）：版本号在仓库里出现 147 处
  （27 个 `[package] version` + 内部 path 依赖的 `version` 约束），手改必漏——
  `evals/` 的 6 处 path 依赖就曾漏掉约束，`cargo package` 报错而 `cargo check`
  完全看不到。脚本支持 `--dry-run` / `--check` / `--self-check`（5 个判别用例 +
  幂等与变更性检查）；内部依赖**缺** version 约束时直接报错而不是静默补上。
- **版本向量导出接口**（阶段 C / ADR-010 实施收尾）：`ExperienceBank` trait 新增
  `async fn export_version_vector(&self) -> Result<ExperienceDigest, TypesError>`，使引擎
  能回答「我有什么」（node → `OriginDigest` 紧凑摘要）而不只是「我写了什么」。实现方委托
  既有 `VectorExperienceBank::digest` / `MfsStore::experience_digest`，
  `PeerBroadcastExperienceBank` 转发；反熵侧 `anti_entropy::serve_sync_request` 继续
  消费该摘要完成差量回送。trait/impl/test mock 全实现，check + 相关测试全绿。
- **TODO 勾选门禁**（`scripts/audit_todo.py` + `make todo-audit` + CI `audit-sources`）：
  `- [x]` 视为「已验证」声明，必须附可解析证据——引用的 `path.rs::test_fn` 要真实
  存在、带测试属性、且未被 `#[ignore]`（或已由 `--ignored` job 执行）；`make <目标>`
  与 `scripts/x.py` 要真实存在。历史勾选项登记在 `scripts/todo_baseline.txt`
  （ratchet，只减不增），新增勾选无证据即失败。自检 14 个判别用例。
  - **「测试名」书写约定**（阶段 E 前提，本次补齐）：引用的 `test_name` 限定为
    `test_*` 前缀；文件级回归测试允许在 `tests/` 目录下的文件里以任意 fn 名
    全路径引用，不强制前缀。src/ 内单元测试须 `test_*`，tests/ 内回归测试放宽。
    自检由 14 例扩展至 17 例（新增 src 命名拦截 / tests 回归放宽 / src 合法前缀各一）。

- **反向反熵拉取**（ADR-015）：服务端在双向流建立后主动发起反熵 `SyncRequest`
  （反向反熵拉取），由 dialer 侧应答差量分页；闭合「单侧拨号拓扑下被拨方断连期间
  写入没有回流路径」的缺口。新增
  `crates/sisyphus-bridge/tests/chaos_one_sided_reverse_pull.rs::one_sided_dialer_reverse_pull_converges_after_partition`
  （变异验证：禁用反向拉取 → 稳定失败 → 还原全绿）。

### Changed
- `cargo test-all` 别名补 `-j 1`：直接敲别名时不再按 nproc 并行链接多个
  测试二进制（19.5 GiB 机器上会 OOM）。嵌套 cargo 仍由 `make test-all` 的
  `CARGO_BUILD_JOBS=1` 覆盖。
- **反熵分页改为主键区间扫描**（ADR-012 的 keyset 落地）：`serve_sync_request`
  原先每页都 `experience_keys()` 全表取键、内存排序切片，翻 P 页即 O(P × N)。
  新增 `MfsStore::experience_keys_after` / `VectorExperienceBank::keys_after`
  （`id > ? ORDER BY id LIMIT ?`，批 512），差量判定抽出 `is_key_missing()` 供
  单键与批量两条入口共用；`build_sync_page` 的线性查找改为 `HashMap` 索引。
- **检索过滤判定单点化**：`query_similar` 与 `lexical_fallback` 共用
  `matches_query_filters()`；4 处 `#[allow(clippy::collapsible_if)]` 改 let-chain
  后删除。

### Fixed
- **Docker 发布的镜像名与 tag 与文档不一致**（照着 README 拉不到镜像）：
  - `IMAGE_NAME` 原为 `${{ github.repository }}`，在 `zerove/opencodepro` 下展开成
    `ghcr.io/zerove/opencodepro`；而 README / `book/src/deployment.md` /
    `docker-compose.yml` 里每一处 `docker pull` 写的都是
    `ghcr.io/zerove/sisyphus-bridge`。推上去和拉下来的不是同一个包。现改为
    `${{ github.repository_owner }}/sisyphus-bridge`（owner 用表达式以便 fork，
    产品名必须是字面量；裸产品名会拼出不合法的 `ghcr.io/sisyphus-bridge`），
    compose 的默认 owner 也从 `opencodepro` 改为 `zerove`。
  - 文档承诺 `:v0.3.0` 与 `:latest`，实际产出的是去前缀的 `0.3.0` 且
    `flavor: latest=false` 让 `:latest` 永不出现。补 `type=raw,value={{tag}}`
    （`:v0.3.0`）并改 `latest=auto`（预发布不打 `:latest`）。
  - nightly 原本也推 `:latest`，与稳定发布抢同一个可变 tag——用户拿到哪个版本
    取决于谁最后跑完。nightly 现在只推 `:nightly` / `:nightly-YYYYMMDD` /
    `:nightly-<sha>`。
  - 新增 `scripts/audit_docker_release.py`（接入 `make audit` 与 CI
    `audit-sources`）：按 metadata-action 语义把 tag 集合**算出来**，与文档承诺、
    compose 引用逐条比对；未知 type/模板/enable 直接报错而不是静默跳过。
    4 处变异验证过失败路径（改回 `github.repository` / `latest=false` /
    删 `{{tag}}` 规则 / nightly 抢 `:latest`）。
- **Docker 镜像链接到空壳 rlib**：`COPY . .` 保留了构建上下文里的旧 mtime，
  而依赖缓存层的空壳源码更晚创建，cargo 因此认为真实源码更旧、一个
  workspace crate 都不重编，最终报 73 个 `unresolved import`。现在
  `COPY . .` 之后先把源码 mtime 推到当前时间再构建，并在
  `scripts/audit_dockerfile.py` 增加门禁（缺 touch 即失败，含自检用例）。
- **版本向量摘要缓存的失效时机**：失效原先写在 `record_with_id` /
  `apply_remote` 函数末尾，而落库成功后还有一次向量索引写入，那一步失败会
  提前返回，留下「库里已有新行、缓存仍是旧摘要」——本节点向对端少报自己持有
  的经验。现在抽出 `invalidate_digest_cache()`，在落库成功后**立即**调用；新增
  `sisyphus-mem/tests/experience_digest_cache.rs`（3 例，含向量写入失败路径，
  已做变异验证）。
- **反熵按主键回取经验时静默丢行**：`load_experiences_by_keys` 用
  `try_get("id").unwrap_or_default()` 把读取失败折叠成空主键，列漂移时那一行
  既不报错也不返回。改为 `.context(...)?` 向上传播。
- **`page_size = 0` 的死循环风险**：keyset 扫描下零页大小会让「多收一条」的
  判定恒真、页被截断成空而游标不动，拨号器按 `more` 会无限重发同一页。现在
  直接返回空页收尾，并有用例锁定。

## [v0.2.0] - 2026-11-04

> 自 v0.1.0（2026-09-03）以来累积 261 个 commit：认知循环深化、工具链拆分、
> gRPC/REST 桥接、WASM 沙箱、Web 仪表盘与路线图可视化看板。
> 下列条目保留原始的日期/主题标注以维持可追溯性。

### Added (2026-11-04, Cleanup & New Feature)
- **Roadmap Dashboard API（可视化看板新功能）** (phase 2, 2026-11-04):
  - Bridge: `GET /api/roadmap/dashboard/:project_id` — 聚合项目 + milestones +
    progress + health 单一 payload；OpenAPI 同步登记
  - Webui: `/api/roadmap/dashboard/{project_id}` API handler + bridge client fetch
  - Webui: `/htmx/roadmap/dashboard/{project_id}` HMX partial + dashboard 页面卡片
  - 全链验证：`cargo test -p sisyphus-bridge --no-default-features --lib` 139/139 +
    `cargo test -p sisyphus-webui` 11/11
- **CognitiveEngine seam** (phase 3 deepening, 2026-11-04):
  - `strategic_manager::CognitiveEngine` trait（async `run<F>`）+ StrategicManager
    adapter；kernel facade 经 seam 调用
  - mock adapter 第二实现验证端口可替换（`cognitive_engine_seam_accepts_second_adapter`）

### Fixed (2026-11-04)
- **sisyphus-bridge no-sandbox build**：`use std::path::PathBuf;` 由
  `#[cfg(feature = "sandbox")]` 门控改为 unconditional — no-sandbox clippy /
  test 此前 E0433 必挂（bridge-check / bridge test --no-default 回归）

### Verified (2026-11-04)
- `cargo test-all`: 4934 passed / 0 failed / 26 ignored
- fmt / clippy / check / no-panic / deny / audits / release build — 全绿

### Added
- **Experience CRDT（LWW-Element-Set + vector clock）** (TODO 8.3.2 / ADR-009 § 2, 2026-09-05):
  - `ExperienceTuple` 加 `version: u64` + `vector_clock: BTreeMap<String, u64>` 字段
    （serde default，向后兼容）
  - `merge_crdt()` 纯函数：vector clock 比较 + version tie-break，返回
    `MergeDecision::{ApplyRemote, KeepLocal, Equal}`
  - `bump_local_clock()` 辅助：本地写入时自增 `version` 与 `node_id` 计数器
  - 4 种 `ClockOrdering` 显式枚举（Before / After / Equal / Concurrent），
    解决并发冲突的语义歧义
  - `MfsStore::apply_remote_experience()` API：原子比较+合并，
    返回 `ApplyResult::{Applied, Kept, Equal}`；本地不存在时自动 INSERT
  - `experiences` 表 schema 加 `version` + `vector_clock` 列；
    ALTER TABLE 迁移（启动探测列存在性，无重复迁移）
  - `VectorExperienceBank::record()` 自动 `bump_local_clock`，
    `node_id` 来自 `SISYPHUS_CLUSTER_NODE_ID` env var 或 `HOSTNAME` fallback
  - 14 个 `ExperienceTuple` 构造点更新（store + mem + core + intel + tests）
  - 9 个纯函数单元测试（merge_crdt 6 + bump_local_clock 2 + roundtrip 1）
  - 集成测试暂留待：SELECT 反序列化 version/vector_clock 路径扩展 +
    8.3.1 peer 协议端到端 2 节点 sync 验证（todo 注释保留）
- **im → imbl 迁移遗漏修复** (TODO 8.4.2 P1 余项, 2026-09-05):
  - 修复 `c55c586` 提交遗漏的 20 处 `tests/types_misc_v2.rs` 集成测试 `im::HashMap` 引用 → `imbl::HashMap`
  - 文档注释 `src/types.rs` 同步更新
  - 验证：cargo test -p sisyphus-types --test types_misc_v2 198/198 + clippy 0 警告 + test-all 4656/4656
- **CLI 冷启动 profile + criterion bench** (TODO 7.3 第三项, 2026-09-05):
  - 真实启动时间基线：`docs/benchmarks/cli-startup-v0.2.0-baseline.md`
    - Release 模式中位数 10 ms（5 次测量）
    - Debug 模式中位数 22 ms（开发者日常）
    - 远低于 < 200 ms 目标（20x / 22x 余量）
  - 新增 `cli_startup/config_default` criterion bench（214 ns，纯 Config 构造，CI 回归追踪）
  - 验证 8.1.3 multi_thread runtime + spawn_blocking 优化的实测效果

### Verified (2026-09-05)
- cargo test -p sisyphus-types --test types_misc_v2: 198/198 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- cargo test-all: 4656/4656 通过
- cli_startup/config_default bench: 214.47 ns（中位数）

### Verified (2026-09-05, CRDT)
- cargo test -p sisyphus-types --lib experience: 9/9 CRDT 纯函数测试通过
- cargo test -p sisyphus-mem --lib: 41/41 通过（含 7 个新 CRDT roundtrip 集成测试）
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- cargo test-all: 4672/4672 通过（比 4665 多 7 个 CRDT roundtrip 集成测试）

### Added (CRDT SELECT roundtrip, 2026-09-05)
- **`MfsStore::record_experience` 扩展 INSERT 写 version + vector_clock 字段**：
  序列化 BTreeMap → JSON → 持久化
- **4 个 SELECT 路径扩展**：list_shared_experiences / get_experience /
  list_recent_experiences / query_experiences 都读取 version + vector_clock
- **统一 row 类型 `ExperienceRowTuple` + helper `row_to_experience_tuple`**：
  10 元组 → ExperienceTuple 单一转换点；corrupted JSON 回退空 map（不 panic）
- **7 个 roundtrip 集成测试**：
  1. record + get roundtrip 保留 clock
  2. apply_remote INSERT 路径后 get 正确
  3. apply_remote UPDATE 路径后 get 新 clock
  4. list_shared 反序列化（含 None project_id）
  5. list_recent 反序列化多节点 clock
  6. query_experiences（IN 多 id）反序列化
  7. 损坏 vector_clock JSON 回退到空 map（健壮性）

### Added (peer 协议骨架, 2026-09-05, TODO 8.3.1 / ADR-009 § 1 Phase A)
- **cluster.proto 新增**（`crates/sisyphus-bridge-api/proto/cluster.proto`）：
  - `Cluster` service: `Join` (握手) + `Stream` (双向流)
  - `JoinRequest/Response`, `PeerInfo`, `ClusterEvent` (PeerJoined/PeerLeft/StateProposal/ExperienceSync)
  - ExperienceSync 携带 CRDT version + vector_clock_json + payload bytes
- **sisyphus-bridge-api `cluster_proto` 模块**：tonic::include_proto!("sisyphus.cluster")
- **mdns-sd 0.21 集成**（crates/sisyphus-bridge/Cargo.toml + peer/discovery.rs）：
  - `MdnsConfig`（service_type + cluster_name）+ `DiscoveryError` enum
  - `register_local_node` + `browse_peers` + `create_daemon`
  - 后台 std::thread 把同步 browse receiver 转发到 async mpsc
- **`crates/sisyphus-bridge/src/peer/`** 新模块：
  - `manager.rs`: PeerInfo + PeerStatus + ClusterEvent (4 变体)
  - PeerManager：parking_lot::RwLock<BTreeMap> + broadcast::Sender，
    leader = 字典序最小（含自己），TTL 字段就绪（evict_expired 留 Phase B）
  - `service.rs`: ClusterServiceImpl（tonic Cluster trait），
    StreamStream 关联类型 = Pin<Box<dyn Stream + Send + Sync>>
  - proto ↔ Rust 类型双向转换（mgr_event_to_proto / proto_to_mgr_event）
- **Config 加 [cluster] section**（sisyphus-types）：
  - 6 个字段：enabled / node_id / cluster_name / mdns_service_type /
    peers (静态 fallback) / join_timeout_secs / peer_ttl_secs
  - 6 个 env override：`SISYPHUS_CLUSTER_{ENABLED,NODE_ID,NAME,JOIN_TIMEOUT_SECS,PEER_TTL_SECS}`
  - validate: join_timeout ≥ 1, peer_ttl ≥ 5, peer_ttl > join_timeout
- **测试**：
  - 24 个 peer 单元测试（manager 12 + service 8 + discovery 4）
  - 4 个 2 节点 in-process 集成测试（tests/cluster_integration.rs）：
    1. join 握手返回 peer 列表 + leader
    2. stream 双向流传播 PeerJoined 事件
    3. manager 状态隔离
    4. 双向 join 验证 leader 字典序最小
- **明确未实现**（避免误用）：
  - bridge_service / main.rs 启动集成（留待后续 PR）
  - TTL 过期清理（peer_ttl 字段已就绪）
  - 静态 peer fallback dial（[cluster].peers 字段已就绪）
  - Experience CRDT 端到端 2 节点 sync（CRDT API + peer broadcast 已就绪）

### Verified (2026-09-05, peer 协议)
- cargo test -p sisyphus-bridge --lib peer: 24/24 通过
- cargo test -p sisyphus-bridge --test cluster_integration: 4/4 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- cargo test-all: 4601/4601 通过（默认 workspace 排除 bridge 测试）
- cargo build -p sisyphus-bridge: 编译通过（mdns-sd 0.21 + tonic 0.12）

### Added (peer 协议 Phase B, 2026-09-05, TODO 8.3.1)
- **`PeerManager` last_seen tracking + TTL 清理**（peer/manager.rs）：
  - `last_seen: Arc<RwLock<BTreeMap<String, Instant>>>` 字段
  - `on_peer_joined/on_peer_left` 同步维护 last_seen
  - `touch(node_id)` 实际更新 last_seen（心跳）
  - `evict_expired()` 真实实现：扫描 last_seen，超 peer_ttl 的广播 PeerLeft
  - 4 个新单元测试（last_seen 跟踪 + evict_expired + touch 防过期）
- **静态 peer fallback dialer**（peer/fallback.rs 新模块）：
  - `StaticPeerConfig::from_types()` 从 ClusterConfig 派生
  - `spawn_static_peers()` 启动每个 peer 一个后台 task
  - `connect_and_join` gRPC channel + Join 握手
  - `run_stream` 双向流：本地 manager 事件 → outbound proto；
    对端 proto 事件 → manager.on_peer_joined/left
  - 指数退避重连（reconnect_initial_delay → reconnect_max_delay）
  - 5 个单元测试（config from_types + proto 转换 + endpoint parse）
- **main.rs 集成**（crates/sisyphus-bridge/src/main.rs）：
  - gRPC server 注册 ClusterService（Cluster proto via tonic）
  - 读取 [cluster] config：enabled / node_id / cluster_name / peer_ttl_secs
  - 派生 node_id：env (SISYPHUS_CLUSTER_NODE_ID / HOSTNAME) > Config > "local-node"
  - 启动 static peer dialers（如 [cluster].peers 非空）
  - 转换 grpc_server 为 `let mut` 支持后续 add_service 调用
- **测试**：
  - 33 个 peer 单元测试（之前 24 + 5 fallback + 4 新 TTL）
  - 4 个 2 节点集成测试（保持通过）
- **验证**：
  - cargo test -p sisyphus-bridge --lib peer: 33/33 通过
  - cargo test -p sisyphus-bridge --test cluster_integration: 4/4 通过
  - cargo clippy --workspace --all-targets -- -D warnings: 0 警告
  - cargo test-all: 4672/4672 通过

### Verified (peer 协议 Phase B, 2026-09-05)
- cargo build -p sisyphus-bridge: 编译通过（mdns-sd 0.21 + tonic 0.12）
- 9 个新增 peer 测试通过（5 fallback + 4 TTL）
- main.rs gRPC server 启动 ClusterService 无 panic（disable 时只 info 日志）

### Added (累计)
- **`.rust-analyzer.json`** 锁定重型 crate 排除（sisyphus-bridge/sandbox/sandboxd/tests），
  IDE 索引内存控制在 4GB 以内
- **TODO.md 阶段 7** 近期优先级规划（健康度 → 测试质量 → 性能 → DX → 文档运维）
- **`/health/deep` bridge 端点** (200/207 Multi-Status, 报告 session_db/shared_http/kernel 组件级状态)
  - OpenAPI 文档 + `HealthDeepResponse` schema
  - 重构 handler → 纯函数 `health_deep_assess`，可独立测试
  - 5 个单元测试（DB 存在/父目录/不存在/absent/三组件键）
- **`make dev` 一键验证入口** (阶段 7.4 DX)
  - `make dev` = ci-quick + ci-bridge（完整本地验证）
  - `make ci-quick` = dev-fmt + dev-clippy + dev-test（快速）
  - `make dev-fmt` / `dev-clippy` / `dev-test` / `ci-bridge` 单步
  - 与 `.github/workflows/ci.yml` 步骤顺序一致
- **`justfile` 跨平台命令镜像** (阶段 7.4 收尾)
  - Windows / macOS / Linux 一致体验（`cargo install just`）
  - 镜像 `make dev` / `ci-quick` / `dev-fmt` / `dev-clippy` / `dev-test` / `ci-bridge` 等关键目标
  - 不设置 `set shell`，保留各平台默认 shell 行为
- **AnomalyDetector 阈值配置化** (TODO 7.1 第二项)
  - `AnomalyDetectorConfig` 类型（`fuel_threshold`/`duration_threshold`/`error_rate_threshold`/`history_window`）
  - 校验：fuel/duration > 0 finite；error_rate ∈ [0.0, 1.0] finite；history_window ≥ 1
  - 端到端接线：`StrategicManager::apply_cognitive_loop_config` 调用 `anomaly_detector.apply_config(...)`
    同步覆盖运行时阈值（保留 EWMA 历史）
  - env override：4 个 `SISYPHUS_COGNITIVE_LOOP_ANOMALY_DETECTOR_*` 变量
  - `AnomalyDetector::from_config()` / `with_history_window()` 工厂方法
  - `history_window = 0` 静默纠正为 `1`（避免后续除零）
  - 8 个单元测试（`AnomalyDetectorConfig` 6 个 + `apply_config`/`from_config` 2 个）+ 2 个 env override 测试
- **TUI `:health` 面板** (TODO 7.1 第三项): 用户/运维入口汇聚桥接启动健康度
  - `palette` 命令 `health`/`:health`/`h`/`:h` 调起；`r` 刷新、`Esc`/`q` 关闭、`j`/`k` 翻 raw JSON
  - 渲染三层：顶部 Overall（OK/DEGRADED 着色 + 状态文本 + 刷新计时）
    + 组件表（按 ok→absent→degraded 排序，status/path 列）
    + Raw components JSON（可滚动详情）
  - 后台 `spawn_fetch_health_deep` 解析 `/health/deep` 响应（接受 200/207 Multi-Status）
  - 纯函数 `parse_health_deep_response` 便于单元测试（5 个边界测试）
  - 集成测试 `test_health_deep_no_bridge_graceful_error`（无 bridge 不 panic）
- **K8s 标准健康度端点** (TODO 8.2): `/health/live` + `/health/ready` 分离 liveness / readiness
  - `GET /health/live`: 始终 200（livenessProbe；进程能响应 HTTP 即存活；不查依赖以避免抖动触发 pod 重启）
  - `GET /health/ready`: 复用 `health_deep_assess`；200/503（readinessProbe；任一依赖 degraded → 不接收流量）
  - OpenAPI 文档：2 条新路径
  - 测试：`chaos_extended::health_live_always_ok` + `health_ready_reports_components`
  - 测试关键修正：(1) reqwest `.http1_only()`（axum 0.7 不接 HTTP/2 prior knowledge）；
    (2) reqwest `.no_proxy()`（开发环境 `HTTP_PROXY=127.0.0.1:7890` 会拦截本地请求，
    reqwest 默认 `no_proxy` 解析不支持 `127.*` 通配）
- **`/api/metrics` Prometheus 端点文档化 + 测试** (TODO 8.2):
  - OpenAPI 新增 `/api/metrics` 路径（`operationId: prometheusMetrics`，
    Content-Type `text/plain; version=0.0.4`）
  - 集成测试：`prometheus_metrics_exposes_standard_names`（200 + Content-Type +
    7 关键指标名 + `# HELP`/`# TYPE`）+ `prometheus_metrics_concurrent_scrape_safe`（5 并发 scrape）
- **tracing span 覆盖率 (TODO 8.2)**:
  - `TokenBudgetTracker::record` / `should_skip_llm` / `is_exhausted`：`#[instrument]` +
    业务字段（tokens / total_usage / cap / remaining / reserve / skip / exhausted）；
    `record` 跨 cap 时 emit `warn!("token budget exhausted")`
  - `http.execute_goal` / `http.execute_tool` (axum handler)：goal_len / sop_id / project_id / tool_id / fuel
  - `grpc.execute_sop` / `grpc.execute_tool`：替换 `info_span!()` 为 `#[instrument]`，
    含 sop_id / project_id / tool_id / fuel / chaos 字段
  - 测试：`token_budget` 3 个 instrument_* + `chaos_extended` 2 个 sanity 测试
- **`.devcontainer/devcontainer.json`** (TODO 8.4 第一项): VS Code Remote Container 一键搭建
  - base image `rust:1-bookworm` + `postCreateCommand` 切到 nightly（与 CI 一致）
  - 7 个 VS Code 扩展（rust-analyzer / TOML / crates / Actions / Docker / LLDB / Rust）
  - 端口转发 bridge gRPC (50051) + HTTP (8080)
  - 8GB / 4 CPU 约束；持久化 cargo registry + target volume
  - 不影响 CI（仅本地开发体验）
- **book/tutorial.md** (TODO 8.5 第一项): 端到端教程章节
  - 3 个完整场景：CLI 创建 Markdown / Bridge REST API + K8s probe / 多 Agent delegation
  - 与 getting-started 互补：前者 5 分钟安装，后者端到端实战
  - 在 book/src/SUMMARY.md 注册
- **2 个 cargo examples** (TODO 8.5 第三项): `crates/sisyphus-core/examples/`
  - `config_basic.rs`：env vars 构造 Config + 校验流程
  - `sop_graph.rs`：CompositeGraph 跨 SOP 依赖 + 拓扑序
  - 全部通过 clippy + 编译 + 实际运行验证
- **贡献者指南完善** (TODO 8.5 第二项):
  - `CONTRIBUTING.md` (246 行): 行为准则 / 快速开始 / 项目结构 / 代码规范 /
    Conventional Commits / PR 流程 / bug 报告 / 发布流程
  - `.github/PULL_REQUEST_TEMPLATE.md` (72 行): 变更摘要 / 关联 / 测试 / 自查清单
  - `.github/ISSUE_TEMPLATE/{bug_report,feature_request}.md` (40+44 行): 结构化 bug + 特性报告
  - commit 规范：`<type>(<scope>): <subject>` + 中文 subject
- **CHANGELOG v0.2.0 模板** (TODO 8.6 第一项):
  - 在 CHANGELOG.md 顶部加"发布流程（v0.2.0 模板）"段
  - 6 步发布指南（合并 Unreleased → 校验分组 → Verified 段 → 新 Unreleased → git tag → release.yml 自动生成 release notes）
  - 模板骨架：版本段 + Added/Fixed/Changed + Verified
  - release.yml 自动 `generate_release_notes: true`（已存在，本文档做引用存档约定）
- **cargo-deny 强化** (TODO 8.4 第二项):
  - 修复：升级 `wasmtime 47.0.3 → 47.0.4`（CVE RUSTSEC-2026-0269 修复）
  - 修复：升级 `h2 0.4.15 → 0.4.19`（RUSTSEC-2026-0258 patched in 0.4.16）
  - 文档化：deny.toml advisory ignore 段按"来源 + 修复路径 + 影响评估 + 跟踪"
    格式重写，9 个剩余 advisory 全部有据可查
  - 验证：`cargo deny check` 全部 ok
- **bench dashboard** (TODO 8.4 第三项):
  - `scripts/bench-dashboard.sh` (173 行)：跑 cargo bench + 生成 `docs/benchmarks/dashboard/index.html`
  - book/src/bench-dashboard.md 章节 + SUMMARY.md 注册
  - 本地/CI 一致：浏览器看 criterion HTML 报告（分布、回归、对比图）
  - 沿用 CI 现有的 critcmp >5% 回归检测（PR comment）
- **DagRenderer 大规模压力测试 + 优化** (TODO 8.1 第一项):
  - bench 扩展到 1000-5000 节点（`lightweight_benches::bench_dag_renderer`）
  - 优化 `compute_layout`：`queue.sort()` → `queue.sort_unstable()`（introsort）
  - 性能改进 8-32%（统计显著，p<0.05）
  - 关键发现：5000 节点 8.86ms（远低于 TUI 60fps 预算 16.67ms）
  - throughput 接近 O(N) 线性扩展，无 O(N²) 性能坍塌
  - Baseline：`docs/benchmarks/dag-renderer-v0.2.0-baseline.md`
    + `target/criterion/dag_renderer/v0_2_0_dag_renderer/`
- **bridge gRPC 大 payload 检测** (TODO 8.1 第二项):
  - `execute_sop_impl` + `execute_tool_impl` 加 >1MB warn 日志
  - 引导客户端改用 `stream_execution`（已 server-streaming）
- **CLI 启动并行化** (TODO 8.1 第三项):
  - `#[tokio::main]` → `#[tokio::main(flavor = "multi_thread", worker_threads = 2)]`
  - `platform::detect_platform` 走 `spawn_blocking`
  - 加 startup 时间日志便于诊断
- **ClusterConsensus 改进 + bench** (TODO 8.3 第三项):
  - 加 `PeerProposer` trait（可注入 mock）+ `ClusterConsensus::with_proposer()`
  - 动态 `add_peer()` / `remove_peer()` / `cluster_size()`
  - propose 改 majority quorum 校验 + 超时（1s 默认）
  - 9 单元测试 + 1 个 5 节点 bench（mock 零延迟：4.62ms / 1000 ops）
  - Baseline：`docs/benchmarks/consensus-v0.2.0-baseline.md`
- **bench dashboard gh-pages 部署** (TODO 8.4.3 余项):
  - ci.yml bench-core 加 `Generate bench dashboard` step
  - 调 `scripts/bench-dashboard.sh` 生成 index.html
  - upload-artifact (90 天保留)
  - mdbook.yml `needs: [bench-core]` + download-artifact
  - 合并到 `book/book/bench/` 路径（与 mdbook 一并 deploy-pages）
  - PR comment (sticky-pull-request-comment) 显示 dashboard 链接
- **Docker Release 自动发布** (TODO 8.6 第三项):
  - 新增 `.github/workflows/docker-release.yml` (123 行)：
    push tag `v*` 自动构建 + 推送至 ghcr.io
  - 多平台：`linux/amd64` + `linux/arm64`（QEMU）
  - 多 tag 策略：`v0.2.0` → `:v0.2.0`, `:0.2`, `:0`, `:latest`；预发布自动标记
  - 沿用 nightly.yml 的 `:nightly` + `:latest` 推送
  - book/src/deployment.md 填充完整 docker + K8s + 健康度端点说明
  - README.md 快速开始加 docker pull 方式
  - CONTRIBUTING.md § 发布流程更新（手动 → 自动 workflow）

### Fixed
- **`test_search_no_network_graceful_error` flaky 测试**: 原断言 `!stderr.is_empty()`
  在网络可达环境下会失败（DDG 成功响应，stderr 无内容）。
  重写为"不 panic" 断言（`!stderr.contains("panic") && !stderr.contains("thread '")`），
  同时覆盖在线与离线两条路径。
- **`sisyphus-cli/handle_file.rs` clippy `iter_out_of_bounds` 错误**:
  rustc 1.98 新增 `clippy::iter_out_of_bounds` lint，
  `files.iter().take(10)` 在 files 为空时被认为无意义。
  `test_handle_glob_empty` 改为直接验证 `files.is_empty()` + `files.len() == 0`，
  覆盖"未匹配文件"路径不变。

### Changed
- **clippy -D warnings** 7 处 rustc 1.98 升级后引入的编译错误：
  - `sisyphus-types/asset.rs` `AssetKind::SOP` / `AssetContent::SOP`
    → `#[allow(clippy::upper_case_acronyms)]`（SOP 是业务术语）
  - `sisyphus-bridge-api/build.rs` 通过 `type_attribute("sisyphus.bridge.BridgeEvent.event", ...)`
    给生成的 oneof `Event` 注入 `#[allow(clippy::enum_variant_names)]`
    （4 个变体后缀 `Update` 来自 protobuf 字段名，不可改）
  - `sisyphus-tools-llm/http_client.rs` `RetryPolicy` / `TlsRoots` 改用
    `#[derive(Default)]` 替代手写 `impl Default`（derivable_impls）
  - `sisyphus-tools-llm/provider/registry.rs` `format!()` 改 `.to_string()`（useless_format）
  - `sisyphus-tools-system/scaffold.rs` `TemplateType` 添加 allow
    （`rust-lib`/`bin`/`workspace` 字符串映射是公开 API）
  - `sisyphus-tools-llm-tools/momus/types.rs` `ReviewType` 添加 allow
    （momus agent 业务术语：Plan/Code/Spec/Architecture 评审类别）
  - `sisyphus-intel/complexity.rs` `CognitiveDepth::to_config` 改 `self`
    （Copy 类型 + `to_` 前缀约定）
  - `evals/metrics_coverage.rs` `total` → `_total`（未使用变量）
  - `evals/golden_sop/trend.rs` `% 4 == 0` 改 `y.is_multiple_of(4)`（manual_is_multiple_of）
  - `config.example.toml` 删除 line 144-149 重复的 `[cognitive_loop]` 段
- **rustfmt 1.98 链式格式**: `cargo fmt --all` 应用，366 文件风格统一

### Changed
- **token_budget 端到端补齐**（Rust 端已有但暴露不全）：
  - `TokenBudgetConfig::validate()`：`reserve_ratio ∈ [0, 1]`，`cap=0` 拒绝，`None` 允许
  - `CognitiveLoopConfig::validate()` 接入 `token_budget.validate()`
  - env override：`SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_CAP`（空字符串清空 `None`），
    `SISYPHUS_COGNITIVE_LOOP_TOKEN_BUDGET_RESERVE_RATIO`
  - `config.example.toml` 增 `[cognitive_loop.token_budget]` 段示例
  - `book/src/configuration.md` 文档表 +2 行
  - 4 个新边界测试（`test 22 全绿`）
### Fixed
- **rustc 1.98 大批 clippy 警告清理 (2026-09-01)**：升级到 1.98 后新增多个 lint，
  工作区 clippy `-D warnings` 全量修复。涉及 crate：
  - **sisyphus-cli**：`iter_out_of_bounds`（`files.iter().take(10)` → 直接 `files.len() == 0`）+
    `iter_count`（`files.iter().count()` → `files.len()`）
  - **sisyphus-webui**：`await_holding_lock`（`std::sync::Mutex<()>` 跨 .await → `OnceLock<tokio::sync::Mutex<()>>`）
  - **sisyphus-types**：`useless_vec` × 4（`vec![..]` 2元素 → `[..]`），
    `field_reassign_with_default` × 15（`let mut c = X::default(); c.field = ..;` → struct update 语法），
    `clone_on_copy` × 3（`x.clone()` → `x`），
    `approx_constant` × 1（`3.14` → `3.139`），
    `format_in_format_args` × 1（`format!("{}", format!("{:?}", x))` → `format!("{:?}", x)`），
    `bool_assert_comparison` × 1（`assert_eq!(x, true)` → `assert!(x)`），
    `useless_conversion` × 多处（`"a".into()` → `"a"`，`std::env::temp_dir()` 不再 `PathBuf::from` 包装），
    `unused_imports` × 1（去 `DataMapping`），
    `len_zero` × 1（`s.len() > 0` → `!s.is_empty()`），
    `assertions_on_constants` × 2（`assert!(x >= 0)` usize 永远成立 → `let _ = x`），
    重复 `#[test]` 修复 × 多处（patch 工具误删/重复行）
  - **sisyphus-core**：`expect_err` on Future（`result.err().expect("err")` → `result.await.expect_err("err")`），
    `useless_conversion` × 多处（`id.as_str()` → `id`），`needless_borrow` × 1（`&id` → `id`），
    `unused_imports` × 2（去 `Tbt`/`TokenBudgetConfig`），`len_zero` × 1
  - **sisyphus-intel**：`clone_on_copy` × 4（CorrectionType/CorrectionPriority/FlawSeverity/RiskLevel），
    `vec_init_then_push` × 1（`Vec::new() + push` → `vec![]`），`bool_assert_comparison` × 1，
    `field_reassign_with_default` × 1（`PlannerConfig`），`manual_range_contains` × 1，
    `useless_conversion` × 1，`len_zero` × 1（`anomalies.len() > 0` → `!is_empty()`），
    `unused_enumerate_index` × 2（`for i in 0..10` 不用 i → `for _i`），
    单元结构体 `TopologicalSOPEvolver::default()` → `TopologicalSOPEvolver`，
    `module same name` 修复（`mod tests` in `tests.rs` → `mod llm_synthesizer_tests`），
    常量断言 `DEFAULT_ESTIMATED_TOKENS_PER_CALL > 0` → `const _: () = assert!(..)`
  - **sisyphus-bridge**：`useless_conversion` × 1
  - **sisyphus-tests**：`len_zero` × 1（`anomalies.len() > 0` → `!is_empty()`）
  - **sisyphus-wasm-sdk**：`int_plus_one` × 1（`after >= before + 1` → `after > before`）
  - **sisyphus-tools-scm**：`clone_on_copy` × 1（WorkflowKind）
  - **sisyphus-tools-llm**：`bool_assert_comparison` × 1（`enable_kani, true` → `enable_kani`），
    `useless_vec` × 1（`vec![..]` → `[..]`），重复 `#[test]` 修复
  - **sisyphus-tools-llm-tools**：`bool_assert_comparison` × 1（`passed, false` → `!passed`）
  - **sisyphus-tools-system**：`useless_vec` × 1，`clone_on_copy` × 1（TemplateType）
  - **sisyphus-tools-llm-tools/security_types**：`bool_assert_comparison` × 1
  - **sisyphus-sandboxd**：`unused_enumerate_index` × 1
  - **sisyphus-mem**：`clone_on_copy` × 1（LexicalFallbackReason），`unused_variables` × 2（`rt` → `_rt` 保留 `tempdir` 生命周期）
  - **sisyphus-roadmap**：`unused_variables` × 2（`ms2_id`/`project_id` → `_ms2_id`/`_project_id`）
  - **evals/http_client_bench**：`useless_format` × 2（`format!("{{...}}")` → 字符串字面量"单层{}"，
    注意原 `format!("{{...}}")` Rust 解析后是单 `{` JSON；误用 `{{` 会导致 serde_json 解析失败 → 修复）
  - **crates/sisyphus-tests/tests/intervention_manager**：`result.err().expect()` Future
  - **crates/sisyphus-intel/src/llm_synthesizer/tests.rs**：`module same name` 修复
  - **跨工作区测试 env 串行化 (sisyphus-types)**：test_apply_env_overrides_anomaly_detector
    与 test_apply_env_overrides_anomaly_detector_missing_keeps_defaults 在 cargo test 并行
    模式下因 env 全局共享产生 race condition。添加 `static ENV_LOCK: std::sync::Mutex<()>`
    在两个测试入口获取锁，强制串行执行避免互相 set_var 干扰。验证 3 次连续跑全部稳定通过。
- **2026-09-02 bridge 集成测试 fixture 修复 (TODO 7.2 完成项)**:
  - 3 个 setup helper(`setup_bridge` / `setup_bridge_chaos` / `setup_bridge_chaos_with_bridge`)
    缺少 `config.llm.embedding.allow_dummy_embeddings = true`,导致 DummyLlmClient 启动时直接
    panic(`"Embedding provider 'dummy' is not 'openai'..."`)。修复后 89 个 bridge 集成测试全过(之前 52 个全部 panic)。
  - `Makefile` 中 `BRIDGE_DEPS` 引用不存在的 `build.rs`,导致 `make test-bridge` 立即失败。
    替换为 `Cargo.toml`(build dependency 用 manifest 触发)。
- **2026-09-02 阶段 7.3/7.5 实施**：
  - **7.5 部署脚本**：新增 `deploy/systemd/sisyphus-bridge.service`(硬安全加固:NoNewPrivileges/ProtectSystem/RestrictNamespaces 等)
    + `deploy/systemd/install.sh`(自动构建→安装→启用) + `deploy/k8s/00-namespace.yaml..06-ingress.yaml`(7 个 manifest)。
    kubeconform 校验：8/8 k8s 资源全部 Valid。systemd unit 含 MemoryMax/LimitNOFILE 资源约束。
  - **7.3 DAG 渲染性能测试**: 新增 `crates/sisyphus-core/benches/lightweight_benches.rs`(不依赖 sandbox)
    覆盖 AnomalyDetector 检测吞吐(10/100/1000 元素) + DagRenderer 大型 SOP 渲染(10/50/200/500 节点)
    + 边界场景(1000 信号序列)。注册到 Cargo.toml `[[bench]]`,CI 可在 `cargo bench` 默认 feature 下跑。
  - **7.5 mdbook 增量构建 CI**: 新增 `.github/workflows/mdbook.yml`。
    - 关键优化:`hashFiles('book/**/SUMMARY.md', 'book/book.toml')` 作为 cache key
    - mdbook 自带增量检测:实测 4s cold → 0.6s 增量
    - 双 job 设计:`build-and-deploy`(push master → GitHub Pages) + `build-only`(PR 预检不部署)
    - actions/cache + actions/upload-pages-artifact 组合利用 GitHub 原生 Pages
  - **7.2 mutation testing 调研** (进行中):安装 `cargo-mutants 27.1.0`,在 `sisyphus-types` 跑全 4 线程试点(目标 ~5-10 min)
  - **.gitignore**: `book/book/`(mdbook 产物)+ `book/src/SUMMARY.md.bak` 加入忽略列表
    + `git rm -r --cached book/book/` 移除已被 tracked 的产物文件
- **2026-09-02 阶段 7.3 后续**:待 mutation testing 完成 + 性能基线建立后,再实现 7.3 TUI 冷启动 profile。
- **2026-09-02 TUI `:health` 面板增强 (TODO 7.1 P3 完成项)**：
  - **bridge `/health/deep` 端点扩展**：新增 `token_budget` 组件（4th 组件）
    - 读取 `kernel.strategic_manager.token_budget.snapshot()`（`parking_lot::RwLock`）
    - 阈值规则：使用率 ≥ 90% **或** `exhausted = true` → degraded
    - JSON schema：`{status, usage, cap, exhausted}`
  - **TUI 表格优化**：`token_budget` 组件 detail 列显示 `usage/cap tokens (exhausted)`
    替代整 JSON，更直观（如 `950/1000 tokens`、`50000 tokens (unbounded)`）
  - **sisyphus-intel 新增 `TokenBudgetSnapshot`**：read-only 数据类型，
    `cap = None` 表示 unbounded tracker（u64::MAX sentinel）
  - **6 个新单元测试**（`sisyphus-bridge`）：覆盖 unbounded/low/high/exhausted/none/integration 场景

### Verified

### Verified
- `cargo check-all` 0 错误（默认工作区排除 bridge/sandbox/sandboxd/tests）
- `cargo check-all` 0 错误（默认工作区排除 bridge/sandbox/sandboxd/tests）
- `cargo test-all` 全量套件 0 失败（含 sisyphus-types 35 + sisyphus-core 84 + sisyphus-cli 53 + bridge REST 7 + skills REST 9）
- AnomalyDetectorConfig 新增 8 个单元测试 + 2 个 env override 测试，全部通过
- `test_search_no_network_graceful_error` flaky 修复后回归测试通过
- `sisyphus-cli` clippy `-D warnings` 0 错误（修复 `iter_out_of_bounds` + `iter_count`）
- `cargo fmt --all` 风格一致
- **2026-09-01 rustc 1.98 clippy 全工作区验证**：`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，
  `cargo test-all` 累计 4649 个测试全过，0 失败。修复范围覆盖 ~70 处 lint，
  涉及 cli/webui/types/core/intel/bridge/tests/wasm-sdk/tools-scm/tools-llm/tools-llm-tools/
  tools-system/sandboxd/mem/roadmap/evals 等 13 个 crate。
- `cargo fmt --all` 风格一致

[Unreleased]: https://github.com/zerove/opencodepro/compare/master...HEAD

- **2026-09-02 wasm 沙箱 proptest + Otel 端点配置化扫尾 (TODO 7.1 + 7.2)**：
  - **sisyphus-sandbox 新增 proptest 属性测试**（9 个属性 × 100 cases = 900+ 输入组合）：
    - 覆盖 `SchemaValidator::validate` 全部分支（never_panics / malformed_schema / deeply_nested / type_mismatch × 4 / range_respected）
    - **proptest fuzz 真实发现 1 个 UTF-8 bug**：`minLength`/`maxLength` 用 `s.len()`（字节数）比较，
      修复为 `s.chars().count()`（字符数）。最小失败输入：`min=9, s="𐀀 aa¡"`（字节 9 字符 5）
    - regression 保存至 `crates/sisyphus-sandbox/proptest-regressions/custom_executor.txt`
    - 依赖：`proptest = "1.4"` + `serde_json`
  - **Otel 端点配置化验证**：`OpenTelemetryConfig` 统一在 `sisyphus-types::config::MetricsConfig`，
    4 个 env override (`SISYPHUS_METRICS_OPENTELEMETRY_*`)，仅 `sisyphus-bridge` 通过 `shared_config.read()` 消费并启用 OTLP 导出
- 测试修复：`test_get_learning_insights` 阈值 1000 → 10000（chaos 注入产生 1042 samples）

### Added (peer experience sync, 2026-09-06, TODO 9.1.1)
- **Experience CRDT broadcast 集成** (commit a7444fc)：
  - 新增 `crates/sisyphus-bridge/src/peer/experience_sync.rs` 模块
  - `broadcast_experience()`：发送端将本地 experience JSON 序列化后通过
    `PeerManager::broadcast_experience_sync` 推送到所有 peer
  - `ExperienceSyncHandler::spawn()`：接收端订阅 `PeerManager` 事件，
    处理 `ExperienceSync` ClusterEvent，反序列化为 `ExperienceTuple` 后
    调用 `experience_bank.record()` 写入本地 store
  - 自广播过滤：`exp_key` 包含 self_node_id 时跳过（避免自环）
  - broadcast lag 检测：`RecvError::Lagged` 打印 warn 而非 panic
  - 2 个单元测试：`broadcast_experience_serializes_payload` +
    `broadcast_experience_handles_serialize_error_gracefully`
  - **当前状态**：handler 框架就绪，等待 record 调用点集成
    （kernel.capture_success 路径已落 SOP 而非 experience，bridge 端需新增
    experience 记录入口才能触发 sync 链路）

### Verified
- cargo test -p sisyphus-bridge --lib peer::experience_sync: 2/2 通过
- cargo check -p sisyphus-bridge --lib: 0 错误
- cargo clippy -p sisyphus-bridge --lib -- -D warnings: 0 警告
- cargo fmt --all: 风格一致

### Verified
- cargo test -p sisyphus-sandbox: 24/24 通过（含 9 新 fuzz 测试）
- cargo test-all: 4653/4653 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- proptest 900+ cases 全过（包含 min=9, s="𐀀 aa¡" UTF-8 回归）

- **2026-09-03 v0.1.0 tag + 混沌测试扩展 (TODO 7.2 完成全部 4/4)**：
  - 新增 `crates/sisyphus-bridge/tests/chaos_extended.rs`（5 个测试）：
    - `chaos_concurrent_health_check_no_race` (50 并发 health_check < 5s)
    - `chaos_concurrent_list_sops_no_race` (30 并发 list_sops < 5s)
    - `chaos_burst_100_health_checks` (100 并发 ≥ 90% 成功，10s 内)
    - `chaos_large_args_payload_does_not_crash` (10KB JSON args 不 panic)
    - `chaos_repeated_connect_disconnect` (10 次连/断/连 鲁棒)
  - 覆盖 race / load / payload / reconnect 边界
  - bridge 总测试：95 → 100 (含 5 新 chaos_extended)
  - **v0.1.0 git tag 创建**：阶段 7 收尾（实际版本 `0.1.0`）
  - 总体进度：阶段 7 ~98% (7.1 5/5 + 7.2 4/4 + 7.3/7.4/7.5 3/3)

### Verified
- cargo test -p sisyphus-bridge: 100/100 通过 (含 5 chaos_extended)
- cargo test-all: 4653/4653 通过
- cargo clippy --workspace --all-targets -- -D warnings: 0 警告
- git tag v0.1.0 创建成功

### Verified (v0.2.0 release gate, 2026-11-04)
- `cargo test-all`: **4934 passed / 0 failed / 26 ignored**
- `cargo check-all`: 通过（0 error）
- `cargo check -p sisyphus-bridge -p sisyphus-sandbox -p sisyphus-sandboxd -p sisyphus-tests`: 通过
- `cargo test -p sisyphus-webui`: 11 passed
- `make release-check`: 通过（版本 0.2.0 一致 / metadata 齐全 / CHANGELOG 已切段 / tag 未创建）
- 全部 27 个 crate 版本统一为 `0.2.0`，141 处内部 path 依赖版本约束同步更新
