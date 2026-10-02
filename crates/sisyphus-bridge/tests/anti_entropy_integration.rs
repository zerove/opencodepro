//! ADR-010 反熵补偿的端到端集成测试。
//!
//! 与 `anti_entropy.rs` 里的纯函数单元测试互补：这里走**真实数据库 +
//! 真实经验库**，验证的是纯函数覆盖不到的部分——
//!
//! - 差量真的能从 SQLite 里按主键取回载荷（列名取值路径）；
//! - 游标 `next_offset` 真的能推进到下一页而不重复、不漏发；
//! - dial 方按 `more` 标志翻页，最终恰好补齐全量。
//!
//! 这一层是 ADR-010 的核心断言所在：翻页闭环一旦断掉，差量超过一页的节点
//! 就会永远只拿到第一页，而这种缺陷在纯函数测试里看不出来。

use sisyphus_bridge::peer::anti_entropy;
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
use sisyphus_tools_llm::DummyEmbeddingClient;
use sisyphus_types::experience::{digest_from_keys, ExperienceDigest, ExperienceTuple};
use std::collections::HashSet;
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

/// 按 ADR-010 的 wire 协议完整走一遍翻页循环，返回 dial 方收到的全部主键。
///
/// 这段刻意复刻 `fallback.rs::run_stream` 的翻页逻辑：dial 方发 offset=0，
/// peer 分页回送并附 `more`，dial 方据此请求下一页。
async fn drain_sync(peer: &Arc<VectorExperienceBank>, dialer: &ExperienceDigest) -> Vec<String> {
    let mut collected = Vec::new();
    let mut offset = 0u64;

    loop {
        let page = anti_entropy::serve_sync_request(peer, dialer, offset as usize, 8)
            .await
            .expect("serve sync request");

        collected.extend(page.experiences.iter().map(|e| e.exp_key.clone()));
        offset = page.next_offset;

        if !page.more {
            break;
        }
        assert!(offset < 10_000, "翻页不应无限循环");
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

/// 差量恰好是单页整数倍时，最后一页的 `more` 必须为 false。
///
/// 否则 dial 方会多发一次注定返回空页的请求；若对端就此报错，
/// 反熵会变成一个每次重连都失败的流程。
#[tokio::test]
async fn exact_multiple_of_page_size_ends_paging() {
    let peer = setup_bank("peer").await;
    seed(&peer, 16).await; // 16 = 2 页 × 8

    let first = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), 0, 8)
        .await
        .expect("first page");
    assert_eq!(first.experiences.len(), 8);
    assert!(first.more, "第 8 条之后还有第 9~16 条");
    assert_eq!(first.next_offset, 8);

    let second = anti_entropy::serve_sync_request(
        &peer,
        &ExperienceDigest::new(),
        first.next_offset as usize,
        8,
    )
    .await
    .expect("second page");
    assert_eq!(second.experiences.len(), 8);
    assert!(
        !second.more,
        "整数倍页大小下最后一页必须结束翻页，否则 dial 方会多发一次空请求"
    );
    assert_eq!(second.next_offset, 16);

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
    let page = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), 0, 128)
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
    let peer_view = anti_entropy::serve_sync_request(&peer, &dialer_digest, 0, 128)
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
        anti_entropy::serve_sync_request(&peer, &dialer_digest, 0, 128).await.expect("serve");
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
        anti_entropy::serve_sync_request(&peer, &seen_all, 0, 128).await.expect("serve after");
    assert!(
        after.experiences.is_empty(),
        "dial 方已持有 peer 全部经验时不应再收到数据"
    );
}

/// 反熵为空库对端时不 panic，且以空页 + more=false 结束。
#[tokio::test]
async fn empty_peer_bank_serves_empty_page() {
    let peer = setup_bank("peer").await;
    let page = anti_entropy::serve_sync_request(&peer, &ExperienceDigest::new(), 0, 128)
        .await
        .expect("empty peer must still answer");
    assert!(page.experiences.is_empty());
    assert!(!page.more);
    assert_eq!(page.next_offset, 0);
}
