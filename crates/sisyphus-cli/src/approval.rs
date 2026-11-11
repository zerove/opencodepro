//! 审批 broker（T2.2）：后台工具执行在权限判定为 `Ask` 时，向 TUI 请求放行。
//!
//! 流程：后台任务提交审批请求 → TUI 弹窗展示工具与明细 → 用户 `y`/`n` 应答 →
//! 后台任务收到决策继续执行或中止。审批决策同时写入权限审计（`asked-approved` /
//! `asked-denied`），保持与 CLI 模式同一口径。
//!
//! 未注册 TUI（纯 CLI 场景）时提交失败，调用方回退到既有的 stdin 审批路径。
//!
//! broker 拆成两端：`Requester`（提交请求，可移动到后台任务）与 `Broker`
//! （消费待审批项，由 TUI 事件循环轮询）。全局实例由 [`register`] 安装，
//! 测试可创建独立实例，避免并行测试之间共享状态。

use std::sync::{Mutex, MutexGuard, OnceLock};

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot::Sender;

/// 一次待审批请求（含应答通道）。
#[derive(Debug)]
pub struct ApprovalRequest {
    /// 触发审批的工具标识（如 `exec`）。
    pub tool: String,
    /// 面向用户的补充说明（命令文本等）。
    pub detail: String,
    /// 应答通道：放行 `true` / 拒绝 `false`。
    respond: Sender<bool>,
}

impl ApprovalRequest {
    /// 提交用户决策；接收端已消失时静默忽略。
    pub fn respond(self, approve: bool) {
        let _ = self.respond.send(approve);
    }

    /// 构造审批请求并返回应答接收端（供测试与非 broker 调用方使用）。
    pub fn create(
        tool: &str,
        detail: &str,
    ) -> (ApprovalRequest, tokio::sync::oneshot::Receiver<bool>) {
        let (respond, wait) = tokio::sync::oneshot::channel::<bool>();
        (
            ApprovalRequest {
                tool: tool.to_string(),
                detail: detail.to_string(),
                respond,
            },
            wait,
        )
    }
}

/// 审批请求端：后台任务用它提交请求并等待用户决策。
#[derive(Clone)]
pub struct Requester {
    tx: UnboundedSender<ApprovalRequest>,
}

impl Requester {
    /// 提交审批请求并等待用户决策。
    ///
    /// 通道关闭（消费端已释放）时返回 `Err`。
    pub async fn request(&self, tool: &str, detail: &str) -> Result<bool, String> {
        let (request, wait) = ApprovalRequest::create(tool, detail);
        self.tx.send(request).map_err(|_| "tui approval channel closed".to_string())?;
        wait.await.map_err(|_| "tui approval request dropped".to_string())
    }
}

/// 审批消费端：TUI 事件循环用它取出待审批项。
pub struct Broker {
    pending_rx: UnboundedReceiver<ApprovalRequest>,
}

impl Broker {
    /// 创建 broker 两端。
    pub fn new() -> (Broker, Requester) {
        let (tx, pending_rx) = tokio::sync::mpsc::unbounded_channel();
        (Broker { pending_rx }, Requester { tx })
    }

    /// 取出下一个待审批请求。
    pub fn take_pending(&mut self) -> Option<ApprovalRequest> {
        self.pending_rx.try_recv().ok()
    }
}

struct GlobalState {
    broker: Broker,
    requester: Requester,
    /// 是否已由 TUI 显式注册。
    ///
    /// 必须显式标记：全局 broker 是**惰性**创建的，若用「通道是否关闭」推断注册状态，
    /// 未注册时也会被判为已注册 —— CLI 模式下的 `shell::exec` 会因此走进 TUI 审批分支，
    /// 等待一个永远不会到来的应答（挂死）。
    registered: bool,
}

static GLOBAL: OnceLock<Mutex<GlobalState>> = OnceLock::new();

fn global() -> &'static Mutex<GlobalState> {
    GLOBAL.get_or_init(|| {
        let (broker, requester) = Broker::new();
        Mutex::new(GlobalState {
            broker,
            requester,
            registered: false,
        })
    })
}

/// 取锁（毒化时取内部值，避免 panic）。
fn lock_global() -> MutexGuard<'static, GlobalState> {
    global().lock().unwrap_or_else(|e| e.into_inner())
}

/// 注册 TUI 审批通道（TUI 启动时调用一次；重复注册会替换旧通道）。
pub fn register() {
    let (broker, requester) = Broker::new();
    let mut guard = lock_global();
    *guard = GlobalState {
        broker,
        requester,
        registered: true,
    };
}

/// 是否已注册 TUI 审批通道（仅 TUI 启动时调用 [`register`] 后为真）。
pub fn is_registered() -> bool {
    lock_global().registered
}

/// 提交审批请求并等待用户决策（走全局实例）。
///
/// 未注册 TUI 时返回 `Err`，调用方应回退到 CLI 的 stdin 审批路径。
pub async fn request(tool: &str, detail: &str) -> Result<bool, String> {
    let requester = {
        let guard = lock_global();
        resolve_requester(&guard)?
    };
    requester.request(tool, detail).await
}

/// 从全局状态取出可用的请求端；未注册时返回错误（调用方回退到 CLI 审批路径）。
fn resolve_requester(state: &GlobalState) -> Result<Requester, String> {
    if !state.registered {
        return Err("tui approval channel not registered".to_string());
    }
    Ok(state.requester.clone())
}

/// 取出下一个待审批请求（TUI 事件循环轮询）。
pub fn take_pending() -> Option<ApprovalRequest> {
    lock_global().broker.take_pending()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broker_delivers_request_and_response() {
        // 使用独立实例，避免与其它测试共享全局状态
        let (mut broker, requester) = Broker::new();
        let handle = tokio::spawn(async move {
            requester.request("exec", "echo hi").await.expect("审批请求应被受理")
        });

        let mut pending = None;
        for _ in 0..50 {
            if let Some(req) = broker.take_pending() {
                pending = Some(req);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let req = pending.expect("应取到待审批请求");
        assert_eq!(req.tool, "exec");
        assert_eq!(req.detail, "echo hi");
        req.respond(true);

        let approved = handle.await.expect("任务应结束");
        assert!(approved, "用户放行应返回 true");
    }

    #[test]
    fn unregistered_state_has_no_requester() {
        // 回归：全局 broker 是惰性创建的，若「是否注册」推断错了，
        // CLI 模式的 shell::exec 会走进 TUI 审批分支并永久等待应答。
        let (broker, requester) = Broker::new();
        let state = GlobalState {
            broker,
            requester,
            registered: false,
        };
        assert!(
            resolve_requester(&state).is_err(),
            "未注册时不应拿到请求端（否则会挂死等待应答）"
        );

        let (broker, requester) = Broker::new();
        let state = GlobalState {
            broker,
            requester,
            registered: true,
        };
        assert!(resolve_requester(&state).is_ok(), "已注册时应拿到请求端");
    }

    #[tokio::test]
    async fn requester_fails_when_consumer_dropped() {
        let (broker, requester) = Broker::new();
        drop(broker);
        let err = requester.request("exec", "echo hi").await;
        assert!(err.is_err(), "消费端释放后应返回错误");
    }
}
