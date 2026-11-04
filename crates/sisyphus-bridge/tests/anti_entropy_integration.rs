//! ADR-010 反熵补偿的端到端集成测试。
//!
//! 与 `anti_entropy.rs` 里的纯函数单元测试互补：这里走**真实数据库 +
//! 真实经验库**，验证的是纯函数覆盖不到的部分——
//!
//! - 差量真的能从 SQLite 里按主键取回载荷（列名取值路径）；
//! - 游标 `next_cursor` 真的能推进到下一页而不重复、不漏发；
//! - dial 方按 `more` 标志翻页，最终恰好补齐全量。
//!
//! 这一层是 ADR-010 的核心断言所在：翻页闭环一旦断掉，差量超过一页的节点
//! 就会永远只拿到第一页，而这种缺陷在纯函数测试里看不出来。

use sisyphus_bridge::peer::anti_entropy;
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
use sisyphus_tools_llm::DummyEmbeddingClient;
use sisyphus_types::experience::{digest_from_keys, ExperienceDigest, ExperienceTuple};
use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

/// 在内存数据库上建一个真实经验库。
async fn setup_bank(node_id: &str) -> Arc<VectorExperienceBank> {
    let mut config = sisyphus_types::Config::default();
    config.cluster.node_id = node_id.to_string();
    let store = SisyphusStore::new("sqlite::memory:", &config).await.expect("store init");
    let state_store = store.state.clone();
    let vector_store =
        Arc::new(VectorMemoryStore::new(store.pool()).await.expect("vector store init"));
    Arc::new(
        VectorExperienceBank::new(state_store, vector_store, Arc::new(DummyEmbeddingClient))
            .await
            .expect("exp bank init"),
    )
}

/// 构造一条指定 counter 的经验。
///
/// 主键由 `record_with_id` 依据时间戳 + origin + 序号生成，因此这里只用
/// `context_text` 携带 counter 以便断言，实际主键由经验库决定。
fn make_exp(counter: u64) -> ExperienceTuple {
    ExperienceTuple {
        context_text: format!("ctx-{counter}"),
        context_hash: format!("hash-{counter}"),
        action_id: "act".into(),
        project_id: None,
        outcome_success: true,
        deviation_score: 0.0,
        cost: counter,
        timestamp: 1_700_000_000 + counter,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: counter,
        vector_clock: Default::default(),
    }
}

/// 写入 `count` 条经验，返回它们的主键。
async fn seed(bank: &Arc<VectorExperienceBank>, count: u64) -> Vec<String> {
    let mut keys = Vec::with_capacity(count as usize);
    for i in 1..=count {
        let key = bank.record_with_id(make_exp(i)).await.expect("record");
        keys.push(key);
    }
    keys
}

/// 按 ADR-012 的 wire 协议完整走一遍翻页循环，返回 dial 方收到的全部主键。
///
/// 这段刻意复刻 `fallback.rs::run_stream` 的翻页逻辑：dial 方发空游标，
/// peer 分页回送并附 `more` 与 `next_cursor`，dial 方据此请求下一页。
///
/// 游标是主键而非下标。dial 方每收到一条就会真的写入本地，于是下一轮的
/// 差量会变短——这正是 ADR-010 用下标时丢尾部行的原因。本辅助保留原始
/// digest（不随接收更新），因此覆盖的是协议层不推进行为的状态；
/// 「差量随接收缩短」这一真实形态由
/// `paging_survives_delta_shrinking_between_requests` 直接覆盖。
async fn drain_sync(peer: &Arc<VectorExperienceBank>, dialer: &ExperienceDigest) -> Vec<String> {
    let mut collected = Vec::new();
    let mut cursor = String::new();

    loop {
        let page = anti_entropy::serve_sync_request(peer, dialer, &cursor, 8)
            .await
            .expect("serve sync request");

        collected.extend(page.experiences.iter().map(|e| e.exp_key.clone()));
        cursor = page.next_cursor;

        if !page.more {
            break;
        }
        assert!(!cursor.is_empty(), "有下一页时游标不得为空");
    }

    collected
}

/// 差量超过单页时，翻页循环必须补齐全部经验且不重复。
///
/// 这是 ADR-010 最关键的不变式：页边界丢失或游标不推进时，本用例会失败。
#[tokio::test]
async fn paging_loop_delivers_every_exactly_once() {
    let peer = setup_bank("peer").await;
    let seeded = seed(&peer, 20).await;

    // dial 方是空库（全新节点）：应当拿到全部 20 条。
    let received = drain_sync(&peer, &ExperienceDigest::new()).await;

    assert_eq!(
        received.len(),
        20,
        "空库 dial 方应拿到全部经验，实际只拿到 {} 条——翻页很可能在第一页就停了",
        received.len()
    );
    let unique: HashSet<&String> = received.iter().collect();
    assert_eq!(unique.len(), 20, "不应重复补齐");
    assert_eq!(received.len(), seeded.len(), "回送条数应等于写入条数");
}

/// 混合版本：新 dial 方 ↔ 旧 server（服务端不解释 `cursor`）仍必须收敛。
///
/// 旧 server 一律按 `offset = 0` 从头回送、不填 `next_cursor`，因此新 dial 方
/// 每轮都只能从头再取。它依然收敛，靠的是**摘要随持有量增长而使差量缩短**：
///
/// ```text
/// 20 条差量、页大小 8：
/// 第 1 轮：差量 20，取前 8 条 → 持有 8
/// 第 2 轮：差量 12，取前 8 条 → 持有 16
/// 第 3 轮：差量 4，取 4 条 → more = false
/// ```
///
/// 这正是 ADR-010 用下标游标时**不成立**的前提：那里假设「同一轮内差量恒定」，
/// 而真实 dial 方每轮都更新摘要。
///
/// 代价是整批差量被重发多轮（带宽 O(n^2 / page_size)）。这比错漏可接受。
#[tokio::test]
async fn mixed_version_new_dialer_old_server_still_converges() {
    let peer = setup_bank("peer").await;
    seed(&peer, 20).await;

    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut delivered: Vec<String> = Vec::new();
    let mut pages = 0usize;

    loop {
        pages += 1;
        assert!(pages <= 10, "场景 A 死循环：10 轮后仍未收敛");

        // 每轮带上**已更新**的摘要——这是真实 dial 方的行为。
        let keys: Vec<&str> = held.iter().map(|s| s.as_str()).collect();
        let digest = digest_from_keys(keys);

        // 旧 server 不解释 `cursor`：这里恒传空串，等价于它按 offset=0 取数。
        let page = anti_entropy::serve_sync_request(&peer, &digest, "", 8).await.expect("serve");

        for e in &page.experiences {
            delivered.push(e.exp_key.clone());
            held.insert(e.exp_key.clone());
        }
        if !page.more {
            break;
        }
    }

    let unique: HashSet<&String> = delivered.iter().collect();
    assert_eq!(
        unique.len(),
        20,
        "新 dial 方必须能向旧 server 收敛到全部经验"
    );
    assert_eq!(pages, 3, "20 条 / 页大小 8、每轮从头：应为 20→12→4 三轮");
}

/// 混合版本：旧 dial 方 ↔ 新 server（服务端不再解释 `offset`）仍必须收敛。
///
/// 与场景 A 走的是**同一段代码**：被测的都是新 `serve_sync_request`，而非真正
/// 的旧实现。因此它证明的是「服务端忽略 `offset`、恒置 `next_offset = 0` 时，
/// dial 方仅靠摘要缩短也能收敛」，而不是「旧 dial 方与新 server 互通」。
///
/// 真正的跨版本互通无法在进程内构造：老节点的 proto 解析器已被替换掉了。
/// 保留本用例是因为它锁住了「服务端不再填 `next_offset`」这个决定所依赖的
/// 收敛前提——若将来摘要改为不随持有量缩短，本用例会先失败。
#[tokio::test]
async fn mixed_version_old_dialer_new_server_still_converges() {
    let peer = setup_bank("peer").await;
    seed(&peer, 20).await;

    let mut held: BTreeSet<String> = BTreeSet::new();
    let mut delivered: Vec<String> = Vec::new();
    let mut pages = 0usize;

    loop {
        pages += 1;
        assert!(pages <= 10, "场景 B 死循环：10 轮后仍未收敛");

        let keys: Vec<&str> = held.iter().map(|s| s.as_str()).collect();
        let digest = digest_from_keys(keys);

        let page = anti_entropy::serve_sync_request(&peer, &digest, "", 8).await.expect("serve");

        for e in &page.experiences {
            delivered.push(e.exp_key.clone());
            held.insert(e.exp_key.clone());
        }
        if !page.more {
            break;
        }
    }

    let unique: HashSet<&String> = delivered.iter().collect();
    assert_eq!(
        unique.len(),
        20,
        "旧 dial 方必须能向新 server 收敛到全部经验"
    );
}

/// 差量恰好是单页整数倍时，最后一页的 `more` 必须为 false。
///
/// 否则 dial 方会多发一次注定返回空页的请求；若对端就此报错，
/// 反熵会变成一个每次重连都失败的流程。
#[tokio::test]
async fn exact_multiple_of_page_size_ends_paging() {
    let peer = setup_bank("peer").await;
    seed(&peer, 16).await; // 16 = 2 页 × 8

    let first = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), "", 8)
        .await
        .expect("first page");
    assert_eq!(first.experiences.len(), 8);
    assert!(first.more, "第 8 条之后还有第 9~16 条");
    assert_eq!(
        first.next_cursor,
        first.experiences.last().expect("非空页").exp_key,
        "游标应为本页最后一个主键"
    );

    let second =
        anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), &first.next_cursor, 8)
            .await
            .expect("second page");
    assert_eq!(second.experiences.len(), 8);
    assert!(
        !second.more,
        "整数倍页大小下最后一页必须结束翻页，否则 dial 方会多发一次空请求"
    );
    assert_eq!(
        second.next_cursor,
        second.experiences.last().expect("非空页").exp_key
    );

    // 第一页与第二页不得重叠。
    let first_keys: HashSet<&String> = first.experiences.iter().map(|e| &e.exp_key).collect();
    let second_keys: HashSet<&String> = second.experiences.iter().map(|e| &e.exp_key).collect();
    assert!(first_keys.is_disjoint(&second_keys), "相邻两页不得有交集");
}

/// 断线重连：只补齐断连期间新增的差量。
#[tokio::test]
async fn reconnect_delivers_only_the_gap() {
    let peer = setup_bank("peer").await;
    seed(&peer, 5).await;

    // dial 方先同步一次。
    let first_pass = drain_sync(&peer, &ExperienceDigest::new()).await;
    assert_eq!(first_pass.len(), 5);

    // dial 方落库后导出摘要，模拟它当前的状态。
    let dialer_digest = digest_from_keys(first_pass.iter().map(|s| s.as_str()));
    assert_eq!(dialer_digest.len(), 1, "单节点写入应折叠成一个 origin");

    // 断连期间 peer 又写了 3 条。
    let new_keys = seed(&peer, 3).await;

    // 重连后只应补这 3 条。
    let second_pass = drain_sync(&peer, &dialer_digest).await;
    assert_eq!(second_pass.len(), 3, "重连只应补差量，不应重发已持有的历史");
    for key in &new_keys {
        assert!(
            second_pass.contains(key),
            "断连期间新增的 {} 必须被补齐",
            key
        );
    }
}

/// 幂等：已完全同步的节点再拉一次，差量为空。
#[tokio::test]
async fn fully_synced_peer_yields_empty_delta() {
    let peer = setup_bank("peer").await;
    seed(&peer, 4).await;

    let received = drain_sync(&peer, &ExperienceDigest::new()).await;
    let dialer_digest = digest_from_keys(received.iter().map(|s| s.as_str()));

    let again = drain_sync(&peer, &dialer_digest).await;
    assert!(
        again.is_empty(),
        "已同步节点不应再收到任何经验，实际收到 {} 条",
        again.len()
    );
}

/// 回送的载荷必须能被 dial 方解码并按 CRDT 落库。
///
/// 反熵不引入第二套合并语义：它回送的必须是 `ExperienceSync`，让接收端走与
/// 实时广播完全相同的 `apply_remote` → `merge_crdt` 路径。这里直接验证载荷
/// 可解码且 version / vector_clock 完整。
#[tokio::test]
async fn delivered_payload_is_decodable_and_merged_idempotently() {
    let peer = setup_bank("peer").await;
    seed(&peer, 3).await;

    let dialer = setup_bank("dialer").await;
    let page = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), "", 128)
        .await
        .expect("serve");
    assert_eq!(page.experiences.len(), 3);

    for sync in &page.experiences {
        let exp: ExperienceTuple = serde_json::from_slice(&sync.payload).expect("反熵载荷应可解码");
        assert_eq!(exp.version, sync.version, "版本号必须随载荷一同回送");
        let clock: std::collections::BTreeMap<String, u64> =
            serde_json::from_str(&sync.vector_clock_json).expect("时钟应可解析");
        assert!(
            !clock.is_empty(),
            "向量时钟必须随载荷一同回送，否则接收端无法做 CRDT 裁决"
        );

        // 连续应用两次同一行：CRDT 裁决应当幂等，不产生重复或回退。
        dialer.apply_remote(&exp, &sync.exp_key).await.expect("apply remote");
        let second = dialer.apply_remote(&exp, &sync.exp_key).await.expect("apply remote (second)");
        assert!(
            !matches!(second, sisyphus_mem::store::ApplyResult::Applied),
            "重复应用同一行不应再次写入（CRDT 应当判为 Equal）"
        );
    }

    let dialer_keys = dialer.keys().await.expect("keys");
    assert_eq!(dialer_keys.len(), 3, "重复应用不应产生重复行");

    // 再从 dial 方导出摘要并反推：它应与 peer 对这些行的认知一致，
    // 因此 peer 认为 dial 方「不需要」任何差量。
    let dialer_digest = dialer.digest().await.expect("digest");
    let peer_view = anti_entropy::serve_sync_request(&peer, &dialer_digest, "", 128)
        .await
        .expect("serve after merge");
    assert!(
        peer_view.experiences.is_empty(),
        "dial 方合并后不应再从 peer 拉取任何经验"
    );
}

/// 多 origin：dial 方只见过另一个节点时，只补本节点自己的差量。
///
/// 注意 origin 来自 `local_node_id()`（进程级）而非 `config.cluster.node_id`，
/// 因此这里不硬编码 origin 字符串，而是从实际写入的主键解析出来再断言。
#[tokio::test]
async fn delta_is_scoped_to_unseen_origins() {
    let peer = setup_bank("peer").await;
    let seeded = seed(&peer, 3).await;

    let peer_origin = seeded[0]
        .strip_prefix("exp:")
        .and_then(|rest| {
            let mut parts: Vec<&str> = rest.rsplitn(3, ':').collect();
            parts.reverse();
            parts.get(1).copied()
        })
        .expect("主键应含 origin")
        .to_string();

    // 构造一个「见过 node-other 但没见过 peer」的摘要。
    let dialer_digest = digest_from_keys(["exp:1:node-other:9"]);

    let page =
        anti_entropy::serve_sync_request(&peer, &dialer_digest, "", 128).await.expect("serve");
    assert_eq!(page.experiences.len(), 3, "peer 自己的经验应全部补发");
    for sync in &page.experiences {
        assert!(
            sync.exp_key.contains(&format!(":{peer_origin}:")),
            "不应回发其他 origin 的经验：{}",
            sync.exp_key
        );
    }

    // 反向确认：dial 方已见过 peer 的全部经验后，差量应为空。
    let seen_all = digest_from_keys(seeded.iter().map(|s| s.as_str()));
    let after =
        anti_entropy::serve_sync_request(&peer, &seen_all, "", 128).await.expect("serve after");
    assert!(
        after.experiences.is_empty(),
        "dial 方已持有 peer 全部经验时不应再收到数据"
    );
}

/// ADR-012 根因用例：翻页期间差量集合会**逐页缩短**，游标仍必须不漏发。
///
/// ADR-010 用 `offset` 下标定位下一段，而差量每翻一页就缩短一次——dial 方
/// 收到即持有，那些行随即从差量中消失，差量整体前移。于是按旧下标切片会
/// 跳过一批仍缺失的行：差量跨 3 页以上时，尾部静默丢失且无人再提。
///
/// 本用例真实重现那个缩短过程：每收到一页就把它们真正写入 dial 方，
/// 下一轮的摘要因而反映最新持有状态。这是与 `drain_sync` 的关键区别。
#[tokio::test]
async fn paging_survives_delta_shrinking_between_requests() {
    let peer = setup_bank("peer").await;
    let seeded = seed(&peer, 20).await; // 20 条，页大小 8 → 3 页

    let dialer = setup_bank("dialer").await;
    let mut cursor = String::new();
    let mut collected: Vec<String> = Vec::new();

    loop {
        // 每轮都用 dial 方**当前**的摘要，忠实复刻真实拨号器行为。
        let digest = dialer.digest().await.expect("dialer digest");
        let page =
            anti_entropy::serve_sync_request(&peer, &digest, &cursor, 8).await.expect("serve");

        assert!(
            !page.experiences.is_empty() || !page.more,
            "尚有差量却回送空页且 more=false，游标错位导致剩余行被永久跳过"
        );

        for sync in &page.experiences {
            let exp: ExperienceTuple = serde_json::from_slice(&sync.payload).expect("可解码");
            dialer.apply_remote(&exp, &sync.exp_key).await.expect("apply");
            collected.push(sync.exp_key.clone());
        }

        cursor = page.next_cursor;
        if !page.more {
            break;
        }
    }

    let expected: HashSet<&String> = seeded.iter().collect();
    let actual: HashSet<&String> = collected.iter().collect();
    assert_eq!(
        actual,
        expected,
        "差量逐页缩短时按游标翻页必须补齐全量且不重复。\n\
         仍缺：{:?}\n\
         多余：{:?}",
        expected.difference(&actual).take(10).collect::<Vec<_>>(),
        actual.difference(&expected).take(10).collect::<Vec<_>>()
    );
    assert_eq!(collected.len(), 20, "不应重复补齐");
}

/// 反熵为空库对端时不 panic，且以空页 + more=false 结束。
#[tokio::test]
async fn empty_peer_bank_serves_empty_page() {
    let peer = setup_bank("peer").await;
    let page = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), "", 128)
        .await
        .expect("empty peer must still answer");
    assert!(page.experiences.is_empty());
    assert!(!page.more);
    assert_eq!(
        page.next_cursor, "",
        "空页不应移动游标，否则 dial 方会跳过尚未送达的行"
    );
}

/// keyset 扫描必须跳过游标之前的大量「已持有」键，并补齐尾部差量。
///
/// 这是 `serve_sync_request` 从「每页全表取键」改为「主键区间扫描」后的
/// 直接覆盖：600 行里前 550 行都在 dial 方摘要内，扫描要跨过一个完整的
/// 512 键批次才能找到尾部 50 条缺失。若扫描在第一批后停止、或把已持有的
/// 键算进页容量，本用例会漏发。
#[tokio::test]
async fn keyset_scan_skips_held_prefix_and_delivers_tail() {
    let peer = setup_bank("peer").await;
    let seeded = seed(&peer, 600).await;

    // dial 方持有前 550 条：差量只剩尾部 50 条。
    let held = digest_from_keys(seeded[..550].iter().map(|s| s.as_str()));
    assert_eq!(held.len(), 1, "单节点写入应折叠成一个 origin");

    let received = drain_sync(&peer, &held).await;

    assert_eq!(
        received.len(),
        50,
        "应恰好补齐尾部 50 条，实际 {} 条——keyset 扫描可能提前停止或误算了页容量",
        received.len()
    );
    let expected: HashSet<&String> = seeded[550..].iter().collect();
    let got: HashSet<&String> = received.iter().collect();
    assert_eq!(got, expected, "补齐的必须是尾部缺失行，不能重复也不能漏");
}

/// `page_size = 0` 必须收敛，而不是让拨号器陷入无进展的死循环。
///
/// 拨号器完全按服务端的 `more` 标志决定是否再请求一页，并把游标置为
/// `next_cursor`。若零页大小被当成正常分页处理，服务端会给出「还有下一页 +
/// 游标不变」的组合，拨号器于是无限重发同一页——现象是同步卡住且没有任何
/// 错误。零大小是配置错误，正确表现是空页收尾。
#[tokio::test]
async fn zero_page_size_terminates_without_looping() {
    let peer = setup_bank("peer").await;
    seed(&peer, 3).await;

    let page = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), "", 0)
        .await
        .expect("serve sync request");

    assert!(page.experiences.is_empty(), "零页大小不应回送任何经验");
    assert!(
        !page.more,
        "零页大小必须表示「没有下一页」，否则拨号器会死循环"
    );
    assert_eq!(page.next_cursor, "", "游标不应凭空前进或重置到未确认的位置");
}
