# 并行编码场景证据（模板生成）

- scenario: `M1.3`
- timestamp_utc: `2026-11-19T01:41:54Z`
- git_head: `254786eeb671c53861a9b3cc2b2c304e27813b16`
- git_dirty_files: `24`
- output_dir: `docs/benchmarks/parallel-coding/2026-11-19-m13-sse-replay`
- note: M1.3 SSE reconnect replay: publish-time monotonic event ids + bounded replay buffer (SseReplayBuffer/PublishedSseEvent/SsePublisher); GET /api/events honors Last-Event-ID header (preferred) or ?last_event_id=, replays buffered events with strictly greater ids, dedupes the subscribe/snapshot overlap, falls back to live on stale ids after restart, keeps dimension/type filters on replay, and is verified through the real axum Router. /api/events/health reports replay window stats. Counts from actual output.

## 环境

```text
timestamp_utc=2026-11-19T01:41:54Z
scenario=M1.3
host=zeron
kernel=Linux zeron 6.18.58.1-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC Tue Oct 27 00:24:11 UTC 2026 x86_64 GNU/Linux
git_head=254786eeb671c53861a9b3cc2b2c304e27813b16
git_dirty_files=24
pwd=/home/zeron/dev/opencodepro
rustc=rustc 1.100.0 (0ef1bc386 2026-11-09)
cargo=cargo 1.100.0 (3acc79c0c 2026-11-05)
```

## 执行

### Command

```bash
bash -lc cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ sse_replay\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ replay\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ stale_cursor\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ router_replays\ \&\&\ cargo\ test\ -j\ 1\ -p\ sisyphus-bridge\ --lib\ http_openapi 
```

### Result

- exit_code: `0`
- elapsed_ms: `8235`
- log: `run.log`
- summary_json: `summary.json`

### 结论

- PASS：5 条命令均成功（`exit_code=0`，`elapsed_ms=8235`）。过滤执行结果（命令间有重叠，计数以 `run.log` 为准）：`sse_replay` 6 passed；`replay` 15 passed（含 handler 级补发/header 优先/维度过滤/无游标/健康指标）；`stale_cursor` 1 passed；`router_replays` 1 passed（真实 axum Router 的 `Last-Event-ID` 传递）；`http_openapi` 5 passed。全部 `0 failed`。
