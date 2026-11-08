//! TLS/mTLS material loading for cluster control plane (ADR-019).
//!
//! Loads PEM files from [`sisyphus_types::ClusterControlPlaneConfig`] paths.
//! **Fail-closed**: invalid paths/PEM or incomplete config when `tls_enabled=true`
//! must prevent control-plane arming (no false success). Production enablement still
//! requires dedicated-runner network/TLS evidence.

use sisyphus_types::ClusterControlPlaneConfig;
use std::path::Path;
use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};

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

/// Load TLS material when `tls_enabled=true`. Returns `wired=false` with an error note
/// if required material cannot be loaded (caller must fail-closed / refuse arming).
pub fn load_control_plane_tls(cfg: &ClusterControlPlaneConfig) -> ControlPlaneTlsMaterial {
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
}
