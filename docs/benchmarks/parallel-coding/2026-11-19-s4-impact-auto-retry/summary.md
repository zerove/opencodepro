# 并行编码场景证据（模板生成）

- scenario: `S4`
- timestamp_utc: `2026-11-19T00:51:09Z`
- git_head: `254786eeb671c53861a9b3cc2b2c304e27813b16`
- git_dirty_files: `18`
- output_dir: `docs/benchmarks/parallel-coding/2026-11-19-s4-impact-auto-retry`
- note: M1.2 workspace-impact auto targeted retry: FailAffectedPending marks affected pending tasks failed; execute_plan_with_impact_retry re-runs only those tasks (ImpactRetry event), 0 rounds keeps legacy behavior, non-impact failures are not retried. Counts from actual output.

## 环境

```text
timestamp_utc=2026-11-19T00:51:09Z
scenario=S4
host=zeron
kernel=Linux zeron 6.18.58.1-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Tue Oct 27 00:24:11 UTC 2026 x86_64 GNU/Linux
git_head=254786eeb671c53861a9b3cc2b2c304e27813b16
git_dirty_files=18
pwd=/home/zeron/dev/opencodepro
rustc=rustc 1.100.0 (0ef1bc386 2026-11-09)
cargo=cargo 1.100.0 (3acc79c0c 2026-11-05)
```

## 执行

### Command

```bash
bash -lc cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_impact_retry_recovers_failed_pending_task\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_impact_retry_disabled_with_zero_rounds\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_impact_retry_skips_non_impact_failures 
```

### Result

- exit_code: `0`
- elapsed_ms: `4319`
- log: `run.log`
- summary_json: `summary.json`

### 结论

- PASS：3 个过滤执行的测试均 `ok`（每个 `1 passed; 0 failed`，各次 `168 filtered out`），`exit_code=0`；命令与输出见 `run.log`（计数以实跑输出为准）。
