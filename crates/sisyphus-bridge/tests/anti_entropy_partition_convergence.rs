//! ADR-010 反熵：分区 → 恢复后双向收敛的端到端集成测试。
//!
//! 与 `anti_entropy_integration.rs`（单进程内 bank 共用一个 origin 的场景）
//! 互补：本文件用独立进程内的两个 bank 落到**两个不同 origin**（node-a / node-b），
//! 构造「分区前双方都有独立写入」的真实状态，验证恢复后双向 anti-entropy
//! 交换是否能收敛到并集。

use sisyphus_bridge::peer::anti_entropy;
use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
use sisyphus_tools_llm::DummyEmbeddingClient;
use sisyphus_types::env_guard::EnvVarGuard;
use sisyphus_types::experience::{digest_from_keys, ExperienceBank, ExperienceTuple};
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

/// 分区 → 恢复后双向收敛：双方分区前都有独立写入，恢复后双向同步必须收敛到并集。
///
/// 场景：A（node-a）与 B（node-b）在网络分区前各自写了自己的经验（不同 origin）。
/// 分区期间互不知晓；恢复后 A 向 B 同步、B 向 A 同步，双方都应持有「全部」。
/// 收敛判定：双方导出的版本向量摘要都覆盖两个 origin（即持有并集）。
#[tokio::test]
async fn partition_heal_converges_both_sides_via_bidirectional_sync() {
    // B 先建库并写入 3 条（origin=node-b）。
    let _g_b = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "node-b") };
    let b = setup_bank("node-b").await;
    let b_keys = seed(&b, 3).await;
    let _g_a = unsafe { EnvVarGuard::set("SISYPHUS_CLUSTER_NODE_ID", "node-a") };
    let a = setup_bank("node-a").await;
    let a_keys = seed(&a, 2).await;

    let a_only = digest_from_keys(a_keys.iter().map(|s| s.as_str()));
    let b_only = digest_from_keys(b_keys.iter().map(|s| s.as_str()));
    assert_eq!(a_only.len(), 1, "单节点写入应折叠成一个 origin");
    assert_eq!(b_only.len(), 1, "单节点写入应折叠成一个 origin");

    // 恢复：A 向 B 补 B 的写入，B 向 A 补 A 的写入（双向）。
    let page_a_gets_b = anti_entropy::serve_sync_request(&b, &a_only, "", 128)
        .await
        .expect("A 向 B 发起同步应成功");
    assert_eq!(
        page_a_gets_b.experiences.len(),
        3,
        "A 恢复后应收到 B 的全部写入"
    );
    for sync in &page_a_gets_b.experiences {
        let exp: ExperienceTuple = serde_json::from_slice(&sync.payload).expect("载荷可解码");
        a.apply_remote(&exp, &sync.exp_key).await.expect("A 应用 B 的经验");
    }

    let page_b_gets_a = anti_entropy::serve_sync_request(&a, &b_only, "", 128)
        .await
        .expect("B 向 A 发起同步应成功");
    assert_eq!(
        page_b_gets_a.experiences.len(),
        2,
        "B 恢复后应收到 A 的全部写入"
    );
    for sync in &page_b_gets_a.experiences {
        let exp: ExperienceTuple = serde_json::from_slice(&sync.payload).expect("载荷可解码");
        b.apply_remote(&exp, &sync.exp_key).await.expect("B 应用 A 的经验");
    }

    // 双向收敛后，双方导出的版本向量摘要都应覆盖两个 origin（持有并集）。
    let a_dig = a.export_version_vector().await.expect("A 导出版本向量");
    let b_dig = b.export_version_vector().await.expect("B 导出版本向量");
    assert_eq!(
        a_dig.len(),
        2,
        "A 收敛后应同时持有 node-a 与 node-b 两个 origin 的经验"
    );
    assert_eq!(
        b_dig.len(),
        2,
        "B 收敛后应同时持有 node-a 与 node-b 两个 origin 的经验"
    );
}
