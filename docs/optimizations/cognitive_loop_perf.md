# 认知循环性能优化计划

## 1. 目标
降低 sisyphus-core 认知循环的端到端延迟，减少内存分配频率，并提升大规模图执行时的稳定性。

## 2. 优化项详细设计

### 2.1 短期优化 (Quick Wins)

| 优化项 | 描述 | 预期效果 | 优先级 |
| :--- | :--- | :--- | :--- |
| **减少序列化开销** | 将 `CognitiveEvent` 的传输从 `serde_json::Value` 改为强类型结构，推迟序列化至边缘端。 | $\downarrow$ CPU 占用, $\downarrow$ 内存碎片 | P0 |
| **NodeOutput 引用计数化** | 将 `NodeOutput` 内部的 `Value` 包装在 `Arc` 中，消除深拷贝。 | $\downarrow$ 内存带宽压力, $\downarrow$ 延迟 | P0 |
| ** Trace 构建优化** | 使用 `String` 缓冲区替代 `join("\n")`，优化日志拼接方式。 | $\downarrow$ 临时内存分配 | P1 |
| **结构化错误传递** | 消除 `.map_err(|e| CoreError::Internal(e.to_string()))`，使用强类型错误变体。 | $\downarrow$ 字符串分配, $\uparrow$ 可调试性 | P1 |

### 2.2 长期架构优化 (Architectural)

| 优化项 | 描述 | 预期效果 | 优先级 |
| :--- | :--- | :--- | :--- |
| **流水线执行 (Pipelining)** | 在 `Reflect` 阶段同步启动下一轮的投机性 `Predict`。 | $\downarrow$ 循环总时长 | P2 |
| **ExecutionContext 复用** | 引入对象池，减少 `DashMap` 等结构在每轮迭代中的重新分配。 | $\downarrow$ 分配频率 | P2 |
| **经验库高效检索** | 实现基于向量/哈希的快速经验匹配，加速认知收敛。 | $\downarrow$ 迭代次数 (算法级优化) | P1 |
| **异步锁精细化** | 迁移至 `tokio::sync` 锁并优化锁粒度。 | $\uparrow$ 高并发吞吐量 | P3 |

## 3. 实施路径
1. **基准测试** $\rightarrow$ 运行 `cargo bench -p sisyphus-core` 记录初始数据。
2. **实施 P0 项** $\rightarrow$ 验证 $\rightarrow$ 提交。
3. **实施 P1 项** $\rightarrow$ 验证 $\rightarrow$ 提交。
4. **长期规划** $\rightarrow$ 分阶段引入架构变更。

## 4. 验证标准
- **性能指标**：认知迭代平均耗时下降 $\ge 15\%$。
- **内存指标**：单次循环产生的临时分配量下降 $\ge 20\%$。
- **正确性**：通过 `cargo test -p sisyphus-core` 且行为与原版本一致。
