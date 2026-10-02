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
    /// ADR-013：已收到的 `SyncRequest` 计数（含空游标的首轮拉取与补齐轮）。
    ///
    /// 存在的理由是**可观测性**：此前无法区分「补齐信号被消费并触发了补齐请求」
    /// 与「信号被 `resubscribe` 静默丢弃、最终靠建连全量拉取补齐」。两者最终
    /// 都收敛，数据上看不出差别，于是丢信号这件事在任何测试里都不可见。
    /// 有了这个计数，测试才能断言「确实发出了第二次请求」。
    sync_requests_seen: Arc<std::sync::atomic::AtomicU64>,
    /// ADR-013：`Join` 被调用的总次数（含被闸门挡住而挂起的那些）。
    ///
    /// 用于证明「重连确实发生过」：被闸门挡住时第一次 `Join` 会超时失败并进入
    /// 退避重试，计数因此能区分「只尝试过一次」与「已经重试过」。
    join_attempts_seen: Arc<std::sync::atomic::AtomicU64>,
    /// ADR-013：测试用的「强制断流」信号。
    ///
    /// 广播通道：每次 `stream()` 调用各自订阅一份，信号只投递给**当时已建立的**
    /// 流。断流后再建的新流订阅时刻晚于信号，因此不会被误伤——这正是重连用例
    /// 需要的语义（断一次流，而不是让后续重连反复断）。
    stream_break: tokio::sync::broadcast::Sender<()>,
    /// ADR-013：断流**已生效**的确认广播，由 stream 断流分支在退出前发出。
    break_ack_tx: tokio::sync::broadcast::Sender<()>,
    /// 断流已生效的既成事实。标志位用于消除「订阅晚于确认」的竞态。
    break_ack_seen: Arc<std::sync::atomic::AtomicBool>,
    /// ADR-013：Join 闸门标志。为 `false` 时 `Join` 挂起，直到显式置为 `true`。
    ///
    /// 刻意用「布尔标志 + 唤醒通道」而不是 `Semaphore`：闸门需要在**运行期反复
    /// 开/关**——先放行第一次 Join 以建出可用的流，断流后再关闸门把重连的那次
    /// Join 挡住。信号量是单调发放的（许可发出去不会因被取走而「回到 0」），
    /// 无法表达「关」，且 `add_permits` 过大还会 panic。
    ///
    /// 唤醒用**广播**而非 `Notify`：标志位检查与 await 之间可能已有等待者被唤走
    /// （惊群），此时后到的 `notify_one` 会丢失唤醒并让 Join 永久挂起；广播不会
    /// 丢失，且配合「先查标志再 await」的双重检查没有竞态。
    ///
    /// 仅用于制造确定性的建连/断流窗口。生产路径恒为放行（标志初始即 `true`）。
    join_admit: Arc<std::sync::atomic::AtomicBool>,
    /// 闸门翻转时的唤醒广播。容量 1 足够：它是纯粹的「状态已变，请重查」信号，
    /// 真实状态在 `join_admit` 上。
    join_wake: tokio::sync::broadcast::Sender<()>,
}

impl ClusterServiceImpl {
    /// 新建 service impl（不参与反熵）
    pub fn new(manager: Arc<PeerManager>) -> Self {
        Self::with_gate(manager, None)
    }

    /// 新建 service impl，并挂上本节点经验库以提供反熵差量。
    pub fn with_experience_bank(
        manager: Arc<PeerManager>,
        experience_bank: Arc<sisyphus_mem::experience_bank::VectorExperienceBank>,
    ) -> Self {
        Self::with_gate(manager, Some(experience_bank))
    }

    /// 统一构造：闸门默认打开（1 个许可），`bank` 为 `None` 即不参与反熵。
    fn with_gate(
        manager: Arc<PeerManager>,
        experience_bank: Option<Arc<sisyphus_mem::experience_bank::VectorExperienceBank>>,
    ) -> Self {
        // ADR-013：生产路径闸门恒开，因此 Join 不受任何额外延迟影响。
        Self {
            manager,
            experience_bank,
            sync_requests_seen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            join_attempts_seen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            stream_break: tokio::sync::broadcast::Sender::new(1),
            break_ack_tx: tokio::sync::broadcast::Sender::new(1),
            break_ack_seen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            join_admit: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            join_wake: tokio::sync::broadcast::Sender::new(1),
        }
    }

    /// ADR-013：构造一个**闸门关闭**的服务（Join 会一直挂起）。
    ///
    /// 用于测试稳定制造「拨号器已 spawn、但尚未建连」的窗口：在这个窗口里发出
    /// 补齐信号，才能验证信号是跨建连被保留的，而不是被建连那一步的
    /// `resubscribe` 吃掉。
    ///
    /// 无条件编译（而非 `#[cfg(test)]`）：集成测试位于 `tests/`，链接的是**未
    /// 开启 `cfg(test)` 的库**，用 `cfg` 门控会让这个构造函数在测试里根本不存在。
    pub fn gated(
        manager: Arc<PeerManager>,
        experience_bank: Arc<sisyphus_mem::experience_bank::VectorExperienceBank>,
    ) -> (Self, JoinGateHandle) {
        let service = Self {
            manager,
            experience_bank: Some(experience_bank),
            sync_requests_seen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            join_attempts_seen: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            stream_break: tokio::sync::broadcast::Sender::new(1),
            break_ack_tx: tokio::sync::broadcast::Sender::new(1),
            break_ack_seen: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            // 闸门关闭：用例需要先显式放行才能建连。
            join_admit: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            join_wake: tokio::sync::broadcast::Sender::new(1),
        };
        let handle = JoinGateHandle {
            gate: Arc::clone(&service.join_admit),
            wake: service.join_wake.clone(),
            seen: Arc::clone(&service.sync_requests_seen),
            attempts: Arc::clone(&service.join_attempts_seen),
            break_tx: service.stream_break.clone(),
            break_ack_tx: service.break_ack_tx.clone(),
            break_ack_seen: Arc::clone(&service.break_ack_seen),
        };
        (service, handle)
    }

    /// ADR-013：已收到的 `SyncRequest` 总数。
    pub fn sync_requests_seen(&self) -> u64 {
        self.sync_requests_seen.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// ADR-013：闸门与计数的外部句柄，供测试打开闸门并读取请求数。
#[derive(Clone)]
pub struct JoinGateHandle {
    gate: Arc<std::sync::atomic::AtomicBool>,
    wake: tokio::sync::broadcast::Sender<()>,
    seen: Arc<std::sync::atomic::AtomicU64>,
    attempts: Arc<std::sync::atomic::AtomicU64>,
    break_tx: tokio::sync::broadcast::Sender<()>,
    break_ack_tx: tokio::sync::broadcast::Sender<()>,
    break_ack_seen: Arc<std::sync::atomic::AtomicBool>,
}

impl JoinGateHandle {
    /// 放行或关闭 Join 闸门。
    ///
    /// 可**反向**使用（关闸门）是重连用例的前提：先放行以建出流，断流后再关闸门
    /// 把重连的那次 Join 挡住，从而把「断流」与「重连」之间那段窗口稳定暴露出来。
    ///
    /// 唤醒用广播：无等待者时发送失败属于正常状态（此时没有 Join 需要被唤醒），
    /// 因此刻意忽略返回值。任何正在挂起的 Join 都会因这次广播重新检查标志位。
    pub fn set_join_gate(&self, admit: bool) {
        self.gate.store(admit, std::sync::atomic::Ordering::SeqCst);
        let _ = self.wake.send(());
    }

    /// 闸门当前是否放行。
    pub fn join_gate_admitted(&self) -> bool {
        self.gate.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 服务端已收到的 `SyncRequest` 总数。
    pub fn sync_requests_seen(&self) -> u64 {
        self.seen.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 服务端被调用过的 `Join` 总次数（含被闸门挡住而挂起的）。
    pub fn join_attempts_seen(&self) -> u64 {
        self.attempts.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// ADR-013：强制断开**当前已建立**的服务端流，让拨号方回到重连路径。
    ///
    /// 这是重连用例与「建连前」用例的关键区别：后者用 Join 闸门把拨号器挡在
    /// 建连之前，从未有过成功流；本方法则让一条**已经跑通**的流中断，从而制造
    /// 「已建立流 → 断开 → 重连」的真实窗口。
    ///
    /// 广播语义保证只断一次：断流后拨号方重连时，`stream()` 里的新订阅时刻晚于
    /// 本次信号，不会被再次误伤，否则流会反复重建、用例永远等不到稳定态。
    pub fn break_established_stream(&self) {
        let _ = self.break_tx.send(());
    }

    /// ADR-013：等待 `break_established_stream` 真正作用到某条流上。
    ///
    /// 广播的 `send` 只保证信号进了通道，**不保证**对应那条流已经结束。用例必须
    /// 在发信号之前就确认旧流已死：否则洪泛产生的补齐信号可能被**旧流**抢先
    /// 消费掉，信号根本没落在目标窗口内，测的其实是另一条路径。断流分支在退出前
    /// 发一次确认，使「断流已完成」成为可等待的确定事实。
    pub async fn await_stream_break_applied(&self, timeout: std::time::Duration) -> bool {
        let mut ack = self.break_ack_tx.subscribe();
        loop {
            // 先看已发生的事实，再等未来的确认，避免订阅竞态漏掉确认。
            if self.break_ack_seen.load(std::sync::atomic::Ordering::SeqCst) {
                return true;
            }
            // tokio 的 broadcast 接收器没有 `recv_timeout`，用外层超时表达期限。
            match tokio::time::timeout(timeout, ack.recv()).await {
                Ok(Ok(())) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {},
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return false,
                // `timeout` 到期即视为未在期限内生效。
                Err(_) => return false,
            }
        }
    }
}

#[tonic::async_trait]
impl Cluster for ClusterServiceImpl {
    type StreamStream =
        Pin<Box<dyn Stream<Item = Result<ProtoEvent, Status>> + Send + Sync + 'static>>;
    /// Join 握手：返回当前已知 peer 列表 + leader
    async fn join(&self, request: Request<JoinRequest>) -> Result<Response<JoinResponse>, Status> {
        // ADR-013：闸门默认放行（标志初始为 true），这一步不产生任何额外延迟；只有
        // 测试用 `gated` 构造时才会真的挂起。放在最前面是为了让「被闸门挡住」的
        // 时刻精确落在拨号器 spawn 之后、进入 run_stream 之前。
        self.join_attempts_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        // ADR-013：闸门关闭时在此挂起，直到被放行。
        //
        // 「先查标志、再等唤醒」的双重检查不可省：检查通过到真正挂起之间可能已有
        // 一次唤醒广播被其他等待者抢先消费（惊群），只查一次会让 Join 在闸门已经
        // 放行的情况下仍永久等待。
        while !self.join_admit.load(std::sync::atomic::Ordering::SeqCst) {
            let mut wake = self.join_wake.subscribe();
            if self.join_admit.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            // `Lagged` 表示期间错过多次唤醒：闸门是布尔量，重查即可，不需要知道
            // 错过几次。`Closed` 不可能发生——句柄与服务共享同一个发送端。
            match wake.recv().await {
                Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {},
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }

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
        // ADR-013：计数器要 clone 进这个 task（它 'static'，不能借用 self）。
        let sync_requests_seen = Arc::clone(&self.sync_requests_seen);
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
                            // ADR-013：计数在**收到请求**时就自增，早于应答。
                            //
                            // 关键是它要覆盖「拨号器建连时的首次拉取」与「补齐信号触发的
                            // 补齐请求」两种来源且能区分二者：首轮是空游标，随后若因
                            // 补齐信号又发一次，计数就变成 2。信号被 `resubscribe`
                            // 静默吃掉时计数恒为 1——这正是本用例要断言的差异。
                            sync_requests_seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            handle_sync_request(
                                bank_for_inbound.as_ref(),
                                &sync_request,
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

        // ADR-013：测试要求断流时，向拨号方**结束** outbound 流（而不是仅关闭
        // inbound 处理任务）。只有让 `inbound.next()` 在拨号侧返回 `None`，
        // `run_stream` 才会走「inbound stream ended」分支返回 Err，从而确定性地
        // 回到 `run_dial_loop` 的重连路径。用 `select!` 包住原流：断流信号先到就
        // 直接结束，之后原流被 drop。
        let mut break_rx = self.stream_break.subscribe();
        let break_ack_tx = self.break_ack_tx.clone();
        let break_ack_seen = Arc::clone(&self.break_ack_seen);
        let raw = UnboundedReceiverStream::new(out_rx);
        let out_stream = async_stream::stream! {
            let mut raw = raw;
            loop {
                tokio::select! {
                    biased;
                    _ = break_rx.recv() => {
                        // ADR-013：先记既成事实再发确认。任何先检查标志位的
                        // 等待者都会立刻返回，无需依赖广播的投递时机。
                        break_ack_seen.store(true, std::sync::atomic::Ordering::SeqCst);
                        let _ = break_ack_tx.send(());
                        break;
                    },
                    item = raw.next() => match item {
                        Some(evt) => yield Ok(evt),
                        None => break,
                    },
                }
            }
        };
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
    out_tx: &mpsc::UnboundedSender<ProtoEvent>,
) {
    let Some(bank) = experience_bank else {
        tracing::debug!(
            "anti-entropy: SyncRequest received but this node has no experience bank; skipping"
        );
        return;
    };

    let requested = anti_entropy::parse_sync_request_payload(request);
    // ADR-012：游标取主键而非下标。老节点的 `offset` 一律忽略（按空游标
    // 从头回送）：合并路径幂等，重发一轮只浪费带宽，不会造成错漏。
    let cursor = request.cursor.as_str();
    match anti_entropy::serve_sync_request(bank, &requested, cursor, anti_entropy::SYNC_PAGE_SIZE)
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
                            // 已废弃字段恒置 0：老 dial 方读它推进，而新语义下
                            // 「从哪一页继续」由 `next_cursor` 决定。老方读到 0
                            // 会从第一页重来，靠摘要缩短收敛（见 ADR-012）。
                            #[allow(deprecated)]
                            next_offset: 0,
                            next_cursor: page.next_cursor.clone(),
                            delivered: count as u32,
                        },
                    ),
                ),
            };
            let _ = out_tx.send(end);
            tracing::info!(
                sent = count,
                more = page.more,
                next_cursor = %page.next_cursor,
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
