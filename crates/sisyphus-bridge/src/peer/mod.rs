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

pub mod anti_entropy;
pub mod discovery;
pub mod experience_sync;
pub mod fallback;
pub mod manager;
pub mod service;

pub use experience_sync::{
    broadcast_experience, ExperienceSyncHandler, PeerBroadcastExperienceBank,
};

pub use anti_entropy::{
    build_sync_page, digest_to_sync_request_payload, local_digest, parse_sync_request_payload,
    request_sync, serve_sync_request, sync_request_event, SyncPage, SYNC_PAGE_SIZE,
};

pub use fallback::{spawn_static_peers, StaticPeerConfig};
pub use manager::{ClusterEvent as MgrEvent, PeerInfo, PeerManager, PeerStatus};
pub use service::ClusterServiceImpl;
