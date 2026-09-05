//! Peer protocol module (TODO 8.3.1 / ADR-009 § 1)
//!
//! 多实例 peer-to-peer 集群协议，支持：
//! - mDNS 服务发现（零配置，自动发现 LAN 内其他节点）
//! - 静态 peer fallback（K8s 等多播受限环境）
//! - gRPC bidirectional streaming（Join 握手 + 双向事件流）
//!
//! 模块结构：
//! - `manager`: 成员关系管理（peer 列表 + 事件广播）
//! - `discovery`: mDNS 服务注册与浏览（mdns-sd crate）
//! - `service`: Cluster gRPC service impl（Join + Stream）
//! - `client`: 主动 dial 其他 peer 的 gRPC client

pub mod discovery;
pub mod fallback;
pub mod manager;
pub mod service;

pub use fallback::{spawn_static_peers, StaticPeerConfig};
pub use manager::{ClusterEvent as MgrEvent, PeerInfo, PeerManager, PeerStatus};
pub use service::ClusterServiceImpl;
