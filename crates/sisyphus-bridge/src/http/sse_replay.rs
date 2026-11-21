//! SSE 断线重连的事件重放（M1.3）。
//!
//! `/api/events` 的 `Last-Event-ID` 语义要求：事件在**发布时**获得全局
//! 单调 ID，客户端断线重连时携带最后收到的 ID，服务端把该 ID 之后的事件
//! 补发。本模块提供三件事：
//!
//! - [`SseReplayBuffer`]：有界环形缓冲，保存最近 `capacity` 条已发布事件
//!   （单调递增 ID）。缓冲有界：超出容量后旧事件淘汰，重放退化为
//!   「尽力补发仍然在缓冲内的事件」。
//! - [`PublishedSseEvent`]：`{ id, event }` —— 广播通道里传输的单元。
//!   ID 必须在发布时确定：订阅侧无法在收到事件后反推它的全局序号。
//! - [`SsePublisher`]：唯一写入口。**先记录后广播**，保证订阅者收到的
//!   事件一定能被后续重连者按 ID 找回（只要仍在缓冲窗口内）。
//!
//! ## 为什么 ID 不能由订阅端生成
//! 旧实现每条连接对每条事件生成随机 UUID，客户端重连携带的 ID 与任何
//! 服务端状态都无法对应，`last_event_id` 因此只能解析、不能生效。ID 必须
//! 是**发布侧**的全局序号：同一条事件对所有订阅者、以及重连后的补发，
//! 都必须使用同一个 ID。
//!
//! ## 重启语义
//! 序号在进程内从 1 开始单调递增。服务重启后序号归零：若客户端携带的
//! `Last-Event-ID` 大于当前最新序号（来自上个进程），历史不可用，连接从
//! 实时流继续，避免把新事件永久跳过。若客户端携带的是旧版本产生的 UUID，
//! 解析失败按「未提供」处理（与旧行为一致：只接收实时事件）。

use std::collections::VecDeque;
use std::ops::Deref;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::broadcast;

use crate::http::sse_handlers::SseEvent;

/// 默认重放缓冲容量（条）。
///
/// 容量决定断线窗口：按每秒 100 条事件估算，256 条约覆盖 2.5 秒；
/// 需要更长窗口时通过 `SsePublisher::new` 显式传入更大值。
pub const SSE_REPLAY_CAPACITY: usize = 256;

/// 广播通道里传输的事件：全局序号 + 原始事件。
///
/// `Deref<Target = SseEvent>` 让既有订阅侧（方法调用 / `&SseEvent` 参数）
/// 无需逐个改写成 `.event`；需要所有权时用 [`PublishedSseEvent::into_event`]。
#[derive(Debug, Clone)]
pub struct PublishedSseEvent {
    /// 发布侧全局单调 ID（从 1 开始，只在当前进程内有效）。
    pub id: u64,
    /// 原始 SSE 事件（类型、payload、维度均未改动）。
    pub event: SseEvent,
}

impl PublishedSseEvent {
    /// 构造一条带序号的事件。
    pub fn new(id: u64, event: SseEvent) -> Self {
        Self { id, event }
    }

    /// 取出原始事件，丢弃序号。
    pub fn into_event(self) -> SseEvent {
        self.event
    }
}

impl Deref for PublishedSseEvent {
    type Target = SseEvent;

    fn deref(&self) -> &SseEvent {
        &self.event
    }
}

impl AsRef<SseEvent> for PublishedSseEvent {
    fn as_ref(&self) -> &SseEvent {
        &self.event
    }
}

/// 互斥保护的重放状态。
#[derive(Debug)]
struct ReplayInner {
    /// 下一条事件将获得的序号。
    next_id: u64,
    /// 仍在窗口内的事件（按发布顺序，最旧在前）。
    entries: VecDeque<PublishedSseEvent>,
}

/// 有界事件重放缓冲（见模块文档）。
#[derive(Debug)]
pub struct SseReplayBuffer {
    capacity: usize,
    inner: Mutex<ReplayInner>,
}

impl SseReplayBuffer {
    /// 创建容量为 `capacity` 的缓冲（至少保留 1 条，`0` 归一化为 `1`）。
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: Mutex::new(ReplayInner {
                next_id: 1,
                entries: VecDeque::new(),
            }),
        }
    }

    /// 锁中毒（持锁线程 panic）不是数据损坏：内部只是「序号 + 队列」，
    /// 继续使用可取回的最后一致状态，避免整个事件流因一次 panic 永久失效。
    fn lock(&self) -> MutexGuard<'_, ReplayInner> {
        match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 记录事件并分配全局单调 ID（只记录，不广播）。
    pub fn record(&self, event: SseEvent) -> PublishedSseEvent {
        let mut inner = self.lock();
        let id = inner.next_id;
        // saturating_add：u64 溢出在实际运行中不可达，但生产代码不 panic。
        inner.next_id = inner.next_id.saturating_add(1);
        let published = PublishedSseEvent { id, event };
        inner.entries.push_back(published.clone());
        while inner.entries.len() > self.capacity {
            inner.entries.pop_front();
        }
        published
    }

    /// 返回 ID **严格大于** `last_id` 的事件（按发布顺序）。
    ///
    /// `None` 表示客户端未声明断点（新连接）：返回空，避免把历史事件
    /// 重播给每个新客户端。
    pub fn since(&self, last_id: Option<u64>) -> Vec<PublishedSseEvent> {
        let Some(last_id) = last_id else {
            return Vec::new();
        };
        let inner = self.lock();
        inner.entries.iter().filter(|entry| entry.id > last_id).cloned().collect()
    }

    /// 当前最新序号（从未发布过事件时为 0）。
    pub fn latest_id(&self) -> u64 {
        self.lock().next_id.saturating_sub(1)
    }

    /// 缓冲内事件条数。
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// 缓冲是否为空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 缓冲容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// 一次发布的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishOutcome {
    /// 本次事件的全局序号（写入 SSE `id:` 字段）。
    pub id: u64,
    /// 广播时活跃的订阅者数量（`0` = 当前无人订阅）。
    pub receivers: usize,
}

/// SSE 发布器：广播通道 + 重放缓冲的唯一写入口。
///
/// 克隆代价很低（`broadcast::Sender` + `Arc`），可随 `RestApiState` 复制。
#[derive(Clone)]
pub struct SsePublisher {
    tx: broadcast::Sender<PublishedSseEvent>,
    replay: Arc<SseReplayBuffer>,
}

impl SsePublisher {
    /// 创建发布器：广播通道容量 `channel_capacity`，重放窗口 `replay_capacity`。
    pub fn new(channel_capacity: usize, replay_capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(channel_capacity.max(1));
        Self::with_parts(tx, Arc::new(SseReplayBuffer::new(replay_capacity)))
    }

    /// 用已有的通道/缓冲组装（测试或需要共享 buffer 时使用）。
    pub fn with_parts(
        tx: broadcast::Sender<PublishedSseEvent>,
        replay: Arc<SseReplayBuffer>,
    ) -> Self {
        Self { tx, replay }
    }

    /// 订阅实时事件流（返回的接收端只收到**此后**发布的事件）。
    pub fn subscribe(&self) -> broadcast::Receiver<PublishedSseEvent> {
        self.tx.subscribe()
    }

    /// 发布事件：先在重放缓冲分配 ID，再广播同一条 `{ id, event }`。
    ///
    /// 顺序不可交换 —— 先广播后记录会让「已收到但无法重放」的窗口存在，
    /// 客户端重连时恰好携带该 ID 就会丢失后续事件。
    pub fn publish(&self, event: SseEvent) -> PublishOutcome {
        let published = self.replay.record(event);
        let id = published.id;
        // send 失败仅表示当前无订阅者，属正常情况（没人订阅不该让发布失败）。
        let receivers = self.tx.send(published).unwrap_or(0);
        PublishOutcome { id, receivers }
    }

    /// 断点重放：ID 严格大于 `last_id` 的缓冲事件（`None` 返回空）。
    pub fn replay_since(&self, last_id: Option<u64>) -> Vec<PublishedSseEvent> {
        self.replay.since(last_id)
    }

    /// 当前最新序号（从未发布过事件时为 0）。
    pub fn latest_id(&self) -> u64 {
        self.replay.latest_id()
    }

    /// 只读访问重放缓冲（健康检查/指标用）。
    pub fn replay(&self) -> &Arc<SseReplayBuffer> {
        &self.replay
    }

    /// 底层通道（测试断言/构造接收端用）。
    pub fn channel(&self) -> &broadcast::Sender<PublishedSseEvent> {
        &self.tx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn custom(event_type: &str) -> SseEvent {
        SseEvent::Custom {
            event_type: event_type.to_string(),
            payload: json!({ "k": event_type }),
            timestamp: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn record_assigns_monotonic_ids_starting_at_one() {
        let buffer = SseReplayBuffer::new(8);
        assert_eq!(buffer.latest_id(), 0, "未发布时最新 ID 为 0");
        assert_eq!(buffer.record(custom("a")).id, 1);
        assert_eq!(buffer.record(custom("b")).id, 2);
        assert_eq!(buffer.latest_id(), 2);
        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn since_returns_only_events_after_the_cursor() {
        let buffer = SseReplayBuffer::new(8);
        for name in ["a", "b", "c"] {
            buffer.record(custom(name));
        }
        let ids: Vec<u64> = buffer.since(Some(1)).iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3], "严格大于游标，按发布顺序");
        assert_eq!(buffer.since(Some(3)).len(), 0);
        // 新连接不带断点：不重播历史，避免每个新客户端都收到旧事件
        assert!(buffer.since(None).is_empty());
    }

    #[test]
    fn bounded_buffer_evicts_oldest_but_keeps_ids_monotonic() {
        let buffer = SseReplayBuffer::new(2);
        for name in ["a", "b", "c"] {
            buffer.record(custom(name));
        }
        assert_eq!(buffer.len(), 2, "容量上限生效");
        assert_eq!(buffer.capacity(), 2);
        let ids: Vec<u64> = buffer.since(Some(0)).iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3], "淘汰最旧事件，但新事件 ID 继续递增");
    }

    #[test]
    fn zero_capacity_still_keeps_latest_event() {
        let buffer = SseReplayBuffer::new(0);
        assert_eq!(buffer.capacity(), 1, "容量 0 归一化为 1");
        buffer.record(custom("a"));
        assert_eq!(buffer.len(), 1);
    }

    #[tokio::test]
    async fn publish_records_and_broadcasts_the_same_id() {
        let publisher = SsePublisher::new(16, 16);
        let mut rx = publisher.subscribe();

        let outcome = publisher.publish(custom("x"));
        assert_eq!(outcome.id, 1);
        assert_eq!(outcome.receivers, 1);

        let got = rx.recv().await.expect("订阅者应收到事件");
        assert_eq!(got.id, 1, "广播侧必须携带与重放缓冲一致的 ID");
        assert_eq!(got.event_type(), "x");
        let replayed = publisher.replay_since(Some(0));
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].id, got.id);
    }

    #[tokio::test]
    async fn publish_without_subscribers_still_records_for_replay() {
        let publisher = SsePublisher::new(16, 16);
        let outcome = publisher.publish(custom("nobody-listening"));
        assert_eq!(outcome.receivers, 0, "无订阅者不应视为发布失败");
        // 关键：无人订阅时的事件仍必须可被后续重连补发
        let replayed = publisher.replay_since(Some(0));
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].event_type(), "nobody-listening");
    }
}
