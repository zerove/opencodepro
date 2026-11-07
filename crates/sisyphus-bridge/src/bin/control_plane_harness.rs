//! ADR-019 控制面专用运行器证据采集工具（A=gRPC/内部+只读）。
//!
//! 不创建网络分区/混沌（由专用运行环境注入）；提供标准化的 `SubmitControlLease`/
//! `GetControlLease` 场景与 JSON 证据输出。默认 `cluster.control_plane.enabled=false`，
//! 启用前须满足 ADR-019 准入条件。
//!
//! 用法：
//!   cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
//!     --addr http://127.0.0.1:50051 --key lease:demo --owner n1 --request-id r1
//!   cargo run -p sisyphus-bridge --no-default-features --bin control_plane_harness -- \
//!     --addr http://127.0.0.1:50051 --get lease:demo
//!
//! 专用运行器矩阵（in-process ignored tests）：
//!   make test-control-plane-dedicated
//! 证据清单：docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md

use serde_json::json;
use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
use sisyphus_bridge_api::cluster_proto::{ControlProposeRequest, GetControlLeaseRequest};

#[derive(Debug, Default)]
struct Args {
    addr: String,
    get: Option<String>,
    key: String,
    owner: String,
    request_id: String,
    operation: String,
    ttl_secs: u64,
    expected_generation: u64,
    output: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        addr: "http://127.0.0.1:50051".into(),
        get: None,
        key: "lease:demo".into(),
        owner: "harness".into(),
        request_id: format!("harness-{}", uuid::Uuid::new_v4()),
        operation: "lease.acquire".into(),
        ttl_secs: 30,
        expected_generation: 0,
        output: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--addr" => a.addr = it.next().unwrap_or(a.addr),
            "--get" => a.get = it.next(),
            "--key" => a.key = it.next().unwrap_or(a.key),
            "--owner" => a.owner = it.next().unwrap_or(a.owner),
            "--request-id" => a.request_id = it.next().unwrap_or(a.request_id),
            "--operation" => a.operation = it.next().unwrap_or(a.operation),
            "--ttl-secs" => {
                a.ttl_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(a.ttl_secs)
            },
            "--expected-generation" => {
                a.expected_generation = it.next().and_then(|v| v.parse().ok()).unwrap_or(0)
            },
            "--help" | "-h" => {
                eprintln!("ADR-019 control_plane_harness: --addr URL [--get KEY | --key K --owner O --request-id R --operation lease.acquire|renew|release --ttl-secs N --expected-generation G]");
                std::process::exit(0);
            },
            _ => {},
        }
    }
    a
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = parse_args();
    let mut client = ClusterClient::connect(a.addr.clone()).await?;
    if let Some(key) = a.get {
        let resp = client
            .get_control_lease(GetControlLeaseRequest { key: key.clone() })
            .await?
            .into_inner();
        let out = json!({
            "kind": "get_control_lease",
            "addr": a.addr,
            "key": key,
            "found": resp.found,
            "owner": resp.owner,
            "generation": resp.generation,
            "expires_at_unix_ms": resp.expires_at_unix_ms,
            "last_request_id": resp.last_request_id,
            "status_code": resp.status_code,
            "status_message": resp.status_message,
        });
        println!("{}", out);
        if let Some(p) = &a.output {
            std::fs::write(p, serde_json::to_vec_pretty(&out)?)?;
        }
        return Ok(());
    }
    let payload = serde_json::to_vec(&json!({"owner": a.owner, "ttl_secs": a.ttl_secs}))?;
    let resp = client
        .submit_control_lease(ControlProposeRequest {
            request_id: a.request_id.clone(),
            key: a.key.clone(),
            operation: a.operation.clone(),
            payload_json: payload,
            source_node_id: "harness".into(),
            expected_generation: a.expected_generation,
        })
        .await?
        .into_inner();
    let out = json!({
        "kind": "submit_control_lease",
        "addr": a.addr,
        "request_id": a.request_id,
        "key": a.key,
        "operation": a.operation,
        "accepted": resp.accepted,
        "applied_generation": resp.applied_generation,
        "current_generation": resp.current_generation,
        "error_code": resp.error_code,
        "error_message": resp.error_message,
        "idempotent_replay": resp.idempotent_replay,
        "accepts": resp.accepts,
        "cluster_size": resp.cluster_size,
        "uncertain": resp.uncertain,
        "guidance": if resp.uncertain || (!resp.accepted && resp.error_code.is_empty()) {
            "uncertain/timeout: do NOT assume success; retry with same request_id and verify via GetControlLease"
        } else if resp.accepted {
            "committed by quorum (v1 majority ack, not Raft linearizable); verify if needed via GetControlLease"
        } else {
            "rejected; check error_code/error_message"
        }
    });
    println!("{}", out);
    if let Some(p) = &a.output {
        std::fs::write(p, serde_json::to_vec_pretty(&out)?)?;
    }
    Ok(())
}
