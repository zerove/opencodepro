//! 跨节点经验反熵补偿（ADR-010）
//!
//! 经验同步事件是**即发即失**的：流建立前的广播永久丢失，断连期间其他节点
//! 产生的经验重连后不会补回，新节点加入时对既有经验一无所知。这个模块补上
//! 收敛机制，闭合三个丢失窗口。
//!
//! 协议（挂在既有双向 `Stream` 上，不新增 RPC）：
//!
//! ```text
//! dialer ──Join──> peer
//! dialer ──SyncRequest{ vector_clock }──> peer
//! peer   ──ExperienceSync[]（分页, more 标志）──> dialer
//! ```
//!
//! 关键设计：
//! - **传输量正比于真实差量**，与历史总量无关。dial 方发的是 (node_id →
//!   最大 counter) 的紧凑摘要，不是整个经验库。
//! - **复用既有 `ExperienceSync` 载荷**，因此接收端走与实时广播完全相同的
//!   `apply_remote` → `merge_crdt` 路径，不引入第二套合并语义，天然幂等。
//! - **有界**：每页最多 `SYNC_PAGE_SIZE` 条，防止一次回灌撑爆单条 gRPC 消息。
//! - **不得阻塞写入路径**：反熵是连接建立后触发的旁路，失败只记日志。

use sisyphus_bridge_api::cluster_proto::{ClusterEvent, ExperienceSync};
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_types::experience::{
    is_key_missing, missing_keys, sorted_missing_keys, ExperienceDigest, ExperienceTuple,
};
use std::collections::HashMap;
use std::sync::Arc;

/// 单次 `SyncResponse` 的条目上界。
///
/// 必须有界：经验库随运行时间单调增长，一次无界回灌会让单条 gRPC 消息
/// 超出默认 4MB 限制，整条流被服务端拒绝——反而比不同步更糟。
/// 取 128 与 ADR-010 一致；单条经验典型在数 KB 量级。
pub const SYNC_PAGE_SIZE: usize = 128;

/// 反熵应答：算好差量、装好一页，返回是否还有后续页。
///
/// 纯逻辑（I/O 已在上层完成），因此可脱离数据库与 gRPC 单独验证。
///
/// `local` 是 dial 方报告的摘要；`remote_rows` 是本节点库中
/// (主键, 载荷) 的完整列表。返回的是本节点应当回送的那一页。
///
/// 分页是**无状态**的：`offset` 表示「从排好序的完整差量集合里跳过多少条」。
/// 调用方每次都用同一个 `local` 重新计算差量，因此顺序必须稳定——
/// 这由 [`sorted_missing_keys`] 保证，否则两次请求可能对同一区间给出不同
/// 结果，造成重复或漏发。
pub fn build_sync_page(
    local: &ExperienceDigest,
    remote_rows: &[(String, ExperienceTuple)],
    offset: usize,
    page_size: usize,
) -> (Vec<ExperienceSync>, bool) {
    let keys: Vec<&str> = remote_rows.iter().map(|(id, _)| id.as_str()).collect();
    let delta = sorted_missing_keys(missing_keys(local, keys));

    // 一次建索引，避免对每个差量键线性扫描整页（原先 O(page_size²)）。
    // 键在同一页内不会重复：调用方用 `id IN (...)` 取行，同一主键只有一行。
    let by_key: HashMap<&str, &ExperienceTuple> =
        remote_rows.iter().map(|(id, exp)| (id.as_str(), exp)).collect();

    let page = &delta[offset.min(delta.len())..];
    let take = &page[..page.len().min(page_size)];

    let experiences = take
        .iter()
        .filter_map(|key| {
            let exp = *by_key.get(key)?;
            // 序列化失败（如 NaN 浮点）不应中断整页同步：跳过该条并继续。
            // 反熵是尽力而为的旁路，一条坏数据不该挡住其余经验的补齐。
            let payload = match serde_json::to_vec(exp) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(exp_key = %key, error = %e, "anti-entropy: skipping unserializable experience");
                    return None;
                },
            };
            let vector_clock_json = match serde_json::to_string(&exp.vector_clock) {
                Ok(j) => j,
                Err(e) => {
                    tracing::warn!(exp_key = %key, error = %e, "anti-entropy: skipping experience with unserializable clock");
                    return None;
                },
            };
            Some(ExperienceSync {
                exp_key: (*key).to_string(),
                version: exp.version,
                vector_clock_json,
                payload,
            })
        })
        .collect();

    let consumed = offset.min(delta.len()) + take.len();
    let more = consumed < delta.len();
    (experiences, more)
}

/// 分页应答结果：本页经验 + 是否还有后续页 + 下一游标。
#[derive(Debug, Clone, PartialEq)]
pub struct SyncPage {
    /// 本页经验（已按主键字典序稳定排序）。
    pub experiences: Vec<ExperienceSync>,
    /// 是否还有后续页。
    pub more: bool,
    /// 请求方下一次 `SyncRequest` 应携带的游标键。
    ///
    /// 这是**主键**而不是下标（ADR-012）。原先用 `offset` 下标定位下一段，
    /// 但差量集合每翻一页就会缩短：B 收到一页就多持有一批行，那些行随即
    /// 从差量里消失，差量整体前移。于是按旧下标切片必然跳过一批仍缺失的行，
    /// 表现为差量跨 3 页以上时尾部静默丢失。
    ///
    /// 游标改为「最后一个已送达主键」，服务端据此取严格大于它的下一段：
    /// 无论差量如何缩短或前移，位置都由内容本身决定，不再依赖会漂移的下标。
    pub next_cursor: String,
}

/// 分页游标：上一页最后一个已送达的主键；空串表示从头开始。
pub type SyncCursor = String;

/// keyset 扫描的批大小。
///
/// 与 [`SYNC_PAGE_SIZE`] 解耦：游标之后的键里可能夹着大量对端已持有的行
/// （最常见的是「只缺尾部几条」），这些行只参与摘要比对、不占页容量。
/// 批量取键让它们只花少数几次索引区间扫描，而不是每页一次全表扫描。
///
/// 取 512 与 `load_experiences_by_keys` 的分批上限同量级：键很短，
/// 512 条键的内存占用远低于一页经验载荷。
const KEY_SCAN_BATCH: usize = 512;

/// 服务端角色：算出 dial 方缺的差量并分页回送。
///
/// 复用同一批 `ExperienceSync` 载荷 → dial 端 `ExperienceSyncHandler` 的处理
/// 路径与实时广播完全相同，不引入第二套合并语义，因此天然幂等。
///
/// 分页是**无状态**的：每次都从本地库重新计算差量，再取游标之后的
/// `page_size` 条。差量集合会随 dial 方接收而缩短，但游标是内容而非下标，
/// 因此缩短只影响「还剩多少条」，不影响「从哪一条继续」——这是 ADR-010
/// 用 `offset` 下标时丢失尾部行的根因。
///
/// 差量扫描走**主键区间**（`id > cursor ORDER BY id LIMIT n`），不是把全表键
/// 读进内存再切片。后者让每页成本都是 O(库大小)，翻 P 页即 O(P × N)；
/// 而 keyset 扫描的单页成本只与本页实际扫过的键数有关。
pub async fn serve_sync_request(
    experience_bank: &Arc<VectorExperienceBank>,
    requested: &ExperienceDigest,
    cursor: &str,
    page_size: usize,
) -> Result<SyncPage, String> {
    // `page_size = 0` 会让「多收一条」的判定恒真：收到第一条缺失键就判定
    // `more = true`，但页随即被截断成空、游标不前进。拨号器是完全按 `more`
    // 翻页的（见 `fallback.rs` 的 paging 循环），于是它会用同一个游标无限
    // 重发同一页——一个没有进展的死循环，且只表现为「同步卡住」。
    // 这里直接返回空页：配置错误应当表现为「同步不到东西」，而不是死循环。
    if page_size == 0 {
        tracing::warn!("anti-entropy: page_size = 0, returning an empty page");
        return Ok(SyncPage {
            experiences: Vec::new(),
            more: false,
            next_cursor: cursor.to_string(),
        });
    }

    // 1. 从游标处按主键区间扫描，收集对端确实缺失的键。
    //    多收一条用于判定 `more`：只有确知后面还有缺失行，才让 dial 方继续翻页。
    let mut page_keys: Vec<String> =
        Vec::with_capacity(page_size.min(KEY_SCAN_BATCH).saturating_add(1));
    let mut scan_cursor = cursor.to_string();

    loop {
        let batch =
            experience_bank.keys_after(&scan_cursor, KEY_SCAN_BATCH).await.map_err(|e| {
                tracing::warn!(error = %e, "anti-entropy: failed to scan local experience keys");
                format!("key listing failed: {e}")
            })?;
        if batch.is_empty() {
            break;
        }

        for key in &batch {
            // 主键解析不出 origin/counter 的旧格式行永远不会被请求；这与
            // `missing_keys` 的取舍一致，且由 `is_key_missing` 单点保证。
            if !is_key_missing(requested, key) {
                continue;
            }
            page_keys.push(key.clone());
            if page_keys.len() > page_size {
                break;
            }
        }

        if page_keys.len() > page_size {
            break;
        }
        // 本批不足一批 → 已到表尾，不会再有缺失行。
        let Some(last) = batch.last() else {
            break;
        };
        scan_cursor = last.clone();
        if batch.len() < KEY_SCAN_BATCH {
            break;
        }
    }

    let more = page_keys.len() > page_size;
    page_keys.truncate(page_size);

    // 2. 只取这一页的完整载荷。
    let rows = experience_bank.load_by_keys(&page_keys).await.map_err(|e| {
        tracing::warn!(error = %e, "anti-entropy: failed to load delta rows");
        format!("row load failed: {e}")
    })?;

    // `build_sync_page` 会再用 `requested` 过滤一遍 `rows`：若并发删除使某键消失，
    // 过滤会把它剔除而不会发出一条悬空引用。因此实际发出条数可能少于
    // `page_keys`，而游标必须按**请求区间**而非 `experiences.len()` 推进——
    // 否则每页都会因过滤少一条而反复重算同一段，永远翻不到下一页。
    let (experiences, _) = build_sync_page(requested, &rows, 0, page_size);

    // 游标取本区间最后一个**差量主键**（而非实际发出的最后一条）：
    // 前者在过滤发生后仍然正确地把位置推过整段，后者在「末条被过滤」时
    // 会把游标留在段内，造成下一页重复发送已发过的内容。
    let next_cursor = page_keys
        .last()
        .cloned()
        // 本页为空（差量已耗尽）时游标不动：留在原地让 dial 方以
        // `more = false` 收尾，而不是凭空跳到一个未经确认的位置。
        .unwrap_or_else(|| cursor.to_string());

    tracing::debug!(
        page_len = experiences.len(),
        more,
        next_cursor = %next_cursor,
        "anti-entropy: served sync page"
    );

    Ok(SyncPage {
        experiences,
        more,
        next_cursor,
    })
}

/// dial 方角色：导出本地摘要，交给调用方随流发出。
///
/// 单独成函数是为了让「摘要导出失败」不阻断连接建立：反熵是旁路，
/// 拿不到摘要就退化成只依赖实时广播，与反熵落地前的行为一致。
pub async fn local_digest(
    experience_bank: &Arc<VectorExperienceBank>,
) -> Result<ExperienceDigest, String> {
    experience_bank.digest().await.map_err(|e| format!("digest export failed: {e}"))
}

/// 把 dial 方摘要与游标封装成一条 `SyncRequest` 事件。
pub fn sync_request_event(digest: &ExperienceDigest, cursor: &str) -> ClusterEvent {
    ClusterEvent {
        event: Some(
            sisyphus_bridge_api::cluster_proto::cluster_event::Event::SyncRequest(
                digest_to_sync_request_payload(digest, cursor),
            ),
        ),
    }
}

/// 把 dial 方摘要转成 proto 的 `SyncRequest` 载荷。
pub fn digest_to_sync_request_payload(
    digest: &ExperienceDigest,
    cursor: &str,
) -> sisyphus_bridge_api::cluster_proto::SyncRequest {
    let origins = digest
        .iter()
        .map(
            |(origin, d)| sisyphus_bridge_api::cluster_proto::OriginDigest {
                origin: origin.clone(),
                max: d.max,
                holes: d.holes.clone(),
            },
        )
        .collect();
    sisyphus_bridge_api::cluster_proto::SyncRequest {
        vector_clock: Default::default(),
        // 已废弃字段必须显式置 0：proto3 的标量默认值本身就是 0，但显式写出
        // 能让「我们不再发送 offset」这件事在代码里可见，而不是靠省略。
        //
        // `#[allow(deprecated)]` 与这段注释是一体：显式赋 0 必然触碰已废弃字段，
        // 没有这个属性 clippy 的 `-D warnings` 会把告警升级为错误、整个 crate
        // 编不过。它抑制的是「使用已废弃字段」这一个诊断，不影响其它告警。
        #[allow(deprecated)]
        offset: 0,
        cursor: cursor.to_string(),
        origins,
    }
}

/// 解析对端发来的 proto `SyncRequest` 载荷。
///
/// `origins` 为空时按「请求全量」处理：新节点首次连接时摘要本就为空，而对端
/// 也可能是老版本（只发已废弃的 `vector_clock`，不发 `origins`）。此时多传
/// 一些数据是可接受的代价，因为合并路径幂等；反过来把空当成「不需要」会导致
/// 新节点永远收不到任何东西。
pub fn parse_sync_request_payload(
    payload: &sisyphus_bridge_api::cluster_proto::SyncRequest,
) -> ExperienceDigest {
    if payload.origins.is_empty() {
        tracing::debug!("anti-entropy: empty sync request treated as full pull");
        return ExperienceDigest::new();
    }
    payload
        .origins
        .iter()
        .map(|o| {
            // 空洞必须升序去重，否则 `holds` 的二分查找会失效——那会把实际
            // 持有的行误判成缺失，反过来白拉一遍。
            let mut holes = o.holes.clone();
            holes.sort_unstable();
            holes.dedup();
            (
                o.origin.clone(),
                sisyphus_types::experience::OriginDigest { max: o.max, holes },
            )
        })
        .collect()
}

/// 拨号侧触发一次反熵：导出摘要并写入流。
///
/// 返回发送的摘要规模，便于调用方打点。
pub async fn request_sync(
    experience_bank: &Arc<VectorExperienceBank>,
    out_tx: &tokio::sync::mpsc::UnboundedSender<ClusterEvent>,
    cursor: &str,
) -> Result<usize, String> {
    let digest = local_digest(experience_bank).await?;
    let count = digest.len();

    out_tx
        .send(sync_request_event(&digest, cursor))
        .map_err(|_| "outbound channel closed".to_string())?;

    tracing::info!(
        origin_count = count,
        cursor = %cursor,
        "anti-entropy: sync request sent"
    );
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 仅测试使用：非测试代码不构造摘要（摘要来自真实经验库）。
    use sisyphus_types::experience::digest_from_keys;

    fn exp(version: u64, vc: &[(&str, u64)]) -> ExperienceTuple {
        ExperienceTuple {
            context_text: "ctx".into(),
            context_hash: "h".into(),
            action_id: "a".into(),
            project_id: None,
            outcome_success: true,
            deviation_score: 0.0,
            cost: 0,
            timestamp: 1,
            embedding: None,
            visual_embedding: None,
            insight: None,
            version,
            vector_clock: vc.iter().map(|(k, v)| ((*k).to_string(), *v)).collect(),
        }
    }

    fn rows(pairs: &[(&str, u64)]) -> Vec<(String, ExperienceTuple)> {
        pairs.iter().map(|(id, v)| ((*id).to_string(), exp(*v, &[("node-a", *v)]))).collect()
    }

    /// 新节点（空摘要）应拿到全部经验。
    #[test]
    fn empty_digest_gets_everything() {
        let rows = rows(&[("exp:1:node-a:1", 1), ("exp:2:node-a:2", 2)]);
        let (page, more) = build_sync_page(&ExperienceDigest::new(), &rows, 0, 128);
        assert_eq!(page.len(), 2);
        assert!(!more);
        assert_eq!(page[0].exp_key, "exp:1:node-a:1");
    }

    /// 已同步的节点差量为空——幂等性的直接证据。
    #[test]
    fn synced_digest_yields_nothing() {
        let rows = rows(&[("exp:1:node-a:1", 1), ("exp:2:node-a:2", 2)]);
        let digest = digest_from_keys(["exp:1:node-a:1", "exp:2:node-a:2"]);
        let (page, more) = build_sync_page(&digest, &rows, 0, 128);
        assert!(page.is_empty());
        assert!(!more);
    }

    /// 断线补偿：只补差量。
    #[test]
    fn only_gap_is_sent() {
        let rows = rows(&[
            ("exp:1:node-a:1", 1),
            ("exp:2:node-a:2", 2),
            ("exp:3:node-a:3", 3),
        ]);
        let digest = digest_from_keys(["exp:1:node-a:1"]);
        let (page, _) = build_sync_page(&digest, &rows, 0, 128);
        let keys: Vec<_> = page.iter().map(|e| e.exp_key.as_str()).collect();
        assert_eq!(keys, vec!["exp:2:node-a:2", "exp:3:node-a:3"]);
    }

    /// 分页上界：单页不得超过 page_size，且 more 标志正确。
    #[test]
    fn page_size_is_bounded_and_more_flag_is_set() {
        let many: Vec<(String, ExperienceTuple)> = (1..=10)
            .map(|i| {
                (
                    format!("exp:1:node-a:{i}"),
                    exp(i as u64, &[("node-a", i as u64)]),
                )
            })
            .collect();

        let (page, more) = build_sync_page(&ExperienceDigest::new(), &many, 0, 4);
        assert_eq!(page.len(), 4);
        assert!(more);

        // 翻到最后一页
        let (last, more) = build_sync_page(&ExperienceDigest::new(), &many, 8, 4);
        assert_eq!(last.len(), 2);
        assert!(!more, "最后一页之后不应再有更多");
    }

    /// 翻页不重不漏：把差量按固定页大小切开，合起来恰好覆盖全部且互不重叠。
    #[test]
    fn paging_covers_exactly_once() {
        let many: Vec<(String, ExperienceTuple)> = (1..=7)
            .map(|i| {
                (
                    format!("exp:1:node-a:{i}"),
                    exp(i as u64, &[("node-a", i as u64)]),
                )
            })
            .collect();
        let local = ExperienceDigest::new();
        let page_size = 3;

        let mut collected = Vec::new();
        let mut offset = 0;
        loop {
            let (page, more) = build_sync_page(&local, &many, offset, page_size);
            collected.extend(page.iter().map(|e| e.exp_key.clone()));
            if !more {
                break;
            }
            offset += page_size;
            assert!(offset < 100, "分页不应无限循环");
        }

        assert_eq!(collected.len(), 7, "每条经验恰好补齐一次");
        let unique: std::collections::HashSet<_> = collected.iter().collect();
        assert_eq!(unique.len(), 7, "不应重复补齐");
    }

    /// offset 超过差量长度时返回空页而不是 panic。
    #[test]
    fn offset_beyond_delta_is_safe() {
        let rows = rows(&[("exp:1:node-a:1", 1)]);
        let (page, more) = build_sync_page(&ExperienceDigest::new(), &rows, 999, 128);
        assert!(page.is_empty());
        assert!(!more);
    }

    /// 载荷与版本号必须随行一同回送，否则接收端无法做 CRDT 合并。
    #[test]
    fn payload_carries_version_and_clock() {
        let rows = vec![(
            "exp:5:node-a:5".to_string(),
            exp(5, &[("node-a", 5), ("node-b", 2)]),
        )];
        let (page, _) = build_sync_page(&ExperienceDigest::new(), &rows, 0, 128);
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].version, 5);

        let clock: std::collections::BTreeMap<String, u64> =
            serde_json::from_str(&page[0].vector_clock_json).expect("clock 应可解析");
        assert_eq!(clock.get("node-a"), Some(&5));
        assert_eq!(clock.get("node-b"), Some(&2));

        let decoded: ExperienceTuple =
            serde_json::from_slice(&page[0].payload).expect("载荷应可解析");
        assert_eq!(decoded.version, 5);
    }

    /// 多 origin 差量：各来源独立比较。
    #[test]
    fn multi_origin_delta_is_per_origin() {
        let rows = rows(&[("exp:1:node-a:5", 5), ("exp:1:node-b:1", 1)]);
        // 本地已同步 node-a 到 5，node-b 一条没有
        let digest = digest_from_keys(["exp:1:node-a:5"]);
        let (page, _) = build_sync_page(&digest, &rows, 0, 128);
        let keys: Vec<_> = page.iter().map(|e| e.exp_key.as_str()).collect();
        assert_eq!(keys, vec!["exp:1:node-b:1"]);
    }

    /// digest ↔ proto 载荷往返不丢信息。
    #[test]
    fn digest_payload_roundtrip() {
        let digest = digest_from_keys(["exp:1:node-a:3", "exp:2:node-b:7"]);
        let payload = digest_to_sync_request_payload(&digest, "");
        let parsed = parse_sync_request_payload(&payload);
        assert_eq!(parsed, digest);
    }

    /// 空 SyncRequest 等价于请求全量——对端没有摘要时只能如此。
    #[test]
    fn empty_sync_request_means_full_pull() {
        let parsed = parse_sync_request_payload(&Default::default());
        assert!(parsed.is_empty());
    }
}
