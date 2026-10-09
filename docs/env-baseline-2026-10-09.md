# 环境基线记录（不属项目贡献）

这些数字只说明「测的时候机器本来就不空」，不构成对项目的任何判断。
把环境负载和项目自身的回归混为一谈，是本项目之前踩过的坑。

## 2026-10-09 13:20–14:10 期间

| 项 | 值 |
|---|---|
| load average | 11.34 → **39.83**（1min） |
| 内存 | 19 GiB 总量，used 9，available 9–10 |
| top 占用者 | `beam.smp` 165% CPU、`podman` 多实例、多个 `java`（含 972 MB / 1.4 GB RSS）、`grafana` |

## 归属判定（阳性对照）

阳性对照：同期 `pgrep -a cargo` / `pgrep -a rustc` 确认
**我们自己的编译进程只有 2 个**（`CARGO_BUILD_JOBS=2`），
`-pcpu` 榜首是 `beam.smp` 与 `podman`，都不是本项目进程。

结论：load 39.83 由 Erlang/Ecto、podman、java、grafana 造成，
**与本仓库无关**。同期 `sisyphus-tests` 的编译仍正常推进。

## 与 flaky 的关系

`cognitive_trace_persist_test` 在全量并发下要 306.47s、隔离复跑只要
29.29s。SQLite 的 `busy_timeout` 是 5s（sqlx 默认，仓库内**没有**
显式 `PRAGMA busy_timeout`），锁竞争时写入会在 5s 内失败。

注意区分两件事：
- 磁盘与内存回收是**本项目**的贡献（`target/` 130 GB → 38 GB，-40% 并发 RSS）；
- load 39.83、beam/podman/java 的占用是**环境**噪声，不是本项目造成的。

所以：不能因为这台机器负载高，就把测试耗时变长或偶发失败算到项目头上；
同样也不能把项目自身的 93.4 GB 回收说成是环境优化的功劳。