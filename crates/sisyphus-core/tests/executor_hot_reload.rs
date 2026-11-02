//! 执行器并发上限热重载（`Executor::apply_config`）。
//!
//! 回归背景：`Executor::apply_config` 曾经只打一行
//! “runtime limit updated via restart” 日志，什么也不改；
//! `SisyphusKernel::apply_executor_config_hot_reload` 同样是个空占位。
//! 于是 `executor.max_concurrent_tasks` 的运行时变更完全不生效。
//!
//! 现在的实现就地增删 `Semaphore` 的许可：既影响在途批次（它们持有同一个
//! 闸门），也支持「许可正被占用时缩减」——差额记为待回收，下一次调用继续
//! 回收，直到与目标值一致。

use sisyphus_core::Executor;
use sisyphus_types::ExecutorConfig;
use std::sync::Arc;

fn make_executor() -> Executor {
    Executor::new(
        Arc::new(sisyphus_tools_executor::executor::test_cmds::TestExecutor::new()),
        Arc::new(sisyphus_core::intervention_manager::InterventionManager::new()),
        Arc::new(
            sisyphus_core::internalization_manager::InternalizationManager::new(
                Arc::new(sisyphus_tools_llm::metis::DummyLlmClient),
                Arc::new(sisyphus_knowledge::AssetManager::new(
                    ".omo/test_executor_reload".into(),
                )),
            ),
        ),
        None,
    )
}

fn cfg(limit: usize) -> ExecutorConfig {
    ExecutorConfig {
        max_concurrent_tasks: limit,
        ..ExecutorConfig::default()
    }
}

#[test]
fn defaults_to_sixteen_concurrent_nodes() {
    let executor = make_executor();
    assert_eq!(executor.configured_limit(), 16);
    assert_eq!(executor.concurrency_limit.available_permits(), 16);
}

#[test]
fn growth_adds_permits_in_place() {
    let executor = make_executor();
    let gate = Arc::clone(&executor.concurrency_limit);

    executor.apply_config(&cfg(48));

    assert_eq!(executor.configured_limit(), 48);
    assert_eq!(executor.concurrency_limit.available_permits(), 48);
    assert!(
        Arc::ptr_eq(&gate, &executor.concurrency_limit),
        "hot reload must not swap the semaphore: in-flight batches hold the old Arc"
    );
}

#[test]
fn shrink_removes_available_permits() {
    let executor = make_executor();
    executor.apply_config(&cfg(4));
    assert_eq!(executor.configured_limit(), 4);
    assert_eq!(executor.concurrency_limit.available_permits(), 4);
}

#[test]
fn zero_is_clamped_to_one() {
    let executor = make_executor();
    // 配置校验会拒绝 0，但热重载不应把执行器变成永久阻塞。
    executor.apply_config(&cfg(0));
    assert_eq!(executor.configured_limit(), 1);
    assert_eq!(executor.concurrency_limit.available_permits(), 1);
}

#[test]
fn shrink_while_permits_are_held_is_completed_on_next_call() {
    let executor = make_executor();

    // 占用 12 个许可（模拟 12 个在途节点）。
    let held = executor.concurrency_limit.try_acquire_many(12).expect("12 permits available");

    // 16 → 2：想回收 14 个，但只有 4 个可用，因此只能回收 4 个。
    executor.apply_config(&cfg(2));
    assert_eq!(executor.configured_limit(), 2);
    assert_eq!(executor.concurrency_limit.available_permits(), 0);

    // 在途节点结束后归还许可。
    drop(held);

    // 再次应用同一配置：把上次没回收的 10 个补上（12 可用 → 2 可用）。
    executor.apply_config(&cfg(2));
    assert_eq!(executor.concurrency_limit.available_permits(), 2);
    assert_eq!(executor.configured_limit(), 2);

    // 现在闸门容量确实等于目标值：只能拿到 2 个许可。
    let a = executor.concurrency_limit.try_acquire_many(2).expect("2 permits");
    assert!(
        executor.concurrency_limit.try_acquire().is_err(),
        "capacity must not exceed the configured limit"
    );
    drop(a);
}

/// 持续压测：一边有大量任务抢/还许可，一边反复热重载并发上限。
///
/// 这是热重载最容易出错的地方——`forget_permits` 只能回收当前空闲的许可，
/// 如果记账有偏差（例如 pending_reduction 覆盖而不是累加、或在增长时忽略
/// 待回收量），压测结束后闸门容量就会与配置值永久偏离，表现为「并发上限
/// 越调越大」或「许可慢性泄漏直到饿死」。
///
/// 用 `try_acquire`（而非 `acquire().await`）保证在 1 许可窗口里 worker
/// 不会阻塞，测试不会因为上限被压到 1 而死锁。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_reload_under_concurrent_load_keeps_gate_consistent() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    let executor = Arc::new(make_executor());
    executor.apply_config(&cfg(8));

    let stop = Arc::new(AtomicBool::new(false));

    let mut workers = Vec::new();
    for _ in 0..32 {
        let exec = Arc::clone(&executor);
        let stop = Arc::clone(&stop);
        workers.push(tokio::spawn(async move {
            let mut acquired = 0u64;
            while !stop.load(Ordering::Relaxed) {
                if let Ok(permit) = exec.concurrency_limit.try_acquire() {
                    acquired += 1;
                    tokio::task::yield_now().await;
                    drop(permit);
                } else {
                    tokio::task::yield_now().await;
                }
            }
            acquired
        }));
    }

    // 限幅反复横跳：先缩到 1（在途许可无法立刻回收），再放大，再缩……
    let limits = [1usize, 16, 4, 48, 2];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(300);
    let mut round = 0usize;
    while tokio::time::Instant::now() < deadline {
        executor.apply_config(&cfg(limits[round % limits.len()]));
        round += 1;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    stop.store(true, Ordering::Relaxed);
    let mut acquired_total = 0u64;
    for worker in workers {
        acquired_total += worker.await.expect("worker task must not panic");
    }
    assert!(
        acquired_total > 0,
        "workers must have acquired at least one permit"
    );

    // 收尾：所有许可已归还，重新应用目标配置把待回收差额补齐。
    // 调用两次是刻意为之——第一次消化 pending_reduction，第二次验证幂等。
    executor.apply_config(&cfg(8));
    executor.apply_config(&cfg(8));

    assert_eq!(executor.configured_limit(), 8);
    assert_eq!(
        executor.concurrency_limit.available_permits(),
        8,
        "gate capacity must converge back to the configured limit (no leaked/stolen permits)"
    );

    // 容量确实只有 8：拿满后不应再有空闲许可。
    let all = executor.concurrency_limit.try_acquire_many(8).expect("8 permits");
    assert!(executor.concurrency_limit.try_acquire().is_err());
    drop(all);
}
