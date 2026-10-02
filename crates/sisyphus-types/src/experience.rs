use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::errors::TypesError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Experience Tuple.
pub struct ExperienceTuple {
    pub context_text: String,
    pub context_hash: String,
    pub action_id: String,
    pub project_id: Option<String>,
    pub outcome_success: bool,
    pub deviation_score: f64,
    pub cost: u64,
    pub timestamp: u64,
    pub embedding: Option<Vec<f32>>,
    pub visual_embedding: Option<Vec<f32>>,
    pub insight: Option<String>,
    /// CRDT: 本地版本号（每次 update 自增），默认 0
    #[serde(default)]
    pub version: u64,
    /// CRDT: vector clock — 每个 node_id 的最近 seen 版本，默认空
    /// 用于多节点 sync 时检测 happened-before 关系
    #[serde(default)]
    pub vector_clock: BTreeMap<String, u64>,
}

/// 经验行的存储主键。
///
/// 历史上这里只用 `exp:{timestamp}`，而 `timestamp` 是**秒级**的
/// `SystemTime::now()...as_secs()`。同一秒内的两次写入会撞主键：实测第二条
/// INSERT 直接失败，且上层只 `warn!` 后继续，**丢失完全静默**。
///
/// 改为纳入来源节点与该节点内的写入计数（两者在 `bump_local_clock` 之后都已
/// 存在于 `vector_clock` 中），因此：
/// - 同一秒内的不同写入不再冲突；
/// - 行标识编码了来源与计数，dial 方持有 (node_id → max counter) 即可推出
///   自己缺哪些行——这是跨节点反熵补偿（ADR-010）成立的前提。
///
/// 不新增字段，因此不改变跨节点序列化格式，wire 兼容性不受影响。
pub fn experience_store_id(timestamp: u64, origin: &str, counter: u64) -> String {
    format!("exp:{timestamp}:{origin}:{counter}")
}

/// 从 vector clock 推断该行的来源节点与其写入计数。
///
/// 对**本地**写入，`bump_local_clock` 会自增 `node_id` 对应的条目，因此
/// 「哪个条目刚被自增」由调用方给出更可靠，这里只作为无法得知 origin 时的
/// 兜底：取计数最大的条目。跨节点场景下 origin 应由发送方通过
/// [`experience_store_id`] 算好后随 `exp_key` 一同传输。
pub fn infer_origin(vector_clock: &BTreeMap<String, u64>, fallback: &str) -> (String, u64) {
    vector_clock
        .iter()
        .max_by_key(|(_, v)| **v)
        .map(|(node, counter)| (node.clone(), *counter))
        .unwrap_or_else(|| (fallback.to_string(), 0))
}

/// 从存储主键中取出来源节点。
///
/// 主键形如 `exp:{timestamp}:{origin}:{counter}`，来源在倒数第二段——因此
/// **不能用后缀匹配来判断是否为本节点自己产生**（origin 不是末段）。
pub fn experience_origin_from_key(key: &str) -> Option<&str> {
    let rest = key.strip_prefix("exp:")?;
    let mut parts = rest.rsplitn(2, ':');
    // 第一段是 counter，第二段是 origin + 更早的字段。
    let counter = parts.next()?;
    if counter.is_empty() || !counter.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let head = parts.next()?;
    head.rsplit_once(':').map(|(_, origin)| origin)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Experience Query.
pub struct ExperienceQuery {
    pub query_text: String,
    pub context_hash: String,
    pub project_id: Option<String>,
    pub action_id: Option<String>,
    pub visual_embedding: Option<Vec<f32>>,
    pub limit: usize,
}

#[async_trait]
/// Experience Bank.
pub trait ExperienceBank: Send + Sync {
    async fn record(&self, exp: ExperienceTuple) -> Result<(), TypesError>;
    async fn query_similar(
        &self,
        query: ExperienceQuery,
    ) -> Result<Vec<ExperienceTuple>, TypesError>;
}

// ---------------------------------------------------------------------------
// CRDT（TODO 8.3.2 / ADR-009 § 2）：LWW-Element-Set with vector clock
// ---------------------------------------------------------------------------

/// CRDT 合并决策（纯函数，可独立测试）。
///
/// LWW-Element-Set + vector clock 的合并结果：
/// - 收到的版本严格大于本地 → 应用 remote（ApplyRemote）
/// - 本地版本严格大于收到 → 保留本地（KeepLocal）
/// - vector clock 严格大于（happened-before）→ 应用 remote
/// - 完全等价 → 无操作（Equal）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeDecision {
    /// 接受 remote 版本
    ApplyRemote,
    /// 保留本地版本
    KeepLocal,
    /// 两边等价，无需操作
    Equal,
}

/// Vector clock 比较结果。
///
/// - `Before`: self 严格在 other 之前（self < other，happened-before）
/// - `After`: self 严格在 other 之后（self > other）
/// - `Equal`: 完全相同
/// - `Concurrent`: 既不 ≤ 也不 ≥（真正并发冲突，需 tie-break）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClockOrdering {
    Before,
    After,
    Equal,
    Concurrent,
}

/// 比较 vector clock：
/// - `Before`: self ≤ other 且 self ≠ other
/// - `After`: other ≤ self 且 self ≠ other
/// - `Equal`: 完全相同
/// - `Concurrent`: 既不 ≤ 也不 ≥（真正并发）
fn compare_vector_clocks(
    self_vc: &BTreeMap<String, u64>,
    other_vc: &BTreeMap<String, u64>,
) -> ClockOrdering {
    let mut self_le_other = true;
    let mut other_le_self = true;

    // 遍历所有 key
    let keys: std::collections::BTreeSet<&String> = self_vc.keys().chain(other_vc.keys()).collect();
    for k in keys {
        let s = self_vc.get(k).copied().unwrap_or(0);
        let o = other_vc.get(k).copied().unwrap_or(0);
        if s > o {
            self_le_other = false;
        } else if o > s {
            other_le_self = false;
        }
    }

    match (self_le_other, other_le_self) {
        (true, true) => ClockOrdering::Equal,
        (true, false) => ClockOrdering::Before, // self 在 other 之前
        (false, true) => ClockOrdering::After,  // self 在 other 之后
        (false, false) => ClockOrdering::Concurrent, // 并发冲突
    }
}

/// CRDT 合并：纯函数，决定本地/remote 哪个胜出。
///
/// 算法（按优先级）：
/// 1. **完全等价**：Equal
/// 2. **vector clock 严格 happened-before**：
///    - self 在 remote 之前 → ApplyRemote
///    - self 在 remote 之后 → KeepLocal
/// 3. **vector clock 并发冲突**（既不 ≤ 也不 ≥）：用 `version` 做 tie-break
///    - remote.version > self.version → ApplyRemote
///    - self.version > remote.version → KeepLocal
///    - 完全相等 → Equal
///
/// 输入约定：
/// - `self_vc` / `self_version`：本地状态
/// - `other_vc` / `other_version`：收到的 remote 状态
pub fn merge_crdt(
    self_vc: &BTreeMap<String, u64>,
    self_version: u64,
    other_vc: &BTreeMap<String, u64>,
    other_version: u64,
) -> MergeDecision {
    if self_version == other_version && self_vc == other_vc {
        return MergeDecision::Equal;
    }

    match compare_vector_clocks(self_vc, other_vc) {
        ClockOrdering::Before => MergeDecision::ApplyRemote,
        ClockOrdering::After => MergeDecision::KeepLocal,
        ClockOrdering::Equal | ClockOrdering::Concurrent => {
            // Equal: vector clock 完全相同（异常：version 应也相同，但前面已排除）
            // Concurrent: 真并发，用 version tie-break
            if other_version > self_version {
                MergeDecision::ApplyRemote
            } else if self_version > other_version {
                MergeDecision::KeepLocal
            } else {
                MergeDecision::Equal
            }
        },
    }
}

/// 构造一个新 vector clock entry：递增本地 node_id 的计数器，保留其他节点的最近值。
///
/// 用于本地写入时：self_vc[node_id] += 1；self_version += 1。
///
/// 返回 `(new_vc, new_version)`。
pub fn bump_local_clock(
    self_vc: &BTreeMap<String, u64>,
    self_version: u64,
    node_id: &str,
) -> (BTreeMap<String, u64>, u64) {
    let mut new_vc = self_vc.clone();
    let entry = new_vc.entry(node_id.to_string()).or_insert(0);
    *entry += 1;
    (new_vc, self_version + 1)
}

#[cfg(test)]
mod crdt_tests {
    use super::*;

    fn vc(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn merge_crdt_equal_when_identical() {
        let vc = vc(&[("node-a", 3), ("node-b", 1)]);
        assert_eq!(merge_crdt(&vc, 5, &vc, 5), MergeDecision::Equal);
    }

    #[test]
    fn merge_crdt_apply_remote_when_remote_strictly_newer() {
        // remote 在 self 之后（happened-before）
        let self_vc = vc(&[("node-a", 1)]);
        let remote_vc = vc(&[("node-a", 2)]);
        assert_eq!(
            merge_crdt(&self_vc, 1, &remote_vc, 2),
            MergeDecision::ApplyRemote
        );
    }

    #[test]
    fn merge_crdt_keep_local_when_local_strictly_newer() {
        let self_vc = vc(&[("node-a", 3)]);
        let remote_vc = vc(&[("node-a", 1)]);
        assert_eq!(
            merge_crdt(&self_vc, 3, &remote_vc, 1),
            MergeDecision::KeepLocal
        );
    }

    #[test]
    fn merge_crdt_concurrent_uses_version_tiebreak() {
        // 真正的并发：self 增加 node-a，remote 增加 node-b（互不影响）
        let self_vc = vc(&[("node-a", 2), ("node-b", 1)]);
        let remote_vc = vc(&[("node-a", 1), ("node-b", 2)]);
        // vector clock 既不 ≤ 也不 ≥ → 真并发
        // version 相同 → Equal（无操作）
        assert_eq!(merge_crdt(&self_vc, 2, &remote_vc, 2), MergeDecision::Equal);

        // version tie-break: remote > self → ApplyRemote
        assert_eq!(
            merge_crdt(&self_vc, 1, &remote_vc, 2),
            MergeDecision::ApplyRemote
        );

        // version tie-break: self > remote → KeepLocal
        assert_eq!(
            merge_crdt(&self_vc, 5, &remote_vc, 2),
            MergeDecision::KeepLocal
        );
    }

    #[test]
    fn merge_crdt_three_node_concurrent() {
        // 3 节点并发：a/b 各增加自己
        let self_vc = vc(&[("a", 5), ("b", 1), ("c", 3)]);
        let remote_vc = vc(&[("a", 3), ("b", 2), ("c", 3)]);
        // a 在 self 增加、b 在 remote 增加 → concurrent
        // c 相同 → tie-break by version
        assert_eq!(
            merge_crdt(&self_vc, 5, &remote_vc, 3),
            MergeDecision::KeepLocal // self.version 5 > 3
        );
        assert_eq!(
            merge_crdt(&self_vc, 3, &remote_vc, 5),
            MergeDecision::ApplyRemote // remote.version 5 > 3
        );
    }

    #[test]
    fn merge_crdt_multinode_happened_before() {
        // 3 节点：self 看到 (a:2, b:1)，remote 看到 (a:3, b:1)
        // → remote 在 self 之后（strict）
        let self_vc = vc(&[("a", 2), ("b", 1)]);
        let remote_vc = vc(&[("a", 3), ("b", 1)]);
        assert_eq!(
            merge_crdt(&self_vc, 2, &remote_vc, 3),
            MergeDecision::ApplyRemote
        );
    }

    #[test]
    fn bump_local_clock_increments_self_node() {
        let vc = vc(&[("a", 3), ("b", 5)]);
        let (new_vc, new_ver) = bump_local_clock(&vc, 10, "a");
        assert_eq!(new_vc.get("a"), Some(&4));
        assert_eq!(new_vc.get("b"), Some(&5)); // 保留
        assert_eq!(new_ver, 11);
    }

    #[test]
    fn bump_local_clock_creates_new_entry() {
        let vc = vc(&[("a", 1)]);
        let (new_vc, new_ver) = bump_local_clock(&vc, 1, "c");
        assert_eq!(new_vc.get("a"), Some(&1));
        assert_eq!(new_vc.get("c"), Some(&1));
        assert_eq!(new_ver, 2);
    }

    #[test]
    fn bump_then_merge_roundtrip() {
        // 本地 bump 一次，与 remote 比较，bump 后的版本应当严格大于之前
        let vc = vc(&[("a", 1)]);
        let (new_vc, new_ver) = bump_local_clock(&vc, 1, "a");
        assert_eq!(
            merge_crdt(&vc, 1, &new_vc, new_ver),
            MergeDecision::ApplyRemote
        );
    }
}
