//! ADR-019：控制面 mTLS **测试** PKI 生成器（仅测试/专用 runner 使用）。
//!
//! 用 `rcgen` 生成 CA + 节点证书（`serverAuth`+`clientAuth` 双用途），供
//! `docs/benchmarks/control-plane-dedicated-runner-evidence-2026-11-08.md` 的
//! TLS 场景使用。**不得**用于生产信任链。
//!
//! 节点证书 SAN = `DNS:<node>`, `DNS:localhost`, `IP:127.0.0.1`：
//! - `DNS:<node>`：节点间按节点 DNS 名互连；
//! - `DNS:localhost` / `IP:127.0.0.1`：同机多进程 loopback 证据采集。
//!
//! 部署语义提醒：
//! - `[cluster.control_plane].tls_domain_name` 必须匹配**被 dial 节点**证书的 SAN；
//! - `tls_enabled=true` 时 TLS 应用于共享 gRPC 端口，主 bridge 客户端也必须使用 TLS；
//! - `submit_auth_mode="mtls_subject_allowlist"` 的 `allowed_submit_subjects` 必须列出
//!   所有允许提交控制面的节点身份（如 `DNS:node-1` / `CN:node-1`）。
//!
//! 用法：
//!   cargo run -p sisyphus-bridge --no-default-features --example gen_control_plane_test_certs -- \
//!     --out ./control-plane-certs --node node-1 --node node-2 --node node-3
//!
//! 或：`make gen-control-plane-test-certs ARGS="--out ./certs --node node-1"`
//!
//! 私钥以 `0600` 写入（Unix），且已加入 `.gitignore` 建议（请勿提交）。

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use std::error::Error;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

fn write_secret(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)?;
    }
    Ok(())
}

fn write_public(path: &Path, contents: &str) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, contents)?;
    Ok(())
}

fn usage() {
    eprintln!(
        "gen_control_plane_test_certs: --out DIR [--node NAME ...]\n\
         default: --out ./control-plane-certs --node node-1 --node node-2 --node node-3"
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut out = PathBuf::from("./control-plane-certs");
    let mut nodes: Vec<String> = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--out" => {
                out = PathBuf::from(it.next().ok_or("--out requires a directory")?);
            },
            "--node" => {
                nodes.push(it.next().ok_or("--node requires a name")?);
            },
            "--help" | "-h" => {
                usage();
                return Ok(());
            },
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    if nodes.is_empty() {
        nodes = vec![
            "node-1".to_string(),
            "node-2".to_string(),
            "node-3".to_string(),
        ];
    }
    std::fs::create_dir_all(&out)?;

    // CA（仅测试用途）
    let ca_key = KeyPair::generate()?;
    let mut ca_params = CertificateParams::new(Vec::new())?;
    ca_params.distinguished_name.push(DnType::CommonName, "sisyphus-control-plane-test-ca");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_cert = ca_params.self_signed(&ca_key)?;
    write_public(&out.join("ca.pem"), &ca_cert.pem())?;
    write_secret(&out.join("ca.key"), &ca_key.serialize_pem())?;
    println!("==> test CA: {}", out.join("ca.pem").display());

    for node in &nodes {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![node.clone(), "localhost".to_string()])?;
        params.subject_alt_names.push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        params.distinguished_name.push(DnType::CommonName, node);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        let cert = params.signed_by(&key, &ca_cert, &ca_key)?;
        write_public(&out.join(format!("{node}.pem")), &cert.pem())?;
        write_secret(&out.join(format!("{node}.key")), &key.serialize_pem())?;
        println!("==> node certificate: {node}");
    }

    let subjects = nodes.iter().map(|n| format!("\"DNS:{n}\"")).collect::<Vec<_>>().join(", ");
    println!();
    println!("✅ test PKI ready in {}", out.display());
    println!("   nodes: {}", nodes.join(", "));
    println!();
    println!("示例（bridge 配置片段，按需替换路径/身份）：");
    println!("[cluster.control_plane]");
    println!("enabled = true");
    println!("tls_enabled = true");
    println!("tls_ca_cert_path = \"{}/ca.pem\"", out.display());
    println!(
        "tls_client_cert_path = \"{}/{}.pem\"",
        out.display(),
        nodes[0]
    );
    println!(
        "tls_client_key_path = \"{}/{}.key\"",
        out.display(),
        nodes[0]
    );
    println!("tls_domain_name = \"localhost\"   # 必须匹配被 dial 节点证书 SAN");
    println!("submit_auth_mode = \"mtls_subject_allowlist\"");
    println!("allowed_submit_subjects = [{subjects}]");
    println!();
    println!("注意：生产启用前仍须满足 ADR-019 准入（真实网络分区/混沌/重启/TLS 长稳/回滚证据）。");
    println!("测试 CA/证书禁止用于生产信任链；请勿提交私钥。");
    Ok(())
}
