# ADR-017: 集群策略权重同步——每节点快照 + generation 取新

**Status**: Implemented（2026-11-06，v0.4.0 Phase B；Amendment 2（Phase D）：节点离开回收槽位；
Amendment 3（Phase E）：执行路径消费集群权重）
**Deciders**: core team
**Related**: ADR-009（分布式 v2 架构 / 集群事件总线）、ADR-010（反熵补偿）、ADR-016（联邦经验聚合，Phase A）

## 背景

ADR-016 解决了「入站经验要不要收」，但它的判定单位是**单条经验**：一条
`outcome_success = false` 的经验被直接丢弃，等于把「这个策略在当前集群不灵」
和「这个节点今天环境坏了」混为一谈。

真正想要的是**集群级的策略有效性**：同一策略在多个节点上各跑了多少次、
成功多少次。这个数字天然是「按节点分槽」的——每个节点只知道自己跑过什么。

问题在于传播路径。表面上 `ClusterConsensus` 有个 `propose()` 可用，但生产装配是：

```rust
ClusterConsensus::new("node-1", vec![])   // proposer: None
```

`propose()` 在 `proposer` 为 `None` 时**只写一行日志**。用它做策略权重共识，
会得到一个最难排查的失败模式：代码看着在同步、日志看着在同步、数据永远不到。

集群事件总线的另一端则是通的：`mgr_event_to_proto` 早就支持把
`StateProposal` 编成 proto 发出去，但**入站方向从来没有人接**——
`proto_to_mgr_event` 把它丢掉了。于是 `StateProposal` 实际上只有「出去」
没有「回来」。

## 决策

**用已打通出站方向的 `StateProposal` 事件传输每节点策略快照，入站补齐投递；
合并规则 = 同节点按 `generation` 严格取新，聚合 = 按 `uses` 加权求和。**

### 1. 数据模型：每节点一个槽，而不是全集群一个计数器

```rust
pub struct StrategyStat { uses: u64, successes: u64, avg_duration_secs: f64 }

pub struct NodeStrategySnapshot {
    node_id: String,
    generation: u64,
    strategies: BTreeMap<String, StrategyStat>,
}

pub struct FederatedStrategyWeights { nodes: BTreeMap<String, NodeStrategySnapshot> }
```

选「每节点一份快照」而不是「全节点共享一组计数器」，唯一原因是**幂等**：

- 共享计数器：同一份快照重放两次 → 计数翻倍。而 `StateProposal` 是
  「每个已建立的双向流各发一次」，节点数一多，同一份快照必然重复到达。
- 每节点分槽：重放只会覆盖同一个槽，且 `generation` 相同则直接拒绝。

代价是聚合语义有 ±1 的取舍：3 个节点各跑 10 次会被计为 30 次。这正是
「样本越多越可信」要的语义，但它**不等于**集群里真实执行了 30 次不同执行。
需要去重执行次数的话得另加去重键，本 ADR 不提供（见「遗留」）。

### 2. `generation` 必须是单调递增的本地时间戳

`generation` 取 unix 毫秒，但**先与「本节点上一次 generation + 1」取大**：

```rust
let generation = current_generation_plus_one.max(now_ms);
```

原因是测试与将来的高频场景里同一毫秒可能发两次；纯时间戳会让第二次被自己的
合并规则拒绝，表现为「发布成功但数据不更新」——又是一个「看着在工作」的坑。

合并规则是 `existing.generation >= snapshot.generation => 拒绝`。用 `>=` 而非 `>`
是幂等的前提：相同 generation 的内容按「先到者胜」处理。生产中同节点同毫秒的
两次上报内容必然相同；即便不同，下一拍（更晚的 generation）也会覆盖。

### 3. 入站投递走 `deliver_`，不回广播

`proto_to_mgr_event` 补上 `Event::StateProposal → MgrEvent::StateProposal`，
`apply_mgr_event` 用 `manager.deliver_inbound_state_proposal(key, value)` 投递到
**入站总线**。

不能用 `broadcast_state_proposal`：事件已经来自远端，再进本地广播通道会被
拨号器转发回集群，形成回环。回环虽然会被 `generation` 幂等挡住，但会白白放大
广播量（与 ADR-016 中经验同步的 `deliver_inbound_experience_sync` 同一处理）。

> **Phase C 修正**：上面说的是「**落地即回灌**」不行。Phase C 发现只做到这一步
> 无法在非全连接拓扑下收敛，因此改为「**被接受后转发**」。见文末
> [Amendment](#amendment2026-11-06phase-c多跳转发)。

### 4. handler 订阅入站、周期发布

`StrategyWeightsHandler::spawn` 启动后：

- 每 `publish_interval` 调一次 `publish_once()`：从 `MetaLearner::learn()`
  取 `LearningInsight`，转成快照写入共享视图并广播；
- 同时消费入站 `StateProposal`，key 匹配才解析，`apply_remote` 合并。

三处**刻意的保守设计**：

- **key 带 `/v1`**（`sisyphus.strategy-weights/v1`）：载荷是 JSON 且字段名即
  契约。将来改结构时旧节点必须能安全忽略新载荷，而不是把解析失败当成
  「对端没数据」。key 不匹配即忽略。
- **解析失败只 `warn`，不清空本地视图**：远端载荷不可信，一条坏包不该让
  聚合视图归零。
- **自回环防御性过滤**（`snapshot.node_id == self_node_id` 直接拒绝）：入站
  通道本应只有远端事件，但这条过滤让「某天有人把广播接进入站总线」这类
  改动不会静默污染数据。

### 5. 发布周期可配，`0` = 只收不发

`ClusterConfig.strategy_sync_interval_secs`（默认 60，env
`SISYPHUS_CLUSTER_STRATEGY_SYNC_INTERVAL_SECS`）。默认 60 秒是「统计变化率」与
「广播开销」的折中：`MetaLearner` 只在执行结束后变化，秒级同步没有意义；
而分钟级足以让 REST 视图与 Phase C 的收敛断言在几轮内看到对端数据。

`0` 表示只消费入站快照、不主动发布——用于「只要聚合视图」的消费者
（例如把 bridge 当只读观察点）。校验只在 `!= 0` 时要求 `>= 1`。

### 6. 共享视图挂在桥接上，对外暴露 REST

`MySisyphusBridge` 持有 `Arc<RwLock<FederatedStrategyWeights>>`，handler 与
REST handler 共用。挂在这里而不是 `RestApiState`，是为了避免改
`RestApiState` 的三处构造点（bridge 侧所有 REST 处理器都已持有 `state.bridge`）。

`GET /api/strategy/weights` 返回 `to_json()`：`node_count` / `nodes`（每节点
`node_id` + `generation` + `strategies`）/ `aggregate`（策略名 → `uses` /
`successes` / `success_rate` / `avg_duration_secs`）/ `best`（`best_strategy(1)`，
可为 null）。字段名即 API 契约，`webui_contract` 与 `openapi_live_contract`
两个契约测试守门。

## 后果

### 正面

- 策略有效性有了集群视角，且**默认零行为变化**：不启用集群时 handler 不装配，
  单机看到的仍只有本地 `MetaLearner`。
- 合并幂等、可交换、可乱序：重复投递与多流冗余都不需要额外处理。
- 复用已打通的 `StateProposal` 通道，没有新增 proto 消息、没有新增连接管理。
- 聚合与选择是纯函数（`sisyphus-types`），REST / TUI / Phase C 测试可直接复用。

### 负面 / 待观察

- **30 次 ≠ 30 次去重执行**（见决策 1 的取舍）。若将来要「集群真实执行次数」，
  需要引入执行 ID 去重，这会改变载荷结构（`/v1` → `/v2`）。
- **快照是全量而非增量**：策略名数量随使用线性增长，但典型工作负载下只有
  个位数到几十个策略，全量 JSON 仍然很小。若策略名暴涨，需要改成只发 top-K
  或增量（同样走 `/v2`）。
- **`best_strategy` 的 `min_uses` 目前是 1**：REST 只作展示，没做统计显著性
  门槛。真正的推荐接入（让执行器按集群成功率选策略）必须先定 `min_uses`，
  否则会把「1 次成功」当结论——这正是 ADR-016 背景下要避免的错误。
- **节点下线后其槽位不回收**：没有 TTL。视图会一直包含已离线节点的最后一份
  快照。影响有限（统计是历史累计），但「当前集群里谁在报数」这个问题的答案
  会偏大。**Phase C 仍未加**：要判断「离线」需要消费 `PeerLeft`，而策略视图
  目前只订阅 `StateProposal`。

> **Phase D 修正**：已回收。见文末
> [Amendment 2](#amendment-22026-11-06phase-d节点离开回收槽位)。

## Amendment（2026-11-06，Phase C）：多跳转发

### 触发问题

Phase B 的入站处理是「合并进本地视图，到此为止」。这在**全连接**拓扑下足够：
每个节点都与其他节点有直连流，一份快照一次直达。但集群的连通性由
`[cluster].peers` 静态列表决定，生产里常见的是**只列种子节点**：

```
node-a: peers = []                      # 种子
node-b: peers = ["node-a:50051"]
node-c: peers = ["node-b:50051"]        # 只知道 B，不知道 A
```

此时 A 与 C 之间没有任何流。A 的快照只能到 B，C 的快照只能到 B——
**两端永远看不到对方**，「联邦」退化成「每个节点只认识自己和一个邻居」。
Phase C 的 3 节点用例在**不转发**的实现上稳定失败：`node-a` 的
`node_count()` 停在 2（自己 + B），聚合里没有 C 的 `breadth-first`。

### 修正

`StrategyWeightsHandler::apply_remote` 在**快照被接受**后，把**原始载荷**再
广播一次：

```rust
let accepted = self.shared.write().merge(snapshot.clone());
if accepted {
    self.peer_manager.broadcast_state_proposal(STRATEGY_WEIGHTS_KEY, value.to_vec());
}
```

三条性质保证「转发次数有界、不成环」，全部是**已有的**，没有新增状态：

1. **同节点同 `generation` 只接受一次** → 每份快照在每个节点至多转发一次；
   重复到达直接拒绝，不会再触发转发。
2. **源节点自过滤**（`snapshot.node_id == self_node_id`）→ 快照绕一圈回到
   源节点时被丢弃，不会无限循环。
3. **`generation` 严格递增** → 不存在「越转越新」的正反馈。

载荷原样搬运（`value.to_vec()`）而不是重新序列化本地视图，是为了让接收方
仍然按「某节点的快照」语义处理；如果转发的是聚合视图，节点数会随跳数增加。

### 代价

- **放大**：全连接拓扑下每条快照从 O(N) 次发送涨到 O(N²)；链式拓扑下是
  O(N) 跳。载荷是分钟级的策略统计（KB 以内），可接受；但节点数很大时需要
  重新评估（可改为只转发给「未持有该节点槽位」的对端，但那需要按对端定向发送
  的能力，当前 `broadcast_state_proposal` 没有）。
- **语义仍是「最终收敛」而非「有界时延」**：链路断开期间不转发，靠下一拍
  全量快照补上——与反熵的即时补差量不同，这里没有补齐信号。

### 验证

| 变异 | 预期失败用例 | 结果 |
|------|--------------|------|
| 删掉「接受后转发」 | `test_three_node_strategy_weights_converge_across_chain_topology` | ✅ **稳定失败**（`node-a` 的 `node_count()` 停在 2，断言消息指出链式拓扑需要转发） |
| 改为「无条件转发」（拒绝路径也转发） | `peer::strategy_weights::tests::apply_remote_forwards_only_accepted_snapshots` | ✅ **稳定失败**（重放的同 generation 快照被转发） |

还原后：`cluster_integration` 23 passed、`peer::` 61 passed。

## Amendment 2（2026-11-06，Phase D）：节点离开回收槽位

### 触发问题

Phase B/C 的视图只从 `StateProposal` 学习，因此**只增不减**：一台机器退出后，
它最后一份快照会永久留在 `nodes` 里。`GET /api/strategy/weights` 的
`node_count` 于是回答的是「历史上曾有几个节点报过数」，而不是「当前集群里有
几个节点」——两个都合理的语义，但 REST 的调用方（运维面板、健康检查）要的
是后者。

### 修正

`FederatedStrategyWeights::remove_node` + handler 的 `forget_node`，在收到
`PeerLeft` 时丢弃该节点的槽位。

**语义选择「丢弃」而不是「标记离线」**：视图回答的是「当前集群里哪些策略真的
有效」。保留一台已下线机器的历史累计，会让推荐被一个不再参与工作的节点持续
影响。数据不会永久丢失——节点重启后由它自己的 `MetaLearner` 重新发布全量
快照，槽位从初始 `generation` 重新开始。

**必须额外订阅本地通道**：这是本次改动唯一的真坑。`PeerLeft` 只出现在
`event_tx` 上——`on_peer_left` 与 `evict_expired` 都发它，入站通道
`inbound_tx` 里根本没有。此前 handler 只 `subscribe_inbound()`，也就是说
「消费 `PeerLeft`」这件事在结构上不可能发生。修正为同时订阅
`peer_manager.subscribe()` 作为 `members`。

副作用：本地通道上还能看到**自己发布的** `StateProposal`（`publish_once`
走 `event_tx`）。`members` 分支必须忽略它，否则每拍都会把自己的快照再处理
一遍（虽然会被合并逻辑当"同 generation"拒掉，但会白跑一遍反序列化与日志）。

### 分区语义

链路断开（TTL 过期或断流）→ `PeerLeft` → 槽位移除；重连后下一拍快照加回。
因此**聚合值在分区期间会变小**，收敛后才恢复。这符合「当前视图」的定义，
但与「历史累计」不同，写进 `book/src/rest-api.md` 以免调用方误读。

### 验证

| 变异 | 预期失败用例 | 结果 |
|------|--------------|------|
| `PeerLeft` 分支不调用 `forget_node` | `peer::strategy_weights::tests::peer_left_drops_node_slot_and_keeps_others` | ✅ **稳定失败**（「必须触发槽位回收，实际仍剩 2 个节点」） |

还原后：`peer::strategy_weights` 7 passed、`sisyphus-types` 该模块 10 passed。

## Amendment 3（2026-11-06，Phase E）：执行路径消费集群权重

### 背景

Phase B 落地「节点快照 + 集群聚合视图 + REST 暴露」，但**聚合结果从未进入
执行路径**：`crates/sisyphus-core/src/strategic_manager/cognitive_loop.rs` 里
`best_strategy_params` 的结果被赋值进未使用变量（`_current_strategy` /
`_current_params`），真正产生提示的入口是 `sisyphus-intel` 的
`MetaLearningSynthesizer::synthesize`，它只看本地 `MetaLearner`。

结果是：即使三个节点已经通过 Phase C 收敛到一致视图，每次执行仍然只参考
本机历史——联邦只在「对外展示」生效，没有实际把「更好的策略」交给执行。

### 决策

通过配置 `[cluster.strategy_selection]`（默认 `enabled = false`）把消费接上。
新增纯函数 `sisyphus_types::strategy_weights::select_strategy`，显式锁定优先级：

1. 本地证据充分（`local.uses >= min_uses`）→ 用本地；
2. 集群证据充分（`cluster.uses >= min_uses`）→ 用集群；
3. 两侧不足 → 回退本地薄证据；否则 `None`。

`MetaLearningSynthesizer` 在决策后拼入提示文本；本地来源保持原提示字符串不变，
集群来源使用独立文案并带样本量（让 LLM 能判断推荐的可信度）。

### 取舍

- **默认关闭**：v0.3.0 行为零回归；联邦增强是显式、可回退的运营开关。
- **`min_uses = 5` 默认**：3 次成功在二项分布下置信下界仅 ~0.29，把这种量级当
  结论正是 ADR-017 要避免的错误。
- **门槛按来源各自适用**：本地 10 次与集群 40 次，各自用自己的阈值判；用聚合总数
  同时判会导致「同一策略在 3 个节点各跑 10 次」（聚合 30）被误判。

### 验证

- 新增纯函数单测 10 个：默认关闭 / 关闭忽略集群 / 本地充分胜集群 / 局部薄让位集群 /
  两侧薄回退本地 / 无本地补位集群 / 无证据 None / 边界含 / 部分 TOML 解析。
- 新增执行路径单测 3 个（`MetaLearningSynthesizer` + capturing mock）：
  关闭 → 本地；局部薄 → 集群；本地充分 → 本地。
- **变异验证 2 处**：启用后忽略本地充分门槛（变 `local.uses >= min_uses` 为恒真）
  → 本地充分用例稳定失败；两侧不足时回退改给集群 → 回退用例稳定失败。还原后全绿。

## 实施结果（2026-11-06）

| 位置 | 内容 |
|------|------|
| `crates/sisyphus-types/src/strategy_weights.rs` | `StrategyStat` / `NodeStrategySnapshot` / `FederatedStrategyWeights`（`merge` / `aggregate` / `best_strategy` / `to_json`）+ 9 个单测 |
| `crates/sisyphus-types/src/config/sub.rs` | `ClusterConfig.strategy_sync_interval_secs`（默认 60）+ 校验 |
| `crates/sisyphus-types/src/config/mod.rs` | `SISYPHUS_CLUSTER_STRATEGY_SYNC_INTERVAL_SECS` 覆盖 + 2 个测试 |
| `crates/sisyphus-bridge/src/peer/manager.rs` | `deliver_inbound_state_proposal()`（入站投递，不转广播） |
| `crates/sisyphus-bridge/src/peer/service.rs` | `proto_to_mgr_event` 保留 `StateProposal`；`apply_mgr_event` 投递入站总线 + 2 个测试 |
| `crates/sisyphus-bridge/src/peer/strategy_weights.rs` | `StrategyWeightsHandler`（`new` / `with_interval` / `spawn` / `publish_once` / `apply_remote`）+ 5 个单测 |
| `crates/sisyphus-bridge/src/bridge_service.rs` | 共享视图字段 + `strategy_weights()` getter |
| `crates/sisyphus-bridge/src/main.rs` | 按 `strategy_sync_interval_secs` 装配 handler，独立 shutdown 通道，随 `sig_rx` 停止 |
| `crates/sisyphus-bridge/src/http/{mod.rs,handlers.rs}` | `GET /api/strategy/weights` |
| `crates/sisyphus-bridge/src/http_openapi.rs` | 对应 path + `operationId: getStrategyWeights` |
| `crates/sisyphus-bridge/tests/cluster_integration.rs` | 2 节点收敛用例（gRPC 全链路） |
| `crates/sisyphus-bridge/tests/strategy_weights_rest_test.rs` | REST 端点接线用例 2 个（空视图 200 / 合并视图 JSON 契约） |
| `crates/sisyphus-bridge/src/peer/strategy_weights.rs`（Amendment） | `apply_remote` 接受后转发（多跳传播）+ 单测 `apply_remote_forwards_only_accepted_snapshots` |
| `crates/sisyphus-bridge/tests/cluster_integration.rs`（Amendment） | 3 节点链式拓扑收敛用例 `test_three_node_strategy_weights_converge_across_chain_topology` |
| `crates/sisyphus-types/src/strategy_weights.rs`（Amendment 2） | `remove_node` + 单测 `remove_node_drops_only_that_slot` |
| `crates/sisyphus-bridge/src/peer/strategy_weights.rs`（Amendment 2） | `forget_node` + 订阅本地成员通道 + 单测 `peer_left_drops_node_slot_and_keeps_others` |
| `crates/sisyphus-types/src/strategy_weights.rs`（Amendment 3） | `StrategySelectionConfig` + `StrategyDecision` + `select_strategy` + 10 个决策单测 |
| `crates/sisyphus-intel/src/synthesis.rs`（Amendment 3） | `MetaLearningSynthesizer::with_federated_selection` + 3 个执行路径单测 |
| `config.example.toml` / `book/src/configuration.md` | `strategy_sync_interval_secs` 文档 |
| `book/src/rest-api.md` | `GET /api/strategy/weights` 响应示例与语义说明 |

## 验证

### 门禁 / 测试

| 项目 | 结果 |
|------|------|
| `cargo test -p sisyphus-types --lib` | ✅ 146 passed（新增 11：模型 9 + 配置 2） |
| `cargo test -p sisyphus-bridge --lib peer::` | ✅ 60 passed（其中 `peer::strategy_weights` 5、`peer::service` 新增 2） |
| `cargo test -p sisyphus-bridge --no-default-features --test cluster_integration` | ✅ 22 passed（新增 1 个端到端用例） |
| `cargo test -p sisyphus-bridge --no-default-features --test anti_entropy_integration` | ✅ 12 passed |
| `anti_entropy_partition_convergence` / `chaos_partition_convergence` / `chaos_one_sided_reverse_pull` | ✅ 各 1 passed |
| `strategy_weights_rest_test` | ✅ 2 passed（空视图返回 200 而非 404；合并视图的 `node_count` / 聚合求和 / `best` 契约） |
| `cluster_integration`（Phase C） | ✅ 23 passed（新增 3 节点链式收敛用例：含线上重放幂等断言） |
| `peer::strategy_weights`（Phase C） | ✅ 6 passed（新增 `apply_remote_forwards_only_accepted_snapshots`） |
| `peer::strategy_weights`（Amendment 2） | ✅ 7 passed（新增 `peer_left_drops_node_slot_and_keeps_others`，走真实 `on_peer_left` → 本地通道 → handler） |
| `strategy_weights::tests`（Amendment 3） | ✅ 20 passed（新增决策矩阵 10 个）；
  `synthesis::tests::test_meta_synth` 3 passed（新增 3 个） |
| `webui_contract` | ✅ 2 passed |
| `openapi_live_contract` | ✅ 3 passed（新路由必须进 OpenAPI，否则 `every_route_is_documented` 失败） |
| `cargo fmt --all` | ✅ 无 diff |
| `cargo clippy -p sisyphus-bridge --no-default-features --all-targets -- -D warnings` | ✅ 0 warning |
| `cargo clippy -p sisyphus-types --all-targets -- -D warnings` | ✅ 0 warning |
| `cargo check-all` | ✅ 通过 |
| `make audit` | ✅ 全绿（含 TODO 勾选门禁、workflow 审计、Docker 契约） |

### 端到端用例

`test_two_node_strategy_weights_propagate_and_merge` 在进程内起两个真实 gRPC 节点
（A 无拨号目标、B 静态拨号到 A），发布周期缩到 50ms，断言：

1. A 记录 2 次 `depth-first` 成功、B 记录 1 次失败；
2. B 最终持有 A 的快照且 `depth-first.uses == 2 / successes == 2`；
3. B 的聚合为 `uses == 3 / successes == 2`、`success_rate == 2/3`；
4. A 反向也持有 B 的快照（`uses == 1`）。

用例里踩到并修掉的两个真问题，都写进了注释，避免后人重犯：

- **`broadcast::Sender` 必须比 handler 任务活得久**。测试辅助函数若不持有
  sender，`shutdown_rx.recv()` 立刻返回 `Closed`，任务在第一次 `select!`
  就静默退出——表现为「tick 从不触发」，而数据链路上看不出任何错误。
- **等待条件不能只查 `generation > 0`**。`tokio::time::interval` 的**首个
  tick 是即时的**，handler 启动瞬间就会发一次空快照；只按 generation 判定会
  拿到那份空快照，然后断言 `strategies["depth-first"]` 直接 panic。
  等待条件必须是「快照里出现目标策略」。

### 变异验证

| 变异 | 预期失败用例 | 结果 |
|------|--------------|------|
| `merge` 的比较 `>=` 改为 `>`（同 generation 重放被接受） | `strategy_weights::tests::merge_is_idempotent` | ✅ **稳定失败** |
| 删掉 `apply_remote` 的自回环过滤 | `peer::strategy_weights::tests::apply_remote_rejects_self_echo_other_keys_and_garbage` | ✅ **稳定失败** |
| `publish_once` 不调用 `broadcast_state_proposal`（只写本地视图） | `test_two_node_strategy_weights_propagate_and_merge` | ✅ **稳定失败**（20s 超时等不到 A 的快照） |

三处变异还原后各自重跑：`9 passed` / `5 passed` / `1 passed`。

结论：这些断言锁住的是**行为本身**（幂等、自过滤、传播链），而不是「看起来
通过」的返回值。第三条尤其重要——没有它，「只写本地视图、从不广播」这种退化
可以让所有单测保持绿色。

## 遗留

- **Phase C 已完成**（Amendment）：3 节点链式拓扑收敛 + 线上重放幂等已由
  `cluster_integration` 覆盖，两处变异验证过。**未覆盖**：乱序到达（已被
  `generation` 语义覆盖于单测）。
- **Phase D 已完成**（Amendment 2）：节点离开回收槽位。**未覆盖**：集群整体
  重启（所有节点同时 `PeerLeft` 后视图归零，再由各自快照重建）的端到端用例。
- **Phase E 已完成**（Amendment 3）：执行路径消费集群权重（默认关闭，`min_uses = 5`；
  决策矩阵见上）。**未覆盖**：启用后跨节点协商（谁的本地证据更强、谁的集群快照更早），
  当前策略是「本地优先、集群只补位」，天然单参。
- **转发放大未量化**：全连接拓扑下 O(N²) 次发送只在 3 节点上验证过；
  更大集群需要先量化再决定是否改为按对端定向转发。
- **节点槽位 TTL 已加**（Amendment 2），但溢出的 `Lagged` 会漏掉 `PeerLeft`
  → 槽位残留（已记 warn，未做对账）。治本方案是用 `list_peers()` 定期
  对账视图里的节点集合。
- **全量快照**：策略名规模暴涨时需改增量或 top-K（`/v2`）。
