# 并行编码场景证据（模板生成）

- scenario: `M1.3`
- timestamp_utc: `2026-11-19T01:11:00Z`
- git_head: `254786eeb671c53861a9b3cc2b2c304e27813b16`
- git_dirty_files: `22`
- output_dir: `docs/benchmarks/parallel-coding/2026-11-19-m13-team-sse-endpoint`
- note: M1.3 team execution endpoint + SSE broadcast wiring: POST /api/team/execute executes an explicit plan via bridge team orchestrator, forwards TeamEvent as Custom SSE events with goal_id/project_id/trace_id scope, supports ?goal_id= filter, and impact_retry_rounds. Tests cover handler end-to-end broadcast, forwarder scope injection, goal_id filtering, and OpenAPI route documentation. Counts from actual output.

## 环境

```text
timestamp_utc=2026-11-19T01:11:00Z
scenario=M1.3
host=zeron
kernel=Linux zeron 6.18.58.1-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Tue Oct 27 00:24:11 UTC 2026 x86_64 GNU/Linux
git_head=254786eeb671c53861a9b3cc2b2c304e27813b16
git_dirty_files=22
pwd=/home/zeron/dev/opencodepro
rustc=rustc 1.100.0 (0ef1bc386 2026-11-09)
cargo=cargo 1.100.0 (3acc79c0c 2026-11-05)
```

## 执行

### Command

```bash
bash -lc cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ team_execute_handler_broadcasts_scoped_events\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ forward_team_events_injects_scope_dimensions\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ goal_filter\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ every_route_is_documented 
```

### Result

- exit_code: `0`
- elapsed_ms: `3514`
- log: `run.log`
- summary_json: `summary.json`

### 结论

- PASS：4 条命令均成功（`exit_code=0`）。过滤执行的测试全部 `ok`：handler 端到端广播 1、转发器维度注入 1、`?goal_id=` 过滤 2、OpenAPI 路由收录 1；命令与输出见 `run.log`（计数以实跑输出为准）。
