use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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

/// 从存储主键中取出写入计数（该节点的第几次写入）。
///
/// 与 [`experience_origin_from_key`] 成对使用：两者要么都成功，要么在主键
/// 不符合 `exp:{timestamp}:{origin}:{counter}` 时都返回 `None`。
///
/// 用途是进程重启后把本地序号恢复到历史最大值——序号回退会让 `version` 也
/// 回退，跨节点 CRDT 的版本裁决会把重启后的新写入误判成旧版本而丢弃。
pub fn experience_counter_from_key(key: &str) -> Option<u64> {
    let rest = key.strip_prefix("exp:")?;
    let counter = rest.rsplit(':').next()?;
    if counter.is_empty() || !counter.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    counter.parse().ok()
}

/// 单个 origin 的持有情况：最大写入计数 + 该范围内的空洞。
///
/// ADR-010 最初只记 `max`，隐含前提是「单个 origin 内持有的一定是 1..=max
/// 的连续前缀」。这个前提在**广播 Lagged** 下不成立：接收端可能丢中间几条而
/// 保有尾部，此时 `max` 仍然很大，对端据此算出的差量为空，那个洞永久留存。
///
/// 因此额外携带空洞列表。正常节点空洞恒为空（写入序号由 `fetch_add` 单调
/// 分配），所以这个字段不改变常规传输量；只有真正存在丢失时才变长，
/// 而那正是需要修复的量。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OriginDigest {
    /// 本地持有的最大 counter。0 表示该 origin 一条都没有。
    pub max: u64,
    /// `1..=max` 之中本地**缺失**的 counter，升序。
    ///
    /// 保持升序是为了让 [`OriginDigest::holds`] 能二分查找——空洞很多时线性
    /// 查找会让每次差量计算退化成 O(总行数)。
    pub holes: Vec<u64>,
}

impl OriginDigest {
    /// 本地是否确实持有该 origin 的第 `counter` 条。
    ///
    /// 语义是「有证据地持有」而非「没被告知缺失」：只有落在 `1..=max` 且不在
    /// 空洞里才算持有。未出现在摘要里的 origin 一律视为不持有。
    pub fn holds(&self, counter: u64) -> bool {
        counter > 0 && counter <= self.max && self.holes.binary_search(&counter).is_err()
    }
}

/// 节点的紧凑摘要：(node_id → 该节点的持有情况)。
///
/// 只依赖行标识而不依赖行内容，因此计算成本与经验正文大小无关。这是
/// ADR-010 反熵补偿成立的前提——行标识编码了来源与计数，于是「我有什么」
/// 可以用一个小得多的 map 表达，而不是把整个经验库发过去。
pub type ExperienceDigest = BTreeMap<String, OriginDigest>;

/// 把若干条经验主键折叠成紧凑摘要。
///
/// 主键不符合 `exp:{timestamp}:{origin}:{counter}` 的行（ADR-010 之前遗留的
/// 旧格式 `exp:{timestamp}`）解析不出 origin/counter，直接跳过——它们既不
/// 参与摘要，也不会被差量计算请求，因此不会被反熵补齐。这是已记录的取舍：
/// 项目处于 0.x，清库重建即可，不写迁移。
pub fn digest_from_keys<'a, I>(keys: I) -> ExperienceDigest
where
    I: IntoIterator<Item = &'a str>,
{
    // 先按 origin 收集实际持有的 counter，再折叠成「最大计数 + 空洞」。
    // 不能直接增量折叠：只看 max 的话，空洞信息在折叠那一步就永久丢失了。
    let mut per_origin: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    for key in keys {
        let (Some(origin), Some(counter)) = (
            experience_origin_from_key(key),
            experience_counter_from_key(key),
        ) else {
            continue;
        };
        per_origin.entry(origin.to_string()).or_default().insert(counter);
    }

    per_origin
        .into_iter()
        .map(|(origin, present)| {
            // 空洞必须如实上报，否则对端无从知道该补什么。
            //
            // 这里用「相邻元素的间隙」来枚举空洞，而不是遍历 `1..=max`：
            // 后者是 O(max)，在「某节点写过千万条而本地只收到零星几条」时会
            // 退化成千万次无意义迭代。前者是 O(rows)——而我们为了建这个集合
            // 本来就得遍历全部 rows，因此不引入任何额外量级。
            let max = present.iter().next_back().copied().unwrap_or(0);
            let mut holes: Vec<u64> = Vec::new();
            let mut prev = 0u64;
            for &counter in present.iter() {
                // checked_add：`prev == u64::MAX` 时不再产出空洞，
                // 而不是回绕成 0 并把整个区间错报为缺失。
                let Some(gap_start) = prev.checked_add(1) else {
                    break;
                };
                if counter > gap_start {
                    holes.extend(gap_start..counter);
                }
                prev = counter;
            }
            (origin, OriginDigest { max, holes })
        })
        .collect()
}

/// 计算反熵差量：对端应当回送给 `local` 的经验主键。
///
/// 纯函数，不触碰 I/O，因此可以脱离数据库与网络单独验证。
///
/// 语义：回送 `local` **确实没有**的每一行，而不是「counter 超出已持有上界」
/// 的行。早期版本用 `counter > local[origin]` 判断，那等价于假定本地持有的
/// 一定是 `1..=max` 的连续前缀；广播 Lagged 会在中间打洞而尾部照常抵达，
/// 于是该假定不成立——对端看到本地 `max` 很大，算出差量为空，洞永久留存。
/// 改用 [`OriginDigest::holds`] 逐条断言，代价是对端多发一个空洞列表。
///
/// 前提是主键编码了来源与计数。旧格式主键（`exp:{timestamp}`）解析不出
/// origin，会被这里丢弃；理由见 [`digest_from_keys`]。
///
/// 对端应当对结果排序（[`sorted_missing_keys`]）后再分页，否则顺序不稳定的
/// 集合在分页时会重复或漏发。
pub fn missing_keys<'a, I>(local: &ExperienceDigest, remote_keys: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    remote_keys.into_iter().filter(|key| is_key_missing(local, key)).collect()
}

/// 单条主键是否不在 `local` 摘要的「确认持有」范围内。
///
/// 与 [`missing_keys`] 是同一判定的两种入口：批量差量计算用前者，反熵服务端
/// 的 keyset 扫描（ADR-012）逐条判断用本函数。两者若各写一份，空洞语义迟早
/// 会漂移——ADR-011 的教训正是「差量判定只能有一处」。
///
/// 主键解析不出 origin/counter（旧格式 `exp:{timestamp}`）时返回 `false`：
/// 这类行既不参与摘要也不参与补齐，这是已记录的取舍。
pub fn is_key_missing(local: &ExperienceDigest, key: &str) -> bool {
    let (Some(origin), Some(counter)) = (
        experience_origin_from_key(key),
        experience_counter_from_key(key),
    ) else {
        return false;
    };
    // 摘要里没有这个 origin，就等于一条也没有（`None` 时 `holds` 恒假）。
    !local.get(origin).is_some_and(|d| d.holds(counter))
}

/// 按字典序排好差量，让分页在多次请求间保持稳定。
///
/// 分页是无状态的（每页都从完整的差量集合里取一段），因此顺序必须稳定：
/// 否则两次请求可能对同一区间给出不同结果，造成重复或漏发。
pub fn sorted_missing_keys(mut keys: Vec<&str>) -> Vec<&str> {
    keys.sort_unstable();
    keys
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

    /// 导出本地经验库的紧凑版本向量摘要（ADR-010 反熵）。
    ///
    /// 回答「我有什么」而不只是「我写了什么」：返回 `(node_id → OriginDigest)` 的
    /// 紧凑摘要，只依赖行主键而不依赖行内容，因此可作为 `SyncRequest` 的载荷。
    /// dial 方导出本摘要后随流发出，对端据此算出自己应补发哪些行
    /// （见 `missing_keys` / `anti_entropy::serve_sync_request`）。
    async fn export_version_vector(&self) -> Result<ExperienceDigest, TypesError>;
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

#[cfg(test)]
mod anti_entropy_tests {
    use super::*;

    /// 反熵差量的核心前提：行标识能推出来源与计数。
    ///
    /// 若这一条失效，摘要就无法表达「我有什么」，ADR-010 整体不成立。
    #[test]
    fn digest_folds_keys_into_max_counter_per_origin() {
        let digest = digest_from_keys([
            "exp:100:node-a:1",
            "exp:101:node-a:2",
            "exp:102:node-a:3",
            // 连续持有：生产中写入序号由 fetch_add 单调分配，不会跳号。
            "exp:200:node-b:1",
            "exp:201:node-b:2",
            "exp:202:node-b:3",
        ]);
        assert_eq!(digest.get("node-a").map(|d| d.max), Some(3));
        assert_eq!(digest.get("node-b").map(|d| d.max), Some(3));
        // 连续持有时不应产生空洞——这是 ADR-011 保持常规传输量不变的前提。
        assert!(digest.values().all(|d| d.holes.is_empty()));
    }

    /// 稀疏 origin：只持有 counter 7 时，1..6 就是真实缺失，必须如实上报。
    ///
    /// 生产中单个 origin 的写入序号不会跳号，所以这种形态只出现在测试数据里；
    /// 但语义必须成立：持有集合是事实，不是推测。把它谎报成「持有 1..=7」
    /// 只会让真正缺失的行永远补不上。
    #[test]
    fn sparse_origin_reports_every_absent_counter() {
        let digest = digest_from_keys(["exp:200:node-b:7"]);
        let node_b = digest.get("node-b").expect("node-b 应有摘要条目");
        assert_eq!(node_b.max, 7);
        assert_eq!(node_b.holes, vec![1, 2, 3, 4, 5, 6]);
    }

    /// 连续性：一个origin 内空洞确实被摘要捕捉到。
    ///
    /// 摘要的核心断言是「有洞就要说得出洞」，否则对端无从知道该补什么。
    #[test]
    fn digest_records_holes_within_contiguous_prefix() {
        let digest = digest_from_keys([
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            // 缺 3
            "exp:4:node-a:4",
            "exp:5:node-a:5",
        ]);
        let node_a = digest.get("node-a").expect("node-a 应有摘要条目");
        assert_eq!(node_a.max, 5);
        assert_eq!(node_a.holes, vec![3], "缺失的 counter 必须进入空洞列表");
        assert!(!node_a.holds(3), "空洞内不得被判定为持有");
        assert!(node_a.holds(4));
        assert!(!node_a.holds(6), "超出 max 不得被判定为持有");
    }

    /// 空库摘要为空——新节点正是从空摘要出发拉取全量的。
    #[test]
    fn digest_of_empty_store_is_empty() {
        assert!(digest_from_keys(Vec::<&str>::new()).is_empty());
    }

    /// 旧格式主键解析不出 origin/counter，必须跳过而不是计入某个节点。
    ///
    /// 若把它们计到某个 origin 下，会把计数虚高，对端据此判断「你都有了」
    /// 从而永远不再补齐真实的新经验。
    #[test]
    fn digest_skips_legacy_key_format() {
        let digest = digest_from_keys(["exp:1600000000", "exp:100:node-a:4"]);
        assert_eq!(digest.get("node-a").map(|d| d.max), Some(4));
        assert_eq!(digest.len(), 1, "旧格式行不应产生任何 origin 条目");
    }

    /// 新节点加入：对端持有的全部经验都是差量。
    #[test]
    fn new_node_requests_everything() {
        let remote = ["exp:1:node-a:1", "exp:2:node-a:2", "exp:3:node-b:1"];
        let missing = sorted_missing_keys(missing_keys(&ExperienceDigest::new(), remote));
        assert_eq!(
            missing,
            vec!["exp:1:node-a:1", "exp:2:node-a:2", "exp:3:node-b:1"]
        );
    }

    /// 断线补偿：只请求 counter 超出已持有范围的部分。
    ///
    /// 这是 ADR-010 相对「连接后全量拉取」的核心优势：传输量正比于真实差量。
    #[test]
    fn reconnect_requests_only_the_gap() {
        let local = digest_from_keys(["exp:1:node-a:1", "exp:2:node-a:2", "exp:3:node-a:3"]);
        let remote = [
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            "exp:3:node-a:3",
            "exp:9:node-a:4",
            "exp:9:node-a:5",
        ];
        let missing = sorted_missing_keys(missing_keys(&local, remote));
        assert_eq!(missing, vec!["exp:9:node-a:4", "exp:9:node-a:5"]);
    }

    /// 已完全同步时差量为空——幂等性的前提。
    #[test]
    fn already_synced_yields_empty_delta() {
        let keys = ["exp:1:node-a:1", "exp:2:node-a:2"];
        let local = digest_from_keys(keys);
        assert!(missing_keys(&local, keys).is_empty());
    }

    /// 多 origin 各自独立比较：一个节点落后不影响另一个节点的判断。
    #[test]
    fn delta_is_computed_per_origin() {
        // 本地对 node-a 已同步到 5，对 node-b 一条都没有
        let local = digest_from_keys(["exp:1:node-a:1", "exp:5:node-a:5"]);
        let remote = [
            "exp:5:node-a:5",
            "exp:6:node-a:6", // 本地缺
            "exp:1:node-b:1", // 本地全缺
        ];
        let missing = sorted_missing_keys(missing_keys(&local, remote));
        assert_eq!(missing, vec!["exp:1:node-b:1", "exp:6:node-a:6"]);
    }

    /// ADR-011：广播 Lagged 造成的尾部空洞必须能被反熵发现。
    ///
    /// 这是 ADR-010 摘要语义的直接后果，也是本用例存在的唯一理由：
    /// `ExperienceDigest` 只记 (origin → 最大 counter)，因此只能表达
    /// 「持有 0..N 连续区间」。本地持有 1..4 与 6..7、缺 5 时，摘要仍报 7，
    /// 对端据此算出差量为空——那个洞永久留在库里，没有任何机制会补它。
    #[test]
    fn hole_inside_contiguous_prefix_is_still_detected() {
        let local = digest_from_keys([
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            "exp:3:node-a:3",
            "exp:4:node-a:4",
            // 缺 5
            "exp:6:node-a:6",
            "exp:7:node-a:7",
        ]);
        let remote = [
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            "exp:3:node-a:3",
            "exp:4:node-a:4",
            "exp:5:node-a:5",
            "exp:6:node-a:6",
            "exp:7:node-a:7",
        ];
        let missing = sorted_missing_keys(missing_keys(&local, remote));
        assert_eq!(
            missing,
            vec!["exp:5:node-a:5"],
            "尾部空洞内的缺失行必须仍被识别为差量"
        );
    }

    /// 旧格式行不会被请求（它们无法参与摘要），这是已记录的取舍而非缺陷。
    #[test]
    fn delta_never_requests_legacy_rows() {
        let local = ExperienceDigest::new();
        let missing = sorted_missing_keys(missing_keys(&local, ["exp:1600000000"]));
        assert!(missing.is_empty());
    }

    /// 单键判定必须与批量差量完全一致（keyset 扫描依赖这一条）。
    ///
    /// 反熵服务端改为按游标逐条扫描后，用 `is_key_missing` 代替
    /// `missing_keys` 的过滤。两者语义若有一点偏差，翻页就会重复或漏发。
    #[test]
    fn single_key_missing_matches_batch_delta() {
        let local = digest_from_keys([
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            // 缺 3
            "exp:4:node-a:4",
        ]);
        let remote = [
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            "exp:3:node-a:3", // 空洞 → 缺失
            "exp:4:node-a:4",
            "exp:5:node-a:5", // 超出 max → 缺失
            "exp:9:node-b:1", // 未知 origin → 缺失
            "exp:1600000000", // 旧格式 → 两边都不请求
        ];

        let batch: std::collections::BTreeSet<&str> =
            missing_keys(&local, remote).into_iter().collect();
        for key in remote {
            assert_eq!(
                is_key_missing(&local, key),
                batch.contains(key),
                "单键判定与批量差量在 {key} 上不一致"
            );
        }
        assert!(is_key_missing(&local, "exp:3:node-a:3"));
        assert!(!is_key_missing(&local, "exp:2:node-a:2"));
        assert!(!is_key_missing(&local, "exp:1600000000"));
    }

    /// 排序必须稳定，否则无状态分页会重复或漏发。
    ///
    /// 模拟分页场景：三次不同顺序的输入，排好后分页取区间结果一致。
    #[test]
    fn sorted_delta_is_stable_across_input_orders() {
        let a = [
            "exp:1:node-a:1",
            "exp:2:node-a:2",
            "exp:3:node-a:3",
            "exp:4:node-a:4",
        ];
        let mut b = a;
        b.reverse();

        let local = ExperienceDigest::new();
        let page_a = sorted_missing_keys(missing_keys(&local, a));
        let page_b = sorted_missing_keys(missing_keys(&local, b));
        assert_eq!(page_a, page_b);

        // 按固定页大小切片，每页互不重叠且合起来覆盖全部
        let page_size = 3;
        let first: Vec<_> = page_a[..page_size].to_vec();
        let second: Vec<_> = page_a[page_size..].to_vec();
        assert_eq!(first.len(), 3);
        assert_eq!(second.len(), 1);
        assert!(first.iter().all(|k| !second.contains(k)));
    }

    /// origin 中含分隔符时仍能正确解析。
    ///
    /// node_id 来自环境变量，在容器编排里完全可能是含 `-`、`.` 甚至 `:` 的
    /// 字符串（如 k8s 的 `node.cluster.local`）。解析只按最后两段切分，
    /// 因此中间的 `:` 会被归到 head 里一并丢弃。
    #[test]
    fn origin_with_dots_and_hyphens_parses() {
        let key = "exp:1700000000:node-1.cluster.local:42";
        assert_eq!(
            experience_origin_from_key(key),
            Some("node-1.cluster.local")
        );
        assert_eq!(experience_counter_from_key(key), Some(42));
    }
}
