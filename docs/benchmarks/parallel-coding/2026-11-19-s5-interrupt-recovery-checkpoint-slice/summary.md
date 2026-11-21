# 并行编码场景证据（模板生成）

- scenario: `S5`
- timestamp_utc: `2026-11-18T22:47:20Z`
- git_head: `254786eeb671c53861a9b3cc2b2c304e27813b16`
- git_dirty_files: `18`
- output_dir: `docs/benchmarks/parallel-coding/2026-11-19-s5-interrupt-recovery-checkpoint-slice`
- note: S5 最小中断恢复与周期性检查点确定性切片（持久化文件恢复/周期性保存/从检查点续跑）；计数以实跑输出为准

## 环境

```text
timestamp_utc=2026-11-18T22:47:20Z
scenario=S5
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
bash -lc cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ team::checkpoint::tests::checkpoint_round_trip_preserves_state\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_resume_plan_from_checkpoint_continues_remaining_tasks\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_interrupt_recovery_via_persisted_checkpoint_file\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_periodic_checkpoint_saved_during_execution 
```

### Result

- exit_code: `0`
- elapsed_ms: `3767`
- log: `run.log`
- summary_json: `summary.json`

### 结论

- 请基于 run.log 与实际验收命令填写 PASS/FAIL/BLOCKED（不得伪造）。
