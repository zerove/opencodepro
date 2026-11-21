# 并行编码场景证据（模板生成）

- scenario: `S2`
- timestamp_utc: `2026-11-18T10:28:33Z`
- git_head: `254786eeb671c53861a9b3cc2b2c304e27813b16`
- git_dirty_files: `10`
- output_dir: `docs/benchmarks/parallel-coding/2026-11-18-s2-write-conflict-serialization`
- note: 确定性单测：同文件写冲突检测+串行化波次执行（M0.3/S2, M1.3 conflict_stats）。

## 环境

```text
timestamp_utc=2026-11-18T10:28:33Z
scenario=S2
host=zeron
kernel=Linux zeron 6.18.58.1-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Tue Oct 27 00:24:11 UTC 2026 x86_64 GNU/Linux
git_head=254786eeb671c53861a9b3cc2b2c304e27813b16
git_dirty_files=10
pwd=/home/zeron/dev/opencodepro
rustc=rustc 1.100.0 (0ef1bc386 2026-11-09)
cargo=cargo 1.100.0 (3acc79c0c 2026-11-05)
```

## 执行

### Command

```bash
bash -lc cargo\ test\ -j\ 1\ -p\ sisyphus-tools-agent\ --lib\ test_parallel_write_conflict_is_detected_and_serialized 
```

### Result

- exit_code: `0`
- elapsed_ms: `4103`
- log: `run.log`
- summary_json: `summary.json`

### 结论

- 请基于 run.log 与实际验收命令填写 PASS/FAIL/BLOCKED（不得伪造）。
