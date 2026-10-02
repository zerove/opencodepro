//! Cluster gRPC service impl (TODO 8.3.1)
//!
//! 实现 `sisyphus.cluster.Cluster` 服务：
//! - `Join`: 单次握手（节点启动时调用）
//! - `Stream`: 双向流（事件广播通道）
//!
//! `ClusterServiceImpl` 持有 `PeerManager` 的共享引用，
//! 接收到的 peer 事件直接喂给 manager.on_peer_joined/left，
//! manager 内部 broadcast 给订阅者。

use crate::peer::anti_entropy;
use crate::peer::manager::{ClusterEvent as MgrEvent, PeerInfo, PeerManager};
use sisyphus_bridge_api::cluster_proto::{
    cluster_server::Cluster, ClusterEvent as ProtoEvent, ExperienceSync, JoinRequest, JoinResponse,
    PeerInfo as ProtoPeerInfo, PeerJoined, PeerLeft, StateProposal,
};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::UnboundedReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status};

/// 类型别名：用于测试时的请求流构造
#[allow(dead_code)]
pub type RequestStream = Pin<Box<dyn Stream<Item = Result<ProtoEvent, Status>> + Send + 'static>>;

/// Cluster service 实现
#[derive(Clone)]
pub struct ClusterServiceImpl {
    manager: Arc<PeerManager>,
    /// 本节点经验库（ADR-010 反熵：收到 `SyncRequest` 时据此计算差量）。
    ///
    /// `None` 表示本节点未启用经验库（如测试夹具或单节点模式）。此时收到
    /// `SyncRequest` 会记一条日志并跳过——反熵是尽力而为的旁路，不应因为
    /// 它缺席而影响流的其余部分。
    experience_bank: Option<Arc<sisyphus_mem::experience_bank::VectorExperienceBank>>,
}

impl ClusterServiceImpl {
    /// 新建 service impl（不参与反熵）
    pub fn new(manager: Arc<PeerManager>) -> Self {
        Self {
            manager,
            experience_bank: None,
        }
    }

    /// 新建 service impl，并挂上本节点经验库以提供反熵差量。
    pub fn with_experience_bank(
        manager: Arc<PeerManager>,
        experience_bank: Arc<sisyphus_mem::experience_bank::VectorExperienceBank>,
    ) -> Self {
        Self {
            manager,
            experience_bank: Some(experience_bank),
        }
    }
}

#[tonic::async_trait]
impl Cluster for ClusterServiceImpl {
    type StreamStream =
        Pin<Box<dyn Stream<Item = Result<ProtoEvent, Status>> + Send + Sync + 'static>>;
    /// Join 握手：返回当前已知 peer 列表 + leader
    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        let req = request.into_inner();
        let info = PeerInfo::new(req.node_id.clone(), req.grpc_addr.clone());

        // 把新 peer 加入 manager（如果不在）
        let _ = self.manager.on_peer_joined(info.clone());

        // 构造响应：当前所有 peer + leader
        let peers = self
            .manager
            .list_peers()
            .into_iter()
            .map(|p| ProtoPeerInfo {
                node_id: p.node_id,
                grpc_addr: p.grpc_addr,
                joined_at: p.joined_at,
            })
            .collect();

        Ok(Response::new(JoinResponse {
            peers,
            leader_id: self.manager.leader_id(),
        }))
    }

    /// 双向流：建立后保持长连接
    async fn stream(
        &self,
        request: Request<tonic::Streaming<ProtoEvent>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        let mut inbound = request.into_inner();
        let manager = Arc::clone(&self.manager);

        // 创建一个 outbound channel，把 manager 事件转成 proto 推到客户端
        let (out_tx, out_rx) = mpsc::unbounded_channel::<ProtoEvent>();
        // 克隆必须在 `move` 进广播 task **之前**完成：task 拿走原 `out_tx`，
        // 应答反熵请求用自己的那一份（ADR-010）。
        let out_tx_sync = out_tx.clone();
        let mut sub = manager.subscribe();

        // 后台任务：从 manager broadcast 转发到 outbound channel
        tokio::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(evt) => {
                        if let Some(proto_evt) = mgr_event_to_proto(evt) {
                            if out_tx.send(proto_evt).is_err() {
                                break; // 客户端断开
                            }
                        }
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // 订阅者落后，跳过丢失的事件
                        continue;
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // 处理客户端发来的事件：feed 给 manager
        let manager_for_inbound = Arc::clone(&manager);
        let bank_for_inbound = self.experience_bank.clone();
        tokio::spawn(async move {
            while let Some(result) = inbound.next().await {
                match result {
                    Ok(proto_evt) => {
                        // ADR-010：先识别反熵请求。它不进入 manager 事件总线——
                        // 而是把差量直接回写到 outbound 通道，因为应答的传输方向
                        // 与请求相反（请求来自 dial 方，应答回到 dial 方）。
                        if let Some(sync_request) =
                            match proto_evt.event.clone() {
                                Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::SyncRequest(
                                    req,
                                )) => Some(req),
                                _ => None,
                            }
                        {
                            let offset = sync_request.offset;
                            handle_sync_request(
                                bank_for_inbound.as_ref(),
                                &sync_request,
                                offset,
                                &out_tx_sync,
                            ).await;
                            continue;
                        }
                        // `SyncPageEnd` 是 dial 方发来的控制消息（本节点作为
                        // dial 方时的翻页信号），服务端不消费它。
                        if is_page_end(&proto_evt) {
                            continue;
                        }
                        if let Some(mgr_evt) = proto_to_mgr_event(proto_evt) {
                            apply_mgr_event(&manager_for_inbound, mgr_evt);
                        }
                    },
                    Err(_status) => break, // 流断开
                }
            }
        });

        // outbound: 把 mpsc::UnboundedReceiver 转成 Stream
        let out_stream = UnboundedReceiverStream::new(out_rx).map(Ok);
        let pinned: Self::StreamStream = Box::pin(out_stream);
        Ok(Response::new(pinned))
    }
}

/// 判定事件是否为反熵页尾标记（纯控制消息，不进入 manager 事件总线）。
pub(crate) fn is_page_end(evt: &ProtoEvent) -> bool {
    matches!(
        evt.event,
        Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::SyncPageEnd(_))
    )
}

/// ADR-010：应答一条 `SyncRequest`，把差量分页回写给请求方。
///
/// **差量以既有 `ExperienceSync` 事件的形式发送**，因此请求方的
/// `ExperienceSyncHandler` 用与实时广播**完全相同**的路径接收并落库——
/// 反熵不引入第二套合并语义，这是它幂等的根源。
///
/// 每页末尾补发 [`SyncPageEnd`] 标记：差量事件与实时广播在 wire 上无法区分，
/// 缺少页边界时 dial 方只能靠计数推测，而实时广播会被误计入页内使游标错乱。
///
/// 单页有界是有意为之：经验库随运行时间单调增长，无界回灌会让单条 gRPC
/// 消息超出默认 4MB 限制，整条流被拒——反而比不同步更糟。
async fn handle_sync_request(
    experience_bank: Option<&Arc<sisyphus_mem::experience_bank::VectorExperienceBank>>,
    request: &sisyphus_bridge_api::cluster_proto::SyncRequest,
    offset: u64,
    out_tx: &mpsc::UnboundedSender<ProtoEvent>,
) {
    let Some(bank) = experience_bank else {
        tracing::debug!(
            "anti-entropy: SyncRequest received but this node has no experience bank; skipping"
        );
        return;
    };

    let requested = anti_entropy::parse_sync_request_payload(request);
    match anti_entropy::serve_sync_request(
        bank,
        &requested,
        offset as usize,
        anti_entropy::SYNC_PAGE_SIZE,
    )
    .await
    {
        Ok(page) => {
            let count = page.experiences.len();
            for sync in page.experiences {
                let evt = ProtoEvent {
                    event: Some(
                        sisyphus_bridge_api::cluster_proto::cluster_event::Event::ExperienceSync(
                            sync,
                        ),
                    ),
                };
                if out_tx.send(evt).is_err() {
                    tracing::debug!("anti-entropy: outbound channel closed while serving sync");
                    return;
                }
            }
            // 页尾标记必须在该页所有差量之后发出，否则请求方会提前翻页，
            // 造成后续页与本页交错。
            let end = ProtoEvent {
                event: Some(
                    sisyphus_bridge_api::cluster_proto::cluster_event::Event::SyncPageEnd(
                        sisyphus_bridge_api::cluster_proto::SyncPageEnd {
                            more: page.more,
                            next_offset: page.next_offset,
                            delivered: count as u32,
                        },
                    ),
                ),
            };
            let _ = out_tx.send(end);
            tracing::info!(
                sent = count,
                more = page.more,
                next_offset = page.next_offset,
                "anti-entropy: replied to sync request"
            );
        },
        Err(e) => {
            // 反熵失败不影响实时广播与本地写入，只记日志。
            tracing::warn!(error = %e, "anti-entropy: failed to serve sync request");
        },
    }
}

/// Manager ClusterEvent → proto ClusterEvent（部分子集，不含 inbound only 事件）
pub(crate) fn mgr_event_to_proto(evt: MgrEvent) -> Option<ProtoEvent> {
    match evt {
        MgrEvent::PeerJoined(p) => Some(ProtoEvent {
            event: Some(
                sisyphus_bridge_api::cluster_proto::cluster_event::Event::PeerJoined(PeerJoined {
                    peer: Some(ProtoPeerInfo {
                        node_id: p.node_id,
                        grpc_addr: p.grpc_addr,
                        joined_at: p.joined_at,
                    }),
                }),
            ),
        }),
        MgrEvent::PeerLeft { node_id, reason } => Some(ProtoEvent {
            event: Some(
                sisyphus_bridge_api::cluster_proto::cluster_event::Event::PeerLeft(PeerLeft {
                    node_id,
                    reason,
                }),
            ),
        }),
        MgrEvent::StateProposal { key, value } => Some(ProtoEvent {
            event: Some(
                sisyphus_bridge_api::cluster_proto::cluster_event::Event::StateProposal(
                    StateProposal {
                        key,
                        value,
                        vector_clock: Default::default(),
                    },
                ),
            ),
        }),
        MgrEvent::ExperienceSync {
            exp_key,
            version,
            vector_clock_json,
            payload,
        } => Some(ProtoEvent {
            event: Some(
                sisyphus_bridge_api::cluster_proto::cluster_event::Event::ExperienceSync(
                    ExperienceSync {
                        exp_key,
                        version,
                        vector_clock_json,
                        payload,
                    },
                ),
            ),
        }),
    }
}

/// proto ClusterEvent → Manager ClusterEvent
pub(crate) fn proto_to_mgr_event(evt: ProtoEvent) -> Option<MgrEvent> {
    use sisyphus_bridge_api::cluster_proto::cluster_event::Event;
    evt.event.and_then(|e| match e {
        Event::PeerJoined(p) => {
            p.peer.map(|info| MgrEvent::PeerJoined(PeerInfo::new(info.node_id, info.grpc_addr)))
        },
        Event::PeerLeft(pl) => Some(MgrEvent::PeerLeft {
            node_id: pl.node_id,
            reason: pl.reason,
        }),
        Event::StateProposal(_) => None, // state proposal inbound handled separately
        Event::ExperienceSync(e) => Some(MgrEvent::ExperienceSync {
            exp_key: e.exp_key,
            version: e.version,
            vector_clock_json: e.vector_clock_json,
            payload: e.payload,
        }),
        // ADR-010 控制消息，不是成员/经验事件：与 `is_page_end` 一样不进入
        // 事件总线，由调用方在转 manager 之前就分流掉。
        Event::SyncRequest(_) | Event::SyncPageEnd(_) => None,
    })
}

/// 应用一个 manager 事件
pub(crate) fn apply_mgr_event(manager: &PeerManager, evt: MgrEvent) {
    match evt {
        MgrEvent::PeerJoined(p) => {
            let _ = manager.on_peer_joined(p);
        },
        MgrEvent::PeerLeft { node_id, reason } => {
            let _ = manager.on_peer_left(&node_id, reason);
        },
        MgrEvent::StateProposal { .. } => {
            // 状态提案尚无消费方（TODO 8.3.3 共识），保持丢弃。
        },
        MgrEvent::ExperienceSync {
            exp_key,
            version,
            vector_clock_json,
            payload,
        } => {
            // 投递到本地事件总线，由 ExperienceSyncHandler 按 CRDT 合并落库。
            // 此前这里直接丢弃，跨节点经验同步实际从未打通。
            // 注意用 deliver_ 而非 broadcast_：已经来自远端，再转发会形成回环。
            manager.deliver_inbound_experience_sync(exp_key, version, vector_clock_json, payload);
        },
    }
}

/// 启动 cluster service：把 service impl 注册到 tonic ServerBuilder
///
/// 典型用法：
/// ```no_run
/// # use std::sync::Arc;
/// # use std::time::Duration;
/// # use sisyphus_bridge::peer::manager::PeerManager;
/// # use sisyphus_bridge::peer::service::ClusterServiceImpl;
/// # use sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer;
/// # use tonic::transport::Server;
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let manager = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
/// let service = ClusterServiceImpl::new(manager);
/// Server::builder()
///     .add_service(ClusterServer::new(service))
///     .serve("127.0.0.1:50051".parse()?)
///     .await?;
/// # Ok(())
/// # }
/// ```
/// 此前该示例标记为 ignore（不参与编译），因此连 `Arc` / `Duration` 的
/// 导入都缺失也无人发现。改为 no_run 后它会被编译检查，示例随 API 演进
/// 腐烂时能立刻暴露。
/// tonic 0.12 中 Server::add_service 是 consuming method（返回 Self），无需 helper。
/// 直接调用方链式：Server::builder().add_service(ClusterServer::new(...)).serve(addr)
/// 本函数仅作为文档示例保留。
#[allow(dead_code)]
pub fn add_cluster_service_example(manager: Arc<PeerManager>) -> ClusterServiceImpl {
    ClusterServiceImpl::new(manager)
}

// Re-export for ergonomic usage
pub use sisyphus_bridge_api::cluster_proto::cluster_server::ClusterServer;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_mgr_event_to_proto_peer_joined() {
        let mgr = PeerManager::new("node-a", Duration::from_secs(30));
        let evt = MgrEvent::PeerJoined(PeerInfo::new("node-b", "addr-b:50051"));
        let proto = mgr_event_to_proto(evt).expect("should convert");
        // 验证：proto 包含 PeerJoined
        match proto.event {
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::PeerJoined(p)) => {
                let info = p.peer.expect("peer info");
                assert_eq!(info.node_id, "node-b");
                assert_eq!(info.grpc_addr, "addr-b:50051");
            },
            _ => panic!("expected PeerJoined"),
        }
        let _ = mgr; // silence unused
    }

    #[test]
    fn test_mgr_event_to_proto_peer_left() {
        let evt = MgrEvent::PeerLeft {
            node_id: "node-b".into(),
            reason: "shutdown".into(),
        };
        let proto = mgr_event_to_proto(evt).expect("should convert");
        match proto.event {
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::PeerLeft(pl)) => {
                assert_eq!(pl.node_id, "node-b");
                assert_eq!(pl.reason, "shutdown");
            },
            _ => panic!("expected PeerLeft"),
        }
    }

    #[test]
    fn test_mgr_event_to_proto_state_proposal() {
        let evt = MgrEvent::StateProposal {
            key: "k".into(),
            value: vec![1, 2, 3],
        };
        let proto = mgr_event_to_proto(evt).expect("should convert");
        match proto.event {
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::StateProposal(sp)) => {
                assert_eq!(sp.key, "k");
                assert_eq!(sp.value, vec![1, 2, 3]);
            },
            _ => panic!("expected StateProposal"),
        }
    }

    #[test]
    fn test_mgr_event_to_proto_experience_sync() {
        let evt = MgrEvent::ExperienceSync {
            exp_key: "exp:123".into(),
            version: 5,
            vector_clock_json: r#"{"a":3}"#.into(),
            payload: vec![10, 20, 30],
        };
        let proto = mgr_event_to_proto(evt).expect("should convert");
        match proto.event {
            Some(sisyphus_bridge_api::cluster_proto::cluster_event::Event::ExperienceSync(es)) => {
                assert_eq!(es.exp_key, "exp:123");
                assert_eq!(es.version, 5);
                assert_eq!(es.vector_clock_json, r#"{"a":3}"#);
                assert_eq!(es.payload, vec![10, 20, 30]);
            },
            _ => panic!("expected ExperienceSync"),
        }
    }

    #[test]
    fn test_proto_to_mgr_event_peer_joined() {
        let proto = ProtoEvent {
            event: Some(
                sisyphus_bridge_api::cluster_proto::cluster_event::Event::PeerJoined(PeerJoined {
                    peer: Some(ProtoPeerInfo {
                        node_id: "node-c".into(),
                        grpc_addr: "addr-c:50051".into(),
                        joined_at: 1000,
                    }),
                }),
            ),
        };
        let mgr = proto_to_mgr_event(proto).expect("should convert");
        match mgr {
            MgrEvent::PeerJoined(p) => {
                assert_eq!(p.node_id, "node-c");
                assert_eq!(p.grpc_addr, "addr-c:50051");
            },
            _ => panic!("expected PeerJoined"),
        }
    }

    #[test]
    fn test_apply_mgr_event_routes_to_manager() {
        let mgr = Arc::new(PeerManager::new("node-a", Duration::from_secs(30)));
        apply_mgr_event(&mgr, MgrEvent::PeerJoined(PeerInfo::new("node-b", "addr")));
        assert_eq!(mgr.cluster_size(), 2);
        apply_mgr_event(
            &mgr,
            MgrEvent::PeerLeft {
                node_id: "node-b".into(),
                reason: "x".into(),
            },
        );
        assert_eq!(mgr.cluster_size(), 1);
    }
}
