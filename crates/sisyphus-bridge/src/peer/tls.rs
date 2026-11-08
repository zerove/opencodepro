//! TLS/mTLS material loading for cluster control plane (ADR-019).
//!
//! Loads PEM files from [`sisyphus_types::ClusterControlPlaneConfig`] paths.
//! **Fail-closed**: invalid paths/PEM or incomplete config when `tls_enabled=true`
//! must prevent control-plane arming (no false success). Production enablement still
//! requires dedicated-runner network/TLS evidence.

use sisyphus_types::ClusterControlPlaneConfig;
use std::path::Path;
use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};
use x509_parser::prelude::*;

#[derive(Clone, Debug, Default)]
pub struct ControlPlaneTlsMaterial {
    pub server: Option<ServerTlsConfig>,
    pub client: Option<ClientTlsConfig>,
    /// True only when TLS config was successfully loaded/wired for the intended role(s).
    pub wired: bool,
    pub note: String,
}

fn read_pem(path: &str, label: &str) -> Result<Vec<u8>, String> {
    if path.trim().is_empty() {
        return Err(format!("{label} path is empty"));
    }
    std::fs::read(Path::new(path)).map_err(|e| format!("read {label} {path}: {e}"))
}

/// 确保进程级 rustls `CryptoProvider` 已安装（ADR-019）。
///
/// 依赖图可能同时启用 `ring` 与 `aws-lc-rs`（例如测试/dev 依赖统一了 features）。
/// 此时 rustls 无法自动判定 provider，**TLS 握手会 panic**。这里显式安装 `ring`
/// provider（幂等，可重复调用）。必须在构造/使用任何 TLS 配置前调用：
/// bridge `main`、`control_plane_harness` 与 `load_control_plane_tls` 都会调用。
pub fn ensure_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Load TLS material when `tls_enabled=true`. Returns `wired=false` with an error note
/// if required material cannot be loaded (caller must fail-closed / refuse arming).
pub fn load_control_plane_tls(cfg: &ClusterControlPlaneConfig) -> ControlPlaneTlsMaterial {
    ensure_rustls_crypto_provider();
    if !cfg.tls_enabled {
        return ControlPlaneTlsMaterial {
            wired: false,
            note: "tls disabled".to_string(),
            ..Default::default()
        };
    }

    let ca_pem = match read_pem(&cfg.tls_ca_cert_path, "tls_ca_cert_path") {
        Ok(p) => p,
        Err(e) => {
            return ControlPlaneTlsMaterial {
                wired: false,
                note: format!("TLS config present but not wired (fail-closed): {e}"),
                ..Default::default()
            }
        },
    };
    let cert_pem = match read_pem(&cfg.tls_client_cert_path, "tls_client_cert_path") {
        Ok(p) => p,
        Err(e) => {
            return ControlPlaneTlsMaterial {
                wired: false,
                note: format!("TLS config present but not wired (fail-closed): {e}"),
                ..Default::default()
            }
        },
    };
    let key_pem = match read_pem(&cfg.tls_client_key_path, "tls_client_key_path") {
        Ok(p) => p,
        Err(e) => {
            return ControlPlaneTlsMaterial {
                wired: false,
                note: format!("TLS config present but not wired (fail-closed): {e}"),
                ..Default::default()
            }
        },
    };

    let ca = Certificate::from_pem(ca_pem);
    let identity = Identity::from_pem(cert_pem, key_pem);

    // Server: present identity, require client CA for mTLS.
    let server = ServerTlsConfig::new().identity(identity.clone()).client_ca_root(ca.clone());

    // Client: trust CA, present identity for mTLS, verify domain.
    let domain = if cfg.tls_domain_name.trim().is_empty() {
        "localhost".to_string()
    } else {
        cfg.tls_domain_name.trim().to_string()
    };
    let client = ClientTlsConfig::new().ca_certificate(ca).identity(identity).domain_name(domain);

    ControlPlaneTlsMaterial {
        server: Some(server),
        client: Some(client),
        wired: true,
        note: "mTLS material loaded (wiring active; production requires dedicated-runner evidence)"
            .to_string(),
    }
}

/// Verify peer certificate subjects against `allowed_submit_subjects` (ADR-019).
///
/// Format per entry: `DNS:<name>`, `URI:<uri>`, `CN:<value>`, or bare value (matches
/// Subject CN, SAN DNS, or SAN URI). Matching is case-sensitive exact match on the extracted
/// value. **Fail-closed**: returns `false` if `allowed` is empty, `peer_certs_der` is empty,
/// no cert parses, or no allowed subject matches.
pub fn verify_peer_subject_allowlist(peer_certs_der: &[Vec<u8>], allowed: &[String]) -> bool {
    if allowed.is_empty() || peer_certs_der.is_empty() {
        return false;
    }
    for der in peer_certs_der {
        let Ok((_, cert)) = X509Certificate::from_der(der) else {
            continue;
        };
        // Subject CN(s)
        let mut subjects: Vec<(String, String)> = Vec::new();
        for rdn in cert.subject().iter() {
            for attr in rdn.iter() {
                if attr.attr_type() == &oid_registry::OID_X509_COMMON_NAME {
                    if let Ok(s) = attr.as_str() {
                        subjects.push(("CN".to_string(), s.to_string()));
                        subjects.push(("ANY".to_string(), s.to_string()));
                    }
                }
            }
        }
        // SANs
        if let Ok(Some(san)) = cert.subject_alternative_name() {
            for name in &san.value.general_names {
                match name {
                    GeneralName::DNSName(d) => {
                        subjects.push(("DNS".to_string(), (*d).to_string()));
                        subjects.push(("ANY".to_string(), (*d).to_string()));
                    },
                    GeneralName::URI(u) => {
                        subjects.push(("URI".to_string(), (*u).to_string()));
                        subjects.push(("ANY".to_string(), (*u).to_string()));
                    },
                    _ => {},
                }
            }
        }
        for allow in allowed {
            let allow = allow.trim();
            if allow.is_empty() {
                continue;
            }
            let (kind, val) = if let Some(v) = allow.strip_prefix("DNS:") {
                ("DNS", v)
            } else if let Some(v) = allow.strip_prefix("URI:") {
                ("URI", v)
            } else if let Some(v) = allow.strip_prefix("CN:") {
                ("CN", v)
            } else {
                ("ANY", allow)
            };
            if subjects.iter().any(|(k, s)| k == kind && s == val) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> ClusterControlPlaneConfig {
        ClusterControlPlaneConfig::default()
    }

    #[test]
    fn tls_disabled_is_not_wired() {
        let mut cfg = base_cfg();
        cfg.tls_enabled = false;
        let m = load_control_plane_tls(&cfg);
        assert!(!m.wired);
        assert!(m.server.is_none() && m.client.is_none());
    }

    #[test]
    fn tls_enabled_empty_paths_fail_closed() {
        let mut cfg = base_cfg();
        cfg.tls_enabled = true;
        cfg.tls_ca_cert_path.clear();
        let m = load_control_plane_tls(&cfg);
        assert!(!m.wired, "missing paths must not wire TLS");
        assert!(m.note.contains("fail-closed"));
    }

    #[test]
    fn tls_enabled_missing_files_fail_closed() {
        let mut cfg = base_cfg();
        cfg.tls_enabled = true;
        cfg.tls_ca_cert_path = "/nonexistent/ca.pem".into();
        cfg.tls_client_cert_path = "/nonexistent/cert.pem".into();
        cfg.tls_client_key_path = "/nonexistent/key.pem".into();
        let m = load_control_plane_tls(&cfg);
        assert!(!m.wired);
        assert!(m.note.contains("fail-closed"));
    }

    #[test]
    fn subject_allowlist_empty_or_missing_fail_closed() {
        assert!(!verify_peer_subject_allowlist(&[], &["DNS:x".into()]));
        assert!(!verify_peer_subject_allowlist(&[vec![1, 2, 3]], &[]));
        assert!(!verify_peer_subject_allowlist(
            &[vec![0u8; 8]],
            &["DNS:x".into()]
        ));
    }
}
