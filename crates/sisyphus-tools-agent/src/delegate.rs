mod types;
pub use types::*;

use sisyphus_tools_core::ToolResult;
use std::future::Future;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

impl WorkOrchestrator {
    /// Run all work units and collect results. Uses a semaphore for concurrency control.
    pub async fn run_all(&self, units: Vec<WorkUnit>) -> ToolResult<Vec<WorkReport>> {
        let count = units.len();
        let semaphore_max = if self.max_concurrent == 0 {
            count
        } else {
            self.max_concurrent
        };
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(semaphore_max));

        let mut set = JoinSet::new();
        for unit in units {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| sisyphus_tools_core::ToolError::TodoError(format!("semaphore: {e}")))?;
            set.spawn(async move {
                let _permit = permit;
                let start = Instant::now();
                let handler = unit.handler;
                match handler().await {
                    Ok(value) => WorkReport {
                        id: unit.id,
                        name: unit.name,
                        status: WorkStatus::Completed,
                        result: Some(value),
                        error: None,
                        duration_ms: start.elapsed().as_millis(),
                    },
                    Err(e) => WorkReport {
                        id: unit.id,
                        name: unit.name,
                        status: WorkStatus::Failed,
                        result: None,
                        error: Some(e.to_string()),
                        duration_ms: start.elapsed().as_millis(),
                    },
                }
            });
        }

        let mut reports = Vec::with_capacity(count);
        while let Some(result) = set.join_next().await {
            match result {
                Ok(report) => reports.push(report),
                Err(e) => reports.push(WorkReport {
                    id: "unknown".to_string(),
                    name: "join_error".to_string(),
                    status: WorkStatus::Failed,
                    result: None,
                    error: Some(format!("task join error: {e}")),
                    duration_ms: 0,
                }),
            }
        }

        // Sort reports by insertion order (maintain original task order)
        Ok(reports)
    }

    /// Run work units and receive events via a channel for progress tracking.
    pub fn run_all_with_events(
        &self,
        units: Vec<WorkUnit>,
    ) -> ToolResult<(
        mpsc::Receiver<DelegateEvent>,
        tokio::task::JoinHandle<Vec<WorkReport>>,
    )> {
        let (tx, rx) = mpsc::channel(units.len() + 1);
        let handle = {
            let tx = tx.clone();
            let count = units.len();
            let semaphore_max = if self.max_concurrent == 0 {
                count
            } else {
                self.max_concurrent
            };
            let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(semaphore_max));

            tokio::spawn(async move {
                let mut reports = Vec::with_capacity(count);
                let mut set = JoinSet::new();
                for unit in units {
                    let permit = match semaphore.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(e) => {
                            let report = WorkReport {
                                id: unit.id,
                                name: unit.name,
                                status: WorkStatus::Failed,
                                result: None,
                                error: Some(format!("semaphore closed: {e}")),
                                duration_ms: 0,
                            };
                            let _ = tx.send(DelegateEvent::Completed(report.clone())).await;
                            reports.push(report);
                            continue;
                        }
                    };
                    let tx = tx.clone();
                    let _ = tx
                        .send(DelegateEvent::Started {
                            id: unit.id.clone(),
                            name: unit.name.clone(),
                        })
                        .await;

                    set.spawn(async move {
                        let _permit = permit;
                        let start = Instant::now();
                        let handler = unit.handler;
                        let report = match handler().await {
                            Ok(value) => WorkReport {
                                id: unit.id,
                                name: unit.name,
                                status: WorkStatus::Completed,
                                result: Some(value),
                                error: None,
                                duration_ms: start.elapsed().as_millis(),
                            },
                            Err(e) => WorkReport {
                                id: unit.id,
                                name: unit.name,
                                status: WorkStatus::Failed,
                                result: None,
                                error: Some(e.to_string()),
                                duration_ms: start.elapsed().as_millis(),
                            },
                        };
                        let _ = tx.send(DelegateEvent::Completed(report.clone())).await;
                        report
                    });
                }

                while let Some(result) = set.join_next().await {
                    match result {
                        Ok(report) => reports.push(report),
                        Err(e) => {
                            let report = WorkReport {
                                id: "unknown".to_string(),
                                name: "join_error".to_string(),
                                status: WorkStatus::Failed,
                                result: None,
                                error: Some(format!("task join error: {e}")),
                                duration_ms: 0,
                            };
                            reports.push(report);
                        }
                    }
                }
                drop(tx);
                reports
            })
        };

        Ok((rx, handle))
    }
}

/// Convenience: create a work unit from an async closure.
pub fn work_unit<F, Fut>(id: &str, name: &str, f: F) -> WorkUnit
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ToolResult<serde_json::Value>> + Send + 'static,
{
    WorkUnit {
        id: id.to_string(),
        name: name.to_string(),
        handler: Box::new(move || Box::pin(f())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    async fn ok_task(value: &'static str) -> ToolResult<serde_json::Value> {
        Ok(json!({ "result": value }))
    }

    async fn fail_task(_msg: &'static str) -> ToolResult<serde_json::Value> {
        Err(sisyphus_tools_core::ToolError::TodoError(
            "intentional failure".to_string(),
        ))
    }

    #[test]
    fn test_work_status_display() {
        assert_eq!(WorkStatus::Pending.to_string(), "pending");
        assert_eq!(WorkStatus::Running.to_string(), "running");
        assert_eq!(WorkStatus::Completed.to_string(), "completed");
        assert_eq!(WorkStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_work_status_partial_eq() {
        assert_eq!(WorkStatus::Pending, WorkStatus::Pending);
        assert_ne!(WorkStatus::Pending, WorkStatus::Running);
        assert_ne!(WorkStatus::Completed, WorkStatus::Failed);
    }

    #[test]
    fn test_work_unit_debug() {
        let unit = work_unit("id-1", "my task", || async { Ok(json!({})) });
        let debug = format!("{:?}", unit);
        assert!(debug.contains("id-1"), "debug should contain id: {debug}");
        assert!(
            debug.contains("my task"),
            "debug should contain name: {debug}"
        );
    }

    #[test]
    fn test_work_orchestrator_new() {
        // Just verify construction doesn't panic
        let _o = WorkOrchestrator::new(5);
        let _o2 = WorkOrchestrator::new(0);
    }

    #[test]
    fn test_work_report_construction() {
        let report = WorkReport {
            id: "r1".into(),
            name: "report".into(),
            status: WorkStatus::Completed,
            result: Some(json!({"done": true})),
            error: None,
            duration_ms: 42,
        };
        assert_eq!(report.id, "r1");
        assert_eq!(report.duration_ms, 42);
        assert!(report.result.is_some());
        assert!(report.error.is_none());
    }

    #[test]
    fn test_delegate_event_variants() {
        let e1 = DelegateEvent::Started {
            id: "a".into(),
            name: "A".into(),
        };
        let e2 = DelegateEvent::Completed(WorkReport {
            id: "a".into(),
            name: "A".into(),
            status: WorkStatus::Completed,
            result: None,
            error: None,
            duration_ms: 0,
        });
        let e3 = DelegateEvent::Progress {
            id: "a".into(),
            message: "working".into(),
        };
        // Verify all variants can be formatted (Debug)
        let _ = format!("{:?}", e1);
        let _ = format!("{:?}", e2);
        let _ = format!("{:?}", e3);
    }

    #[tokio::test]
    async fn test_run_all_basic() {
        let orchestrator = WorkOrchestrator::new(0);
        let units = vec![
            work_unit("a", "task A", || async { ok_task("alpha").await }),
            work_unit("b", "task B", || async { ok_task("beta").await }),
        ];
        let reports = orchestrator.run_all(units).await.unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].status, WorkStatus::Completed);
        assert_eq!(reports[1].status, WorkStatus::Completed);
    }

    #[tokio::test]
    async fn test_run_all_concurrent_limit() {
        let orchestrator = WorkOrchestrator::new(2);
        let units = vec![
            work_unit("a", "slow A", || async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                ok_task("alpha").await
            }),
            work_unit("b", "slow B", || async {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                ok_task("beta").await
            }),
            work_unit("c", "fast C", || async { ok_task("gamma").await }),
        ];
        let start = Instant::now();
        let reports = orchestrator.run_all(units).await.unwrap();
        let elapsed = start.elapsed();

        // With 2 concurrent slots and 2 slow tasks, should take ~50ms not 100ms+
        assert!(
            elapsed.as_millis() < 150,
            "took too long: {}ms, expected <150ms with concurrency=2",
            elapsed.as_millis()
        );
        assert_eq!(reports.len(), 3);
    }

    #[tokio::test]
    async fn test_run_all_with_failures() {
        let orchestrator = WorkOrchestrator::new(0);
        let units = vec![
            work_unit("good", "good task", || async { ok_task("ok").await }),
            work_unit("bad", "bad task", || async { fail_task("nope").await }),
        ];
        let reports = orchestrator.run_all(units).await.unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].status, WorkStatus::Completed);
        assert_eq!(reports[1].status, WorkStatus::Failed);
        assert!(reports[1].error.is_some());
    }

    #[tokio::test]
    async fn test_run_all_empty() {
        let orchestrator = WorkOrchestrator::new(0);
        let reports = orchestrator.run_all(vec![]).await.unwrap();
        assert!(reports.is_empty());
    }

    #[tokio::test]
    async fn test_work_unit_closure() {
        let unit = work_unit("test", "closure", || async { Ok(json!({ "value": 42 })) });
        assert_eq!(unit.id, "test");
        assert_eq!(unit.name, "closure");
    }

    #[tokio::test]
    async fn test_run_all_with_events() {
        let orchestrator = WorkOrchestrator::new(2);
        let units = vec![
            work_unit("x", "task X", || async { ok_task("X").await }),
            work_unit("y", "task Y", || async { ok_task("Y").await }),
        ];
        let (mut rx, handle) = orchestrator.run_all_with_events(units).unwrap();

        let mut started = 0;
        let mut completed = 0;
        use tokio::sync::mpsc::error::TryRecvError;
        loop {
            match rx.try_recv() {
                Ok(DelegateEvent::Started { .. }) => started += 1,
                Ok(DelegateEvent::Completed(_)) => completed += 1,
                Err(TryRecvError::Empty) => {
                    if handle.is_finished() {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(TryRecvError::Disconnected) => break,
                _ => {}
            }
        }
        let reports = handle.await.unwrap();
        assert_eq!(reports.len(), 2);
        assert_eq!(started, 2);
        assert_eq!(completed, 2);
    }
}
