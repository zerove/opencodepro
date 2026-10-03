//! Sisyphus WASM SDK - Tool development interface for WASM-based SOP nodes.
//!
//! This crate provides the types and registration mechanism for tools that
//! run inside the Sisyphus WASM sandbox. Tool authors implement [`SisyphusTool`]
//! and register their tools via [`register_tool`] so the runtime can discover
//! and execute them.

use metrics::counter;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

// ── Types ────────────────────────────────────────────────────────────────

/// Execution context passed to a tool when it is invoked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    pub tool_id: String,
    pub input: Value,
    pub state: Value,
}

/// Output produced by a tool after execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub result: Value,
    pub state_update: Option<Value>,
}

impl ToolOutput {
    /// Convenience constructor for a JSON result with no state update.
    pub fn json(val: Value) -> Self {
        Self {
            result: val,
            state_update: None,
        }
    }
}

// ── Tool trait ────────────────────────────────────────────────────────────

/// Trait that every WASM tool must implement.
///
/// Tools are stateless executors: they receive a [`ToolContext`] and return a
/// [`ToolOutput`].  Any mutable state should be communicated through
/// `ToolContext.state` / `ToolOutput.state_update`.
pub trait SisyphusTool: Send + Sync {
    fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String>;
}

// ── Global tool registry ─────────────────────────────────────────────────

/// Thread-safe global registry that stores tool instances keyed by tool id.
///
/// The registry is initialised lazily on first access and lives for the
/// lifetime of the process (intentionally — WASM tools are loaded once and
/// reused).
struct ToolRegistryInner {
    tools: HashMap<String, Arc<dyn SisyphusTool>>,
}

static REGISTRY: OnceLock<Mutex<ToolRegistryInner>> = OnceLock::new();

fn registry() -> &'static Mutex<ToolRegistryInner> {
    REGISTRY.get_or_init(|| {
        Mutex::new(ToolRegistryInner {
            tools: HashMap::new(),
        })
    })
}

/// Register a tool in the global registry.
///
/// The tool is stored under its type name (`std::any::type_name::<T>()`).
/// If a tool with the same id already exists it is silently replaced.
pub fn register_tool<T: SisyphusTool + 'static>(tool: T) {
    let id = std::any::type_name::<T>().to_string();
    let Some(mut reg) = registry().lock().ok() else {
        tracing::error!("tool registry poisoned, register_tool ignored");
        return;
    };
    reg.tools.insert(id, Arc::new(tool));
}

/// Register a tool with an explicit id.
/// Register tool as.
pub fn register_tool_as<T: SisyphusTool + 'static>(tool_id: &str, tool: T) {
    counter!("sisyphus_wasm_sdk_tools_registered_total").increment(1);
    let Some(mut reg) = registry().lock().ok() else {
        tracing::error!("tool registry poisoned, register_tool_as ignored");
        return;
    };
    reg.tools.insert(tool_id.to_string(), Arc::new(tool));
}

/// Retrieve a tool reference by id.
pub fn get_tool(tool_id: &str) -> Option<Arc<dyn SisyphusTool>> {
    let reg = registry().lock().ok()?;
    reg.tools.get(tool_id).cloned()
}

/// List all registered tool ids.
pub fn list_tools() -> Vec<String> {
    registry().lock().ok().map(|reg| reg.tools.keys().cloned().collect()).unwrap_or_default()
}

/// Number of registered tools.
pub fn tool_count() -> usize {
    registry().lock().ok().map(|reg| reg.tools.len()).unwrap_or_default()
}

/// Remove a tool from the registry by id. Returns `true` if it existed.
pub fn remove_tool(tool_id: &str) -> bool {
    registry().lock().ok().map(|mut reg| reg.tools.remove(tool_id).is_some()).unwrap_or(false)
}

// ── 测试专用：注册表的测试级互斥 ────────────────────────────────────────

/// 串行化「清空 → 注册 → 查找」这类**多步**序列。
///
/// 注册表是进程级全局，而单个 API（`register_tool_as` / `get_tool`）各自加锁
/// 是安全的；不安全的是**把多个调用当成一个整体**：`clear()` 之后、断言之前
/// 锁已释放，别的测试插进来清空或删除，本测试刚注册的工具就没了。`Mutex`
/// 保护不了跨调用的不变量——这不是生产缺陷（生产代码不调 `clear`），而是
/// 测试隔离缺陷。
///
/// 因此在测试层用一把独立的互斥把每个用例的整段序列包起来。它与 `REGISTRY`
/// 的锁是两回事，不可互相替代。
#[cfg(test)]
fn registry_test_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let m = TEST_LOCK.get_or_init(|| Mutex::new(()));
    // 中毒（某次用例 panic）不应连累后续用例：取回内部值而不是传播 poison。
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    // 测试隔离互斥（定义见上方 `registry_test_guard`）。
    use super::registry_test_guard;

    struct EchoTool;
    impl SisyphusTool for EchoTool {
        fn execute(&self, ctx: &ToolContext) -> Result<ToolOutput, String> {
            Ok(ToolOutput::json(ctx.input.clone()))
        }
    }

    struct FailingTool;
    impl SisyphusTool for FailingTool {
        fn execute(&self, _ctx: &ToolContext) -> Result<ToolOutput, String> {
            Err("intentional failure".into())
        }
    }

    #[test]
    fn test_register_and_list() {
        // 隔离互斥：整段「清空 → 注册 → 断言」必须作为一个整体。
        let _guard = registry_test_guard();
        // Clear registry for test isolation
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("echo", EchoTool);
        let tools = list_tools();
        assert!(
            tools.contains(&"echo".to_string()),
            "echo should be registered"
        );
    }

    #[test]
    fn test_tool_count() {
        let _guard = registry_test_guard();
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        assert_eq!(tool_count(), 0);
        register_tool_as("count_a", EchoTool);
        register_tool_as("count_b", FailingTool);
        assert_eq!(tool_count(), 2);
    }

    #[test]
    fn test_remove_tool() {
        let _guard = registry_test_guard();
        let id = format!("removable_{}", std::process::id());
        register_tool_as(&id, EchoTool);
        assert!(remove_tool(&id));
        assert!(!remove_tool(&id)); // already removed
    }

    #[test]
    fn test_duplicate_registration_replaces() {
        let _guard = registry_test_guard();
        // Use a unique id to avoid colliding with other tests in the same process.
        let id = format!("dup_{}", std::process::id());

        register_tool_as(&id, EchoTool);
        register_tool_as(&id, FailingTool);

        // The second registration should have replaced the first
        let tool = get_tool(&id).expect("dup should be registered");
        // FailingTool always errors, so the second registration is observable here.
        let ctx = ToolContext {
            tool_id: id.clone(),
            input: serde_json::json!({}),
            state: serde_json::json!(null),
        };
        assert!(
            tool.execute(&ctx).is_err(),
            "dup should be FailingTool, not EchoTool"
        );

        // Clean up so the global registry is not polluted for other tests.
        remove_tool(&id);
    }

    #[test]
    fn test_tool_output_json() {
        let output = ToolOutput::json(serde_json::json!({"status": "ok"}));
        assert_eq!(output.result, serde_json::json!({"status": "ok"}));
        assert!(output.state_update.is_none());
    }

    #[test]
    fn test_tool_output_with_state() {
        let output = ToolOutput {
            result: serde_json::json!("done"),
            state_update: Some(serde_json::json!({"counter": 1})),
        };
        assert!(output.state_update.is_some());
    }

    #[test]
    fn test_get_tool_returns_arc() {
        let _guard = registry_test_guard();
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        register_tool_as("arc_test", EchoTool);
        let tool = get_tool("arc_test");
        assert!(tool.is_some(), "get_tool should find registered tool");
        let tool = tool.unwrap();
        let ctx = ToolContext {
            tool_id: "arc_test".into(),
            input: serde_json::json!({"hello": "world"}),
            state: serde_json::json!({}),
        };
        let output = tool.execute(&ctx).unwrap();
        assert_eq!(output.result, serde_json::json!({"hello": "world"}));
    }

    #[test]
    fn test_get_tool_missing() {
        let _guard = registry_test_guard();
        {
            let mut reg = registry().lock().unwrap();
            reg.tools.clear();
        }

        assert!(get_tool("nonexistent").is_none());
    }

    #[test]
    fn test_tool_context_serialization() {
        let ctx = ToolContext {
            tool_id: "test".into(),
            input: serde_json::json!({"key": "value"}),
            state: serde_json::json!({}),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let deserialized: ToolContext = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool_id, "test");
    }

    /// 回归测试：`清空 → 注册 → 查找` 这个序列必须在并发下互不干扰。
    ///
    /// 这条用例针对的是一个真实缺陷：注册表是**进程级全局**，而多个测试都在
    /// 序列中途释放锁。另一个测试的 `clear()` 或 `remove_tool()` 只要落在
    /// 窗口内，就会把本测试刚注册的工具抹掉。原先这类失败是概率性的
    /// （实测整轮套件约 2.5%，失败点随机落在
    /// `test_register_and_list` / `test_get_tool_returns_arc` /
    /// `test_remove_tool` / `test_duplicate_registration_replaces` 上），
    /// 靠重跑很难稳定复现，于是被当成偶发噪声长期忽略。
    ///
    /// 这里用 `Barrier` 让 N 个线程**同时起跑**再各自跑完整序列，把概率性
    /// 竞争放大成确定性失败：没有互斥时，先注册的线程会被后一个线程的
    /// `clear()` 抹掉（实测 N=8 时约 19/50 次失败）。
    ///
    /// 它自己**也**在测同一件事，因此同样要遵守「整段序列持锁」的约定。
    /// 但**不能**由本用例统一取一次锁：那样 N 个线程之间就被串行化、互相
    /// 等待，测试会失去发现竞争的能力，而且还会死锁（外层持锁不放，线程
    /// 永远等不到锁）。正确形态是每个线程**自己**取锁并持有到序列结束。
    ///
    /// 屏障必须在**取锁之前**等待，否则先到的线程持锁等屏障、后到的线程
    /// 卡在锁上，永远互不相见——直接死锁。
    #[test]
    fn concurrent_registry_sequences_do_not_interfere() {
        // 扫描：不同并发规模下的失败情况。先全部跑完再汇总断言，这样即使某个
        // 规模先失败，其余规模的数据也仍然可见，便于确认竞争窗口有多宽。
        let mut report = Vec::new();
        let mut worst = 0usize;
        for n in [2usize, 4, 8, 16, 32] {
            let mut failures = 0;
            for _ in 0..50 {
                if run_racy_sequence(n).is_err() {
                    failures += 1;
                }
            }
            report.push(format!("N={n:<3} 失败 {failures}/50"));
            worst = worst.max(failures);
        }
        for line in &report {
            println!("{line}");
        }
        assert_eq!(
            worst,
            0,
            "并发下存在竞争失败：「清空→注册→查找」序列没有互斥保护。实测明细：\n{}",
            report.join("\n")
        );
    }

    /// 跑一次「清空 → 注册 → 让出 → 查找」序列，返回被抹掉时的错误。
    ///
    /// 每个线程自己取一次测试互斥并持有到序列结束——这正是修复后的约定。
    fn run_racy_sequence(n: usize) -> Result<(), String> {
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(n));
        let handles: Vec<_> = (0..n)
            .map(|_| {
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    // 顺序要紧：先在屏障上会合（此刻无人持锁），再取互斥。
                    barrier.wait();
                    let _guard = registry_test_guard();
                    let id = format!("iso_{}", std::process::id());
                    {
                        let mut reg = registry().lock().unwrap();
                        reg.tools.clear();
                    }
                    register_tool_as(&id, EchoTool);
                    // 反复让出调度，给其它线程的 clear() 充分插进来的机会。
                    for _ in 0..4 {
                        std::thread::yield_now();
                    }
                    if get_tool(&id).is_some() {
                        Ok(())
                    } else {
                        Err(format!("线程注册被另一线程的 clear() 抹掉（id={id}）"))
                    }
                })
            })
            .collect();
        // 收集任一线程报告的错误。
        let mut first_err: Option<String> = None;
        for h in handles {
            match h.join() {
                Ok(Ok(())) => {},
                Ok(Err(e)) => {
                    first_err.get_or_insert(e);
                },
                Err(_) => {
                    first_err.get_or_insert("线程 panic".to_string());
                },
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
