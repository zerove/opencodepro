//! P0 ④ 跨租户经验泄漏回归测试：检索必须限定在「本人租户 + 共享经验」。
//!
//! 缺陷：`BankRetriever::retrieve_relevant` 硬编码 `project_id: None`
//! （= 无过滤），proj-b 的检索会命中 proj-a 的私有经验并在反思/合成
//! 阶段注入提示词（跨租户泄漏）；修复后租户作用域由调用方透传。
//!
//! 运行：
//! ```bash
//! cargo test -p sisyphus-mem --test repro_p0_experience_isolation -- --nocapture
//! ```

use std::sync::Arc;

use sisyphus_mem::experience_bank::VectorExperienceBank;
use sisyphus_mem::retrieval::BankRetriever;
use sisyphus_mem::store::MfsStore;
use sisyphus_mem::vector_memory::VectorMemoryStore;
use sisyphus_types::ExperienceRetriever;
use sisyphus_types::experience::{ExperienceBank, ExperienceTuple};

fn experience(text: &str, project: Option<&str>, ts: u64) -> ExperienceTuple {
    ExperienceTuple {
        context_text: text.into(),
        context_hash: format!("hash-{text}"),
        action_id: "deploy".into(),
        project_id: project.map(|s| s.to_string()),
        outcome_success: true,
        deviation_score: 0.1,
        cost: 0,
        timestamp: ts,
        embedding: None,
        visual_embedding: None,
        insight: None,
        version: 0,
        vector_clock: Default::default(),
    }
}

async fn bank() -> Arc<VectorExperienceBank> {
    let cache = sisyphus_types::CacheConfig::default();
    let store = Arc::new(MfsStore::new("sqlite::memory:", &cache).await.expect("建内存状态库"));
    let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.expect("建内存向量库");
    let vectors = Arc::new(VectorMemoryStore::new(pool).await.expect("初始化向量库"));
    Arc::new(
        VectorExperienceBank::new(
            store,
            vectors,
            Arc::new(sisyphus_tools_llm::DummyEmbeddingClient),
        )
        .await
        .expect("初始化经验库"),
    )
}

#[tokio::test]
async fn bank_retriever_scopes_to_tenant_and_keeps_shared() {
    let bank = bank().await;
    let retriever = BankRetriever::new(bank.clone());
    let goal = "deploy the staging service";

    bank.record(experience(goal, Some("proj-a"), 1000)).await.expect("record proj-a");
    bank.record(experience(goal, Some("proj-b"), 2000)).await.expect("record proj-b");
    bank.record(experience(goal, None, 3000)).await.expect("record shared");

    let b_view =
        retriever.retrieve_relevant(goal, 10, Some("proj-b")).await.expect("retrieve proj-b");
    let b_projects: Vec<_> = b_view.iter().map(|e| e.project_id.clone()).collect();
    assert!(
        !b_projects.contains(&Some("proj-a".to_string())),
        "跨租户泄漏：proj-b 检索到了 proj-a 的私有经验 {b_projects:?}"
    );
    assert!(
        b_projects.contains(&Some("proj-b".to_string())),
        "本人经验应可见 {b_projects:?}"
    );
    assert!(
        b_projects.contains(&None),
        "共享经验应对所有租户可见 {b_projects:?}"
    );

    let a_view =
        retriever.retrieve_relevant(goal, 10, Some("proj-a")).await.expect("retrieve proj-a");
    let a_projects: Vec<_> = a_view.iter().map(|e| e.project_id.clone()).collect();
    assert!(
        !a_projects.contains(&Some("proj-b".to_string())),
        "跨租户泄漏：proj-a 检索到了 proj-b 的私有经验 {a_projects:?}"
    );
    assert!(
        a_projects.contains(&Some("proj-a".to_string())),
        "本人经验应可见 {a_projects:?}"
    );

    let all = retriever.retrieve_relevant(goal, 10, None).await.expect("retrieve unscoped");
    assert_eq!(all.len(), 3, "无租户查询（None）应不过滤：{all:?}");
}
