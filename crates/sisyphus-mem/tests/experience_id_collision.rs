//! 经验行主键碰撞的回归测试（TODO P1）。
//!
//! 主键曾只用 `exp:{timestamp}`，而 timestamp 是**秒级**的。同一秒内的两次
//! 写入会撞主键：实测第二条 INSERT 直接返回错误，而 `execute_and_record` 对
//! 该错误只 `warn!` 后继续——**数据静默丢失**。
//!
//! 这组用例同时锁定两个前提：
//! 1. 主键纳入来源节点与本地写入计数后，同秒写入不再冲突；
//! 2. 本地计数确实单调——此前 `bump_local_clock` 只从传入的空时钟自增，
//!    导致每条本地经验的 version 恒为 1、vc 计数恒为 1，CRDT 裁决随之失效。

use std::sync::Arc;

use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::store::MfsStore;
use sisyphus_mem::vector_memory::VectorMemoryStore;
use sisyphus_types::experience::{ExperienceBank, ExperienceQuery, ExperienceTuple};

/// 两个不同内容、**完全相同秒级时间戳**的经验。
fn experience(ts: u64, text: &str) -> ExperienceTuple {
    ExperienceTuple {
        context_text: text.into(),
        context_hash: format!("hash-{text}"),
        action_id: "sync-action".into(),
        project_id: None,
        outcome_success: true,
        deviation_score: 0.0,
        cost: 0,
        timestamp: ts,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: 0,
        vector_clock: Default::default(),
    }
}

async fn bank() -> VectorExperienceBank {
    let cache = sisyphus_types::CacheConfig::default();
    let store = Arc::new(MfsStore::new("sqlite::memory:", &cache).await.expect("建内存状态库"));
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("建内存向量库");
    let vectors = Arc::new(VectorMemoryStore::new(pool).await.expect("初始化向量库"));
    VectorExperienceBank::new(
        store,
        vectors,
        Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
    )
    .await
    .expect("初始化经验库")
}

async fn find(bank: &VectorExperienceBank, hash: &str) -> Option<ExperienceTuple> {
    bank.query_similar(ExperienceQuery {
        query_text: hash.into(),
        context_hash: hash.into(),
        action_id: None,
        project_id: None,
        visual_embedding: None,
        limit: 16,
    })
    .await
    .expect("检索应成功")
    .into_iter()
    .find(|e| e.context_hash == hash)
}

#[tokio::test]
async fn same_second_writes_must_not_collide() {
    let bank = bank().await;
    // 固定同一秒，模拟两次快速连续的目标执行 / 反思写入
    let ts = 1_700_000_000u64;

    bank.record(experience(ts, "first")).await.expect("第一条应写入成功");
    bank.record(experience(ts, "second")).await.expect("第二条不应因主键冲突而失败");

    assert!(find(&bank, "hash-first").await.is_some(), "第一条经验丢失");
    assert!(
        find(&bank, "hash-second").await.is_some(),
        "第二条经验丢失——同秒写入撞了主键"
    );
}

#[tokio::test]
async fn local_clock_is_monotonic() {
    let bank = bank().await;
    let ts = 1_700_000_001u64;

    let first = experience(ts, "a");
    bank.record(first.clone()).await.expect("写入成功");
    let second = experience(ts, "b");
    bank.record(second.clone()).await.expect("写入成功");

    // 每条写入取回自己的主键，确认计数严格递增
    let k1 = bank.record_with_id(first).await.expect("写入成功");
    let k2 = bank.record_with_id(second).await.expect("写入成功");
    let c1: u64 = k1.rsplit(':').next().and_then(|s| s.parse().ok()).expect("计数段");
    let c2: u64 = k2.rsplit(':').next().and_then(|s| s.parse().ok()).expect("计数段");
    assert!(c2 > c1, "本地写入计数必须单调递增，实际 {c1} → {c2}");
}
