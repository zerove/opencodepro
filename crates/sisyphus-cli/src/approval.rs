//! 审批 broker（T2.2）：后台工具执行在权限判定为 `Ask` 时，向 TUI 请求放行。
//!
//! 流程：后台任务提交审批请求 → TUI 弹窗展示工具与明细 → 用户 `y`/`n` 应答 →
//! 后台任务收到决策继续执行或中止。审批决策同时写入权限审计（`asked-approved` /
//! `asked-denied`），保持与 CLI 模式同一口径。
//!
//! 未注册 TUI（纯 CLI 场景）时提交失败，调用方回退到既有的 stdin 审批路径。

use std::sync::{Mutex, OnceLock};

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

struct Broker {
    tx: UnboundedSender<ApprovalRequest>,
    pending: tokio::sync::mpsc::UnboundedReceiver<ApprovalRequest>,
}

static BROKER: OnceLock<Mutex<Option<Broker>>> = OnceLock::new();

fn broker() -> &'static Mutex<Option<Broker>> {
    BROKER.get_or_init(|| Mutex::new(None))
}

/// 取锁（毒化时取内部值，避免 panic）。
fn lock_broker() -> std::sync::MutexGuard<'static, Option<Broker>> {
    broker().lock().unwrap_or_else(|e| e.into_inner())
}

/// 注册 TUI 审批通道（TUI 启动时调用一次；重复注册会替换旧通道）。
pub fn register() {
    let (tx, pending) = tokio::sync::mpsc::unbounded_channel();
    let mut guard = lock_broker();
    *guard = Some(Broker { tx, pending });
}

/// 是否已注册 TUI 审批通道。
pub fn is_registered() -> bool {
    lock_broker().is_some()
}

/// 提交审批请求并等待用户决策。
///
/// 未注册 TUI 时返回 `Err`，调用方应回退到 CLI 的 stdin 审批路径。
pub async fn request(tool: &str, detail: &str) -> Result<bool, String> {
    let (request, wait) = ApprovalRequest::create(tool, detail);
    {
        let guard = lock_broker();
        let Some(broker) = guard.as_ref() else {
            return Err("tui approval channel not registered".to_string());
        };
        broker.tx.send(request).map_err(|_| "tui approval channel closed".to_string())?;
    }
    wait.await.map_err(|_| "tui approval request dropped".to_string())
}

/// 取出下一个待审批请求（TUI 事件循环轮询）。
pub fn take_pending() -> Option<ApprovalRequest> {
    let mut guard = lock_broker();
    guard.as_mut().and_then(|broker| broker.pending.try_recv().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn request_without_register_returns_error() {
        // 未注册时提交失败（调用方回退 CLI 审批）
        let err = request("exec", "echo hi").await;
        assert!(err.is_err(), "未注册 TUI 时应返回错误");
    }

    #[tokio::test]
    async fn registered_channel_delivers_request_and_response() {
        register();
        assert!(is_registered());

        let handle = tokio::spawn(async {
            request("exec", "echo hi").await.expect("审批请求应被受理")
        });

        // 轮询取出待审批请求
        let mut pending = None;
        for _ in 0..50 {
            if let Some(req) = take_pending() {
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
}
