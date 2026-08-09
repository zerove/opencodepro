# sisyphus-mem — 记忆与缓存

SOP 执行历史、中间结果、配置状态与向量嵌入的三层存储。

## 架构

```
┌──────────┐   write-back     ┌──────────┐   optional   ┌────────────┐
│   L1     │ ◄─────────────── │   L2     │ ◄─────────── │  pgvector  │
│  LRU     │    (buffered)    │  SQLite  │   (ANN index) │ (Postgres) │
│ (memory) │ ───────────────► │  (disk)  │ ────────────► │  (exter.)  │
└──────────┘   read-through   └──────────┘   vector        └────────────┘
                (L1 → L2)                   similarity
```

- **L1**：内存 `LruCache<String, Value>`，容量可配置（默认 10,000）
- **L2**：通过 `sqlx` 的 SQLite，WAL 模式 + 异步写回缓冲
- **向量**：可选 PostgreSQL / pgvector，用于 ANN 相似度搜索

## 关键类型：`SqlStore`

```rust
pub struct SqlStore {
    l1: Arc<Mutex<LruCache<String, CacheEntry>>>,
    pool: AnyPool,  // SQLite 或 PostgreSQL
    write_tx: mpsc::UnboundedSender<WriteOp>,
    config: SqlStoreConfig,
}
```

### 构造

```rust
// SQLite 内存模式（用于测试）
let store = SqlStore::new("sqlite::memory:").await?;

// SQLite 持久化
let store = SqlStore::new("sqlite:data/sisyphus.db").await?;

// PostgreSQL 配 pgvector
let config = SqlStoreConfig {
    database_url: "postgres://user:pass@localhost/sisyphus".into(),
    enable_vectors: true,
    vector_dimension: 1536,
    ..Default::default()
};
let store = SqlStore::with_config(config).await?;
```

### 读写策略

| 操作 | 路径 | 原子性 |
|-----------|------|-----------|
| `set(key, value)` | L1 缓存 → 批量写缓冲 → L2 | 缓冲，每 100ms 或 100 次操作刷写 |
| `get(key)` | 查 L1 → 未命中查 L2 → 填充 L1 | 无锁 LRU 读取 |
| `flush()` | 强制将缓冲写入刷到 L2 | oneshot 屏障 |
| `snapshot()` / `restore()` | L1 + L2 往返 | 全表导出 / 清空 + 重载 |

### 向量搜索

```rust
// SQLite 后端：全表扫描 + 内存余弦相似度
// PostgreSQL 后端：pgvector <->（余弦距离）算子 + ANN 索引
let results = store.search_similar(
    Some("my-project"),
    "experiences",
    &[0.1, 0.2, ..., 0.9],
    5,  // top-k
).await?;
```

## 其他存储

| 类型 | 说明 |
|------|-------------|
| `SopSqlStore` | SOP 定义 CRUD（DAG 节点 + 边） |
| `RoadmapSqlStore` | 项目里程碑、阶段与目标 |
| `SkillMarket` | 远程技能市场（发布 / 安装 / 卸载） |
| `WorkspaceManager` | 工作区生命周期（创建 / 切换 / 转移 / 导出） |
| `VectorMemoryStore` | 高阶向量记忆，支持用户作用域集合 |
| `UserMemoryStore` | 长期用户偏好记忆 |
| `MfsStore` | 遗留扁平 KV 存储（建议迁移至 SqlStore） |