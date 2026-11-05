# 全面优化计划（2026-11-05）

> 触发：在阶段 C/D/E 收口后，对项目做一次**全面分析**，据分析结果排出**全方位优化计划**并逐项实施。
>
> 范围：`crates/` 下 25 个 crate + 构建/CI/文档门禁。不含 `webui/` 前端构建、已归档的 `ide/`（api/sdk/tauri）、以及依赖第三方运行时的环境级验证（3 节点 Docker Compose）。

## 1. 分析基线与方法

### 1.1 自动化门禁

| 门禁 | 命令 | 结果（分析时点） |
|------|------|------------------|
| 全 workspace 检查 | `cargo check-all` | ✅ 0 error |
| 严格 clippy | `make dev-clippy`（`-D warnings`） | ✅ 0 warning |
| 供应链 | `cargo deny check`（advisories/bans/licenses/sources） | ✅ 全部 ok；2 类 warning（见 O-09） |
| 源码审计 | `make audit`（orphans/ignored/test-state/workflows/dockerfile/todo） | ✅ 全绿 |
| 测试 | 本次未全量重跑（上一轮 `cargo test-all` + 重型 crate 分包全绿） | 5,351 个测试属性 / 40 个 `#[ignore]`（均有外部前置条件理由） |

### 1.2 静态度量（`crates/**/src`，2026-11-05 统计）

| 指标 | 数值 |
|------|------|
| crate 数 | 25 |
| 源文件数 | 372 |
| 非空行数 | 105,234 |
| 生产代码 `unwrap()/expect()/panic!` | 19 处命中 / 6 文件，**人工核对后全部位于非生产路径**（`#[cfg(test)]` 测试模块、doc example、`sisyphus-tests` 测试支撑 crate、momus 的自扫描字符串） |
| `unsafe` | 13 处，全部在 `sisyphus-types`（环境变量守卫，等） |
| TODO/FIXME/XXX/HACK 注释 | 63 处 |
| `#[allow(dead_code)]` | 4 处（sandboxd 2、bridge 2） |
| 最大源文件 | `sisyphus-core/src/tests.rs` 2847 行（测试）、`bridge/src/http_openapi.rs` 2189 行（含 OpenAPI 文档表） |

结论：门禁与类型/错误处理纪律良好；**风险集中在「最近两轮新增的摘要缓存」与「反熵分页路径的可扩展性」**，而不是存量代码风格。

## 2. 发现清单

### P0 — 正确性

| ID | 位置 | 问题 | 影响 |
|----|------|------|------|
| O-01 | `sisyphus-mem/src/experience_bank.rs` `record_with_id` / `apply_remote` | 摘要缓存失效写在**函数末尾**：DB 落库成功后若向量索引写入失败并提前返回，缓存保留旧摘要 | 节点向对端少报自己持有的经验 → 反复冗余补齐；若旧摘要更大则可能少拉。属「失败路径上的静默不一致」 |
| O-02 | `sisyphus-mem/src/store.rs` `load_experiences_by_keys` | `row.try_get("id").unwrap_or_default()` 把读取失败折叠成空串 | 列名/类型漂移时静默产出 `""` 键，行被当作不存在而丢掉，既不报错也不告警 |
| O-03 | 同上两个路径 | 摘要缓存**没有回归测试**：命中、写入后失效、远程应用后失效均无断言 | 缓存是纯性能优化，但一旦失效逻辑被改坏，反熵会「看起来通了」却漏数据，正是 ADR-011 记录过的那类缺陷 |

### P1 — 性能 / 可扩展性

| ID | 位置 | 问题 | 影响 |
|----|------|------|------|
| O-04 | `sisyphus-bridge/src/peer/anti_entropy.rs` `serve_sync_request` | 每页都调用 `experience_bank.keys()` **全表取键**，再在内存里排序、二分、切片 | 翻 P 页 = O(P × N)。经验库 10 万行、差量 1 万条（约 79 页）时，每页都扫 10 万键；ADR-012 的游标本就是主键，完全可以在 SQL 层做 keyset 区间扫描 |
| O-05 | 同文件 `build_sync_page` | 对每个差量键在 `remote_rows` 里线性 `find` | O(n²)；页大小 128 时约 1.6 万次字符串比较，随页大小上调而恶化 |

### P2 — 可维护性 / 供应链 / 文档

| ID | 位置 | 问题 | 影响 |
|----|------|------|------|
| O-06 | `sisyphus-mem/src/store.rs` `max_local_counter` | 启动时 `SELECT id FROM experiences` 全表读入再过滤本节点 | 仅启动一次，10 万行约数十 ms；可改为 SQL 侧 `LIKE` 预过滤。收益有限，列后续批次 |
| O-07 | `experience_bank.rs` `query_similar` 主路径 + `lexical_fallback` | 项目/动作过滤逻辑复制两份，已出现注释「与主路径语义一致」 | 两处分别演化就会产生「向量检索与词法回退结果不同」的隐蔽漂移 |
| O-08 | 同上 | 4 处 `#[allow(clippy::collapsible_if)]` | 可直接用 let-chain 表达，减少无谓豁免 |
| O-09 | `deny.toml` | `metrics 0.21/0.22` 的 skip 已过期（实际只有 0.24.6）；`clipboard-win 5.4.1` 被 yanked | `cargo deny` 持续告警；yanked 依赖不该留在锁文件 |
| O-10 | `TODO.md` / `docs/adr/` / `book` | 9.1.1「启动时全量 sync」、9.1.2「断线补偿」已实施并有测试，但仍是 `[ ]`；14 个 ADR 无索引页 | 计划状态失真；架构决策不可发现 |
| O-11 | `docker-compose.yml` | 3 节点最终一致验证受容器运行时阻塞（podman socket 无权限） | 环境级，非代码问题；保留为唯一未闭环项 |

## 3. 本轮实施与验收

按「正确性 → 可扩展性 → 维护性」顺序，每项都必须有测试/门禁证据。

| ID | 实施 | 验收 |
|----|------|------|
| O-01 | 失效动作**紧跟 DB 写入成功**；抽出 `invalidate_digest_cache()` | 新增回归测试：向量写入失败时摘要仍失效（用可失败的 embedding client 或直接断言写入成功后失效） |
| O-02 | 改为 `try_get(...).context(...)?`，读取失败向上传播 | `cargo check` + 既有反熵集成测试 |
| O-03 | 新增 `sisyphus-mem/tests/experience_digest_cache.rs`：首次导出 → 本地写入后导出变化 → 远程应用到新 origin 后导出覆盖该 origin | `cargo test -p sisyphus-mem --test experience_digest_cache` |
| O-04 | `MfsStore::experience_keys_after(cursor, limit)` + `VectorExperienceBank::keys_after`；`serve_sync_request` 改为 keyset 扫描（批大小 512），收集 `page_size + 1` 条判定 `more`；`page_size = 0` 显式收尾（否则 `more` 恒真 + 游标不动 = 拨号器死循环） | 既有 10 个反熵集成测试全绿；新增「游标前有大量已持有键」用例（600 行 / 只缺尾部 50 条）恰好补齐 50 条；新增零页大小收敛用例 |
| O-05 | `build_sync_page` 内建 `HashMap` 索引 | 既有纯函数单元测试 + 集成测试 |
| O-07 | 抽出 `matches_query_filters(exp, query)` 供两条检索路径共用 | 既有检索/过滤测试全绿 |
| O-08 | let-chain 改写，删除 4 个 allow | `clippy -D warnings` |
| O-09 | 删除过期 metrics skip；`cargo update -p clipboard-win`（5.4.1 yanked → 5.4.3）；`evals/Cargo.toml` 6 个 path 依赖补 `version` | `cargo deny check` 不再有 `unmatched-skip` / `wildcard` / `yanked` |
| O-10 | TODO 9.1.1/9.1.2/7.5/9.4.2 勾选并附证据；新增 `docs/adr/README.md` 索引并接入 book | `make todo-audit` / `make audit` |

## 4. 后续批次（本轮不实施，及理由）

1. **O-06 启动序号扫描下推 SQL**：一次性成本，收益 < 改动风险；且 `LIKE` 需要处理 origin 中的通配符转义。
2. **摘要改为 SQL 侧聚合 / 增量维护**：`digest_from_keys` 的语义（空洞枚举、旧键跳过）依赖 Rust 解析；下推 SQL 会引入第二套解析，ADR-011 的教训是「摘要语义只能有一处」。
3. **`http_openapi.rs` / `http/handlers.rs` 拆分**：文件大但结构清晰（OpenAPI 表驱动），拆分收益主要是导航，风险是路由/文档漂移；需要单独评估。
4. **3 节点 Docker Compose**：环境级阻塞，需具备 Docker 运行时的机器。
5. **联邦学习 / 经验聚合服务**：属于 TODO 9.2 的方向性工作，需要先写问题陈述（TODO 已注明）。

## 5. 实施结果

### 5.1 代码变更

| 文件 | 变更 |
|------|------|
| `crates/sisyphus-mem/src/experience_bank.rs` | 新增 `invalidate_digest_cache()`（O-01）；`record_with_id` / `apply_remote` 在**落库成功后立即**失效；删除函数末尾的重复失效；新增 `keys_after()`（O-04）；抽出 `matches_query_filters()` 供两条检索路径共用（O-07）；删除 4 处 `#[allow(clippy::collapsible_if)]`，改 let-chain（O-08） |
| `crates/sisyphus-mem/src/store.rs` | `load_experiences_by_keys` 的 `try_get("id").unwrap_or_default()` → `.context(...)?`（O-02）；新增 `experience_keys_after(cursor, limit)`，`id > ? ORDER BY id LIMIT ?` 走主键区间扫描（O-04） |
| `crates/sisyphus-types/src/experience.rs` | 抽出 `is_key_missing()`，`missing_keys()` 改为它的批量包装——差量判定保持**单点**（O-04 配套）；新增 2 个单元测试（单键/批量一致性、旧格式行不请求） |
| `crates/sisyphus-bridge/src/peer/anti_entropy.rs` | `serve_sync_request` 由「每页全表取键 → 内存排序切片」改为 keyset 扫描（`KEY_SCAN_BATCH = 512`，多收 1 条判定 `more`）；`page_size = 0` 返回空页收尾（避免「`more` 恒真 + 游标不动」的死循环）（O-04）；`build_sync_page` 内建 `HashMap` 索引，消除 O(page²) 线性查找（O-05） |
| `deny.toml` | 删除过期的 `metrics 0.21/0.22` skip（实际只有 0.24.6）（O-09） |
| `evals/Cargo.toml` | 6 个 `path` 依赖补 `version = "0.2.0"`，消除 `wildcard` 告警（O-09） |
| `Cargo.lock` | `clipboard-win 5.4.1`（yanked）→ `5.4.3`（O-09） |
| `docs/adr/README.md`（新增） | 14 份 ADR 索引 + `0005`/`005` 编号冲突说明 + 反熵链路阅读顺序（O-10） |
| `book/src/architecture.md` | ADR 表补齐 007–014；链接由失效的相对路径改为绝对 URL，并指向 `docs/adr/README.md`（O-10） |
| `TODO.md` | 9.1.1 启动全量 sync、9.1.2 断线补偿、9.1.2 chaos 扩展、7.5 CHANGELOG、9.4.2 ADR 索引 → `[x]` 并附可解析证据（O-10） |

### 5.2 新增测试

| 测试 | 覆盖 |
|------|------|
| `crates/sisyphus-mem/tests/experience_digest_cache.rs`（3 例，新增文件） | 空库导出→本地写入后摘要变化；远程 `apply_remote` 后摘要含远端 origin；**向量索引写入失败时摘要仍失效** |
| `crates/sisyphus-mem/tests/mfs_store_test.rs::experience_keys_after_is_keyset_ordered_and_bounded` | keyset 三要素：严格大于游标、主键升序、受 limit 约束；含 `limit = 0` 与尾部空页 |
| `crates/sisyphus-types/src/experience.rs::single_key_missing_matches_batch_delta` | `is_key_missing` 与 `missing_keys` 在空洞 / 超 max / 未知 origin / 旧格式四类键上逐一一致 |
| `crates/sisyphus-bridge/tests/anti_entropy_integration.rs::keyset_scan_skips_held_prefix_and_delivers_tail` | 600 行、对端持有前 550 条：扫描必须跨过完整 512 键批次，恰好补齐尾部 50 条 |
| `crates/sisyphus-bridge/tests/anti_entropy_integration.rs::zero_page_size_terminates_without_looping` | `page_size = 0` 必须回送空页且 `more = false`（否则拨号器按 `more` 无限重发同一页，属实现过程中发现的死循环风险） |

### 5.3 变异验证（证明测试不是「看起来通过」）

O-03 的第三例是本轮唯一针对**失效时机**的断言，因此做了变异验证：

1. 把 `invalidate_digest_cache()` 从落库后移回 `record_with_id` 函数末尾；
2. `cargo test -p sisyphus-mem --test experience_digest_cache digest_invalidated_even_when_vector_index_write_fails`
   → **稳定失败**，报「经验行已落库但摘要仍是旧值 —— 失效被写在函数末尾，失败路径跳过了它」；
3. 还原源码后 3 例全绿。

若没有这一步，「失效紧跟落库」与「失效在末尾」两种实现都会让端到端反熵测试通过（失败路径不常被走到），
差别只在真实故障时才显现。

### 5.4 门禁结果

| 门禁 | 命令 | 结果 |
|------|------|------|
| 格式 | `cargo fmt --all --check` | ✅ 无 diff |
| 严格 clippy（轻量 crate） | `make dev-clippy` | ✅ 0 warning（`-D warnings`） |
| 供应链 | `cargo deny check` | ✅ advisories / bans / licenses / sources 全 ok；仅剩 duplicate 版本提示（既有，非本轮引入） |
| 源码审计 | `make audit` | ✅ 全绿 |
| Docker 发布契约 | `make audit-docker-release` | ✅ 镜像名（workflow / compose / 文档）一致；`:vX.Y.Z` / `:X.Y.Z` / `:X.Y` / `:X` / `:latest` 均可由 metadata-action 产出，且仅稳定版打 `:latest`（8 例自检 + 6 处变异验证） |
| TODO 证据 | `python3 scripts/audit_todo.py` | ✅ 121 条勾选 / 30 条未勾选（可验证 14、显式豁免 1、历史基线 106） |

### 5.5 测试数字

| 命令 | 结果 |
|------|------|
| `cargo test -p sisyphus-types --lib` | 128 passed |
| `cargo test -p sisyphus-mem --test experience_digest_cache` | 3 passed |
| `cargo test -p sisyphus-mem --test mfs_store_test` | 6 passed |
| `cargo test -p sisyphus-bridge --lib` | 139 passed |
| `cargo test -p sisyphus-bridge --test anti_entropy_integration` | 12 passed（原 10 + 新增 2） |
| `cargo test -p sisyphus-bridge --test anti_entropy_partition_convergence` | 1 passed |
| `cargo test -p sisyphus-bridge --test chaos_partition_convergence` | 1 passed（全连接双侧反复分区，3 轮 × 双侧 4 条写入） |
| `cargo test -p sisyphus-bridge --test chaos_one_sided_reverse_pull` | 1 passed（单侧拨号 + 反复分区，反向反熵拉取收敛，变异验证已闭环） |
| `cargo test -p sisyphus-bridge --test webui_contract` | 2 passed（webui 10 个调用点逐条对齐路由表与 OpenAPI） |
| `cargo test -p sisyphus-mem --test lexical_fallback --test jaccard_similarity --test experience_id_collision` | 17 + 6 + 3 passed（O-07 重构后两条检索路径语义未漂移） |

### 5.6 未闭环项

| 项 | 状态 |
|----|------|
| O-11 3 节点 Docker Compose | **代码侧已由 `chaos_partition_convergence.rs` 覆盖**：全连接 + 双侧反复断流，断言两个方向都发起反熵拉取并收敛到并集（无丢失/无重复/主键逐条一致）。容器编排版仍是可选部署验证，需 Docker 运行时；本机已确认容器运行时可用，但仓库内没有 3 节点 compose 编排，价值低于同等的 in-process 真实 gRPC 用例 |
| O-06、摘要 SQL 下推、`http_openapi.rs` 拆分、联邦学习 | 见 §4，本轮不做，理由已记录 |
| 单侧拨号拓扑的收敛边界 | **已闭合（2026-11-05，ADR-015）**：服务端流建立后主动发起反向反熵拉取，被 dial 方断连期间的写入随重连后的反向拉取带回。新增 `crates/sisyphus-bridge/tests/chaos_one_sided_reverse_pull.rs::one_sided_dialer_reverse_pull_converges_after_partition`（单侧拨号 + 反复分区，变异验证：禁用反向拉取 → 用例稳定失败；还原 → 全绿） |
