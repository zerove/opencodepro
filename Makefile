BRIDGE_DEPS = crates/sisyphus-bridge/src/main.rs crates/sisyphus-bridge/Cargo.toml crates/sisyphus-bridge/proto/bridge.proto
# Build targets for crates excluded from workspace commands.
#
# Memory notes:
#   Full `cargo check --workspace` OOMs on <16GB machines due to wasmtime/cranelift deps.
#   Three crates pull in wasmtime: sisyphus-sandbox (direct), sisyphus-tests (direct),
#   and sisyphus-bridge (default features enable optional sandbox).
#
# Strategy: never use `cargo check --workspace` in CI. Instead:
#   - Lightweight batch: `--workspace --exclude <3 heavy crates>`
#   - Heavy crates get per-crate jobs (isolated, no OOM)
#
# sisyphus-bridge  — rustc 1.96.0 type-checking ICE (SIGTERM during `cargo check`).
#                    RESOLVED in nightly 1.98.0+; ships with 1.97.0 stable on 2026-07-09.
#                    While on 1.96.0: `make check-bridge-160` uses -C lto=off.
# sisyphus-sandbox — heavy wasmtime/cranelift deps; rarely needs recompiling.
# sisyphus-core   — wasmtime transitive dep; full test needs CARGO_BUILD_JOBS=1.
# sisyphus-tools-* — 10 sub-crates, lightweight, fast to check individually.
#
# 内存约束（2026-10-02 实测）：在 19.5 GiB 机器上并行链接多个重型测试二进制会
# OOM，报 `linking with cc failed` 或 `could not exec the linker cc`。因此所有会产生
# 多个测试二进制的目标都显式带 CARGO_BUILD_JOBS=1。直接敲 `cargo test` 不受此保护，
# 需自行加 -j 1。
#
# ⚠ 修正（2026-10-08 再核）：当时记录的 `vm.overcommit_memory=2`（严格模式）**已不成立**。
#   2026-10-08 实测 `/proc/sys/vm/overcommit_memory` = **0**（启发式宽松模式）。
#   那条观察本身没造假——2026-10-04 那天确实可能是 2——但它是**环境状态**，
#   不是仓库属性，写死在注释里会误导后来的人。
#   保留原结论：链接期撞上限仍会抛 EAGAIN（`what(): Resource temporarily unavailable`）
#   → SIGABRT，即便 free available 还有 10 GiB。**排查务必看 Committed_AS，
#   别拿 free available 当「已排除 OOM」的依据**（我犯过这个错）。
#   `make rustc-abort-forensics` 会实时打印提交内存占限比。
#   换机器或改 sysctl 后，请用下面的命令重新核对再下结论：
#     cat /proc/sys/vm/overcommit_memory
#
# 内存实测（2026-10-08，用 scripts/mem_probe.py 采样，**冷编译**
#   sisyphus-sandbox 含 cranelift/wasmtime 全套）：
#
#     并行度   rustc 并发驻留峰值   进程数   冷编译耗时
#     默认 12        2.22 GB         12      411.9s
#     -j 1           633 MB          1      >600s（未跑完）
#
#   两个要点：
#   1. **单个** rustc 峰值就有 633 MB（cranelift），所以这不是「多进程才有的问题」；
#      -j 1 也不能保证安全，只是把风险从 2.22 GB 降到 633 MB。
#   2. **别把 Committed_AS 绝对值当成编译的影响**——本机 IDE 的 7 个 java 进程
#      就占了 22.9 GB（基线就已 106.9% of CommitLimit），编译根本没参与。
#      真正的编译增量只有 1.58 GB。第一次测量就踩了这个坑，得出「编译导致
#      超限」的错误结论，进而想去调一个根本不相关的旋钮。
#      用 mem_probe.py 看的是**增量**，不是绝对值。
#
#   真正的瓶颈是常驻进程而非编译。但编译侧的 12 并行仍会额外吃掉 2.22 GB，
#   所以本文件里所有会产生多个测试二进制的目标都显式带 CARGO_BUILD_JOBS=1。
#   注意：**裸敲 `cargo` 不受此保护**（.cargo/config.toml 无 [build] jobs 兜底），
#   会用 nproc=12 并行。改并行度请用 make 目标，不要裸敲。
#
# 磁盘约束（2026-10-08 实测）：多套构建变体（--no-default-features、--features sandbox）
#   并存会让 target/ 累积大量陈旧产物，实测涨到 130 GB（deps 下 11003 个 rlib，
#   libtower 一个包就有 184 个不同 hash 的副本）。用 `make target-gc` 回收，
#   默认只删 14 天前的产物且保护 bridge/sandbox 等单独构建的 crate。
#
# Usage:
#   make check-all         # lightweight batch (alias: --exclude bridge/sandbox/tests)
#   make check-sandbox     # sandbox alone (wasmtime, isolated)
#   make check-tests       # integration tests alone (wasmtime, isolated)
#   make check-bridge      # bridge alone
#   make check-bridge-160 # check bridge only (1.96.0 fallback with -C lto=off)
#   make build-bridge     # build bridge only
#   make test-bridge      # test bridge only
#   make test-bridge-dedicated  # manual: ignore 标记的专用 runner 用例 (单任务/串行)
#   make test-core        # full sisyphus-core test suite
#   make test-core-quick  # lib-only sisyphus-core tests (~2s)
#   make test-tools       # test all 10 sisyphus-tools-* sub-crates (lib only, fast)
#   make check-tools      # check all 10 tools sub-crates
#   make target-gc        # 预览可回收的磁盘空间（不会删除）
#   make target-gc-apply  # 真正回收 14 天前的产物


TOOLS_PACKAGES = \
	sisyphus-tools-agent sisyphus-tools-build sisyphus-tools-core \
	sisyphus-tools-executor sisyphus-tools-llm sisyphus-tools-llm-tools \
	sisyphus-tools-mcp sisyphus-tools-scm sisyphus-tools-session \
	sisyphus-tools-system

HEAVY_PACKAGES = sisyphus-sandbox sisyphus-sandboxd sisyphus-tests

.PHONY: all check-all build-all test-all test-all-chunk test-all-chunk-reset scan-flaky flaky-replay rustc-abort-forensics
.PHONY: check-bridge build-bridge test-bridge check-bridge-160
.PHONY: check-bridge-nosandbox test-bridge-nosandbox
.PHONY: check-sandbox build-sandboxd test-sandbox check-sandboxd test-sandboxd test-tests check-tests
.PHONY: test-bridge-full test-bridge-dedicated test-cli-handlers
.PHONY: test-ci
.PHONY: perf-analyze
.PHONY: target-gc target-gc-apply target-size
.PHONY: dev-tui run-tui clean
.PHONY: dev ci-quick dev-fmt dev-clippy dev-test ci-bridge

all: check-all

# — 一键开发入口（阶段 7.4 DX）—

# make dev: 完整本地验证流水线 (轻量级 + bridge)
# 跳过重型 crate (sandbox/tests)，按依赖顺序:
#   1) fmt 校验     → 防止风格漂移
#   2) clippy 严格  → 提前抓 lint
#   3) lib 单元测试  → 快速回归
#   4) bridge 单独   → 单独验证（重型 crate）
dev: ci-quick ci-bridge
	@echo ""
	@echo "✅ 完整本地验证通过（fmt + clippy + lib tests + bridge clippy）"

# make ci-quick: 不含重型 crate 的快速检查（默认开发循环）
ci-quick: dev-fmt dev-clippy dev-test
	@echo "✅ ci-quick: fmt + clippy + lib tests"

# make dev-fmt: 仅 fmt 校验
dev-fmt:
	@echo "━━━ 1/4 fmt check ━━━"
	cargo fmt --all --check

# make dev-clippy: 全工作区 clippy 严格（排除重型）
dev-clippy:
	@echo "━━━ 2/4 clippy ━━━"
	cargo clippy --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests -- -D warnings

# make dev-test: lib 单元测试
dev-test:
	@echo "━━━ 3/4 lib tests ━━━"
	cargo test --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests --lib

# make ci-bridge: bridge 单独 clippy（重型 crate 隔离）
ci-bridge:
	@echo "━━━ 4/4 bridge clippy ━━━"
	cargo clippy -p sisyphus-bridge -- -D warnings

# — workspace commands (exclude heavy crates) —
#
# 2026-10-04 补：`check-all` / `build-all` 原先没带 CARGO_BUILD_JOBS=1，
# 与 `test-all` 不一致。实测本机会在 `cargo check-all` 默认并行下撞
# rustc SIGABRT（ICE 标记 `0x0`），与 Makefile 头部记录的 OOM 现象同源。
# `check` 虽然不链接最终二进制，但 rustc 前端的 codegen 峰值同样吃内存。
#
# ⚠ 实测修正（2026-10-04 晚）：`test-all` **已带** `-j 1` 仍出现过一次 SIGABRT，
# 同一条命令重跑即通过。**根因已查明**（详见头部注释与取证目标）：
# 链接期 rust-lld 撞 `Committed_AS` 上限抛 EAGAIN。`-j 1` 把并行链接数
# 降到 1，确实降低提交峰值——它**不是**无效，只是无法消除单链接器的占用。
# 遇到 SIGABRT：**直接重跑**，并用 `make rustc-abort-forensics` 留现场。

check-all:
	CARGO_BUILD_JOBS=1 cargo check-all

build-all:
	CARGO_BUILD_JOBS=1 cargo build-all

test-all:
	CARGO_BUILD_JOBS=1 cargo test-all
	@echo ""
	@echo "━━━ ⚠️  本次未覆盖的 crate ━━━"
	@echo "sisyphus-bridge / sisyphus-sandbox / sisyphus-sandboxd / sisyphus-tests"
	@echo "因编译开销大被排除。它们是本仓**唯一做跨进程/跨组件验证**的地方"
	@echo "（tonic / gRPC / wasmtime / TcpListener），本地全绿 ≠ 全量验证。"
	@echo "这四个 crate 在 CI 中有对应 job："
	@echo "  .github/workflows/ci.yml — bridge_integration_test / skill_market_rest_test / sisyphus-tests --lib"
	@echo "本地需单独验证请用： cargo test -p sisyphus-bridge --no-default-features --lib"

# 逐 crate 跑 test-all 的每个成员，每跑完一个就把名字追加到 .testall-progress。
#
# 为什么要这个而不是直接用 `make test-all`：
# `cargo test --workspace` 是一次性调用，23 个 crate 串在同一进程里，
# 任一环节卡住（或被 600s 外部超时打断）就**全部白跑**，且 `tail` 缓冲
# 让你看不到中间进度，只能看到最后几行。
# 实测 2026-10-08：`make test-all` 跑满 600s 被杀，已完成 24 个测试
# 二进制却一条结论都拿不到。
#
# 而 `make test-all` 内部那条超时用例（cognitive_trace_persist_test）
# 在隔离复跑时只要 29.29s，在全量并发下却要 306.47s —— 逐 crate 跑
# 既能拿到完整结论，也能看出哪些用例对负载敏感。
#
# 实测耗时（2026-10-09，回收 93.4 GB 磁盘之后）：23 个 crate 全部通过，
# 0 失败 0 超时。**总计约 2650s（44 分钟）**，而不是早期记的 2405s ——
# 那个数字只是 22 个 crate 的和：`sisyphus-core` 是在外层 bash 被超时
# 杀掉后、作为孤儿进程跑完的（32 个 ok 块 / 0 失败，已核验），我随后手动
# 把它记进了 .testall-progress，因此它没有对应的计时行，累加时被漏掉。
# 由进程启动时刻与日志最后写入时间反推，core 约 247–274s。
# 2405s 与「23/23 全过」并不矛盾，但两者不可同时相加使用。
#
# 最慢的几个：sisyphus-cli 364s、sisyphus-knowledge 229s、
# sisyphus-intel 219s、sisyphus-mem 208s。这解释了为什么单次
# `make test-all` 必然超时：总耗时是 600s 上限的四倍多。
#
# 用法：
#   make test-all-chunk        # 跑全部，已完成的自动跳过
#   make test-all-chunk-reset  # 清空进度，下次从头跑
#
# 逐个跑还能给每个 crate 单独计时，从而定位“哪个 crate 是慢的那个”。
test-all-chunk:
	@bash scripts/run_test_chunks.sh

test-all-chunk-reset:
	@rm -f .testall-progress .testall-timings .testall-chunk.log .testall-chunk.log.* .testall-chunk-summary.log
	@echo "已清空进度，下次 make test-all-chunk 将从头开始"

# 概率性失败（flaky test）扫描：把每个 crate 的 lib 测试二进制重复跑多轮，
# 统计失败率。单次 `cargo test` 全绿无法证明不存在竞态——本项目多处缺陷
# 失败率都在 1/16 以下，只在高频复跑下才暴露。
#
# 用法：make scan-flaky ROUNDS=32
#
# 排除项与 test-light 一致：bridge / sandboxd 在本仓库会 OOM（19.5 GiB 下
# -j 2 都不安全）。脚本的 SLOW_CRATES 只排除「慢」crate，不含这两个，
# 所以排除必须写在这里——否则预编译阶段就会被 OOM 杀掉，扫不成。
scan-flaky:
	CARGO_BUILD_JOBS=1 cargo test --workspace --lib --no-run \
		--exclude sisyphus-bridge --exclude sisyphus-sandboxd --exclude sisyphus-sandbox
	python3 scripts/scan_flaky.py --rounds $(or $(ROUNDS),16)

# 仅重跑上次扫描抓到的失败现场（现场已落盘，不必重新扫描）。
flaky-replay:
	@test -d "$(or $(REPORT_DIR),$$JCODE_SCRATCH_DIR/flaky-reports)" || \
		{ echo "没有现场目录；先跑 make scan-flaky"; exit 2; }
	@ls -1 "$(or $(REPORT_DIR),$$JCODE_SCRATCH_DIR/flaky-reports)"/*.log 2>/dev/null || true

# — rustc / 链接器 SIGABRT 取证 —
#
# 背景：实测出现过
#   error: linking with `cc` failed
#     = note: what(): Resource temporarily unavailable        ← EAGAIN
#   collect2: fatal error: ld terminated with signal 6 [Aborted]
# 崩溃发生在 **rust-lld（链接期）**，而非 rustc codegen 阶段。
# 根因（本机 vm.overcommit_memory=2 严格模式 + ratio=80）：
# Committed_AS 撞上 CommitLimit（实测峰值 18.4/21.1 GiB = 87%），
# 即使 free available 还有 10 GiB 也会分配失败。
#
# ⚠ 排查时**务必看 Committed_AS 而非 free available**，后者会误导：
#    真实 RAM 空闲 != 提交额度空闲，两者差一个数量级。
#
# 用法：make rustc-abort-forensics            # 默认 5 轮
#       make rustc-abort-forensics ROUNDS=20  # 加轮次
#       JOBS=4 make rustc-abort-forensics     # 改变构建并发（默认 1）
#
# 原理：每轮 touch 一个 crate 内源文件强制 rustc **真正重编 sisyphus-core**
#       （不加 touch 则 cargo 命中缓存直接返回，循环毫无意义）。
#       每轮打印提交内存占限比；一旦复现，现场由 systemd-coredump 接管。
rustc-abort-forensics:
	@echo "=== 前置检查 ==="
	@echo "  overcommit_memory = $$(cat /proc/sys/vm/overcommit_memory 2>/dev/null || echo '?') (0=启发 1=始终 2=严格)"
	@echo "  overcommit_ratio  = $$(cat /proc/sys/vm/overcommit_ratio 2>/dev/null || echo '?')"
	@c=$$(awk '/Committed_AS/{print $$2}' /proc/meminfo); \
	 l=$$(awk '/CommitLimit/{print $$2}' /proc/meminfo); \
	 echo "  提交内存 Committed_AS = $$((c/1024)) MiB / CommitLimit $$((l/1024)) MiB = $$(( c*100/l ))%"; \
	 echo "  ⚠ 这是实际的墙；free available 高**不代表**提交额度充裕"
	@echo "  ulimit -c = $$(ulimit -c) (0 = 崩溃不留现场，本目标已覆盖)"
	@command -v coredumpctl >/dev/null 2>&1 \
		&& echo "  ✓ coredumpctl 可用（复现后 coredumpctl info <PID> 看栈）" \
		|| echo "  ⚠ coredumpctl 不可用；请人工检查 /var/lib/systemd/coredump/"
	@mkdir -p "$(or $(FORENSIC_DIR),$$JCODE_SCRATCH_DIR/forensics)"
	@echo "=== 循环重编 $(or $(ROUNDS),5) 轮（JOBS=$${JOBS:-1}，每轮强制 touch 重编）==="
	@fail=0; i=0; \
	while [ $$i -lt $(or $(ROUNDS),5) ]; do \
		i=$$((i+1)); \
		touch crates/sisyphus-core/src/lib.rs; \
		log="$(or $(FORENSIC_DIR),$$JCODE_SCRATCH_DIR/forensics)/round-$$i.log"; \
		( ulimit -c unlimited; CARGO_BUILD_JOBS=$${JOBS:-1} \
			cargo build -p sisyphus-core --tests ) >"$$log" 2>&1 & \
		bpid=$$!; \
		while kill -0 $$bpid 2>/dev/null; do \
			c=$$(awk '/Committed_AS/{print $$2}' /proc/meminfo); \
			l=$$(awk '/CommitLimit/{print $$2}' /proc/meminfo); \
			printf "  [%2d/%s] commit=$$((c/1024))/$$((l/1024))MiB($$(( c*100/l ))%%) \r" \
				"$$i" "$(or $(ROUNDS),5)"; \
			sleep 5; \
		done; \
		wait $$bpid; rc=$$?; \
		if [ $$rc -eq 0 ]; then echo " ok  (第 $$i 轮)"; \
		else \
			echo " ✗ 崩溃复现（第 $$i 轮，exit=$$rc）"; fail=1; \
			echo "--- 立即取证 ---"; \
			echo "    日志: $$log"; \
			grep -m2 -E "what\(\)|signal 6|linking with" "$$log" | sed "s/^/    /"; \
			coredumpctl list --no-pager 2>/dev/null | tail -3 \
				|| echo "    (coredumpctl 不可用)"; \
			echo "    栈:   coredumpctl info <PID>"; \
			break; \
		fi; \
	done; \
	if [ $$fail -eq 0 ]; then \
		echo "=== $(or $(ROUNDS),5) 轮均未复现 —— 阴性结果，不证明成因 ==="; \
	fi
	@touch crates/sisyphus-core/src/lib.rs 2>/dev/null || true


# — 结构审计（不需要编译，纯静态扫描）——

# 孤儿源文件：src/ 下从未被 mod 引入的文件，不参与编译。
# 曾出现 crates/sisyphus-mem/src/tests.rs（227 行 7 个用例）从未被引用，
# 看似有覆盖实则从未运行过。
audit-orphans:
	@python3 scripts/audit_orphans.py

# 忽略测试的理由审计：#[ignore] 必须指向外部前置条件（网络/凭据/工具链/
# 运行中服务/已声明缺口），不能是「测试当前失败」。CI 不跑 --include-ignored，
# 被忽略即等于没有覆盖。
audit-ignored:
	@python3 scripts/audit_ignored.py

# 测试隔离审计：检测依赖进程级全局状态（set_current_dir、
# set_global_default、set_var 无 panic 安全清理、改写生产变量名）。
# 本项目已因此付出两次实价：eval bench 的 cwd 竞争（42e0c1e）与
# write_workflow 写入源码树（ab42b75）。这类缺陷比孤儿文件隐蔽——
# 只在并行下偶发，失败现场常被后续测试覆盖，看起来像网络抖动。
audit-test-state:
	@python3 scripts/audit_test_global_state.py

# workflow 引用审计：检测引用了未声明 id 的 step。
# GitHub 只为显式声明 id 的 step 暴露 outputs，无来源的引用会**静默**
# 展开成空字符串——不会让 CI 变红，只在渲染结果里表现为空链接或缺值。
# ci.yml 曾真实存在这种缺陷（PR 评论渲染成 `([详情]())`），靠读 PR 才能发现。
audit-workflows:
	@python3 scripts/audit_workflows.py
	@python3 scripts/audit_workflows.py --self-check

# 全部结构审计（不需编译）
audit: audit-orphans audit-ignored audit-test-state audit-workflows

.PHONY: audit-orphans audit-ignored audit-test-state audit-workflows audit

# — bridge crate (nightly+, no workaround needed) —

check-bridge: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge

build-bridge: $(BRIDGE_DEPS)
	cargo build -p sisyphus-bridge

test-bridge: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge

check-bridge-nosandbox: $(BRIDGE_DEPS)
	cargo check -p sisyphus-bridge --no-default-features

test-bridge-nosandbox: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --no-default-features --lib

test-bridge-full: $(BRIDGE_DEPS)
	cargo clippy -p sisyphus-bridge -- -D warnings
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --test bridge_integration_test

# — bridge dedicated-runner tests（#[ignore = "requires dedicated runner"] 用例）—
#
#   仅在专用运行环境手动执行，不进默认 CI/dev 流水线：
#   - CARGO_BUILD_JOBS=1  限制单编译任务，避免内存受限环境 OOM
#   - --no-default-features  不编译可选 sandbox 依赖
#   - -- --include-ignored --test-threads=1  跑全部用例且串行，避免共享状态互扰
#
#   用法：
#     make test-bridge-dedicated          # 全部用例（含 ignore）
#     make test-bridge-dedicated FILTER=test_execute_tool_strategic_next_goal_missing_project  # 单用例

test-bridge-dedicated: $(BRIDGE_DEPS)
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-bridge --no-default-features --test bridge_integration_test -- --include-ignored --test-threads=1 $(if $(FILTER),$(FILTER) --exact,)

test-cli-handlers:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-cli --lib

check-stress-test:
	cargo check -p sisyphus-bridge --bin stress_test

# — core benchmarks (isolated, sandbox feature required) —

bench-core:
	cargo bench -p sisyphus-core --features sandbox -- --warm-up-time 1 --measurement-time 2 --sample-size 10

# — performance analysis (run benchmarks + critcmp comparison) —
#
#   Runs core benchmarks with --save-baseline current, then compares
#   against the previously saved main-baseline using critcmp.
#   If no baseline exists, saves current results as the baseline.
#
#   Prerequisites:
#     cargo install critcmp --locked   (first time)
#
#   Usage:
#     make perf-analyze                # compare with existing baseline
#     make perf-analyze BASELINE=myref # compare against a custom baseline name

perf-analyze:
	@echo "━━━ Running benchmarks (save as current) ━━━"
	cargo bench -p sisyphus-core --features sandbox -- --save-baseline current --warm-up-time 1 --measurement-time 2 --sample-size 10
	@BASELINE_NAME="${BASELINE:-main-baseline}"; \
	if ls target/criterion/*/"$$BASELINE_NAME"/ 2>/dev/null >/dev/null; then \
		echo "━━━ Comparing current vs $$BASELINE_NAME ━━━"; \
		if command -v critcmp >/dev/null 2>&1; then \
			critcmp current "$$BASELINE_NAME" 2>&1; \
			echo ""; \
			echo "━━━ Regression check (>5% = warning) ━━━"; \
			critcmp current "$$BASELINE_NAME" --json 2>/dev/null | python3 -c " \
import json,sys; data=json.load(sys.stdin); \
regs=[(n,i) for n,i in data.items() if isinstance(i,dict) and isinstance(i.get('change'),(int,float)) and i['change']>5.0]; \
[print(f'WARNING: {n}: {p[\"change\"]:.1f}% slower') for n,p in regs] if regs else print('No regressions >5%'); \
"; \
		else \
			echo "critcmp not installed. Install with: cargo install critcmp --locked"; \
		fi; \
	else \
		echo "━━━ No $$BASELINE_NAME found. Saving current as $$BASELINE_NAME ━━━"; \
		for dir in target/criterion/*/; do \
			if [ -d "$${dir}current" ]; then \
				cp -r "$${dir}current" "$${dir}$$BASELINE_NAME"; \
			fi; \
		done; \
		echo "Benchmark baseline saved as '$$BASELINE_NAME'"; \
	fi

# — E2E smoke test (bridge daemon + stress_test) —

e2e-smoke:
	@echo "━━━ Starting bridge in background ━━━"
	cargo build -p sisyphus-bridge
	cargo build -p sisyphus-bridge --bin stress_test
	@./scripts/e2e-smoke.sh

# — E2E /health/deep smoke test (启动 bridge → curl → 验证 JSON 结构) —

e2e-health: $(BRIDGE_DEPS)
	cargo build -p sisyphus-bridge
	cargo build -p sisyphus-bridge --bin stress_test
	@./scripts/e2e-health.sh

# — bridge crate, 1.96.0 fallback (RUSTFLAGS bypasses rustc 1.96.0 ICE) —

check-bridge-160: $(BRIDGE_DEPS)
	RUSTFLAGS="-C lto=off" cargo check -p sisyphus-bridge

# — sisyphus-tools-* sub-crates (10 independent, no heavy deps) —

test-tools:
	for pkg in $(TOOLS_PACKAGES); do \
		echo "━━━ testing $$pkg (lib)"; \
		CARGO_BUILD_JOBS=1 cargo test -p "$$pkg" --lib; \
	done

check-tools:
	for pkg in $(TOOLS_PACKAGES); do \
		echo "━━━ checking $$pkg"; \
		cargo check -p "$$pkg"; \
	done

# — sisyphus-core (wasmtime transitive dep needs CARGO_BUILD_JOBS=1) —

test-core:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-core

test-core-quick:
	cargo test -p sisyphus-core --lib

test-core-min:
	cargo test -p sisyphus-core --lib --no-default-features

# — CI simulation —

test-ci-core:
	cargo fmt --all --check
	cargo clippy -p sisyphus-core
	cargo test -p sisyphus-core --lib
	cargo test -p sisyphus-core

# — dev workflow for TUI (fast edit → compile → test) —
#
#   Terminal 1: make dev-tui    # auto-builds on file change
#   Terminal 2: make run-tui    # (re)launch after build finishes
#
#   Or in one terminal with tmux:
#     tmux split-window -h 'make dev-tui'
#     make run-tui

SISYPHUS_CLI_SRC = crates/sisyphus-cli/src

dev-tui: $(SISYPHUS_CLI_SRC)
	@echo " Watching $(SISYPHUS_CLI_SRC)/ for changes — Ctrl+C to stop"
	@while inotifywait -q -r -e modify,create,delete,move $(SISYPHUS_CLI_SRC)/; do \
		echo "━━━ changed → cargo build -p sisyphus-cli"; \
		cargo build -p sisyphus-cli 2>&1 | tail -5; \
		echo ""; \
	done

run-tui:
	cargo run -p sisyphus-cli

# — sisyphus-sandbox (wasmtime, heavy, isolated) —

check-sandbox:
	cargo check -p sisyphus-sandbox

test-sandbox:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-sandbox --lib

# — sisyphus-sandboxd (wasmtime via sandbox, heavy, isolated daemon) —

check-sandboxd:
	cargo check -p sisyphus-sandboxd

build-sandboxd:
	cargo build --release -p sisyphus-sandboxd

# cranelift-codegen 即使单任务也会把 rustc 撑爆（实测 rustc-LLVM OOM），
# 关掉调试信息才编得过。代价是该配置下需要全量重编。
test-sandboxd:
	CARGO_BUILD_JOBS=1 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p sisyphus-sandboxd

# — sisyphus-tests (wasmtime via sandbox, heavy, isolated) —

check-tests:
	cargo check -p sisyphus-tests

test-tests:
	CARGO_BUILD_JOBS=1 cargo test -p sisyphus-tests --lib

# — CI simulation (mirrors .github/workflows/ci.yml) —

test-ci:
	@echo "━━━ 1/14 Format ━━━"
	cargo fmt --all --check
	@echo "━━━ 2/14 Lint Light ━━━"
	cargo clippy --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests -- -D warnings
	@echo "━━━ 3/14 Lint Sandbox ━━━"
	cargo clippy -p sisyphus-sandbox -- -D warnings
	@echo "━━━ 4/14 Lint Sandboxd ━━━"
	cargo clippy -p sisyphus-sandboxd -- -D warnings
	@echo "━━━ 5/14 Lint Tests ━━━"
	cargo clippy -p sisyphus-tests -- -D warnings
	@echo "━━━ 6/14 Test Light ━━━"
	cargo test --workspace --exclude sisyphus-bridge --exclude sisyphus-sandbox --exclude sisyphus-sandboxd --exclude sisyphus-tests
	@echo "━━━ 7/14 Test Core ━━━"
	cargo test -p sisyphus-core
	@echo "━━━ 8/14 Test Sandbox ━━━"
	cargo test -p sisyphus-sandbox --lib
	@echo "━━━ 9/14 Test Sandboxd ━━━"
	cargo test -p sisyphus-sandboxd
	@echo "━━━ 10/14 Bridge Full Test ━━━"
	cargo clippy -p sisyphus-bridge -- -D warnings
	cargo test -p sisyphus-bridge --test bridge_integration_test
	@echo "━━━ 11/14 Test CLI Handlers ━━━"
	cargo test -p sisyphus-cli
	@echo "━━━ 12/14 Test Integration ━━━"
	cargo test -p sisyphus-tests --lib
	@echo "━━━ 13/14 Bench Core ━━━"
	cargo bench -p sisyphus-core --features sandbox -- --warm-up-time 1 --measurement-time 2 --sample-size 10
	@echo "━━━ 14/14 CLI Build ━━━"
	cargo build --release -p sisyphus-cli
	@echo "━━━ CI simulation complete ━━━"

# — convenience —

# — agent evals (57 cases across metis/momus/librarian/scribe/security) —

eval:
	cargo run -p eval-runner

eval-check:
	cargo check -p eval-runner

clean:
	cargo clean

# — 磁盘回收（2026-10-08 新增） —
# 为什么要单独做：多套构建变体（--no-default-features、--features sandbox）共存时，
# target/ 会累积大量陈旧产物。实测涨到 130 GB，deps 下有 11003 个 rlib，
# libtower 一个包就积累了 184 个不同 hash 的副本，最旧的可回溯到 2.5 个月前。
# `cargo clean` 太粗（连当前要用的产物一起删），`cargo gc` 仍是 nightly-only，
# 所以这个脚本按「年龄 + 保护名单」回收。
#
# 默认只预览不删除：删除不可逆，且误删会让下次构建全量重编。

target-size:
	@du -sh target 2>/dev/null || echo "target/ 不存在"
	@echo "--- 分层 ---"
	@du -sh target/debug target/release 2>/dev/null || true
	@echo "--- deps 内产物数 ---"
	@ls -1 target/debug/deps 2>/dev/null | wc -l | xargs echo "文件数:"

# 内存探针：包裹一条命令并采样期间的内存。
#
# 默认用 sisyphus-types 只是为了让命令短。它是轻量 crate，**不能代表**
# 重型冷编译的内存峰值——之前就是拿它得出「峰值 326 MB、内存合理」
# 的错误结论。测真实峰值请用 sandbox / bridge 这类带 wasmtime、
# tonic 的 crate，并且必须用独立 CARGO_TARGET_DIR 保证是冷编译。
#
#   make mem-probe                          # 默认（轻量，仅冒烟）
#   make mem-probe PKG=sisyphus-sandbox JOBS=2 COLD=1   # 真冷编译 -j 2
#
# 本机（i7-8750H 6C12T / 19 GiB）冷编译 `cargo check -p sisyphus-sandbox`，
# 独立 CARGO_TARGET_DIR + 全冷依赖树，2026-10-09：
#
#   并行度   rustc RSS 峰值   进程数   耗时
#   -j 12       2.40 GB        12      224s
#   -j  2       1.43 GB         2      542s
#
# 两行可比：同工具、同命令、同冷启动条件，且均为**修正 --jobs 之后**测的
# （即 CARGO_BUILD_JOBS 真正生效）。-j 2 省 0.97 GB（-40%），代价 318s（+142%）。
#
# ⚠ 不要再引用更早的 -j 12 = 2.22 GB / 412s 与 -j 1 = 633 MB：
#   前者测于 mem_probe 修正 --jobs 之前（--jobs 只打印不生效，实际跑的是
#   裸 cargo 默认并行度），已用上表 2.40 GB / 224s 取代；
#   后者是 `cargo build` 的**单进程** RSS（/usr/bin/time -v 口径），
#   与本表「并发驻留总和」不是同一指标，不可并列。
#
# 结论：`-j 2` 是本机的平衡点。注意耗时代价不小（+142%），
# 只在内存吃紧时才用；日常热缓存增量编译两者差别很小。
#
# ⚠ 刻意**不**写进 `.cargo/config.toml` 的 `[build] jobs`：那是仓库级配置，
# 会让 CI 与高性能机器也无条件变慢。真正受限的只有这台机器的重型命令，
# 而 `make check-all` / `test-all` 等已自带 `CARGO_BUILD_JOBS=1`，
# 裸跑 cargo 才会落到 12 并行——那正是需要手动传 `JOBS=2` 的场景。
MEM_PROBE_PKG ?= sisyphus-types
MEM_PROBE_JOBS ?= 1
mem-probe:
	@if [ "$(COLD)" = "1" ]; then \
	    echo "冷编译模式：独立 target 目录（不影响主缓存）"; \
	    d=$$(mktemp -d /home/zeron/.cache/sisyphus-memprobe.XXXXXX); \
	    CARGO_TARGET_DIR="$$d" python3 scripts/mem_probe.py --jobs $(MEM_PROBE_JOBS) -- cargo check -p $(MEM_PROBE_PKG); \
	    rc=$$?; \
	    du -sh "$$d" 2>/dev/null | sed 's/^/本次冷编译占用: /'; \
	    rm -rf "$$d"; \
	    exit $$rc; \
	else \
	    python3 scripts/mem_probe.py --jobs $(MEM_PROBE_JOBS) -- cargo check -p $(MEM_PROBE_PKG); \
	fi

target-gc:
	@python3 scripts/target_gc.py --self-check
	@echo ""
	@python3 scripts/target_gc.py

target-gc-apply:
	@python3 scripts/target_gc.py --self-check
	@echo ""
	@python3 scripts/target_gc.py -y

.PHONY: target-gc target-gc-apply target-size mem-probe
