# 项目交付状态报告（2026-09-18）

**性质**：静态核对与最小交付改进。证据来自工作区文件、git 状态与提交历史、持续集成（CI）配置与构建脚本源码。**未执行 Cargo、测试或外部服务**；所有测试结果要么来自文档记录（标注"文档记录"），要么来自协调方转述（标注"待验"）。本文不覆盖旧报告，仅记录当日快照。

## 1. 已核实事实（文件证据）

### 1.1 工作区与检查边界

- 工作区显式成员 27 个（`Cargo.toml:2-28`），默认成员 10 个，桥接与沙箱从默认 `cargo check/test` 排除，需 `-p` 单独编译（`Cargo.toml:29-42`）。文档计数不一致：`AGENTS.md:4` 写 30 成员，`CONTEXT.md:9` 写 29 个。
- 检查边界三层：
  - `.cargo/config.toml` 别名 `check-all`/`build-all`/`test-all` 排除 bridge、sandbox、sandboxd、tests 四包。
  - CI（`.github/workflows/ci.yml`）覆盖更广：桥接无默认功能（`:126-153`）、桥接全功能集成测试与 REST 测试（`:224-239`）、沙箱守护进程（`:212-222`）、Web UI（`:563`）等 14+ 独立作业。
  - `make test-ci`（`Makefile` 的 `test-ci` 目标）自述"镜像 CI"，但缺 `skill_market_rest_test`，且桥接步骤仅两项，未含无默认功能单元测试。
- 桥接测试 target 由 Cargo 自动发现（`crates/sisyphus-bridge/Cargo.toml` 无显式 `[[test]]` 段）。

### 1.2 桥接专项（ignore 标记）测试缺执行入口（本次改进前）

- `crates/sisyphus-bridge/tests/bridge_integration_test.rs:282、320` 两个战略内核用例带 `#[ignore = "requires dedicated runner"]`。
- 改进前，`test-bridge`、`test-bridge-full`（`Makefile:109-120`）、`make test-ci`（`:294-296`）与 CI 全部作业均不含 `--include-ignored`，这些用例在任何默认流程中都不会执行。
- **本次改进**：新增 `make test-bridge-dedicated`（`Makefile` 的同名目标）与 `just test-bridge-dedicated`（`justfile:64-65`）。特性：单编译任务（`CARGO_BUILD_JOBS=1`）、无默认功能、`--include-ignored --test-threads=1` 串行执行全部用例；Makefile 支持 `FILTER=<test_name>`，任务镜像支持测试名位置参数，均自动追加 `--exact` 精确筛选单用例。未并入 `make dev`/`make test-ci`/CI，默认排除边界不变。

### 1.3 阶段 9 节点同步（peer sync）实际状态

- 已实现（文件证据）：
  - 发送端 `broadcast_experience`（`crates/sisyphus-bridge/src/peer/experience_sync.rs:26-47`）与接收端 `ExperienceSyncHandler`（`:49-163`）。
  - 对等协议（peer protocol）骨架与静态拨号、TTL 清理（`peer/manager.rs`、`peer/fallback.rs`）。
  - 桥接启动仅接入节点管理与静态拨号（`crates/sisyphus-bridge/src/main.rs:218-258`）。
- 待验/缺口（文件证据）：
  - 生产代码无 `broadcast_experience`/`ExperienceSyncHandler` 调用点（全库搜索仅测试引用），经验同步链路未接入生产。
  - 接收端将远程版本清零后走本地 `record()`（`experience_sync.rs:143-150`），未调用已有 CRDT 合并路径（`sisyphus-mem/src/store.rs:595-634` 的 `merge_crdt`/`apply_remote` 逻辑）。
  - "端到端"测试 `tests/cluster_integration.rs:141-219` 仅单管理器广播 + 订阅断言，无双节点、无落库验证、无冲突/重连场景。
- TODO 状态对照：`TODO.md:344-348` 前三项勾选，"启动时全量 sync"与"运行时增量 sync"未勾选，与代码状态一致；但"发送端调用"勾选表述强于实际（仅框架，无生产调用点）。

## 2. 转述状态（来自协调方，本轮未独立验证）

- 桥接 54 项测试首轮：53 通过、1 失败。失败表现为"单用例明确返回项目不存在，但内核此前可读同一项目"。桥接代理正在修复。**待验：修复提交后的完整测试结果。**
- 核心 crate 未恢复节点错误可能误报 success，另一代理修复中。**待验：修复与回归证据。**
- 以上两项均不构成本报告结论，仅作背景。验证责任在对应修复代理的提交与测试记录。

## 3. 未来三阶段计划（建议）

### 阶段 A（最高优先）：修复验收阻断并收口测试入口
1. 由桥接修复代理定位“内核可读项目、工具却返回项目不存在”的路径差异，修复后复核单用例，再跑完整桥接专项。
2. 由核心修复代理修正未恢复节点错误可能误报成功的问题，补失败传播回归。两项均须留下实际执行结果，不能用干运行替代。
3. 在专用运行环境执行 `make test-bridge-dedicated`，核对实际测试总数、忽略项是否运行及退出状态。对齐或明确说明 `make test-ci` 与持续集成的差异，不扩张默认排除边界。

### 阶段 B：远程合并语义与生产接线
1. 先记录远程合并决策，复用已有 `merge_crdt` 路径并审计原子性、重复消息、时钟冲突、自环和索引更新，避免直接把远程消息当成本地新写入。
2. 桥接启动接入 `ExperienceSyncHandler::spawn`，在经验记录成功后广播，明确关闭及丢失消息处理。
3. 补双节点进程内测试：A 写入后 B 可读，反向亦然，验证并发写入冲突和重复投递，不能仅断言单管理器事件。

### 阶段 C（阶段 9 收尾前）：端到端与文档一致性
1. `TODO.md` 9.1.2 三项按真实覆盖勾选：双节点落库、Docker Compose 三节点、分区恢复，均需对应验收执行通过并记录证据后才可勾选。
2. 统一成员计数：`AGENTS.md`（30）、`CONTEXT.md`（29）与 `Cargo.toml`（27）对齐，注明排除范围。
3. 补启动全量同步与断线增量恢复验收，整理阶段 B 的架构决策记录（ADR），在验收通过后更新完成状态。

## 4. 验证方式与边界

- 本次验证：`make -n test-bridge-dedicated`（干运行，仅打印命令）；`just -n test-bridge-dedicated` 已成功打印同一命令；同时干运行已有目标并检查静态差异。单用例干运行只证明参数展开，不证明实际匹配或运行测试。
- 未验证：任何 `cargo` 命令、测试结果、外部服务、桥接/core 修复代理的中间状态。
- 允许修改范围：`Makefile`、`justfile`、本报告、`TODO.md` 顶部。未提交 git。
