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

## 覆盖情况（诚实记录）

**没能为这个修复写出能捕获它的端到端用例。** 尝试过两次，都失败了：

1. 第一版让 A 在信号发出后写入经验、断言 B 最终收到。变异（还原 `resubscribe`）
   下**依然通过**——查日志才发现那条经验是走**实时广播**到的 B，与反熵无关，
   属于典型假覆盖。改成**直写 A 的经验库**（不走 `broadcast.record`）后，
   变异能被捕获了，但**修复后的代码同样失败**：信号是在拨号器 spawn 之前发出的，
   而 spawn 自己那一行 `resubscribe` 就会把它丢掉——这不在本次修复覆盖的窗口内。
2. 想改成「断连期间发信号」来对齐修复窗口，但 `PeerManager` 没有可用的
   断流手段，无法确定性地让流停在某个时刻再发信号。

根本原因是这个修复**没有可观测的行为差异**：有它和没它，最终都收敛，
因为建连时的全量拉取兜住了。要捕获它，必须能观测「信号是否被消费」，
而当前没有任何指标或日志能区分「消费了信号」与「靠全量拉取补齐」。

按本仓库的证据标准（每项声明都要有真实测试 + 变异测试支撑），这个修复
标记为**逻辑上正确但测试未捕获**。不声称它已被验证。

## 代价

`run_stream` 的参数从「拥有接收器」变成「借用接收器」，调用方需持有 `mut`。
无运行时开销——`broadcast` 通道本来就在，只是省掉了每次重连的一次 `resubscribe`。

## 遗留

- 若将来去掉建连时的无条件全量拉取（改为仅靠信号驱动），必须先补上
  「信号可达」的可观测性，否则这类静默丢弃会变成真实的数据丢失窗口。
- `ExperienceSyncHandler` 的 `no subscriber` 日志实际不可达（见上），可考虑
  改为在 dial loop 全部退出时才启用，或直接删除以免误导。
