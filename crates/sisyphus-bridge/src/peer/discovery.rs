//! mDNS 服务发现（TODO 8.3.1 / ADR-009 § 1）
//!
//! 使用 `mdns-sd` crate（同步 API）实现：
//! - 注册：把本节点 `_sisyphus-bridge._tcp.local` 服务实例广播到 LAN
//! - 浏览：监听同一服务类型，发现其他 peer
//!
//! 设计要点：
//! - mdns-sd 是同步 API；浏览事件通过后台线程转发到 async mpsc
//! - 服务名（instance name）= node_id，避免冲突
//! - 浏览结果通过 mpsc channel 推回 manager

use crate::peer::manager::{ClusterEvent, PeerInfo};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use tokio::sync::mpsc;

/// mDNS 发现错误（不依赖 anyhow）
#[derive(Debug)]
pub enum DiscoveryError {
    /// mDNS daemon 创建失败
    DaemonCreate(String),
    /// 服务注册失败
    Register(String),
    /// 浏览启动失败
    Browse(String),
    /// 字符串 → ServiceInfo 转换失败
    ServiceInfo(String),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DaemonCreate(s) => write!(f, "mDNS daemon create failed: {s}"),
            Self::Register(s) => write!(f, "mDNS register failed: {s}"),
            Self::Browse(s) => write!(f, "mDNS browse failed: {s}"),
            Self::ServiceInfo(s) => write!(f, "ServiceInfo build failed: {s}"),
        }
    }
}

impl Error for DiscoveryError {}

/// mDNS 服务类型（RFC 6763）。如 `_sisyphus-bridge._tcp.local`。
#[derive(Clone, Debug)]
pub struct MdnsConfig {
    /// 服务类型（默认 `_sisyphus-bridge._tcp.local`）
    pub service_type: String,
    /// 集群名（用于 multi-cluster 区分，作为 TXT record）
    pub cluster_name: String,
}

impl Default for MdnsConfig {
    fn default() -> Self {
        Self {
            service_type: "_sisyphus-bridge._tcp.local".to_string(),
            cluster_name: "_sisyphus-bridge".to_string(),
        }
    }
}

/// 在 LAN 上注册本节点（同步操作，应在 spawn_blocking 内调用）
///
/// # 参数
/// - `daemon`: mdns-sd daemon
/// - `instance_name`: 服务实例名（通常 = node_id）
/// - `host_name`: 主机名（一般 = hostname）
/// - `port`: gRPC 监听端口
/// - `config`: mDNS 配置（service_type + cluster_name）
pub fn register_local_node(
    daemon: &ServiceDaemon,
    instance_name: &str,
    host_name: &str,
    port: u16,
    config: &MdnsConfig,
) -> Result<(), DiscoveryError> {
    // TXT records：用于 multi-cluster 区分 + 元数据
    let properties = [("cluster", config.cluster_name.as_str()), ("version", "v1")];

    let service_info = ServiceInfo::new(
        &config.service_type,
        instance_name,
        host_name,
        "", // bind to all interfaces
        port,
        &properties[..],
    )
    .map_err(|e| DiscoveryError::ServiceInfo(e.to_string()))?;

    daemon.register(service_info).map_err(|e| DiscoveryError::Register(e.to_string()))?;
    Ok(())
}

/// 浏览 LAN 上其他节点，返回一个 mpsc Receiver 推送发现的事件
///
/// 接收端的事件是 `ClusterEvent::PeerJoined` 或 `ClusterEvent::PeerLeft`。
pub fn browse_peers(
    daemon: Arc<ServiceDaemon>,
    self_node_id: String,
    config: MdnsConfig,
) -> Result<mpsc::UnboundedReceiver<ClusterEvent>, DiscoveryError> {
    let (tx, rx) = mpsc::unbounded_channel::<ClusterEvent>();

    let browse =
        daemon.browse(&config.service_type).map_err(|e| DiscoveryError::Browse(e.to_string()))?;

    // 后台线程把同步事件转换为 async 事件
    std::thread::spawn(move || {
        let mut last_seen: HashSet<String> = HashSet::new();

        while let Ok(event) = browse.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    // instance name = info.get_fullname() 第一段
                    let fullname = info.get_fullname();
                    let node_id =
                        fullname.split('.').next().unwrap_or("").trim_end_matches('.').to_string();
                    if node_id.is_empty() || node_id == self_node_id {
                        continue;
                    }

                    // 取第一个有效的 IP（IPv4 优先）。ScopedIp 是 non-exhaustive，
                    // 需要 wildcard 兜底（应对未来 mdns-sd 新增变体）。
                    let port = info.get_port();
                    let addr = info.get_addresses().iter().find_map(|scoped_ip| {
                        let ip = match scoped_ip {
                            mdns_sd::ScopedIp::V4(v4) => std::net::IpAddr::V4(*v4.addr()),
                            mdns_sd::ScopedIp::V6(v6) => std::net::IpAddr::V6(*v6.addr()),
                            _ => return None,
                        };
                        if ip.is_loopback() || ip.is_unspecified() {
                            None
                        } else {
                            Some(std::net::SocketAddr::new(ip, port))
                        }
                    });

                    if let Some(addr) = addr {
                        if last_seen.insert(node_id.clone()) {
                            let mut peer_info = PeerInfo::new(node_id.clone(), addr.to_string());
                            peer_info.joined_at = chrono_now_secs();
                            let _ = tx.send(ClusterEvent::PeerJoined(peer_info));
                        }
                    }
                },
                ServiceEvent::ServiceRemoved(_, full_name) => {
                    let node_id =
                        full_name.split('.').next().unwrap_or("").trim_end_matches('.').to_string();
                    if !node_id.is_empty() && node_id != self_node_id && last_seen.remove(&node_id)
                    {
                        let _ = tx.send(ClusterEvent::PeerLeft {
                            node_id,
                            reason: "mdns_removed".to_string(),
                        });
                    }
                },
                _ => {},
            }
        }
    });

    Ok(rx)
}

/// 创建 ServiceDaemon（同步初始化）
pub fn create_daemon() -> Result<ServiceDaemon, DiscoveryError> {
    ServiceDaemon::new().map_err(|e| DiscoveryError::DaemonCreate(e.to_string()))
}

/// Unix epoch seconds（helper — 不依赖 chrono crate）
fn chrono_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mdns_config_default() {
        let c = MdnsConfig::default();
        assert_eq!(c.service_type, "_sisyphus-bridge._tcp.local");
        assert_eq!(c.cluster_name, "_sisyphus-bridge");
    }

    #[test]
    fn test_create_daemon_succeeds() {
        // 在大多数环境下创建 mDNS daemon 不会失败（即使无网络）
        let result = create_daemon();
        assert!(result.is_ok());
    }

    #[test]
    fn test_chrono_now_secs_recent() {
        let now = chrono_now_secs();
        // 2024-01-01 epoch
        assert!(now > 1_704_067_200);
    }

    #[test]
    fn test_discovery_error_display() {
        let e = DiscoveryError::DaemonCreate("test".into());
        assert!(e.to_string().contains("test"));
    }
}
