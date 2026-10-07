//! 测试用环境变量作用域守卫。
//!
//! # 为什么需要它
//!
//! `std::env::set_var` 改的是**进程级**状态。并行测试下有两个独立问题：
//!
//! 1. **互相踩踏**。A 测试设了变量还没清，B 测试读到了，于是 B 的断言
//!    在一个它自己没创造的环境下成立或失败。
//! 2. **panic 后永久残留**。这是更隐蔽的一种：
//!    ```ignore
//!    std::env::set_var("K", "v");
//!    assert_eq!(compute(), expected);   // ← 这里 panic
//!    std::env::remove_var("K");         // ← 永不执行
//!    ```
//!    用互斥锁（`static ENV_LOCK: Mutex<()>`）只能解决第 1 条，对第 2 条
//!    完全无效：锁的守卫析构照常运行，但变量已经残留，且当前测试往往
//!    只是断言失败、看不出与环境的因果关系。后续某个不相干的测试开始
//!    莫名其妙地失败，失败现场已被覆盖，极难归因。
//!
//! 本仓库已经为此付出实价：`evals` bench 曾用 `set_current_dir` + 互斥锁
//! 串行化，`write_workflow` 曾因此把文件写进源码树。两处根治方式都是
//! **改为显式传参**；能用显式参数就不要用环境变量。
//!
//! 当被测代码确实只能从环境变量读（例如读 `WEBUI_API_KEY` 的旧中间件）
//! 且无法改造时，用 [`EnvVarGuard`] 保证清理必定执行。
//!
//! # 用法
//!
//! ```ignore
//! let _g = EnvVarGuard::set("MY_KEY", "value");
//! // 测试体，随便 panic
//! drop(_g); // 提前恢复也行；不写也会在作用域结束时恢复
//! ```
//!
//! 守卫会**恢复原值**而非简单删除：若该变量在测试运行前本就有值
//! （开发者本机 `~/.zshrc` 里设过），删除会污染后续测试。
//!
//! # 仍然存在的局限
//!
//! 守卫保证「恢复」，不保证「隔离」。两个并行测试若改**同一个**变量名，
//! 仍会互相影响——守卫只是把残留的窗口从「永久」缩短到「作用域内」。
//! 能显式传参就显式传参；退而求其次，变量名要带测试专用前缀
//! （如 `SISYPHUS_TEST_...`），绝不用生产配置的真实变量名。

use std::ffi::OsString;

/// 环境变量作用域守卫：创建时设值，销毁时恢复原值。
///
/// # Safety 说明
///
/// `std::env::set_var` / `remove_var` 在 Rust 2024 起是 `unsafe`，因为
/// 多线程下与其他线程读环境变量存在数据竞争。本类型**无法**消除该竞争
/// ——它只保证恢复必定发生。
///
/// 正确用法是配合测试串行化，或使用测试专用变量名使其与其他测试无关。
/// 本项目中的典型做法见 `webui` 的旧中间件测试：改用
/// `from_fn_with_state` 显式传入 token，从根本上不碰环境变量。
#[derive(Debug)]
pub struct EnvVarGuard {
    key: std::borrow::Cow<'static, str>,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    /// 设值并返回守卫，作用域结束时恢复原值。
    ///
    /// # Safety
    ///
    /// 调用方须确保没有其他线程在此期间读写同名环境变量。测试代码中
    /// 这通常靠「变量名带测试专用前缀」或串行化来保证。
    pub unsafe fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: 由调用方保证无并发访问同名变量。
        unsafe { std::env::set_var(key, value) };
        Self {
            key: std::borrow::Cow::Borrowed(key),
            previous,
        }
    }

    /// 同 [`EnvVarGuard::set`]，但变量名在运行时构造。
    ///
    /// 用于按进程 PID 作用域化的测试变量名（如
    /// `format!("SISYPHUS_TEST_{}_{}", label, std::process::id())`）——
    /// 这类名字无法写成 `&'static str`，但同样需要 panic 安全的清理。
    ///
    /// # Safety
    ///
    /// 同 [`EnvVarGuard::set`]。
    pub unsafe fn set_owned(key: String, value: &str) -> Self {
        let previous = std::env::var_os(&key);
        // SAFETY: 由调用方保证无并发访问同名变量。
        unsafe { std::env::set_var(&key, value) };
        Self {
            key: std::borrow::Cow::Owned(key),
            previous,
        }
    }

    /// 清除变量并返回守卫，作用域结束时恢复原值。
    ///
    /// 用于「断言变量**未**设置」的场景——这类测试不能只是不设置，
    /// 必须确保开发者本机恰好设过该变量时也能通过。
    ///
    /// # Safety
    ///
    /// 同 [`EnvVarGuard::set`]。
    pub unsafe fn unset(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: 由调用方保证无并发访问同名变量。
        unsafe { std::env::remove_var(key) };
        Self {
            key: std::borrow::Cow::Borrowed(key),
            previous,
        }
    }

    /// 同 [`EnvVarGuard::unset`]，但变量名在运行时构造。
    ///
    /// # Safety
    ///
    /// 同 [`EnvVarGuard::unset`]。
    pub unsafe fn unset_owned(key: String) -> Self {
        let previous = std::env::var_os(&key);
        // SAFETY: 由调用方保证无并发访问同名变量。
        unsafe { std::env::remove_var(&key) };
        Self {
            key: std::borrow::Cow::Owned(key),
            previous,
        }
    }

    /// 该守卫管理的变量名。
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // `Cow<str>` 不实现 `AsRef<OsStr>`，需显式转成 `&str`。
        let key = self.key.as_ref();
        match &self.previous {
            // SAFETY: Drop 时若发生并发读写同名变量仍是 UB，这一点与
            // 构造时相同——本类型并不提供隔离，只提供「必定恢复」。
            // 正确的并发前提是：守卫不得被移到会并发访问同名变量的线程上。
            // 注意本类型**是** `Send`（`Cow<'static, str>` 与 `OsString`
            // 均为 `Send`），所以该前提由调用方负责，无法靠类型系统兜住。
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}

/// 一次性设置多个变量，返回一个统一恢复它们的守卫。
///
/// 适用于「设一批 → 断言 → 清一批」的场景，如配置覆盖测试。
/// 变量名与值需为 `&'static str`，因守卫持有 `&'static str` 键。
///
/// # Safety
///
/// 同 [`EnvVarGuard::set`]。
pub unsafe fn env_var_guards(pairs: &'static [(&'static str, &'static str)]) -> MultiEnvVarGuard {
    let guards = pairs
        .iter()
        .map(|(k, v)| {
            // SAFETY: 转发调用方的安全契约。
            unsafe { EnvVarGuard::set(k, v) }
        })
        .collect();
    MultiEnvVarGuard { guards }
}

/// 多个 [`EnvVarGuard`] 的集合，销毁时按创建顺序逆序恢复。
///
/// 逆序恢复是为了让嵌套的「先设 A 再设 B」在恢复时回到「先撤 B 再撤 A」
/// 的等效状态——多数情况下顺序无关，但逆序更贴近直觉。
#[derive(Debug)]
pub struct MultiEnvVarGuard {
    guards: Vec<EnvVarGuard>,
}

impl MultiEnvVarGuard {
    /// 守卫数量。
    pub fn len(&self) -> usize {
        self.guards.len()
    }

    /// 是否为空（无守卫）。
    pub fn is_empty(&self) -> bool {
        self.guards.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_previous_value_on_drop() {
        const KEY: &str = "SISYPHUS_TEST_GUARD_RESTORE";
        // SAFETY: 单线程测试，且变量名带测试专用前缀。
        unsafe { std::env::set_var(KEY, "original") };

        {
            // SAFETY: 同上。
            let _g = unsafe { EnvVarGuard::set(KEY, "temporary") };
            assert_eq!(std::env::var(KEY).unwrap(), "temporary");
        }

        // 守卫析构后必须回到原值，而非被删除。
        assert_eq!(std::env::var(KEY).unwrap(), "original");
        // SAFETY: 清理本测试自己设的值。
        unsafe { std::env::remove_var(KEY) };
    }

    #[test]
    fn unset_restores_absent_to_absent() {
        const KEY: &str = "SISYPHUS_TEST_GUARD_ABSENT";
        // SAFETY: 单线程测试，变量名带测试专用前缀。
        unsafe { std::env::remove_var(KEY) };

        {
            // SAFETY: 同上。
            let _g = unsafe { EnvVarGuard::set(KEY, "v") };
            assert!(std::env::var(KEY).is_ok());
        }

        assert!(std::env::var(KEY).is_err(), "原本不存在的变量不应残留");
    }

    #[test]
    fn unset_helper_restores_previous() {
        const KEY: &str = "SISYPHUS_TEST_GUARD_UNSET";
        // SAFETY: 单线程测试，变量名带测试专用前缀。
        unsafe { std::env::set_var(KEY, "present") };

        {
            // SAFETY: 同上。
            let _g = unsafe { EnvVarGuard::unset(KEY) };
            assert!(std::env::var(KEY).is_err());
        }

        assert_eq!(std::env::var(KEY).unwrap(), "present");
        // SAFETY: 清理本测试自己设的值。
        unsafe { std::env::remove_var(KEY) };
    }

    #[test]
    fn multi_guard_restores_all() {
        const A: &str = "SISYPHUS_TEST_MULTI_A";
        const B: &str = "SISYPHUS_TEST_MULTI_B";
        static PAIRS: [(&str, &str); 2] = [(A, "va"), (B, "vb")];

        // SAFETY: 单线程测试，变量名带测试专用前缀。
        unsafe { std::env::remove_var(A) };
        // SAFETY: 同上。
        unsafe { std::env::remove_var(B) };

        {
            // SAFETY: 同上。
            let g = unsafe { env_var_guards(&PAIRS) };
            assert_eq!(g.len(), 2);
            assert!(!g.is_empty());
            assert_eq!(std::env::var(A).unwrap(), "va");
            assert_eq!(std::env::var(B).unwrap(), "vb");
        }

        assert!(std::env::var(A).is_err());
        assert!(std::env::var(B).is_err());
    }

    /// 本类型的**全部存在理由**是 panic 安全，而上面三个测试走的都是
    /// 正常析构路径。若守卫只在正常路径有效，`Drop` 就毫无意义——
    /// 用 `catch_unwind` 显式触发展开并验证变量确实被恢复。
    #[test]
    fn restores_after_panic_unwind() {
        const KEY: &str = "SISYPHUS_TEST_GUARD_PANIC";
        // SAFETY: 单线程测试，变量名带测试专用前缀。
        unsafe { std::env::set_var(KEY, "original") };

        let result = std::panic::catch_unwind(|| {
            // SAFETY: 同上。
            let _g = unsafe { EnvVarGuard::set(KEY, "temporary") };
            assert_eq!(std::env::var(KEY).unwrap(), "temporary");
            panic!("故意 panic，验证守卫在展开路径上仍然恢复");
        });

        assert!(result.is_err(), "catch_unwind 应捕获到 panic");
        // 关键断言：panic 之后变量必须回到原值，而不是残留 "temporary"。
        assert_eq!(
            std::env::var(KEY).unwrap(),
            "original",
            "panic 展开后必须恢复原值；这正是不用 remove_var 手动清理的理由"
        );
        // SAFETY: 清理本测试自己设的值。
        unsafe { std::env::remove_var(KEY) };
    }

    /// panic 时若变量**原本不存在**，恢复后也不应凭空出现。
    #[test]
    fn absent_stays_absent_after_panic() {
        const KEY: &str = "SISYPHUS_TEST_GUARD_PANIC_ABSENT";
        // SAFETY: 单线程测试，变量名带测试专用前缀。
        unsafe { std::env::remove_var(KEY) };

        let result = std::panic::catch_unwind(|| {
            // SAFETY: 同上。
            let _g = unsafe { EnvVarGuard::set(KEY, "temporary") };
            panic!("故意 panic");
        });

        assert!(result.is_err());
        assert!(
            std::env::var(KEY).is_err(),
            "原本不存在的变量在 panic 后不应残留"
        );
    }
}
