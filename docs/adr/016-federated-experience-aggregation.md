# ADR-016: 联邦经验聚合——入站经验的质量门槛

**Status**: Implemented（2026-11-05，v0.4.0 Phase A）
**Deciders**: core team
**Related**: ADR-009（分布式 v2 架构 / Experience CRDT）, ADR-010（反熵补偿）, ADR-015（反向反熵拉取）

## 背景

v0.3.0 之后，跨节点经验同步已经是**全量**的：反熵按摘要补差量
（ADR-010）、`Lagged` 走补齐信号（ADR-011/013）、服务端流建立后反向拉取
（ADR-015）。协议层面的问题是「怎么不丢」，答案为「一条都不丢」。

但「一条都不丢」不等于「每一条都该要」。入站经验是**对端算出来的结论**，
而不是本地可复现的事实：

- 一条 `outcome_success = false` 且 `deviation_score = 1.0` 的经验，描述的是
  「这条路走不通」。它对本地策略有价值（负面知识），但**等价于没有信息**，
  因为它与「随机失败」在数据上不可区分。
- 节点各自的负载、模型版本、工具链不同，同一条经验在不同节点上的有效性
  本来就不同。全量接受会让本地经验库的规模随集群规模线性增长，而
  `query_similar` 的召回质量却不一定提升。

`docs/optimization-plan-2026-11-05.md` §4.5 把「联邦学习 / 经验聚合服务」
列为方向性工作，要求先写问题陈述。本节即为问题陈述的第一步：
**只解决「低质入站经验污染本地库」这一个具体问题**，不引入信誉系统、
不做跨节点的策略权重共识（那是 Phase B）。

## 决策

**引入纯函数质量评分 + 可配置的入站门槛，默认关闭。**

1. **评分是纯函数，定义在 `sisyphus-types`。**
   `evaluate_experience_quality(exp, cfg) -> f64` 只依赖 `ExperienceTuple`
   的三个字段（`outcome_success`、`deviation_score`）与配置，不依赖存储、
   不做 I/O：

   ```
   quality = clamp(
       if outcome_success { success_weight } else { 0.0 }
           - deviation_weight * clamp(deviation_score, 0.0, 1.0),
       0.0, 1.0)
   ```

   `deviation_score` 来自远端节点，**不能假定它守规矩**：越界值一律夹紧，
   负偏差视作无偏差、> 1 视作最大偏差。这样评分恒在 `[0, 1]`，
   下游比较无需额外防御。

2. **门槛在桥接的入站路径上，且只影响「是否合并」。**
   `ExperienceSyncHandler::handle_remote_sync` 在 `apply_remote` 之前评估；
   未过门槛直接 `return`，并记 `warn`（携带 `quality` / `min_quality_score` /
   `outcome_success` / `deviation_score`）。**不静默丢弃**——静默丢弃会让
   「对端明明有这条经验、本地就是查不到」变成无法排查的现象。

3. **默认 `enabled = false`，行为与 v0.3.0 完全一致。**
   这不是保守，而是正确性要求：过滤改变的是**收敛结果集**。既有反熵/混沌
   收敛用例写入的经验大量是 `outcome_success = false, deviation_score = 0.0`
   （质量为 0.0），默认开启会让这些用例从「收敛」变成「不收敛」——
   表面上是测试坏了，实际是集群语义被悄悄改掉了。开关式接入让「要不要过滤」
   成为显式运维决策。

4. **装配在构造期完成，不做运行期热切换。**
   `ExperienceSyncHandler::with_federated_aggregation(cfg)` 是构造期一次性装配。
   热切换会引入「同一条流前后半段用不同门槛」的不可解释状态，且
   已经合并的经验不会被回滚。

5. **配置校验只在启用时执行。**
   `min_quality_score` 必须是 `[0,1]` 内的**有限**数，权重必须 ≥ 0 且有限。
   `NaN` 单独拒绝：所有比较对 `NaN` 恒为 false，门槛永不触发、扣分永不生效，
   配置看起来生效了但实际什么都没做，是最难察觉的配置错误。
   未启用时这些字段不参与任何判定，让它们拦住启动只会制造困惑。

## 后果

### 正面

- 低质入站经验不再进入本地库，`query_similar` 的召回信噪比与规模增长解耦。
- 过滤决策可观测（`warn` 日志带分数与阈值），不是黑盒。
- 默认关闭 + 构造期装配：单机与既有集群部署零行为变化，无迁移成本。
- 评分函数是纯函数、定义在 `types`，Phase B（策略成功率共识）可以直接复用。

### 负面 / 待观察

- **门槛是启发式，不是学习出来的。** 权重默认值（0.7 / 0.3 / 0.2）是拍的，
  没有真实集群数据支撑。开启前应先用日志观察被拒绝经验的实际分布。
- **过滤不可逆。** 被拒绝的经验不会在阈值调低后自动补回——它的主键仍在对端，
  但本地摘要不含它，反熵也只会补「本地缺的行」，所以在下次全量拉取前它不会再出现。
  这与 ADR-010 的差量语义一致，但运维上需要知道「调阈值不会补历史」。
- **可能放大单点偏差。** 如果某个节点因为本地环境问题（例如临时缺依赖）持续
  产出 `outcome_success = false` 的经验，它会被整体过滤——那些其实是
  「环境特定」而非「策略错误」的信息就丢了。这正是 Phase B 要解决的问题：
  用集群级策略成功率替代单条经验的二元成败。

## 实施结果（2026-11-05）

| 位置 | 内容 |
|------|------|
| `crates/sisyphus-types/src/experience.rs` | `FederatedAggregatorConfig`（`enabled` / `min_quality_score` / `success_weight` / `deviation_weight`）、`evaluate_experience_quality()`、`validate()` |
| `crates/sisyphus-types/src/config/sub.rs` | `ClusterConfig.federated` 字段 + 启用时校验转发 |
| `crates/sisyphus-types/src/config/mod.rs` | `SISYPHUS_CLUSTER_FEDERATED_*` 四个环境变量覆盖 |
| `crates/sisyphus-bridge/src/peer/experience_sync.rs` | `with_federated_aggregation()`、`HandleRemoteSync` 前的门槛分支 |
| `crates/sisyphus-bridge/src/main.rs` | `cfg.federated.enabled` 时装配，并 `info!` 打印生效参数 |
| `config.example.toml` / `book/src/configuration.md` | `[cluster]` 与 `[cluster.federated]` 文档 |

## 验证

### 门禁 / 测试

| 项目 | 结果 |
|------|------|
| `cargo test -p sisyphus-types --lib` | ✅ 135 passed（新增 6：评分边界、默认关闭、参数校验、TOML 解析、默认值、env 覆盖） |
| `cargo test -p sisyphus-bridge --lib` | ✅ 139 passed + 3 failed（`sandboxd_client::tests::*`，本机 sandboxd 无执行权限，与本次改动无关） |
| `cargo test -p sisyphus-bridge --lib peer::experience_sync` | ✅ 6 passed（3 个为本次新增） |
| `cargo test -p sisyphus-bridge --no-default-features --test cluster_integration` | ✅ 21 passed |
| `cargo test -p sisyphus-bridge --no-default-features --test anti_entropy_integration` | ✅ 12 passed |
| `cargo test -p sisyphus-bridge --no-default-features --test anti_entropy_partition_convergence` | ✅ 1 passed |
| `cargo test -p sisyphus-bridge --no-default-features --test chaos_partition_convergence` | ✅ 1 passed |
| `cargo test -p sisyphus-bridge --no-default-features --test chaos_one_sided_reverse_pull` | ✅ 1 passed |
| `cargo fmt --all --check` | ✅ 无 diff |
| `cargo clippy -p sisyphus-bridge --no-default-features --all-targets -- -D warnings` | ✅ 0 warning |
| `cargo clippy -p sisyphus-types --all-targets -- -D warnings` | ✅ 0 warning |

新增的三个桥接用例走真实 `handle_remote_sync` + 真实 `VectorExperienceBank`
（内存 sqlite），断言落在「库里到底有没有那一行」：

| 用例 | 断言 |
|------|------|
| `federated_aggregation_rejects_low_quality_experience` | 失败 + 偏差 0.9 → 门槛拒绝，库为空 |
| `federated_aggregation_accepts_high_quality_experience` | 成功 + 偏差 0.1 → 合并入库 |
| `federated_none_keeps_unfiltered_behavior` | `None` 时同样的低质经验照样入库（向后兼容硬约束） |

### 变异验证

把门槛比较改成恒假（`if false && quality < cfg.min_quality_score`）：

- `federated_aggregation_rejects_low_quality_experience` **稳定失败**
  （`低质量经验应在聚合门槛处被丢弃，实际入库: ["exp:1700000000:node-b:1"]`）；
- 还原后 6 passed。

结论：该断言锁住的是过滤分支本身，而不是「看起来通过」的返回值。

## 遗留

- **阈值无数据支撑**（见「负面」第一条）：需要在真实多节点环境采样被拒绝经验
  的分数分布后再定默认值。
- **Phase B**：把 `MetaLearner` 的策略成功率经 `Consensus::propose` 做成集群
  级共识，用「策略在当前集群是否有效」替代单条经验的二元成败。
- **Phase C**：3 节点联邦收敛集成测试 + 变异验证。
- 周期性重放（ADR-010 选项 C）仍未启用；这与本 ADR 的过滤不可逆问题叠加，
  意味着被拒绝的经验在一次「调阈值 + 重启」后仍可能长期缺席。
