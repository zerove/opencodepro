//! 版本向量摘要缓存（性能优化）的回归测试。
//!
//! 缓存不改变摘要的**内容**，但改变它「何时过期」。反熵靠摘要回答「我有什么」，
//! 过期摘要会让本节点向对端少报自己持有的经验——表现形式是反复做无用补齐，
//! 而非报错。因此缓存失效时机必须有直接断言，不能只靠「端到端看起来通了」。

use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::store::ApplyResult;
use sisyphus_mem::{SisyphusStore, VectorMemoryStore};
use sisyphus_tools_llm::DummyEmbeddingClient;
use sisyphus_types::experience::{ExperienceBank, ExperienceTuple};
use std::sync::Arc;

struct Harness {
    bank: Arc<VectorExperienceBank>,
    store: SisyphusStore,
}

/// 在内存数据库上建一个真实经验库，并保留 store 以便测试制造写入失败。
async fn setup() -> Harness {
    let mut config = sisyphus_types::Config::default();
    config.cluster.node_id = "digest-cache-test".to_string();
    let store = SisyphusStore::new("sqlite::memory:", &config).await.expect("store init");
    let state_store = store.state.clone();
    let vector_store =
        Arc::new(VectorMemoryStore::new(store.pool()).await.expect("vector store init"));
    let bank = Arc::new(
        VectorExperienceBank::new(state_store, vector_store, Arc::new(DummyEmbeddingClient))
            .await
            .expect("exp bank init"),
    );
    Harness { bank, store }
}

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

/// 本地写入后，已缓存的摘要必须反映新行。
///
/// 先把空摘要读进缓存，再写入——若失效逻辑缺失，第二次导出仍是空摘要，
/// 对端会认为本节点一条经验都没有。
#[tokio::test]
async fn local_write_invalidates_cached_digest() {
    let Harness { bank, .. } = setup().await;

    let before = bank.digest().await.expect("首次导出");
    assert!(before.is_empty(), "空库摘要应为空");
    // 再读一次确保走的是缓存路径（值相同，这里只固定语义）。
    assert!(bank.digest().await.expect("缓存命中").is_empty());

    bank.record_with_id(make_exp(1)).await.expect("record");

    let after = bank.export_version_vector().await.expect("写入后导出");
    assert!(
        !after.is_empty(),
        "落库后摘要仍为空 —— 缓存没有失效，反熵会少报这一行"
    );
    let max = after.values().map(|d| d.max).sum::<u64>();
    assert_eq!(max, 1, "摘要应恰好覆盖 1 条本地写入");
}

/// 远程应用后，已缓存的摘要必须包含远端 origin。
#[tokio::test]
async fn remote_apply_invalidates_cached_digest() {
    let Harness { bank, .. } = setup().await;
    assert!(bank.digest().await.expect("预热缓存").is_empty());

    let outcome =
        bank.apply_remote(&make_exp(7), "exp:1700000007:node-z:1").await.expect("apply remote");
    assert_eq!(outcome, ApplyResult::Applied, "新主键应直接应用");

    let digest = bank.export_version_vector().await.expect("应用后导出");
    assert_eq!(
        digest.get("node-z").map(|d| d.max),
        Some(1),
        "远端 origin 必须出现在摘要里，否则本节点会反复向对端索取同一行"
    );
}

/// 落库成功但向量索引写入失败时，摘要缓存仍必须失效。
///
/// 这是失效时机最容易被写错的分支：把失效放在写入函数末尾，失败路径会
/// 提前返回，留下「库里已有新行、缓存仍是旧摘要」的不一致状态。删掉
/// `vectors` 表即可稳定制造这一失败，而经验表的写入不受影响。
#[tokio::test]
async fn digest_invalidated_even_when_vector_index_write_fails() {
    let Harness { bank, store } = setup().await;
    assert!(bank.digest().await.expect("预热缓存").is_empty());

    sqlx::query("DROP TABLE vectors").execute(&store.pool()).await.expect("删表以制造向量写入失败");

    let result = bank.record_with_id(make_exp(1)).await;
    assert!(result.is_err(), "向量索引写入应失败，本用例才有意义");

    let digest = bank.digest().await.expect("失败后导出");
    assert!(
        !digest.is_empty(),
        "经验行已落库但摘要仍是旧值 —— 失效被写在函数末尾，失败路径跳过了它"
    );
}
