# sisyphus-knowledge — 知识管理

知识 crate 为认知资产（工具、SOP、洞察、约束）提供基于图的存储与检索，使系统能够沉淀所学知识并在会话间复用。

## 核心类型

### KnowledgeGraph

存储资产及其关系的内存图结构：

```rust
pub struct KnowledgeGraph {
    graph: DiGraph<String, RelationType>,
    node_map: HashMap<String, NodeIndex>,
    assets: HashMap<String, CognitiveAsset>,
}
```

关键操作：
- `add_asset(asset)` — 向图中插入资产
- `add_relation(rel)` — 在两个资产之间创建有向边
- `get_asset(id)` — 按 ID 检索资产
- `find_assets_by_kind(kind)` — 按类型过滤资产（Tool、SOP、Insight、Constraint）
- `get_related_assets(id, rel_type)` — 沿边查找关联资产

### AssetManager

`KnowledgeGraph` 的异步包装，支持持久化与智能检索：

```rust
pub struct AssetManager {
    graph: Arc<RwLock<KnowledgeGraph>>,
    storage_path: PathBuf,
}
```

## 功能

### 持久化

知识图可作为 JSON 保存到磁盘并从磁盘加载：

```rust
let manager = AssetManager::new(PathBuf::from(".sisyphus/knowledge.json"));

// 保存资产
manager.save_asset("asset_1", "File Reader".into(), content, 1.0).await?;

// 持久化到磁盘
manager.save().await?;

// 从磁盘加载（在新会话中）
let manager2 = AssetManager::new(PathBuf::from(".sisyphus/knowledge.json"));
manager2.load().await?;
```

### 智能查询

使用不区分大小写的模糊匹配搜索匹配目标的资产：

```rust
// 跨名称、描述与内容字段搜索
let results = manager.query_tools_for_goal("file reading").await;

// 结果排序：名称匹配的排名高于描述匹配
for asset in results {
    println!("{}: {}", asset.name, asset.description);
}
```

### LLM 辅助检索

对复杂查询，使用 LLM 查找最匹配的资产：

```rust
let best = manager.find_best_asset("read configuration files", llm_client).await?;
if let Some(asset) = best {
    println!("Best match: {}", asset.name);
}
```

## 资产类型

| 类型 | 说明 | 内容类型 |
|------|-------------|--------------|
| `Tool` | WASM 工具规范 | `ToolSpec` |
| `SOP` | 标准操作程序图 | `SOPGraphConfig` |
| `Insight` | 带理由的观察 | `InsightData` |
| `Constraint` | 带严格级别的规则 | `ConstraintData` |

## 关系

资产可通过有向边连接：

| 关系 | 含义 |
|----------|---------|
| `Solves` | 资产 A 解决了资产 B 描述的问题 |
| `Requires` | 资产 A 需要资产 B 才能运行 |
| `EvolvedFrom` | 资产 A 源自资产 B |
| `ConflictsWith` | 资产 A 与资产 B 不兼容 |
| `Refines` | 资产 A 是资产 B 的更具体版本 |

## 集成

知识 crate 被 `sisyphus-core` 用于：
- **自我编程管理器**（Self-Programming Manager）— 存储学到的工具与 SOP
- **战略管理器**（Strategic Manager）— 检索规划相关的知识
- **内化管理器**（Internalization Manager) — 将完成的工作沉淀为持久资产