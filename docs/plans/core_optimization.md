# Sisyphus 核心引擎与持久化优化设计方案

本文档针对 `sisyphus-core`（DAG 执行引擎与认知循环）以及 `sisyphus-mem`（持久化存储层）在超大规模并发和长时间运行会话中的性能瓶颈，提出系统性的架构优化设计方案。

---

## 1. 当前架构痛点分析

### 1.1 `sisyphus-core` DAG 执行引擎痛点
- **全局粗粒度锁竞争**：现有的 DAG 状态机和节点状态更新通常使用单一的 `Arc<RwLock<DagState>>` 或全局锁，当并行任务数量激增（如上百个子任务并发派发）时，状态机调度和依赖检查成为严重的串行化瓶颈。
- **任务状态通知机制冗余**：目前的轮询或粗粒度条件变量（Condvar / notify）会导致大量不必要的唤醒和 CPU 空转。
- **认知循环（Cognitive Loop）上下文传递开销**：在 predict → execute → reflect → evolve 循环中，大尺寸上下文通过克隆或整体序列化传递，缺乏按需懒加载和写时复制（COW）机制。

### 1.2 `sisyphus-mem` 持久化存储层痛点
- **全量快照（Full Snapshot）IO 阻塞**：每次状态演进或持久化检查点（Checkpoint）触发时，往往将整个 DAG 状态和内存资产整体序列化写入磁盘，随着时间推移，快照体积膨胀导致主线程卡顿。
- **缺少增量日志（WAL / Delta Log）机制**：缺乏高效的 Write-Ahead Log 与增量状态合并架构，崩溃恢复（Crash Recovery）成本高。

---

## 2. 优化设计目标

1. **无锁/细粒度锁 DAG 调度**：将全局状态机拆分为基于节点（Node-level）的原子状态机与依赖计数器（ArcAtomic / DashMap），实现真正的无锁并发推进。
2. **高效增量持久化（Delta Snapshot & WAL）**：引入基于版本号的增量日志和定期合并（Compaction）机制，将持久化开销降至最低。
3. **零拷贝/流式上下文传递**：在认知循环中优化内存占用，避免不必要的大对象复制。

---

## 3. 详细设计方案

### 3.1 模块一：`sisyphus-core` 细粒度并发 DAG 状态机

#### A. 核心数据结构重构
- **节点状态隔离**：将每个 DAG 节点的执行状态（`NodeState`：Pending / Running / Completed / Failed）独立封装，并使用 `AtomicU8` 或独立 `tokio::sync::watch` 通道进行状态广播。
- **无锁入度追踪**：利用 `AtomicUsize` 维护每个节点的未完成前置依赖数（In-degree），当节点完成时通过原子 `fetch_sub` 递减，当入度降为 0 时直接推入无锁任务就绪队列（`crossbeam::queue::SegQueue`），无需加锁遍历整个 DAG。

#### B. 调度循环升级
```rust
// 伪代码示例：基于原子入度的无锁就绪推进
pub struct DagNode {
    pub id: NodeId,
    pub in_degree: AtomicUsize,
    pub state: AtomicU8,
    pub dependents: Vec<NodeId>,
}
```

### 3.2 模块二：`sisyphus-mem` 增量快照与 WAL 机制

#### A. 增量日志设计
- **Delta Log Entry**：每一次 DAG 节点状态变更、内存资产更新均记录一条轻量级 Delta 记录（`Timestamp`, `NodeId`, `FieldOp`, `Payload`）。
- **后台异步刷盘（Async Flusher）**：Delta 记录追加到内存环形缓冲区，由后台 tokio 任务批量异步刷入磁盘 WAL 文件。

#### B. 快照与压缩（Compaction）
- 每隔 $N$ 个 Delta 或当 WAL 文件大小超过阈值时，触发后台快照生成任务，将最新全量状态安全写入临时文件并原子替换（Atomic Rename），随后清理旧 WAL。

---

## 4. 实施路线图

1. **Phase 1：指标埋点与基准测试**
   - 完善 `sisyphus-core` 和 `sisyphus-mem` 的 criterion benchmark，测定高并发 DAG 调度和全量持久化的耗时基线。
2. **Phase 2：`sisyphus-core` 原子化重构**
   - 引入 `AtomicUsize` 入度管理与 `SegQueue` 就绪队列，替换全局锁调度。
3. **Phase 3：`sisyphus-mem` 增量持久化实现**
   - 实现 WAL 记录追加与异步刷盘，支持崩溃恢复。
4. **Phase 4：集成验证与性能压测**
   - 运行全套集成测试与高并发压测，验证吞吐量提升与内存抖动降低效果。
