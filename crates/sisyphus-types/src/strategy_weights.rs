//! 集群级策略权重聚合（v0.4.0 Phase B，ADR-017）。
//!
//! 单节点 `MetaLearner` 只知道「本地哪些策略成功过」。联邦场景下每个节点
//! 的经验分布不同（负载、模型版本、工具链），直接取本地最优策略会把单节点
//! 的偶然性当成结论。这里定义**按样本数加权**的聚合模型：
//!
//! - **每个节点一个快照**（`NodeStrategySnapshot`），带单调 `generation`；
//! - 合并规则 = 同节点按 `generation` 取新（幂等、可交换，重复投递无副作用）；
//! - 聚合 = 跨节点把 `uses` / `successes` 相加，`avg_duration` 按 `uses` 加权。
//!
//! 选择「每节点一份快照」而不是「全节点共享一个计数器」，是因为后者无法
//! 幂等：同一份快照重放两次会把计数翻倍。按节点分槽后，重放只会覆盖同一个槽。
//!
//! 注意 ±1 的取舍：聚合结果里的 `successes/uses` 是**跨节点求和**，因此
//! 同一个策略在 3 个节点各跑 10 次会被计为 30 次。这正是我们要的语义
//! （样本越多越可信），但它不等于「集群里真实执行了 30 次不同的执行」——
//! 如果调用方要的是去重后的执行次数，需要另加去重键，本模块不提供。

use crate::meta_learner::LearningInsight;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单个策略在若干节点上的累计统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct StrategyStat {
    /// 使用次数总和。
    pub uses: u64,
    /// 成功次数总和。
    pub successes: u64,
    /// 平均耗时（秒），按 `uses` 加权。
    pub avg_duration_secs: f64,
}

impl StrategyStat {
    /// 成功率；`uses == 0` 时为 0.0（而不是 NaN）。
    ///
    /// 返回 0.0 而非 NaN 是刻意的：NaN 会让下游所有比较静默为假，
    /// 表现为「策略看起来既不达标也不淘汰」。
    pub fn success_rate(&self) -> f64 {
        if self.uses == 0 {
            0.0
        } else {
            self.successes as f64 / self.uses as f64
        }
    }

    /// 把另一份统计累加进来（`avg_duration` 按 `uses` 加权）。
    fn accumulate(&mut self, other: &StrategyStat) {
        let total_uses = self.uses.saturating_add(other.uses);
        if total_uses > 0 {
            let weighted = self.avg_duration_secs * self.uses as f64
                + other.avg_duration_secs * other.uses as f64;
            self.avg_duration_secs = weighted / total_uses as f64;
        }
        self.uses = total_uses;
        self.successes = self.successes.saturating_add(other.successes);
    }
}

/// 单个节点上报的策略快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeStrategySnapshot {
    /// 上报节点 ID（用于分槽，也用于拒绝自回环）。
    pub node_id: String,
    /// 单调递增的版本号。生产装配使用上报时刻的 unix 毫秒。
    pub generation: u64,
    /// 策略名 → 统计。
    pub strategies: BTreeMap<String, StrategyStat>,
}

impl NodeStrategySnapshot {
    /// 从本地 `MetaLearner` 的 `LearningInsight` 构造快照。
    ///
    /// `LearningInsight` 里每个策略已带 `total_uses` / `successes` /
    /// `avg_duration_secs`，因此这里只是换一层结构，不做二次计算。
    pub fn from_insight(
        node_id: impl Into<String>,
        generation: u64,
        insight: &LearningInsight,
    ) -> Self {
        let strategies = insight
            .strategies
            .iter()
            .map(|s| {
                (
                    s.strategy.clone(),
                    StrategyStat {
                        uses: s.total_uses,
                        successes: s.successes,
                        avg_duration_secs: s.avg_duration_secs,
                    },
                )
            })
            .collect();
        Self {
            node_id: node_id.into(),
            generation,
            strategies,
        }
    }
}

/// 集群策略权重：按节点分槽的聚合视图。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FederatedStrategyWeights {
    /// node_id → 该节点的最新快照（按 `generation` 取新）。
    nodes: BTreeMap<String, NodeStrategySnapshot>,
}

impl FederatedStrategyWeights {
    /// 合并一份节点快照，返回是否被接受。
    ///
    /// 接受条件：该节点没有快照，或新快照的 `generation` **严格更大**。
    /// 相同 `generation` 的不同内容按「先到者胜」处理——生产中 generation 是
    /// 毫秒时间戳，同一节点在同一毫秒内两次上报的内容必然相同；即便不同，
    /// 下一次上报（更晚的 generation）也会覆盖。
    ///
    /// 合并是**幂等**的：同一份快照重放任意次只产生一个槽的一份拷贝。
    pub fn merge(&mut self, snapshot: NodeStrategySnapshot) -> bool {
        match self.nodes.get(&snapshot.node_id) {
            Some(existing) if existing.generation >= snapshot.generation => false,
            _ => {
                self.nodes.insert(snapshot.node_id.clone(), snapshot);
                true
            },
        }
    }

    /// 移除某个节点的槽位，返回是否真的移除（ADR-017 Amendment 2）。
    ///
    /// 语义刻意选择「**丢弃**」而不是「标记离线」：视图回答的问题是
    /// 「当前集群里哪些策略真的有效」，把一台已下线机器的历史累计留在
    /// 聚合里，会让推荐结果被一个不再参与工作的节点持续影响。节点重启后
    /// 会用自己的 `MetaLearner` 重新发布全量快照，数据不会永久丢失。
    ///
    /// 注意分区场景：链路短暂断开（`PeerLeft` 由 TTL 或断流触发）会先移除
    /// 槽位，重连后的下一拍快照再加回。因此聚合值在分区期间会**变小**，
    /// 这符合「当前视图」而非「历史流水」的定义。
    pub fn remove_node(&mut self, node_id: &str) -> bool {
        self.nodes.remove(node_id).is_some()
    }

    /// 已收录的节点数。
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 某个节点的快照（测试 / 调试用）。
    pub fn node_snapshot(&self, node_id: &str) -> Option<&NodeStrategySnapshot> {
        self.nodes.get(node_id)
    }

    /// 跨节点聚合：`uses` / `successes` 求和，`avg_duration` 按 `uses` 加权。
    pub fn aggregate(&self) -> BTreeMap<String, StrategyStat> {
        let mut out: BTreeMap<String, StrategyStat> = BTreeMap::new();
        for snapshot in self.nodes.values() {
            for (strategy, stat) in &snapshot.strategies {
                out.entry(strategy.clone()).or_default().accumulate(stat);
            }
        }
        out
    }

    /// 聚合结果里成功率最高的策略。
    ///
    /// 样本数不足的策略不参与评选：只看 1 次成功就得出 100% 成功率的结论，
    /// 是把噪声当信号。`min_uses` 由调用方给定（REST 端点默认 1，仅作展示）。
    /// 成功率相同时按策略名字典序取前——排序必须确定，否则同一份数据在不同
    /// 节点上会给出不同推荐。
    pub fn best_strategy(&self, min_uses: u64) -> Option<(String, StrategyStat)> {
        self.aggregate().into_iter().filter(|(_, stat)| stat.uses >= min_uses.max(1)).max_by(
            |(name_a, a), (name_b, b)| {
                a.success_rate()
                    .partial_cmp(&b.success_rate())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    // 成功率相同时取字典序**更小**的名字（max_by 返回较大者，
                    // 因此这里反转比较）。
                    .then_with(|| name_b.cmp(name_a))
            },
        )
    }

    /// 序列化为 REST 响应体。
    ///
    /// 结构固定（`nodes` + `aggregate` + `best`），字段名即 API 契约，
    /// 由 `openapi_live_contract` / REST 用例守护。
    pub fn to_json(&self) -> serde_json::Value {
        let nodes: Vec<serde_json::Value> = self
            .nodes
            .values()
            .map(|snapshot| {
                serde_json::json!({
                    "node_id": snapshot.node_id,
                    "generation": snapshot.generation,
                    "strategies": snapshot.strategies,
                })
            })
            .collect();

        let aggregate: serde_json::Map<String, serde_json::Value> = self
            .aggregate()
            .into_iter()
            .map(|(name, stat)| {
                (
                    name,
                    serde_json::json!({
                        "uses": stat.uses,
                        "successes": stat.successes,
                        "success_rate": stat.success_rate(),
                        "avg_duration_secs": stat.avg_duration_secs,
                    }),
                )
            })
            .collect();

        let best = self.best_strategy(1).map(|(name, stat)| {
            serde_json::json!({
                "strategy": name,
                "success_rate": stat.success_rate(),
                "uses": stat.uses,
            })
        });

        serde_json::json!({
            "node_count": self.nodes.len(),
            "nodes": nodes,
            "aggregate": aggregate,
            "best": best,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_learner::StrategyInsight;

    fn snapshot(
        node: &str,
        generation: u64,
        stats: &[(&str, u64, u64, f64)],
    ) -> NodeStrategySnapshot {
        NodeStrategySnapshot {
            node_id: node.into(),
            generation,
            strategies: stats
                .iter()
                .map(|(name, uses, successes, duration)| {
                    (
                        (*name).to_string(),
                        StrategyStat {
                            uses: *uses,
                            successes: *successes,
                            avg_duration_secs: *duration,
                        },
                    )
                })
                .collect(),
        }
    }

    /// 同一份快照重放任意次只保留一份 —— 重复投递不得把计数翻倍。
    #[test]
    fn merge_is_idempotent() {
        let snap = snapshot("node-a", 10, &[("depth-first", 4, 3, 1.5)]);
        let mut weights = FederatedStrategyWeights::default();

        assert!(weights.merge(snap.clone()));
        assert!(!weights.merge(snap.clone()), "同一 generation 必须被拒绝");
        assert!(!weights.merge(snap));

        assert_eq!(weights.node_count(), 1);
        let agg = weights.aggregate();
        assert_eq!(agg["depth-first"].uses, 4, "重复合并不得放大 uses");
        assert_eq!(agg["depth-first"].successes, 3);
    }

    /// 节点离开后槽位必须被回收，且不影响其他节点（Amendment 2）。
    #[test]
    fn remove_node_drops_only_that_slot() {
        let mut weights = FederatedStrategyWeights::default();
        weights.merge(snapshot("node-a", 1, &[("depth-first", 2, 2, 1.0)]));
        weights.merge(snapshot("node-b", 1, &[("hybrid", 3, 1, 2.0)]));
        assert_eq!(weights.node_count(), 2);

        assert!(weights.remove_node("node-a"), "已存在的槽位应被移除");
        assert!(!weights.remove_node("node-a"), "重复移除必须是 no-op");
        assert!(
            !weights.remove_node("node-zzz"),
            "不存在的节点必须返回 false"
        );

        assert_eq!(weights.node_count(), 1);
        let agg = weights.aggregate();
        assert!(
            !agg.contains_key("depth-first"),
            "被移除节点的策略不得留在聚合里"
        );
        assert_eq!(agg["hybrid"].uses, 3);

        // 重新加入（节点重启）后可再次合并，且从初始 generation 起算。
        assert!(weights.merge(snapshot("node-a", 1, &[("depth-first", 1, 1, 1.0)])));
        assert_eq!(weights.node_count(), 2);
        assert_eq!(weights.aggregate()["depth-first"].uses, 1);
    }

    /// 旧 generation 不得覆盖新内容（乱序到达是网络常态）。
    #[test]
    fn stale_generation_is_rejected() {
        let mut weights = FederatedStrategyWeights::default();
        assert!(weights.merge(snapshot("node-a", 200, &[("breadth-first", 10, 9, 2.0)])));
        assert!(
            !weights.merge(snapshot("node-a", 100, &[("breadth-first", 1, 0, 9.0)])),
            "更旧的快照必须被丢弃"
        );

        let agg = weights.aggregate();
        assert_eq!(agg["breadth-first"].uses, 10);
        assert!((agg["breadth-first"].success_rate() - 0.9).abs() < 1e-9);
    }

    /// 聚合按 uses 加权，而不是对成功率取算术平均。
    ///
    /// 反例：节点 A 10 次全成功（100%）、节点 B 1 次失败（0%）。
    /// 算术平均 = 50%，加权 = 10/11 ≈ 0.909。后者才是「这个策略总体表现」。
    #[test]
    fn aggregate_weights_by_uses() {
        let mut weights = FederatedStrategyWeights::default();
        weights.merge(snapshot("node-a", 1, &[("hybrid", 10, 10, 1.0)]));
        weights.merge(snapshot("node-b", 1, &[("hybrid", 1, 0, 3.0)]));

        let agg = weights.aggregate();
        let stat = agg["hybrid"];
        assert_eq!(stat.uses, 11);
        assert_eq!(stat.successes, 10);
        assert!(
            (stat.success_rate() - 10.0 / 11.0).abs() < 1e-9,
            "必须按样本数加权，实际 {}",
            stat.success_rate()
        );
        // 加权平均耗时 = (1.0*10 + 3.0*1) / 11
        assert!((stat.avg_duration_secs - 13.0 / 11.0).abs() < 1e-9);
    }

    /// 合并顺序不影响聚合结果（可交换 + 可结合）。
    ///
    /// 这是「多节点乱序收敛」的最小保证：不同节点以不同顺序收到同样的快照，
    /// 必须得到同一个聚合视图，否则集群里没有共同结论。
    #[test]
    fn merge_order_does_not_change_aggregate() {
        let snaps = [
            snapshot("node-a", 5, &[("s1", 3, 2, 1.0), ("s2", 1, 1, 2.0)]),
            snapshot("node-b", 7, &[("s1", 4, 1, 3.0)]),
            snapshot("node-c", 9, &[("s2", 6, 5, 0.5)]),
        ];

        let mut expected: Option<BTreeMap<String, StrategyStat>> = None;
        // 6 种排列都要给出一致的聚合结果。
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut weights = FederatedStrategyWeights::default();
            for idx in order {
                weights.merge(snaps[idx].clone());
            }
            let agg = weights.aggregate();
            match &expected {
                None => expected = Some(agg),
                Some(e) => {
                    assert_eq!(e.len(), agg.len());
                    for (name, stat) in e {
                        let got = agg.get(name).expect("策略缺失");
                        assert_eq!(stat.uses, got.uses);
                        assert_eq!(stat.successes, got.successes);
                        assert!((stat.avg_duration_secs - got.avg_duration_secs).abs() < 1e-9);
                    }
                },
            }
        }
    }

    /// `from_insight` 必须原样搬运本地统计（不得二次计算）。
    #[test]
    fn from_insight_maps_local_stats() {
        let insight = LearningInsight {
            total_samples: 5,
            overall_success_rate: 0.6,
            strategies: vec![
                StrategyInsight {
                    strategy: "depth-first".into(),
                    total_uses: 3,
                    successes: 2,
                    failures: 1,
                    avg_duration_secs: 1.25,
                    success_rate: 2.0 / 3.0,
                },
                StrategyInsight {
                    strategy: "breadth-first".into(),
                    total_uses: 2,
                    successes: 1,
                    failures: 1,
                    avg_duration_secs: 4.0,
                    success_rate: 0.5,
                },
            ],
            trend: "stable".into(),
        };

        let snap = NodeStrategySnapshot::from_insight("node-a", 42, &insight);
        assert_eq!(snap.node_id, "node-a");
        assert_eq!(snap.generation, 42);
        assert_eq!(snap.strategies.len(), 2);
        assert_eq!(snap.strategies["depth-first"].uses, 3);
        assert_eq!(snap.strategies["depth-first"].successes, 2);
        assert!((snap.strategies["depth-first"].avg_duration_secs - 1.25).abs() < 1e-9);
    }

    /// 样本数不足的策略不参与 `best` 评选，且同分时按字典序（确定性）。
    #[test]
    fn best_strategy_ignores_thin_samples_and_breaks_ties_by_name() {
        let mut weights = FederatedStrategyWeights::default();
        weights.merge(snapshot(
            "node-a",
            1,
            &[
                ("lucky", 1, 1, 0.1),    // 100% 但只有 1 次
                ("solid", 10, 8, 1.0),   // 80%，10 次
                ("solid-b", 10, 8, 1.0), // 与 solid 同分
            ],
        ));

        let (best, stat) = weights.best_strategy(5).expect("应有胜出者");
        assert_eq!(best, "solid", "样本不足的 lucky 不得胜出，同分取字典序小者");
        assert_eq!(stat.uses, 10);
    }

    /// `uses == 0` 时成功率是 0.0 而不是 NaN —— NaN 会让排序与门槛全部静默失效。
    #[test]
    fn zero_uses_has_zero_rate_not_nan() {
        let stat = StrategyStat::default();
        assert_eq!(stat.success_rate(), 0.0);
        assert!(!stat.success_rate().is_nan());
    }

    /// REST 载荷字段名是契约：改名必须在同一处更新 OpenAPI 与前端。
    #[test]
    fn to_json_exposes_contract_fields() {
        let mut weights = FederatedStrategyWeights::default();
        weights.merge(snapshot("node-a", 3, &[("hybrid", 4, 3, 1.0)]));

        let json = weights.to_json();
        assert_eq!(json["node_count"], 1);
        assert_eq!(json["nodes"][0]["node_id"], "node-a");
        assert_eq!(json["nodes"][0]["generation"], 3);
        assert_eq!(json["aggregate"]["hybrid"]["uses"], 4);
        assert_eq!(json["aggregate"]["hybrid"]["successes"], 3);
        assert!(
            (json["aggregate"]["hybrid"]["success_rate"].as_f64().unwrap() - 0.75).abs() < 1e-9
        );
        assert_eq!(json["best"]["strategy"], "hybrid");
        assert_eq!(json["best"]["uses"], 4);
    }

    /// 空视图也要能序列化（节点刚启动、尚未收到任何快照）。
    #[test]
    fn empty_view_serializes_with_null_best() {
        let json = FederatedStrategyWeights::default().to_json();
        assert_eq!(json["node_count"], 0);
        assert_eq!(json["nodes"].as_array().expect("nodes 必须是数组").len(), 0);
        assert!(json["best"].is_null());
    }
}
