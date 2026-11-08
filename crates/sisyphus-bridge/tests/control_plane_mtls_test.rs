//! ADR-019：in-process mTLS + `mtls_subject_allowlist` 鉴权集成测试（**本地证据**）。
//!
//! 用 `rcgen` 在临时目录生成测试 PKI（CA / server / node-a / node-b），启动带
//! `ServerTlsConfig` 的真实 tonic gRPC server，验证：
//! - `load_control_plane_tls` 对真实 PEM 返回 `wired=true` 且同时给出 server/client 配置；
//! - `SubmitControlLease`/`ProposeControl` 在 `mtls_subject_allowlist` 下允许 allowlist
//!   主体、拒绝非 allowlist 主体（`permission_denied`）；
//! - 无客户端证书的 TLS 连接无法完成（mTLS 在传输层强制）；
//! - `CN:` 条目匹配 Subject CN；
//! - 未知鉴权模式 **fail-closed**（即使绕过 `main.rs` 装配检查也拒绝）。
//!
//! **边界（不得外推）**：这是 loopback + 单进程证据，不构成 ADR-019 专用 runner 所需的
//! 真实跨进程网络分区/少数派不可提交/leader 变更/混沌/重启/TLS 长稳/回滚证据；
//! `cluster.control_plane.enabled` 默认仍为 `false`。

use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use sisyphus_bridge::peer::control_plane::ControlPlaneState;
use sisyphus_bridge::peer::{load_control_plane_tls, ClusterServiceImpl, PeerManager};
use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
use sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer;
use sisyphus_bridge_api::cluster_proto::ControlProposeRequest;
use sisyphus_types::ClusterControlPlaneConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{
    Certificate as TonicCertificate, Channel, ClientTlsConfig, Endpoint, Identity, Server,
    ServerTlsConfig,
};
use tonic::Code;

struct Leaf {
    cert_pem: String,
    key_pem: String,
    cert_path: PathBuf,
    key_path: PathBuf,
}

struct Pki {
    _dir: TempDir,
    ca_pem: String,
    ca_path: PathBuf,
    node_a: Leaf,
    node_b: Leaf,
}

fn issue_leaf(
    dir: &Path,
    ca_cert: &Certificate,
    ca_key: &KeyPair,
    cn: &str,
    sans: Vec<String>,
    ekus: Vec<ExtendedKeyUsagePurpose>,
    file_stem: &str,
) -> Leaf {
    let key = KeyPair::generate().expect("leaf keypair");
    let mut params = CertificateParams::new(sans).expect("leaf params");
    params.distinguished_name.push(DnType::CommonName, cn);
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = ekus;
    let cert = params.signed_by(&key, ca_cert, ca_key).expect("leaf signed by CA");
    let cert_pem = cert.pem();
    let key_pem = key.serialize_pem();
    let cert_path = dir.join(format!("{file_stem}.pem"));
    let key_path = dir.join(format!("{file_stem}.key"));
    std::fs::write(&cert_path, &cert_pem).expect("write leaf cert");
    std::fs::write(&key_path, &key_pem).expect("write leaf key");
    Leaf {
        cert_pem,
        key_pem,
        cert_path,
        key_path,
    }
}

/// 生成测试 PKI：CA + node-a（双用途；SAN `localhost`/`node-a.internal`，CN `node-a`）
/// + node-b（SAN `node-b.internal`，CN `node-b`）。
fn generate_pki() -> Pki {
    sisyphus_bridge::peer::tls::ensure_rustls_crypto_provider();
    let dir = tempfile::tempdir().expect("tempdir");
    let ca_key = KeyPair::generate().expect("ca keypair");
    let mut ca_params = CertificateParams::new(Vec::new()).expect("ca params");
    ca_params.distinguished_name.push(DnType::CommonName, "sisyphus-adr019-test-ca");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key).expect("ca self-signed");
    let ca_pem = ca_cert.pem();
    let ca_path = dir.path().join("ca.pem");
    std::fs::write(&ca_path, &ca_pem).expect("write ca");

    // node 证书为“双用途”：既作为本节点 gRPC server 身份（ServerAuth，SAN 必须
    // 覆盖客户端 dial 的 `tls_domain_name`，此处为 loopback 的 `localhost`），
    // 也作为 coordinator/proposer 的 client mTLS 身份（ClientAuth）。
    let node_a = issue_leaf(
        dir.path(),
        &ca_cert,
        &ca_key,
        "node-a",
        vec!["localhost".to_string(), "node-a.internal".to_string()],
        vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ],
        "node-a",
    );
    let node_b = issue_leaf(
        dir.path(),
        &ca_cert,
        &ca_key,
        "node-b",
        vec!["node-b.internal".to_string()],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
        "node-b",
    );

    Pki {
        _dir: dir,
        ca_pem,
        ca_path,
        node_a,
        node_b,
    }
}

/// 按 `tls_enabled` 真实加载 server/client mTLS 材料（以 node-a 作为本节点身份）。
fn control_plane_cfg(pki: &Pki) -> ClusterControlPlaneConfig {
    ClusterControlPlaneConfig {
        tls_enabled: true,
        tls_ca_cert_path: pki.ca_path.to_string_lossy().into_owned(),
        tls_client_cert_path: pki.node_a.cert_path.to_string_lossy().into_owned(),
        tls_client_key_path: pki.node_a.key_path.to_string_lossy().into_owned(),
        tls_domain_name: "localhost".to_string(),
        ..Default::default()
    }
}

async fn load_server_tls(pki: &Pki) -> ServerTlsConfig {
    let material = load_control_plane_tls(&control_plane_cfg(pki));
    assert!(
        material.wired,
        "TLS material must wire with valid PEM: {}",
        material.note
    );
    assert!(
        material.client.is_some(),
        "client mTLS config must be loaded"
    );
    material.server.expect("server TLS config")
}

async fn start_control_plane_server(
    mode: &str,
    allowed: Vec<String>,
    tls: ServerTlsConfig,
) -> (String, tokio::task::JoinHandle<()>) {
    let manager = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
    let state = Arc::new(ControlPlaneState::new());
    let service = ClusterServiceImpl::with_control_plane(manager, None, state, true)
        .with_control_plane_auth(mode, allowed);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let incoming = TcpListenerStream::new(listener);
    let handle = tokio::spawn(async move {
        let _ = Server::builder()
            .tls_config(tls)
            .expect("server tls config")
            .add_service(ClusterServer::new(service))
            .serve_with_incoming(incoming)
            .await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    (format!("https://127.0.0.1:{port}"), handle)
}

async fn connect_client(
    addr: &str,
    ca_pem: &str,
    identity: Option<(&str, &str)>,
) -> Result<ClusterClient<Channel>, String> {
    let mut tls = ClientTlsConfig::new()
        .ca_certificate(TonicCertificate::from_pem(ca_pem))
        .domain_name("localhost");
    if let Some((cert, key)) = identity {
        tls = tls.identity(Identity::from_pem(cert, key));
    }
    let channel = Endpoint::from_shared(addr.to_string())
        .map_err(|e| e.to_string())?
        .tls_config(tls)
        .map_err(|e| e.to_string())?
        .connect_timeout(Duration::from_secs(5))
        .connect()
        .await
        .map_err(|e| e.to_string())?;
    Ok(ClusterClient::new(channel))
}

fn lease_request(request_id: &str) -> ControlProposeRequest {
    ControlProposeRequest {
        request_id: request_id.to_string(),
        key: "lease:mtls/demo".to_string(),
        operation: "lease.acquire".to_string(),
        payload_json: serde_json::to_vec(&serde_json::json!({"owner": "node-a", "ttl_secs": 30}))
            .expect("payload json"),
        source_node_id: "node-a".to_string(),
        expected_generation: 0,
    }
}

/// allowlist 命中（DNS SAN）→ 进入业务逻辑（未装配 coordinator 时返回 `UNAVAILABLE` 响应，
/// 而非 RPC 错误）；非 allowlist 主体 → `permission_denied`。`ProposeControl` 同样受保护。
#[tokio::test]
async fn mtls_subject_allowlist_enforces_dns_subject() {
    let pki = generate_pki();
    let tls = load_server_tls(&pki).await;
    let (addr, _handle) = start_control_plane_server(
        "mtls_subject_allowlist",
        vec!["DNS:node-a.internal".to_string()],
        tls,
    )
    .await;

    let mut a = connect_client(
        &addr,
        &pki.ca_pem,
        Some((&pki.node_a.cert_pem, &pki.node_a.key_pem)),
    )
    .await
    .expect("node-a mTLS connect");
    let resp = a
        .submit_control_lease(lease_request("mtls-a-1"))
        .await
        .expect("node-a must pass subject allowlist")
        .into_inner();
    assert_eq!(resp.error_code, "UNAVAILABLE", "{resp:?}");
    assert!(!resp.accepted, "{resp:?}");

    let mut b = connect_client(
        &addr,
        &pki.ca_pem,
        Some((&pki.node_b.cert_pem, &pki.node_b.key_pem)),
    )
    .await
    .expect("node-b TLS connect (chain valid, subject not allowlisted)");
    let err = b
        .submit_control_lease(lease_request("mtls-b-1"))
        .await
        .expect_err("node-b must be denied by subject allowlist");
    assert_eq!(err.code(), Code::PermissionDenied, "{err:?}");

    let err = b
        .propose_control(lease_request("mtls-b-2"))
        .await
        .expect_err("ProposeControl must enforce the same subject allowlist");
    assert_eq!(err.code(), Code::PermissionDenied, "{err:?}");
}

/// `CN:` 条目匹配证书 Subject CN（与 DNS SAN 独立）。
#[tokio::test]
async fn mtls_subject_allowlist_matches_cn() {
    let pki = generate_pki();
    let tls = load_server_tls(&pki).await;
    let (addr, _handle) =
        start_control_plane_server("mtls_subject_allowlist", vec!["CN:node-a".to_string()], tls)
            .await;

    let mut a = connect_client(
        &addr,
        &pki.ca_pem,
        Some((&pki.node_a.cert_pem, &pki.node_a.key_pem)),
    )
    .await
    .expect("node-a mTLS connect");
    let resp = a
        .submit_control_lease(lease_request("mtls-cn-1"))
        .await
        .expect("CN:node-a must match subject CN")
        .into_inner();
    assert_eq!(resp.error_code, "UNAVAILABLE", "{resp:?}");
}

/// 没有客户端证书时 mTLS 握手必须失败（传输层强制；不允许退化为单向 TLS）。
#[tokio::test]
async fn mtls_requires_client_certificate() {
    let pki = generate_pki();
    let tls = load_server_tls(&pki).await;
    let (addr, _handle) = start_control_plane_server(
        "mtls_subject_allowlist",
        vec!["DNS:node-a.internal".to_string()],
        tls,
    )
    .await;

    match connect_client(&addr, &pki.ca_pem, None).await {
        // rustls 在 connect 阶段就拒绝（服务端要求客户端证书）。
        Err(_) => {},
        // 若连接被延迟建立，RPC 也必须失败（不能静默降级为无鉴权）。
        Ok(mut c) => {
            let err = c
                .submit_control_lease(lease_request("mtls-anon"))
                .await
                .expect_err("anonymous client must not complete mTLS");
            assert!(
                matches!(
                    err.code(),
                    Code::PermissionDenied
                        | Code::Unauthenticated
                        | Code::Unavailable
                        | Code::Unknown
                ),
                "{err:?}"
            );
        },
    }
}

/// 未知鉴权模式必须 fail-closed（不能因为绕过 `main.rs` 的 arm 检查而放行）。
#[tokio::test]
async fn unknown_auth_mode_is_fail_closed() {
    let pki = generate_pki();
    let tls = load_server_tls(&pki).await;
    let (addr, _handle) = start_control_plane_server("not_a_real_mode", Vec::new(), tls).await;

    let mut a = connect_client(
        &addr,
        &pki.ca_pem,
        Some((&pki.node_a.cert_pem, &pki.node_a.key_pem)),
    )
    .await
    .expect("node-a mTLS connect");
    let err = a
        .submit_control_lease(lease_request("mtls-unknown-mode"))
        .await
        .expect_err("unknown auth mode must deny");
    assert_eq!(err.code(), Code::PermissionDenied, "{err:?}");
}
