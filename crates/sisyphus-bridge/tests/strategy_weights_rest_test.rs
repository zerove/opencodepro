//! REST `/api/strategy/weights` 数据链路测试（v0.4.0 Phase B，ADR-017）。
//!
//! 背景：该端点的数据来自 `MySisyphusBridge.strategy_weights` 共享视图，而视图
//! 由 `StrategyWeightsHandler` 在运行期写入。集群未启用时视图恒为空——如果只测
//! 单机启动路径，就只会永远验证「空视图也返回 200」，与「端点真的把视图序列化
//! 出来了」是两件事。
//!
//! 因此这里直接往共享视图里塞两份节点快照（与 handler 的 `publish_once` /
//! `apply_remote` 写入的是同一个 `FederatedStrategyWeights::merge`），再通过真实
//! axum Router 取值，断言「聚合 → JSON 契约」这一段。
//!
//! 这一层与 `sisyphus_types::strategy_weights` 的单测互补：那边锁合并/聚合语义，
//! 这边锁端点接线（处理器是否读了共享视图、字段名是否与文档一致）。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use sisyphus_bridge::bridge_service::MySisyphusBridge;
use sisyphus_bridge::http::{build_router, RestApiState};
use sisyphus_types::strategy_weights::{NodeStrategySnapshot, StrategyStat};
use sisyphus_types::Config;
use std::sync::{Arc, RwLock};
use tower::ServiceExt;

async fn setup() -> (Router, Arc<MySisyphusBridge>) {
    let mut config = Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.sandbox.wasm_tools_dir = "tests/wasm_tools".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    let shared_config = Arc::new(RwLock::new(config));

    let bridge =
        Arc::new(MySisyphusBridge::new(shared_config, None, None).await.expect("bridge 构造失败"));

    let app = build_router(RestApiState::new(bridge.clone(), std::env::temp_dir()));
    (app, bridge)
}

async fn get(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).expect("request build failed"))
        .await
        .expect("request failed");
    let status = response.status();
    let bytes =
        axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("body read failed");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

fn stat(uses: u64, successes: u64, avg_duration_secs: f64) -> StrategyStat {
    StrategyStat {
        uses,
        successes,
        avg_duration_secs,
    }
}

/// 集群未启用（无节点上报）：必须返回 200 + 空视图，而不是 404 / 500。
///
/// 「空」与「不存在」是两件事：WebUI 与外部集成方据此区分「集群里还没有数据」
/// 与「这个版本没有该端点」。返回 404 会让前端把两者混为一谈。
#[tokio::test]
async fn strategy_weights_endpoint_reports_empty_view_not_missing() {
    let (app, _bridge) = setup().await;

    let (status, json) = get(&app, "/api/strategy/weights").await;
    assert_eq!(status, StatusCode::OK, "空视图也必须返回 200: {json:?}");
    assert_eq!(json["node_count"], 0);
    assert_eq!(json["nodes"], serde_json::json!([]));
    assert_eq!(json["aggregate"], serde_json::json!({}));
    assert!(
        json["best"].is_null(),
        "没有数据时 best 必须为 null: {json:?}"
    );
}

/// 两份节点快照合并后，端点必须返回跨节点聚合结果。
///
/// 断言同时覆盖：`node_count` / `nodes[].node_id` / `aggregate.uses` 求和 /
/// `success_rate` 派生 / `best` 选出高成功率策略。字段名即 API 契约，
/// 与 `book/src/rest-api.md` 及 OpenAPI 描述保持一致。
#[tokio::test]
async fn strategy_weights_endpoint_serves_merged_view() {
    let (app, bridge) = setup().await;

    {
        let mut view = bridge.strategy_weights().write();
        // A：depth-first 2 次全成功；B：depth-first 1 次失败 + hybrid 4 次成功。
        assert!(view.merge(NodeStrategySnapshot {
            node_id: "node-a".into(),
            generation: 10,
            strategies: [("depth-first".to_string(), stat(2, 2, 1.5))].into_iter().collect(),
        }));
        assert!(view.merge(NodeStrategySnapshot {
            node_id: "node-b".into(),
            generation: 20,
            strategies: [
                ("depth-first".to_string(), stat(1, 0, 3.0)),
                ("hybrid".to_string(), stat(4, 4, 0.5)),
            ]
            .into_iter()
            .collect(),
        }));
    }

    let (status, json) = get(&app, "/api/strategy/weights").await;
    assert_eq!(status, StatusCode::OK, "{json:?}");
    assert_eq!(json["node_count"], 2);
    // nodes 按 node_id 字典序（BTreeMap），断言 id 而不依赖插入顺序。
    let node_ids: Vec<&str> = json["nodes"]
        .as_array()
        .expect("nodes 数组")
        .iter()
        .filter_map(|n| n["node_id"].as_str())
        .collect();
    assert_eq!(node_ids, vec!["node-a", "node-b"]);

    // depth-first：2+1=3 uses，2+0=2 successes → 2/3。
    assert_eq!(json["aggregate"]["depth-first"]["uses"], 3);
    assert_eq!(json["aggregate"]["depth-first"]["successes"], 2);
    let rate =
        json["aggregate"]["depth-first"]["success_rate"].as_f64().expect("success_rate 数值");
    assert!(
        (rate - 2.0 / 3.0).abs() < 1e-9,
        "success_rate 应为 2/3，实际 {rate}"
    );

    // hybrid：4 uses 全成功 → 成功率 1.0，因此 best 是 hybrid 而不是 depth-first。
    assert_eq!(json["aggregate"]["hybrid"]["uses"], 4);
    assert_eq!(json["best"]["strategy"], "hybrid");
    assert_eq!(json["best"]["uses"], 4);
    assert!((json["best"]["success_rate"].as_f64().expect("best rate") - 1.0).abs() < 1e-9);
}
