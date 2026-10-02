# ADR-013：补齐信号跨重连复用接收器，消除静默丢弃

## 背景

ADR-011 引入「Lagged → 广播补齐信号 → 拨号器重发 `SyncRequest`」的补偿链路。
ADR-012 在复核其测试覆盖时发现：`catchup_deferred_while_paging_converges_exactly`
曾经是**假覆盖**——把延后改成丢弃（`drop(catchup_pending)`）用例仍然通过。

装上 tracing subscriber 抓日志后，那次失效的真实原因比「信号落在 `paging = None`」
更靠前一层：**两个分支一次都没执行**，信号根本没进拨号器的接收端。

## 现象

`run_dial_loop` 每次（重）连接都给 `run_stream` 传一份**新**接收器：

```rust
// 修复前
run_stream(..., catchup_rx.as_ref().map(broadcast::Receiver::resubscribe)).await
```

`broadcast::Receiver::resubscribe` 只从订阅那一刻开始计数，**看不到订阅之前
已发出的信号**。于是断连期间（或流任务尚未进入 `select!` 时）发出的补齐信号
被整个跳过。

最关键的细节是它**不报错、无日志**：`tx.send(())` 返回 `Ok`，因为 spawn 阶段
就已经存在一个订阅者（`spawn_static_peers_with_catchup` 内部持有的那份）。
服务端日志里的 `catchup_signalled=true` 只表示 `catchup_tx.is_some()`，
不表示有人听见了。临时探针实测对照：

| 时序 | `send()` | `try_recv()` |
|------|----------|--------------|
| 先发后 `resubscribe`（实际发生的） | `Ok`（无错） | `Empty`（信号丢失） |
| 先 `resubscribe` 后发 | `Ok` | `Ok`（收到） |

即**发送成功 ≠ 信号被消费**。

## 决策

把**同一个**接收器跨重连复用，不再每次重连重新订阅：

- `run_dial_loop` 的参数改为 `mut catchup_rx`，按 `&mut` 传给 `run_stream`。
- `run_stream` 的参数改为 `Option<&mut broadcast::Receiver<()>>`。
- 信号因此留在通道里，重连后第一轮 `select!` 就能取到（容量 1 下多次合并为
  `Lagged`，`recv_catchup` 已把 `Lagged` 视作「需要补齐」）。

顺带修正一处**死代码**：`ExperienceSyncHandler` 里 `tx.send()` 失败时打的
`Lagged catchup signal has no subscriber` 日志，在有 peer 任务存活时永远不会
触发——`spawn_static_peers_with_catchup` 一进来就 `resubscribe` 了一份，那份
接收器跟着 dial loop 活到进程结束。保留该分支是为了语义完整（确实可能没有任何
peer），但**不能**把它当作信号可达性的证据。

## 这个窗口会不会丢数据

**不会。** `run_stream` 在每次流建立时无条件发一次全量反熵拉取（ADR-010 引入），
重连即全量补偿。因此即使信号被丢弃，断连期间的丢失也会在重连时补齐，最终仍然收敛。

修复的收益不是「止住数据丢失」，而是：

1. 消除一次**静默的、无日志的**信号丢弃——这类失败一旦出现在别的场景下
   （例如未来去掉建连时的全量拉取）就没有任何可观测性。
2. 让 `catchup_signalled=true` 这个日志重新变得可信。

## 覆盖情况

用例：`catchup_signal_sent_before_connect_is_still_consumed`
（`crates/sisyphus-bridge/tests/cluster_integration.rs`）。

### 关键：断言计数，不断言数据

这个 bug 在数据上**完全不可见**——`run_stream` 每次建连都无条件全量拉取，
所以信号丢不丢，B 的最终行集合一模一样。只断言收敛状态的用例对它恒真。

因此断言的是**服务端收到的 `SyncRequest` 次数**：

- 建连时首次全量拉取 → 1 次
- 建连前发出的信号被消费 → 再触发一次补齐请求 → 共 2 次

计数由 ADR-013 新增的 `sync_requests_seen` 提供（服务端收到请求时自增，早于应答）。

### 关键：用 Join 闸门制造窗口

需要一个「拨号器已 spawn、但尚未建连」的确定性时刻。`PeerManager` 没有可用的
断流手段，所以改为在**被拨号方**把 `Join` 挡住：拨号任务已启动（接收器已创建），
却卡在 `connect_and_join`，此时发出的信号精确落在目标窗口内。

闸门用 `Semaphore`（0 许可 = 关闭）而非 `Notify`：信号量**保留**已发放的许可，
而 `notify_one` 在无等待者时会丢失唤醒。

两个必须踩过才知道的细节：

1. **闸门要装在被拨号方，不是拨号方。** 拨号方 B 连的是 A；把闸门装在 B 自己身上
   毫无作用，B 会直接连上 A 跑完，计数恒为 0（第一版就是这么写的，失败于
   `left: 0`）。
2. `add_permits(n)` 的 `n` 是**新增**许可数，传 `usize::MAX >> 1` 会 panic
   （超过 `MAX_PERMITS`）。发 1 个就够：每个 Join 取走许可后立刻返回并释放。

### 变异验证

把 `run_dial_loop` 还原成 `catchup_rx.as_ref().map(broadcast::Receiver::resubscribe)`
**并同步还原 `run_stream` 的签名**（只改调用点会类型不匹配、编译不过，
那不是有效变异），用例稳定失败：

```
left: 1
right: 2
```

连续 3 次变异运行均为同一签名；修复后连续 8 次运行全绿。

### 注意：`make test-all` 不包含 bridge

`.cargo/config.toml` 的 `test-all` 显式 `--exclude sisyphus-bridge`（因为 bridge
测试在 CI 会 OOM）。因此本 ADR 涉及的用例必须单独跑：

```bash
cargo test -p sisyphus-bridge --test cluster_integration
```

声称「全量套件通过」时若只跑 `make test-all`，并不覆盖这里任何一行代码。

### 一次错误的结论

本文档最初的小节写的是「没能写出能捕获它的用例，标记为测试未捕获」。**那是错的。**
当时把「无法构造确定性窗口」当成了外部约束，实际上那只是**缺少可观测性**——
补上 `SyncRequest` 计数与 Join 闸门之后，窗口是确定性的，变异立刻被捕获。

教训：这个 bug 的数据结果是恒真的，所以在没有计数之前，任何「用例通过」都不构成证据；
反过来，「写不出用例」也不等于「不可验证」——先问是不是缺观测手段。

## 代价

- `run_stream` 的参数从「拥有接收器」变成「借用接收器」，调用方需持有 `mut`。
- `ClusterServiceImpl` 多两个字段（一个 `AtomicU64`、一个 `Semaphore`），
  以及 `Join` 开头一次 `acquire()`。生产路径闸门恒有 1 个许可，这次 `acquire`
  立即返回，不引入额外延迟。
- 无运行时开销——`broadcast` 通道本来就在，只是省掉了每次重连的一次 `resubscribe`。

## 遗留

- 若将来去掉建连时的无条件全量拉取（改为仅靠信号驱动），「首次拉取 vs 信号补齐」
  的计数区分将失效，届时需要新的可观测点。
- `ExperienceSyncHandler` 的 `no subscriber` 日志实际不可达（见上），可考虑
  改为在 dial loop 全部退出时才启用，或直接删除以免误导。
