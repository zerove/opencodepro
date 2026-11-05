//! WebUI → Bridge REST 契约测试：防止前后端接口漂移。
//!
//! 背景：`webui/` 是一个独立 crate，通过 `reqwest` 直接调用 bridge 的 REST 端点。
//! 两边没有任何编译期耦合——bridge 改一个路径、把 `GET` 换成 `POST`、或删掉某个
//! 端点，`webui` 照样编译通过、测试照样全绿，直到用户点开页面才发现「接口不存在」。
//!
//! 这正是本项目反复出现的失败模式：**「看起来接好了」**。编译期类型检查看不见
//! 字符串里的 URL，所以必须在测试期把它变成可判定的对象。
//!
//! 判据（两个方向都要满足，缺一不可）：
//! 1. `webui` 发出的每个 (method, path) 都存在于 bridge 的 axum 路由表；
//! 2. 同一对 (method, path) 也出现在对外发布的 OpenAPI 文档里。
//!
//! 只查 OpenAPI 不够——文档可能停留在旧路径（`every_route_is_documented` 只保证
//! 「路由 ⊆ 文档」，不保证「文档 ⊆ 路由」）；只查路由表也不够——那样 webui 调用的
//! 端点会在 Swagger 里消失，外部集成方看不见。两个方向一起断言，才把「声明」与
//! 「实现」钉在同一处。
//!
//! 路径参数名不参与比较：`webui` 写 `{sop_id}`、axum 写 `:id`、OpenAPI 写 `{id}`，
//! 三者指向同一条路由。比较的是**形状**（字面段相同、参数段位置相同），
//! 参数改名不影响契约，路径改名会立刻失败——这是刻意的边界。

use std::collections::BTreeSet;

/// `webui` 的调用点全部写在 `lib.rs` 里（代理函数集中在 850–1010 行一带）。
const WEBUI_SRC: &str = include_str!("../../../webui/src/lib.rs");
/// bridge 的真实路由表。`mod.rs` 是唯一注册处，测试直接读源文件。
const BRIDGE_ROUTES_SRC: &str = include_str!("../src/http/mod.rs");

/// 把路径里的参数段统一成 `{}`。
///
/// axum 用 `:id`，OpenAPI 用 `{id}`，webui 用 `{sop_id}`。三者形状相同即视为同一条
/// 路由：参数名是调用方的局部命名，不构成契约的一部分。
fn shape(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.starts_with(':') || seg.starts_with('{') {
                "{}"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 从源码里解析 webui 发出的 (method, path) 调用。
///
/// 只认 `format!("{}/...` 这一种写法：它是本仓库里唯一构造 bridge URL 的方式
/// （`bridge_url` 的拼接点）。识别不到足够多的调用点时由调用方断言失败，
/// 避免「解析器失效 → 零断言 → 静默变绿」。
fn webui_requests(src: &str) -> Vec<(String, String)> {
    const METHODS: [&str; 5] = ["get", "post", "put", "delete", "patch"];
    let mut out = BTreeSet::new();

    for (idx, _) in src.match_indices("format!(\"{}/") {
        let Some(rest) = src[idx..].strip_prefix("format!(\"{}/") else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        let path = format!("/{}", &rest[..end]);

        // 方法名可能写在上一行（链式调用被 rustfmt 拆开），因此向前看一段
        // 窗口而不是只看当前行。
        let window_start = idx.saturating_sub(200);
        let window = &src[window_start..idx];
        let method = METHODS
            .iter()
            .filter_map(|m| {
                let needle = format!(".{m}(");
                window.rfind(&needle).map(|pos| (pos, *m))
            })
            .max_by_key(|(pos, _)| *pos)
            .map(|(_, m)| m.to_uppercase());

        let Some(method) = method else { continue };
        out.insert((method, path));
    }

    out.into_iter().collect()
}

/// 从 axum 路由表里解析 (method, path)。
///
/// 与 `http_openapi.rs` 的 `every_route_is_documented` 独立实现：那边解析的是
/// 「文档漏了什么」，这里解析的是「webui 调的东西在不在」。两处都读同一份源文件，
/// 但用途不同——共享解析器会让一次解析缺陷同时骗过两道门禁。
fn bridge_routes(src: &str) -> Vec<(String, String)> {
    const METHODS: [&str; 5] = ["get", "post", "put", "delete", "patch"];
    let mut out = BTreeSet::new();

    let mut cursor = 0usize;
    while let Some(found) = src[cursor..].find(".route(") {
        let start = cursor + found + ".route(".len();
        let Some(quote) = src[start..].find('"') else {
            break;
        };
        let path_start = start + quote + 1;
        let Some(path_end_rel) = src[path_start..].find('"') else {
            break;
        };
        let path_end = path_start + path_end_rel;
        let path = &src[path_start..path_end];

        // 该 `.route(...)` 的方法链延伸到下一次 `.route(` 之前（或到嵌套括号平衡）。
        // 用「下一个 `.route(`」作为上界足够：路由注册是逐行独立的链式调用。
        let next_route = src[path_end..].find(".route(").map(|p| path_end + p).unwrap_or(src.len());
        let body = &src[path_end..next_route];
        // `ServeDir` 之类的非 MethodRouter 目标不构成 REST 端点。
        if path.starts_with("/dashboard") {
            cursor = path_end;
            continue;
        }
        for m in METHODS {
            let needle = format!("{m}(");
            let mut search = 0usize;
            while let Some(hit) = body[search..].find(&needle) {
                let abs = search + hit;
                if abs == 0 || !body.as_bytes()[abs - 1].is_ascii_alphanumeric() {
                    if path.starts_with('/') {
                        out.insert((m.to_uppercase(), path.to_string()));
                    }
                    break;
                }
                search = abs + needle.len();
            }
        }
        cursor = path_end;
    }

    out.into_iter().collect()
}

/// OpenAPI 文档 → (method, shape) 集合。
fn documented_endpoints(doc: &serde_json::Value) -> BTreeSet<(String, String)> {
    let paths = doc["paths"].as_object().expect("OpenAPI 缺少 paths 对象");
    let mut out = BTreeSet::new();
    for (path, item) in paths {
        let Some(ops) = item.as_object() else {
            continue;
        };
        for method in ops.keys() {
            let m = method.to_uppercase();
            if matches!(m.as_str(), "GET" | "POST" | "PUT" | "DELETE" | "PATCH") {
                out.insert((m, shape(path)));
            }
        }
    }
    out
}

/// 解析器自检：三种路径参数写法必须折叠成同一形状，字面段必须保留差异。
///
/// 这条用例的价值在于给上面两个解析器划出**可信边界**：若 `shape` 退化（例如把
/// 参数段也当字面段），跨写法比对会全红；若它过度归一（例如把所有段都抹成 `{}`），
/// 下面「路径改名必须失败」的断言就失去意义。
#[test]
fn path_shape_folds_parameter_names_but_not_literals() {
    assert_eq!(shape("/api/sops/:id"), shape("/api/sops/{sop_id}"));
    assert_eq!(shape("/api/sops/{id}"), shape("/api/sops/:sop_id"));
    assert_ne!(shape("/api/sops/{id}"), shape("/api/sessions/{id}"));
    assert_ne!(shape("/api/sops/{id}"), shape("/api/sops/archive"));
    // 深层参数段同样折叠，且位置必须一致（前缀相同、后缀不同 → 形状不同）。
    assert_eq!(shape("/a/{x}/b/{y}"), shape("/a/:p/b/:q"));
    assert_ne!(shape("/a/{x}/b/{y}"), shape("/a/:p/:q/b"));
}

/// `webui` 发出的每个 bridge 调用都必须是真实存在、且对外文档化的端点。
#[tokio::test]
async fn every_webui_bridge_call_exists_and_is_documented() {
    let calls = webui_requests(WEBUI_SRC);

    // 先把「解析器读到的东西」钉死，再用它做契约比对。
    //
    // 只断言「至少 N 个调用」不够：解析器把两条路径读成同一条时数量照样达标，
    // 于是真正的漂移从后门溜走。这里逐条列出 webui 当前全部调用点——新增或改名
    // 都会让本断言失败，逼迫改动方回来确认「新端点确实存在于 bridge」。
    let expected: BTreeSet<(String, String)> = [
        ("GET", "/health"),
        ("GET", "/api/sessions"),
        ("GET", "/api/sessions/{id}"),
        ("POST", "/api/sessions"),
        ("POST", "/api/sessions/{sid}/messages"),
        ("POST", "/api/vector/search"),
        ("POST", "/api/execute"),
        ("GET", "/api/sops"),
        ("GET", "/api/sops/{sop_id}"),
        ("GET", "/api/skills"),
    ]
    .into_iter()
    .map(|(m, p)| (m.to_string(), p.to_string()))
    .collect();
    let actual: BTreeSet<(String, String)> = calls.iter().cloned().collect();
    assert_eq!(
        actual, expected,
        "webui → bridge 调用点已变化。确认新端点确实存在于 bridge 路由表与 OpenAPI 后，\
         再同步更新本清单（这份清单是契约的显式记录，不是快照噪音）"
    );

    let routes: BTreeSet<(String, String)> =
        bridge_routes(BRIDGE_ROUTES_SRC).into_iter().map(|(m, p)| (m, shape(&p))).collect();
    assert!(
        routes.len() >= 60,
        "只解析出 {} 条 bridge 路由，解析器失效",
        routes.len()
    );

    // 用真实服务返回的文档而不是内部构造函数：文档最终是给外部消费者的，
    // 序列化/装配环节出问题时只有这条路能看见。
    let (_base, server, doc) = serve_and_fetch_openapi().await;
    let documented = documented_endpoints(&doc);
    server.abort();

    let mut not_routed = Vec::new();
    let mut not_documented = Vec::new();
    for (method, path) in &calls {
        let key = (method.clone(), shape(path));
        if !routes.contains(&key) {
            not_routed.push(format!("{method} {path}"));
        }
        if !documented.contains(&key) {
            not_documented.push(format!("{method} {path}"));
        }
    }

    assert!(
        not_routed.is_empty(),
        "webui 调用了 bridge 上不存在的端点（路由表里没有）：{not_routed:?}"
    );
    assert!(
        not_documented.is_empty(),
        "webui 调用的端点没有进 OpenAPI 文档，外部集成方不可见：{not_documented:?}"
    );
}

/// 起真实 axum 服务并拉取 `/openapi.json`（复用 `openapi_live_contract` 的路径）。
async fn serve_and_fetch_openapi() -> (String, tokio::task::JoinHandle<()>, serde_json::Value) {
    use std::sync::{Arc, RwLock};

    let mut config = sisyphus_types::Config::default();
    config.cache.database_url = "sqlite::memory:".to_string();
    config.llm.embedding.allow_dummy_embeddings = true;
    config.llm.embedding.provider = "dummy".into();
    let shared_config = Arc::new(RwLock::new(config));

    let bridge = Arc::new(
        sisyphus_bridge::bridge_service::MySisyphusBridge::new(shared_config, None, None)
            .await
            .expect("bridge 构造失败"),
    );
    let data_dir = std::path::PathBuf::from("target/test-data-webui-contract");
    let state = sisyphus_bridge::http::RestApiState::new(bridge, data_dir);
    let app = sisyphus_bridge::http::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("绑定失败");
    let addr = listener.local_addr().expect("取地址失败");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("服务异常退出");
    });

    let base = format!("http://{addr}");
    let client = reqwest::Client::builder().no_proxy().build().expect("构造 HTTP 客户端失败");
    let resp =
        client.get(format!("{base}/openapi.json")).send().await.expect("请求 /openapi.json 失败");
    assert_eq!(resp.status().as_u16(), 200, "真实服务未返回 200");
    let body = resp.text().await.unwrap_or_default();
    let doc: serde_json::Value = serde_json::from_str(&body).expect("文档不是合法 JSON");

    (base, handle, doc)
}
