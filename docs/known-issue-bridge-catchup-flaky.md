# 已知问题（间歇性）：bridge 集成测试 catchup 补齐信号偶发请求风暴

**状态**：⚠️ 未修复（已定位，间歇性复现；登记待修）
**影响面**：CI `Bridge Integration Tests (no sandbox)` job 偶发红，不影响发布物正确性
**首次定位**：2026-11-17（CI run 43964613856，commit 4ec4a37）
**解决方式**：待 bridge 负责人修复（断言改为有界窗口或加时序重试）；本登记提供定位证据

## 现象

`crates/sisyphus-bridge/tests/cluster_integration.rs` 的
`catchup_signal_sent_while_reconnecting_is_still_consumed` 偶发失败：

```
assertion `left == right` failed: 超出 3 次，说明补齐变成了请求风暴
  left: 4
 right: 3
```

同一 commit 重跑即绿（run 43986331228 全绿），呈间歇性。

## 根因分析（待验证）

断言 `assert_eq!(gate.sync_requests_seen(), 3, ...)` 要求补齐信号恰好触发 1 次
（首次拉取 1 + 重连后首次拉取 1 + 保留信号触发的补齐 1）。
时序敏感点：重连窗口内补齐信号可能被重复投递，或消费端与信号到达存在竞态，
使 `sync_requests_seen()` 偶发为 4。

## 与防回归门禁的关系

失败运行（43964613856）由 commit 4ec4a37 触发，但该 commit 只动
`.github/workflows/ci.yml`、`Makefile`、`AGENTS.md`、`scripts/audit_*.py`，
与 bridge 运行时无关；后续 commit 4ccd2cd 的同一 job 全绿，排除代码回归。

## 复检清单

- [ ] `cargo test -p sisyphus-bridge --test cluster_integration catchup_signal` 本地连续 3 次全绿（环境敏感，需专用 runner）
- [ ] 修复后本文件归档为「已修复」并附实跑输出
