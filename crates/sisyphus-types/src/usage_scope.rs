//! 执行级 token 用量作用域（task-local）。
//!
//! 背景：LLM 客户端只维护**进程级**累计 usage，单次执行（goal/SOP/多节点 DAG）
//! 的 token 成本无法确定性归因 —— 并发执行时「全局差值」会互相污染。
//! 这里提供一个可选的作用域：LLM 客户端在记录 provider 上报 usage 时，
//! 若当前任务存在活动作用域，则**同时**写入该作用域；执行入口在作用域结束时
//! 直接读取作用域快照，得到该次执行的确定性用量（不依赖全局差值）。
//!
//! 为什么放在 `sisyphus-types` 而不是 LLM 客户端 crate：作用域需要被
//! **执行编排层**（`sisyphus-core` 的 DAG 执行器）在 `tokio::spawn` /
//! `JoinSet::spawn` 边界显式传播，而 core 在生产代码中不依赖具体 LLM 客户端
//! crate（那是 dev-dependency）。放在共享类型层可让「记录方」（客户端）与
//! 「编排方」（执行器）共用同一 task-local。
//!
//! 使用约定：
//! - 入口用 [`with_scope`] 包裹整个执行 future，结束后读取返回的用量快照；
//! - 任何**可能发起 LLM 调用的** `tokio::spawn` / `JoinSet::spawn` 边界，
//!   必须在 spawn 前 [`captured_scope`]，并在子任务里用 [`attach_scope`]
//!   重新进入作用域，否则该子任务的调用无法归因（`tokio::task_local!`
//!   不会自动继承到新任务）；
//! - 快照在「从未收到上报 usage」时为 `None`，**未知 ≠ 0**，调用方不得补 0。

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::llm_traits::LlmTokenUsage;

/// 一个执行作用域内的 provider 上报 usage 累加器。
///
/// 全部字段为原子计数，可被同一作用域内的多个并发子任务同时写入。
#[derive(Debug, Default)]
pub struct UsageScope {
    prompt_tokens: AtomicU64,
    completion_tokens: AtomicU64,
    calls: AtomicU64,
}

impl UsageScope {
    /// 记录一次上报。两个字段都为 `None` 时视为无可记录信息（直接返回）。
    ///
    /// 只补写了上报的字段：只有一个字段上报时也只累加那一个，不伪造另一个。
    pub fn record(&self, prompt: Option<u64>, completion: Option<u64>) {
        if prompt.is_none() && completion.is_none() {
            return;
        }
        if let Some(v) = prompt {
            self.prompt_tokens.fetch_add(v, Ordering::Relaxed);
        }
        if let Some(v) = completion {
            self.completion_tokens.fetch_add(v, Ordering::Relaxed);
        }
        self.calls.fetch_add(1, Ordering::Relaxed);
    }

    /// 当前累计值（可能为全 0，表示尚未收到任何上报）。
    pub fn snapshot(&self) -> LlmTokenUsage {
        LlmTokenUsage {
            prompt_tokens: self.prompt_tokens.load(Ordering::Relaxed),
            completion_tokens: self.completion_tokens.load(Ordering::Relaxed),
            calls: self.calls.load(Ordering::Relaxed),
        }
    }

    /// 转为对外可见快照：从未收到 provider 上报 usage 时返回 `None`（未知 ≠ 0）。
    pub fn snapshot_option(&self) -> Option<LlmTokenUsage> {
        let snap = self.snapshot();
        (snap.calls > 0).then_some(snap)
    }
}

tokio::task_local! {
    /// 当前任务的活动执行作用域。由 [`with_scope`] 激活；新 spawn 的任务不会继承。
    static CURRENT_SCOPE: Arc<UsageScope>;
}

/// 取当前任务的作用域句柄（若不在作用域内则为 `None`）。
///
/// 用于在 `tokio::spawn` / `JoinSet::spawn` **之前**捕获，随后在子任务里
/// 通过 [`attach_scope`] 重新进入作用域。
pub fn captured_scope() -> Option<Arc<UsageScope>> {
    CURRENT_SCOPE.try_with(Arc::clone).ok()
}

/// 把 future 挂到给定的作用域上运行；`scope` 为 `None` 时原样运行（不新建作用域）。
///
/// 这是一个组合器，便于在 spawn 点写成
/// `join_set.spawn(attach_scope(captured_scope(), async move { ... }))`，
/// 而不必改动子任务内部结构。
pub async fn attach_scope<Fut>(scope: Option<Arc<UsageScope>>, fut: Fut) -> Fut::Output
where
    Fut: Future,
{
    match scope {
        Some(scope) => CURRENT_SCOPE.scope(scope, fut).await,
        None => fut.await,
    }
}

/// 在当前作用域内运行 future，并返回作用域内累计的 usage 快照。
///
/// - 作用域覆盖到的 LLM 调用会被计入返回值；
/// - 作用域内从未收到 provider 上报 usage 时返回 `None`（未知 ≠ 0）；
/// - 嵌套调用会新建独立作用域，不与外层共享（内层用量不会重复计入外层）。
pub async fn with_scope<Fut, T>(fut: Fut) -> (T, Option<LlmTokenUsage>)
where
    Fut: Future<Output = T>,
{
    let scope = Arc::new(UsageScope::default());
    let result = CURRENT_SCOPE.scope(Arc::clone(&scope), fut).await;
    (result, scope.snapshot_option())
}

/// 当前作用域的用量快照（不在作用域内或无上报时为 `None`）。
pub fn current_snapshot() -> Option<LlmTokenUsage> {
    CURRENT_SCOPE.try_with(|scope| scope.snapshot_option()).ok().flatten()
}

/// 把一次 usage 上报写入当前作用域（不在作用域内时静默忽略）。
///
/// 由 LLM 客户端在记录 provider 上报值时调用；全局累计与作用域累加应同时进行，
/// 以便「作用域缺失」时仍保留进程级观测。
pub fn record_current(prompt: Option<u64>, completion: Option<u64>) {
    let _ = CURRENT_SCOPE.try_with(|scope| scope.record(prompt, completion));
}

/// 继承当前作用域 spawn 一个任务。
///
/// 与 `tokio::spawn` 唯一的区别：子任务在**捕获到的**父作用域内运行，因而其
/// LLM 调用会被计入该执行作用域。父任务不在作用域内时等价于普通 `tokio::spawn`。
pub fn spawn_in_scope<Fut>(fut: Fut) -> tokio::task::JoinHandle<Fut::Output>
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    let scope = captured_scope();
    tokio::spawn(attach_scope(scope, fut))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn with_scope_captures_recorded_tokens() {
        let (_, usage) = with_scope(async {
            record_current(Some(11), Some(7));
        })
        .await;

        let usage = usage.expect("作用域内应有上报");
        assert_eq!(usage.prompt_tokens, 11);
        assert_eq!(usage.completion_tokens, 7);
        assert_eq!(usage.calls, 1);
    }

    #[tokio::test]
    async fn with_scope_is_unknown_without_reported_usage() {
        let (_, usage) = with_scope(async {
            // provider 未上报：两个字段都是 None，不产生 calls。
            record_current(None, None);
        })
        .await;

        assert!(usage.is_none(), "无上报时应为 None（未知 ≠ 0）");
        assert!(current_snapshot().is_none(), "作用域外不应残留作用域");
    }

    #[tokio::test]
    async fn partial_report_counts_as_one_call() {
        let (_, usage) = with_scope(async {
            // 只上报 completion 的 provider 也应计入（不伪造 prompt=0）。
            record_current(None, Some(5));
        })
        .await;

        let usage = usage.expect("部分上报也算一次有效上报");
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 5);
        assert_eq!(usage.calls, 1);
    }

    #[tokio::test]
    async fn spawn_in_scope_propagates_to_child_task() {
        let (_, usage) = with_scope(async {
            // 模拟执行编排层：LLM 调用发生在 spawn 出来的子任务里。
            let handle = spawn_in_scope(async {
                tokio::task::yield_now().await;
                record_current(Some(3), Some(4));
            });
            handle.await.expect("child task");
        })
        .await;

        let usage = usage.expect("子任务的上报应计入父作用域");
        assert_eq!(usage.prompt_tokens, 3);
        assert_eq!(usage.completion_tokens, 4);
        assert_eq!(usage.calls, 1);
    }

    #[tokio::test]
    async fn plain_spawn_does_not_inherit_scope() {
        // 反向对照：普通 tokio::spawn 不继承 task-local，因此该用量不会被归因到
        // 父作用域 —— 这正是编排层必须显式传播的原因。
        let (_, usage) = with_scope(async {
            let handle = tokio::spawn(async {
                record_current(Some(9), Some(9));
            });
            handle.await.expect("child task");
        })
        .await;

        assert!(usage.is_none(), "未传播作用域的子任务不应污染父作用域");
    }

    #[tokio::test]
    async fn attach_scope_runs_without_scope_when_none() {
        // JoinSet/spawn 点在「父任务不在作用域内」时必须保持原语义（只是执行）。
        let mut ran = false;
        attach_scope(None, async {
            ran = true;
        })
        .await;
        assert!(ran);
        assert!(current_snapshot().is_none());
    }

    #[tokio::test]
    async fn concurrent_scopes_are_isolated() {
        let f1 = with_scope(async {
            tokio::task::yield_now().await;
            record_current(Some(10), Some(1));
        });
        let f2 = with_scope(async {
            tokio::task::yield_now().await;
            record_current(Some(20), Some(2));
        });
        let ((_, u1), (_, u2)) = tokio::join!(f1, f2);

        let u1 = u1.expect("scope1");
        let u2 = u2.expect("scope2");
        assert_eq!(u1.prompt_tokens, 10);
        assert_eq!(u1.completion_tokens, 1);
        assert_eq!(u2.prompt_tokens, 20);
        assert_eq!(u2.completion_tokens, 2);
    }

    #[tokio::test]
    async fn nested_scope_does_not_leak_into_outer() {
        let (_, outer) = with_scope(async {
            record_current(Some(1), None);
            let (_, inner) = with_scope(async {
                record_current(Some(100), Some(100));
            })
            .await;
            assert_eq!(inner.expect("inner usage").prompt_tokens, 100);
        })
        .await;

        let outer = outer.expect("outer usage");
        assert_eq!(outer.prompt_tokens, 1, "内层作用域用量不得计入外层");
        assert_eq!(outer.completion_tokens, 0);
    }
}
