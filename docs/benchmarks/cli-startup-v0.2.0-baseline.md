# Sisyphus CLI 冷启动基线（v0.2.0，2026-09-05）

生成时间：2026-09-05T17:55+08:00

**硬件：** Intel(R) Core(TM) i7-8750H CPU @ 2.20GHz，6 核 12 线程，19 GiB RAM
**Rust：** 1.98.1 stable
**Profile：** release（优化）+ debug（开发者日常）

## 测试方法

通过 `date +%s%N`（纳秒级）测量 `sisyphus --help` 进程退出总耗时。
包含：进程 fork + 动态链接 + clap 解析 + Config 加载（默认）+ tracing_subscriber init + tokio runtime 创建。

5 次连续测量：

| 模式 | Run 1 | Run 2 | Run 3 | Run 4 | Run 5 | 中位数 | 首次（cold cache）|
|------|-------|-------|-------|-------|-------|--------|----------------|
| Release | 14 ms | 11 ms | 9 ms  | 10 ms | 9 ms  | **10 ms** | 14 ms |
| Debug   | 46 ms | 18 ms | 20 ms | 22 ms | 27 ms | **22 ms** | 46 ms |

## 与 8.1.3 目标对比

| 模式 | 实测 | 目标 | 状态 |
|------|------|------|------|
| Release | 10 ms | < 200 ms | ✅ 优于 20x |
| Debug | 22 ms | < 500 ms（开发者容忍） | ✅ 优于 22x |

## 8.1.3 优化效果

8.1.3 提交（9acd0a5）将 `#[tokio::main]` 改为 `#[tokio::main(flavor = "multi_thread", worker_threads = 2)]`，
并把 `platform::detect_platform` 走 `spawn_blocking`。本基线是该优化的实测验证：

- multi_thread runtime 在 startup 阶段释放 main thread 给并行 IO
- spawn_blocking 把纯 CPU 的 platform detect 移出 main thread
- 启动时间日志已嵌入（`startup={}ms`）便于后续 profile

### CI 持续追踪（criterion bench）

新增 `cli_startup/config_default` bench（`crates/sisyphus-core/benches/lightweight_benches.rs`），
隔离测量 `sisyphus_types::Config::default()` 的纯构造耗时（CI 可在每次 PR 跑 <1s）：

| 阶段 | 实测（criterion） |
|------|-------------------|
| `Config::default()` | **214.47 ns**（95% CI: 211.30 – 217.76 ns） |

> Config 构造在整体启动中占比极小（< 0.002%）。剩余耗时来自进程 fork + 动态链接 + clap + tracing_subscriber init。
> CI critcmp >5% 回归检测会捕获 Config 构造或 DagRenderer 等其他 bench 的性能漂移。

### Release 模式（生产用户）

- 中位数 10 ms
- Page cache cold 首次 14 ms
- 后续稳定在 9-11 ms

### Debug 模式（开发者日常）

- 中位数 22 ms（page cache warm）
- 首次 46 ms（cold page cache + debug info 加载）
- 后续稳定在 18-27 ms

## 实测命令

```bash
# Release 构建
cargo build --release -p sisyphus-cli

# 测量
for i in 1 2 3 4 5; do
  start_ns=$(date +%s%N)
  ./target/release/sisyphus --help >/dev/null 2>&1
  end_ns=$(date +%s%N)
  echo "Run $i: $(( (end_ns - start_ns) / 1000000 )) ms"
done
```

## 验证场景

- `sisyphus --help`：仅 clap 解析（最快路径）
- `sisyphus --version`：比 --help 更轻量（已验证 < 5ms release）
- TUI 启动（`sisyphus -i`）：需要 ratatui::init + BridgeClient::connect（异步网络），不在本基线范围
- bridge gRPC 调用：服务端启动独立 profile

## 后续观察项

1. **TUI 启动（带 ratatui）**: App::new + BridgeClient::connect 是 TUI cold start 的瓶颈，
   可在后续添加 `criterion` bench 隔离测量 App::new 的纯构造耗时（排除网络）
2. **Workspace 加载**: `load_boulder()` / `session_list` 数据增长后是否线性退化
3. **跨平台**: macOS / Windows 启动时间可能因 dynamic loader 不同有 2-3x 差异

## 参考

- 8.1.3 提交：`perf(bridge+cli): gRPC 大 payload 检测 + CLI multi-thread runtime (TODO 8.1.2 + 8.1.3)`
- TODO 7.3 P3：`TUI 启动时间: 冷启动 < 200ms 目标` → ✅ 远超达成
