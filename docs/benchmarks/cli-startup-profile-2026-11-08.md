# Sisyphus CLI 冷启动 Profile（2026-11-08，post-v0.4.0）

生成时间：2026-11-08

**构建**：`cargo build --release -p sisyphus-cli`（`Finished release profile [optimized]`）
**二进制**：`target/release/sisyphus`，ELF x86-64，16M（与 v0.2.0 基线同量级）

## 非交互启动（`--help` / `--version`）

| 运行 | elapsed (wall) | Max RSS |
|------|----------------|---------|
| `--version` | 0.00 s | ~8.4 MB |
| `--help` #1 | 0.00 s | ~8.7 MB |
| `--help` #2 | 0.00 s | ~8.5 MB |
| `--help` #3 | 0.00 s | ~8.6 MB |

（`/usr/bin/time -v/-f`，同机连续运行；wall clock 低于计时分辨率显示为 0.00s。）

## 说明与边界

- 本 profile 覆盖 **CLI 参数解析/帮助路径**的冷启动成本，不等同于 **ratatui 交互式 TUI**
  首帧到可操作的端到端时间（TUI 需 TTY/终端初始化，交互路径需专用 harness）。
- 与 `docs/benchmarks/cli-startup-v0.2.0-baseline.md`、`docs/benchmarks/v2-lightweight-baseline-2026-09-02.md`
  中的启动数据处于同一量级（release、~8–9 MB RSS），**未观察到 v0.4.0 引入的启动退化信号**。
- 严格 TUI 首帧/交互延迟建议后续用可脚本化的 PTY/expect 或 `hyperfine` + 非交互子命令对比；
  本文档作为 TODO 7.3「TUI 冷启动 profile」的可复现快照证据。

## 证据

```bash
env -u RUSTUP_TOOLCHAIN cargo build --release -p sisyphus-cli
/usr/bin/time -v ./target/release/sisyphus --version
/usr/bin/time -f "elapsed=%e maxrss=%M" ./target/release/sisyphus --help
```
