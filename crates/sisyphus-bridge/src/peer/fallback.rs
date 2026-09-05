//! 静态 peer fallback dialer (TODO 8.3.1 Phase B)
//!
//! 当 mDNS 在 K8s 等多播受限环境下不可用时，使用 `[cluster].peers` 配置
//! 的静态 peer 列表主动 dial 每个 peer 的 gRPC 地址，调用 `Join` 握手，
//! 并保持 `Stream` 双向流连接。
//!
//! 设计要点：
//! - 每个 peer 一个连接，独立的 tokio task（不互相阻塞）
//! - 重连：连接断开后指数退避重试（避免 K8s pod restart 期间风暴）
//! - `Join` 后订阅本地 manager 事件，转发到对端的 `Stream`
//! - 对端推送的事件 → 本地 manager.on_peer_joined/left

use crate::peer::manager::{ClusterEvent as MgrEvent, PeerInfo, PeerManager};
use sisyphus_bridge_api::cluster_proto::cluster_client::ClusterClient;
use sisyphus_bridge_api::cluster_proto::{ClusterEvent as ProtoEvent, JoinRequest};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use tonic::transport::{Channel, Endpoint};

/// 静态 peer dial 配置
#[derive(Clone, Debug)]
pub struct StaticPeerConfig {
    /// 静态 peer 列表（gRPC 地址，如 `http://node-1:50051`）
    pub peers: Vec<String>,
    /// 本节点 ID（用于 JoinRequest）
    pub self_node_id: String,
    /// 本节点 gRPC 地址（告知对端如何反向连接）
    pub self_grpc_addr: String,
    /// Join 超时（秒）
    pub join_timeout_secs: u64,
    /// 重连初始延迟（失败后指数退避）
    pub reconnect_initial_delay: Duration,
    /// 重连最大延迟（避免无限增长）
    pub reconnect_max_delay: Duration,
}

impl StaticPeerConfig {
    /// 从 sisyphus_types::Config 构造（cluster section）
    pub fn from_types(
        cluster_config: &sisyphus_types::ClusterConfig,
        self_node_id: String,
        self_grpc_addr: String,
    ) -> Self {
        Self {
            peers: cluster_config.peers.clone(),
            self_node_id,
            self_grpc_addr,
            join_timeout_secs: cluster_config.join_timeout_secs,
            reconnect_initial_delay: Duration::from_secs(1),
            reconnect_max_delay: Duration::from_secs(60),
        }
    }
}

/// 启动静态 peer dialer：对每个 peer 启动一个后台 task。
///
/// 每个 task 流程：
/// 1. 指数退避重连循环
/// 2. 建立 gRPC channel + Join 握手
/// 3. 双向 Stream：本地 manager 事件 → outbound；对端事件 → manager.on_peer_*
/// 4. 流断开 → 回到重连循环
///
/// 返回 tokio::JoinHandle 列表（每个 peer 一个）。
pub fn spawn_static_peers(
    config: StaticPeerConfig,
    manager: Arc<PeerManager>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(config.peers.len());
    for grpc_addr in &config.peers {
        let cfg = config.clone();
        let mgr = Arc::clone(&manager);
        let addr = grpc_addr.clone();
        let handle = tokio::spawn(async move {
            run_dial_loop(cfg, mgr, addr).await;
        });
        handles.push(handle);
    }
    handles
}

/// 单个 peer 的 dial 循环（指数退避重连）
async fn run_dial_loop(config: StaticPeerConfig, manager: Arc<PeerManager>, grpc_addr: String) {
    let mut backoff = config.reconnect_initial_delay;
    loop {
        match connect_and_join(&config, &manager, &grpc_addr).await {
            Ok(channel) => {
                // 连接成功，重置 backoff
                backoff = config.reconnect_initial_delay;
                if let Err(e) = run_stream(&manager, channel).await {
                    tracing::warn!(
                        grpc_addr = %grpc_addr,
                        error = %e,
                        "static peer stream ended, will reconnect"
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    grpc_addr = %grpc_addr,
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "static peer connect failed, retrying after backoff"
                );
            },
        }
        tokio::time::sleep(backoff).await;
        // 指数退避（×2，封顶到 max_delay）
        backoff = std::cmp::min(backoff * 2, config.reconnect_max_delay);
    }
}

/// 建立 gRPC channel + Join 握手
async fn connect_and_join(
    config: &StaticPeerConfig,
    _manager: &PeerManager,
    grpc_addr: &str,
) -> Result<Channel, String> {
    let endpoint = Endpoint::try_from(grpc_addr.to_string())
        .map_err(|e| format!("invalid endpoint: {e}"))?
        .connect_timeout(Duration::from_secs(config.join_timeout_secs))
        .timeout(Duration::from_secs(config.join_timeout_secs * 2));

    let channel = endpoint.connect().await.map_err(|e| format!("connect: {e}"))?;

    let mut client = ClusterClient::new(channel.clone());

    let response = client
        .join(JoinRequest {
            node_id: config.self_node_id.clone(),
            grpc_addr: config.self_grpc_addr.clone(),
            cluster_name: "default".into(),
        })
        .await
        .map_err(|e| format!("join: {e}"))?
        .into_inner();

    tracing::info!(
        peer_count = response.peers.len(),
        leader_id = %response.leader_id,
        "static peer Join successful"
    );

    Ok(channel)
}

/// 双向 Stream：本地事件 → outbound，对端事件 → manager
async fn run_stream(manager: &PeerManager, channel: Channel) -> Result<(), String> {
    let mut client = ClusterClient::new(channel);

    // outbound: 本地 manager 事件 → proto
    let mut sub = manager.subscribe();
    let (out_tx, out_rx) = mpsc::unbounded_channel::<ProtoEvent>();
    let outbound_task = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(evt) => {
                    if let Some(proto) = mgr_event_to_proto(evt) {
                        if out_tx.send(proto).is_err() {
                            break;
                        }
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let outbound: UnboundedReceiverStream<ProtoEvent> = UnboundedReceiverStream::new(out_rx);
    let response =
        client.stream(tonic::Request::new(outbound)).await.map_err(|e| format!("stream: {e}"))?;
    let mut inbound = response.into_inner();

    // inbound: 对端 proto 事件 → manager
    while let Some(result) = inbound.next().await {
        match result {
            Ok(proto_evt) => {
                if let Some(mgr_evt) = proto_to_mgr_event(proto_evt) {
                    apply_mgr_event(manager, mgr_evt);
                }
            },
            Err(status) => {
                outbound_task.abort();
                return Err(format!("inbound stream error: {status}"));
            },
        }
    }

    outbound_task.abort();
    Err("inbound stream ended".to_string())
}

// ── proto ↔ Rust 类型转换（与 service.rs 中的版本镜像） ──

fn mgr_event_to_proto(evt: MgrEvent) -> Option<ProtoEvent> {
    use sisyphus_bridge_api::cluster_proto::{
        cluster_event::Event, ExperienceSync as ES, PeerInfo as ProtoPeerInfo, PeerJoined as PJ,
        PeerLeft as PL, StateProposal as SP,
    };
    match evt {
        MgrEvent::PeerJoined(p) => Some(ProtoEvent {
            event: Some(Event::PeerJoined(PJ {
                peer: Some(ProtoPeerInfo {
                    node_id: p.node_id,
                    grpc_addr: p.grpc_addr,
                    joined_at: p.joined_at,
                }),
            })),
        }),
        MgrEvent::PeerLeft { node_id, reason } => Some(ProtoEvent {
            event: Some(Event::PeerLeft(PL { node_id, reason })),
        }),
        MgrEvent::StateProposal { key, value } => Some(ProtoEvent {
            event: Some(Event::StateProposal(SP {
                key,
                value,
                vector_clock: Default::default(),
            })),
        }),
        MgrEvent::ExperienceSync {
            exp_key,
            version,
            vector_clock_json,
            payload,
        } => Some(ProtoEvent {
            event: Some(Event::ExperienceSync(ES {
                exp_key,
                version,
                vector_clock_json,
                payload,
            })),
        }),
    }
}

fn proto_to_mgr_event(evt: ProtoEvent) -> Option<MgrEvent> {
    use sisyphus_bridge_api::cluster_proto::cluster_event::Event;
    evt.event.and_then(|e| match e {
        Event::PeerJoined(p) => {
            p.peer.map(|info| MgrEvent::PeerJoined(PeerInfo::new(info.node_id, info.grpc_addr)))
        },
        Event::PeerLeft(pl) => Some(MgrEvent::PeerLeft {
            node_id: pl.node_id,
            reason: pl.reason,
        }),
        Event::StateProposal(_) | Event::ExperienceSync(_) => None,
    })
}

fn apply_mgr_event(manager: &PeerManager, evt: MgrEvent) {
    match evt {
        MgrEvent::PeerJoined(p) => {
            let _ = manager.on_peer_joined(p);
        },
        MgrEvent::PeerLeft { node_id, reason } => {
            let _ = manager.on_peer_left(&node_id, reason);
        },
        MgrEvent::StateProposal { .. } | MgrEvent::ExperienceSync { .. } => {},
    }
}

#[allow(dead_code)]
fn _unused_unbounded_stream() -> UnboundedReceiverStream<ProtoEvent> {
    let (_tx, rx) = mpsc::unbounded_channel::<ProtoEvent>();
    UnboundedReceiverStream::new(rx)
}

// 显式 use 让 Stream trait 可用（run_stream 函数用 .next()）
#[allow(unused_imports)]
use StreamExt as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_peer_config_from_types() {
        let cluster_config = sisyphus_types::ClusterConfig {
            peers: vec![
                "http://node-1:50051".to_string(),
                "http://node-2:50051".to_string(),
            ],
            join_timeout_secs: 5,
            ..sisyphus_types::ClusterConfig::default()
        };

        let cfg = StaticPeerConfig::from_types(
            &cluster_config,
            "self-node".to_string(),
            "http://localhost:50051".to_string(),
        );

        assert_eq!(cfg.peers.len(), 2);
        assert_eq!(cfg.self_node_id, "self-node");
        assert_eq!(cfg.join_timeout_secs, 5);
    }

    #[test]
    fn test_mgr_event_to_proto_peer_joined_in_fallback() {
        let evt = MgrEvent::PeerJoined(PeerInfo::new("node-x", "addr-x"));
        let proto = mgr_event_to_proto(evt).expect("convert");
        assert!(proto.event.is_some());
    }

    #[test]
    fn test_mgr_event_to_proto_peer_left_in_fallback() {
        let evt = MgrEvent::PeerLeft {
            node_id: "node-x".into(),
            reason: "shutdown".into(),
        };
        let proto = mgr_event_to_proto(evt).expect("convert");
        assert!(proto.event.is_some());
    }

    #[test]
    fn test_proto_to_mgr_event_peer_joined_in_fallback() {
        use sisyphus_bridge_api::cluster_proto::{
            cluster_event::Event, PeerInfo as ProtoPeerInfo, PeerJoined as PJ,
        };
        let proto = ProtoEvent {
            event: Some(Event::PeerJoined(PJ {
                peer: Some(ProtoPeerInfo {
                    node_id: "node-c".into(),
                    grpc_addr: "addr-c:50051".into(),
                    joined_at: 1000,
                }),
            })),
        };
        let mgr = proto_to_mgr_event(proto).expect("convert");
        assert!(matches!(mgr, MgrEvent::PeerJoined(_)));
    }

    #[test]
    fn test_endpoint_parse_succeeds() {
        // Endpoint::try_from 校验 URL 格式
        let result = Endpoint::try_from("http://127.0.0.1:50051".to_string());
        assert!(result.is_ok());
        // 非法格式应失败
        let bad = Endpoint::try_from("not a url".to_string());
        assert!(bad.is_err());
    }
}
