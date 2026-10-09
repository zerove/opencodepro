//! SQLite 连接参数的**唯一真实来源**。
//!
//! ## 为什么需要这个模块
//!
//! 认知追踪的 flush 屏障（`sisyphus-core::ExecutionManager`）需要等待
//! writer 把待写记录落盘，而 writer 的单条写入在写锁被占时最多等到
//! `busy_timeout`。如果屏障上限与 `busy_timeout` 相等（历史上两者都是
//! 5s），写入仍在推进而屏障先判超时，flush 便谎报
//! “durability not guaranteed”——**数据其实已写进库里**。
//!
//! 此前 `busy_timeout` 依赖 sqlx 的隐式默认值，与 core 里的字面量
//! `Duration::from_secs(5)` 没有任何关联：两者相等纯属巧合，一旦升级 sqlx
//! 或改默认值，缺陷就会静默回归。
//!
//! 现在 `busy_timeout` 由 [`SQLITE_BUSY_TIMEOUT`] 显式设定，
//! 而 flush 屏障预算由 [`TRACE_FLUSH_BARRIER_TIMEOUT`] 推导，二者同源，
//! 关系由构造式保证而非靠注释约定。

use std::str::FromStr;
use std::time::Duration;

/// SQLite 写锁等待上限（秒）。
///
/// **显式设置**，不依赖 sqlx 默认值（当前恰好也是 5s）。这是让
/// [`TRACE_FLUSH_BARRIER_TIMEOUT`] 能被正确推导的前提：默认值一旦随
/// 依赖升级变化，隐式关系就会失效。
pub const SQLITE_BUSY_TIMEOUT_SECS: u64 = 5;

/// SQLite 写锁等待上限。
pub const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(SQLITE_BUSY_TIMEOUT_SECS);

/// 认知追踪 flush 屏障的等待上限（秒）。
///
/// 必须**严格大于**一条写入的最坏耗时（[`SQLITE_BUSY_TIMEOUT_SECS`]），
/// 否则写入仍在推进时屏障就会放弃。取两个 busy_timeout 加 5s 余量：
/// 覆盖一次完整锁等待，再容纳写入本身的执行与调度延迟。
///
/// 由 busy_timeout 推导而非独立字面量，使「屏障 > busy_timeout」成为
/// 构造式上的事实，无法被无意改成相等。
///
/// 该不变量由本文件末尾的 `const _: () = assert!(...)` 在**编译期**
/// 强制：一旦有人把两者改成相等，`cargo build` 直接失败。因此无需（在
/// `sisyphus-core` 的集成测试里）再写一条只能返回常量的运行期断言——
/// 那既无法失败，clippy 也会报 `assertions_on_constants`。
pub const TRACE_FLUSH_BARRIER_TIMEOUT_SECS: u64 = SQLITE_BUSY_TIMEOUT_SECS * 2 + 5;

/// 认知追踪 flush 屏障的等待上限。
pub const TRACE_FLUSH_BARRIER_TIMEOUT: Duration =
    Duration::from_secs(TRACE_FLUSH_BARRIER_TIMEOUT_SECS);

/// `busy_timeout` 的完整性自检。
///
/// 屏障预算必须严格大于单条写入的最坏耗时。相等即等价于「写入还在重试、
/// 屏障已放弃」，正是本模块要防的缺陷。用秒数比较以便在 const 上下文中
/// 求值（`Duration` 的 `>` 不是 const 函数），使违反不变量时**编译失败**，
/// 而非等到运行期才暴露。
const _: () = assert!(
    TRACE_FLUSH_BARRIER_TIMEOUT_SECS > SQLITE_BUSY_TIMEOUT_SECS,
    "flush 屏障预算必须严格大于 SQLite busy_timeout，否则写锁竞争下 flush 会谎报失败"
);

/// 为 `db_url` 构造统一的 SQLite 连接选项。
///
/// 所有 `SqlitePool` 的建立都应经过这里，避免各处遗漏
/// [`SQLITE_BUSY_TIMEOUT`] 而重新引入隐式默认值。
///
/// 错误类型为 `sqlx::Error`，与 `SqliteConnectOptions::from_str` 一致，
/// 调用方无需额外转换。
pub fn sqlite_options(db_url: &str) -> Result<sqlx::sqlite::SqliteConnectOptions, sqlx::Error> {
    Ok(sqlx::sqlite::SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(SQLITE_BUSY_TIMEOUT))
}
