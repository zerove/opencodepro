#!/usr/bin/env bash
# T1.4 L3 真实闭环 —— 可复跑场景脚本
#
# 三个场景（每个都是「真实仓库任务」，不是 mock）：
#   s1 诊断修复：注入编译错误（缺分号）→ 要求修复，判定 `cargo check` 通过
#   s2 质量门  ：注入 clippy 告警（bool_comparison/needless_bool）→ 要求消除，
#                判定 `cargo clippy -- -D warnings` 通过
#   s3 功能新增：只有 add() → 要求新增 multiply() + 单元测试，判定 `cargo test`
#                通过且确实运行了 ≥1 个测试、multiply 存在
#   s4 多文件重构：add() 与 format_sum() 同处 lib.rs → 要求把 add() 拆到新文件
#                src/math.rs 并声明模块，判定 `cargo test` 通过、math.rs 存在、
#                lib.rs 不再定义 add（复制粘贴不算重构）
#   s5 长上下文：src/lib.rs 有 400 个 helper（约 2.3k token，超过本地 provider 的
#                有效窗口）→ 在**末尾**追加一个函数；判定 `cargo test` 通过、
#                新函数存在、**400 个 helper 一个不少**（长文件不得被改残）
#                **opt-in**：不在默认集合内。本地 3B 模型当前无法完成（见
#                docs/benchmarks/acceptance/2026-11-12/phase1/t1-4-l3-closed-loop.md §4.5），
#                该场景用于覆盖缺口与暴露缺陷，默认跑会返回非零。
#
# 依赖：
#   - 一个 OpenAI 兼容的真实 LLM 端点（默认走本机 ollama）
#   - 可选：`scripts/l3-llm-proxy.py` 记录请求/响应（T1.4 证据要求）
#
# 用法：
#   ./scripts/l3-run-scenarios.sh [s1] [s2] [s3] [s4] [--model M] [--timeout-secs N]
#                               [--upstream URL] [--workdir DIR]
#
# 退出码：0 = 所有被选场景判定 PASS；1 = 有场景未通过（属正常结果，需看结论）。
#
# `--yes`：本脚本是自动化验收场景，需要模型可以执行命令（如 `cargo check`）。
#        CLI 的 dev 路径默认对「需要人工确认的命令」fail-safe 拒绝（非交互环境），
#        自动化必须显式授权；显式 deny 的权限规则仍然拒绝。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/debug/sisyphus"
MODEL="qwen2.5-coder:3b"
UPSTREAM="http://127.0.0.1:11434/v1/chat/completions"
TIMEOUT_SECS=900
WORKDIR="${TMPDIR:-/tmp}/opencode/l3"
RECORDS_DIR=""
SCENARIOS=()

while [ $# -gt 0 ]; do
  case "$1" in
    s1|s2|s3|s4|s5) SCENARIOS+=("$1"); shift ;;
    --model) MODEL="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    --upstream) UPSTREAM="$2"; shift 2 ;;
    --workdir) WORKDIR="$2"; shift 2 ;;
    --records-dir) RECORDS_DIR="$2"; shift 2 ;;
    --cli) CLI="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done
[ ${#SCENARIOS[@]} -eq 0 ] && SCENARIOS=(s1 s2 s3)

mkdir -p "$WORKDIR"
RECORDS_DIR="${RECORDS_DIR:-$WORKDIR/records}"
mkdir -p "$RECORDS_DIR"
LOG="$WORKDIR/llm.jsonl"
PROXY_PORT="${PROXY_PORT:-11500}"
PROXY_PID=""
RESULTS=()

cleanup() {
  [ -n "$PROXY_PID" ] && kill "$PROXY_PID" 2>/dev/null
  wait "${PROXY_PID:-}" 2>/dev/null
}
trap cleanup EXIT

# ── 0. 起记录代理（把 LLM 请求/响应落 JSONL）──────────────────
: > "$LOG"
python3 "$REPO_ROOT/scripts/l3-llm-proxy.py" \
  --port "$PROXY_PORT" --upstream "$UPSTREAM" --log "$LOG" \
  >"$WORKDIR/proxy.log" 2>&1 &
PROXY_PID=$!
for _ in $(seq 1 40); do
  (exec 3<>"/dev/tcp/127.0.0.1/$PROXY_PORT") 2>/dev/null && break
  sleep 0.25
done

CFG="$WORKDIR/config.toml"
cat > "$CFG" <<TOML
bridge_address = "http://127.0.0.1:15401"
http_address = "http://127.0.0.1:15402"

[llm]
api_key = "local"
endpoint = "http://127.0.0.1:$PROXY_PORT/v1/chat/completions"
model = "$MODEL"
request_timeout_secs = 600

[[llm.providers]]
name = "l3-local"
kind = "openai"
endpoint = "http://127.0.0.1:$PROXY_PORT/v1/chat/completions"
api_key = "local"
model = "$MODEL"
priority = 1
enabled = true
TOML

# ── 场景模板 ─────────────────────────────────────────────────
reset_s1() {
  local d="$WORKDIR/s1"
  rm -rf "$d"; mkdir -p "$d/src"
  cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "l3-s1"
version = "0.1.0"
edition = "2021"
TOML
  cat > "$d/src/main.rs" <<'RS'
fn main() {
    let total = add(1, 2)
    println!("{}", total);
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
RS
}

reset_s2() {
  local d="$WORKDIR/s2"
  rm -rf "$d"; mkdir -p "$d/src"
  cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "l3-s2"
version = "0.1.0"
edition = "2021"
TOML
  cat > "$d/src/lib.rs" <<'RS'
pub fn is_ready(flag: bool) -> bool {
    if flag == true {
        true
    } else {
        false
    }
}
RS
}

reset_s3() {
  local d="$WORKDIR/s3"
  rm -rf "$d"; mkdir -p "$d/src"
  cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "l3-s3"
version = "0.1.0"
edition = "2021"
TOML
  # TDD 形态：测试放在**独立文件** tests/multiply.rs 并引用尚未存在的 multiply。
  # - `cargo test` 因此编译失败 → 闸门无法被「删掉测试」绕过；
  # - 提示里明确要求不改测试。
  cat > "$d/src/lib.rs" <<'RS'
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
RS
  mkdir -p "$d/tests"
  cat > "$d/tests/multiply.rs" <<'RS'
use l3_s3::multiply;

#[test]
fn multiply_works() {
    assert_eq!(multiply(3, 4), 12);
    assert_eq!(multiply(-2, 5), -10);
}
RS
}

# s5：长上下文。lib.rs 有 400 个 helper（约 2.3k token），超过本地 provider 的
# 有效窗口（ollama 默认 num_ctx 较小）→ 引擎应检测并提示「prompt 被截断」。
# 任务本身放在**文件末尾**（截断保留尾部，仍可完成），判定额外要求：
# 400 个 helper **一个都不能少**（模型不得把长文件改残）。
reset_s5() {
  local d="$WORKDIR/s5"
  rm -rf "$d"; mkdir -p "$d/src" "$d/tests"
  cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "l3-s5"
version = "0.1.0"
edition = "2021"
TOML
  {
    echo "// 长文件：400 个 helper，用于验证长上下文下的改动不会把文件改残。"
    local i
    for i in $(seq 1 400); do
      echo "pub fn helper_${i}(v: i64) -> i64 { v + ${i} }"
    done
  } > "$d/src/lib.rs"
  # 测试放独立文件，避免「删测试」绕过闸门
  cat > "$d/tests/long.rs" <<'RS'
use l3_s5::scale;

#[test]
fn scale_works() {
    assert_eq!(scale(3, 4), 12);
    assert_eq!(scale(-2, 5), -10);
}
RS
  # 上下文 = 整个长文件（模型需要看到文件末尾才能追加）
  {
    # 上下文顺序刻意与引擎一致：先给长文件，**最后**再给任务说明 ——
    # provider 截断长 prompt 时保留尾部，任务必须落在尾部才不会被裁掉。
    echo "项目是 Rust lib crate。src/lib.rs 里**已经存在** 400 个 helper 函数（下面全文）。"
    echo "---- src/lib.rs 全文开始 ----"
    cat "$d/src/lib.rs"
    echo "---- src/lib.rs 全文结束 ----"
    echo ""
    echo "任务只有一个：在**文件最后**追加下面这个函数，其余内容一个字都不要改："
    echo "pub fn scale(v: i64, k: i64) -> i64 { v * k }"
  } > "$WORKDIR/s5-context.txt"
}

reset_s4() {
  local d="$WORKDIR/s4"
  rm -rf "$d"; mkdir -p "$d/src" "$d/tests"
  cat > "$d/Cargo.toml" <<'TOML'
[package]
name = "l3-s4"
version = "0.1.0"
edition = "2021"
TOML
  # 被测函数与调用方同处 lib.rs；测试从 `l3_s4::math::add` 引用尚未存在的模块。
  # 测试放独立文件，避免「删测试」绕过闸门。
  cat > "$d/src/lib.rs" <<'RS'
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn format_sum(a: i32, b: i32) -> String {
    format!("sum={}", add(a, b))
}
RS
  cat > "$d/tests/api.rs" <<'RS'
use l3_s4::format_sum;
use l3_s4::math::add;

#[test]
fn add_works() {
    assert_eq!(add(2, 3), 5);
}

#[test]
fn format_works() {
    assert_eq!(format_sum(1, 2), "sum=3");
}
RS
}

# 记录基线：闸门修复会读取 git diff 判断「这一步改了什么」。
git_baseline() {
  local d="$1"
  (
    cd "$d" || exit 0
    printf '/target\n' > .gitignore
    git init -q 2>/dev/null || exit 0
    git add -A 2>/dev/null
    git -c user.email=l3@local -c user.name=l3 commit -qm baseline 2>/dev/null
  )
}

# ── 判定 ─────────────────────────────────────────────────────
judge_s1() {
  ( cd "$WORKDIR/s1" && env -u RUSTUP_TOOLCHAIN cargo check >/dev/null 2>&1 )
}

judge_s2() {
  ( cd "$WORKDIR/s2" && env -u RUSTUP_TOOLCHAIN cargo clippy -- -D warnings >/dev/null 2>&1 )
}

judge_s3() {
  ( cd "$WORKDIR/s3" && grep -q "fn multiply" src/lib.rs ) || return 1
  # 既有的公开 API 必须保留：任务只要求「新增 multiply」，删掉 `pub fn add`
  # 是破坏（实测 3B 模型会用 CreateFile 整文件覆盖，闸门只测 multiply 因此
  # 照样 PASS —— 旧判定漏掉了这类静默破坏）。
  ( cd "$WORKDIR/s3" && grep -q "fn add" src/lib.rs ) || return 1
  local out
  out="$( cd "$WORKDIR/s3" && env -u RUSTUP_TOOLCHAIN cargo test 2>&1 )" || return 1
  # 必须真的跑了 ≥1 个测试，而不是「0 passed」的空跑
  printf '%s' "$out" | grep -qE "test result: ok\. [1-9][0-9]* passed"
}

judge_s5() {
  local d="$WORKDIR/s5"
  # 1) 新函数存在
  grep -q "fn scale" "$d/src/lib.rs" || return 1
  # 2) 400 个 helper 一个都不能少（长文件不得被改残）
  local count
  count="$(grep -c "pub fn helper_" "$d/src/lib.rs" || true)"
  [ "$count" = "400" ] || return 1
  # 3) 测试真的跑过
  local out
  out="$( cd "$d" && env -u RUSTUP_TOOLCHAIN cargo test 2>&1 )" || return 1
  printf '%s' "$out" | grep -qE "test result: ok\. [1-9][0-9]* passed"
}

judge_s4() {
  local d="$WORKDIR/s4"
  # 1) 新文件存在且确实承载 add
  [ -f "$d/src/math.rs" ] || return 1
  grep -qE "(pub +)?fn +add" "$d/src/math.rs" || return 1
  # 2) lib.rs 不再定义 add（复制一份到新文件不算重构）
  if grep -qE "^ *(pub +)?fn +add" "$d/src/lib.rs"; then return 1; fi
  # 3) 测试真实通过，且两个用例都跑到
  local out
  out="$( cd "$d" && env -u RUSTUP_TOOLCHAIN cargo test 2>&1 )" || return 1
  printf '%s' "$out" | grep -qE "test result: ok\. [2-9][0-9]* passed"
}

run_case() {
  local tag="$1"; shift
  local dir="$WORKDIR/$tag"
  local goal="$1"; shift
  local ctx="$1"; shift
  local verify="$1"; shift
  local log="$WORKDIR/$tag-run.log"
  local scenario
  case "$tag" in
    s1) scenario="diagnose-fix" ;;
    s2) scenario="quality-gate" ;;
    s3) scenario="feature" ;;
    s4) scenario="refactor" ;;
    s5) scenario="long-context" ;;
    *) scenario="$tag" ;;
  esac
  local record="$RECORDS_DIR/l3-$tag-$scenario.json"

  echo "──────── $tag ────────"
  echo "goal: $goal"
  echo "verify: $verify"

  local before=0
  [ -f "$LOG" ] && before=$(wc -l < "$LOG")

  ( cd "$dir" && local start; start=$(date +%s); \
    timeout $((TIMEOUT_SECS + 60)) "$CLI" --config "$CFG" dev "$goal" \
      --context "$ctx" --timeout-secs "$TIMEOUT_SECS" \
      --verify-cmd "$verify" --record "$record" --scenario "$scenario" \
      --yes >"$log" 2>&1; \
    echo "rc=$?" >"$WORKDIR/$tag.rc"; \
    echo $(( $(date +%s) - start )) >"$WORKDIR/$tag.elapsed" )

  local rc elapsed calls
  rc=$(sed 's/rc=//' "$WORKDIR/$tag.rc" 2>/dev/null)
  elapsed=$(cat "$WORKDIR/$tag.elapsed" 2>/dev/null)
  calls=$(( $(wc -l < "$LOG") - before ))

  local status="FAIL"
  if judge_$tag; then status="PASS"; fi

  printf '%-6s rc=%s elapsed=%ss llm_calls=%s\n' "$status" "${rc:-?}" "${elapsed:-?}" "$calls"
  RESULTS+=("$tag|$status|${rc:-?}|${elapsed:-?}|$calls")
  tail -25 "$log" | sed 's/^/    /'
  echo
}

# ── 执行 ─────────────────────────────────────────────────────
echo "cli:      $CLI"
echo "model:    $MODEL"
echo "upstream: $UPSTREAM (recorded via proxy :$PROXY_PORT)"
echo "workdir:  $WORKDIR"
echo "records:  $RECORDS_DIR"
echo "timeout:  ${TIMEOUT_SECS}s"
echo

for s in "${SCENARIOS[@]}"; do
  case "$s" in
    s1)
      reset_s1
      git_baseline "$WORKDIR/s1"
      run_case s1 "修复 src/main.rs 的编译错误，使 cargo check 通过" \
"项目是单文件 Rust bin crate。src/main.rs 当前内容（注意 let total 这一行缺少分号）：
fn main() {
    let total = add(1, 2)
    println!(\"{}\", total);
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}" "cargo check"
      ;;
    s2)
      reset_s2
      git_baseline "$WORKDIR/s2"
      run_case s2 "消除 src/lib.rs 的 clippy 告警，使 cargo clippy -- -D warnings 通过" \
"项目是单文件 Rust lib crate。src/lib.rs 当前内容：
pub fn is_ready(flag: bool) -> bool {
    if flag == true {
        true
    } else {
        false
    }
}
clippy 报 bool_comparison 与 needless_bool。" "cargo clippy -- -D warnings"
      ;;
    s3)
      reset_s3
      git_baseline "$WORKDIR/s3"
      run_case s3 "实现 src/lib.rs 中缺失的 multiply，使 cargo test 通过" \
"项目是 Rust lib crate。src/lib.rs 当前内容：
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
另外 tests/multiply.rs 已存在（不要修改它）：
use l3_s3::multiply;

#[test]
fn multiply_works() {
    assert_eq!(multiply(3, 4), 12);
    assert_eq!(multiply(-2, 5), -10);
}
multiply 未定义 → cargo test 编译失败。请在 src/lib.rs 实现 pub fn multiply(a: i32, b: i32) -> i32。" "cargo test"
      ;;
    s5)
      reset_s5
      git_baseline "$WORKDIR/s5"
      run_case s5 "只在 src/lib.rs 的**最后一行之后**追加这一个函数，其余内容保持不变：pub fn scale(v: i64, k: i64) -> i64 { v * k }" \
        "$(cat "$WORKDIR/s5-context.txt")" "cargo test"
      ;;
    s4)
      reset_s4
      git_baseline "$WORKDIR/s4"
      run_case s4 "把 src/lib.rs 中的 add 拆分到新文件 src/math.rs，同时保持 cargo test 通过" \
"项目**已存在**（Cargo.toml / src/lib.rs / tests/api.rs 都已就绪），不要新建或修改 Cargo.toml。
src/lib.rs 当前内容：
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn format_sum(a: i32, b: i32) -> String {
    format!(\"sum={}\", add(a, b))
}
tests/api.rs 已存在（不要修改它），它从 l3_s4::math::add 引用 add → 当前 cargo test 编译失败。
目标最终形态（两个文件各写一次，不要改 Cargo.toml 与 tests/api.rs）：
src/math.rs:
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
src/lib.rs:
pub mod math;

pub use math::add;

pub fn format_sum(a: i32, b: i32) -> String {
    format!(\"sum={}\", add(a, b))
}" "cargo test"
      ;;
  esac
done

# ── 汇总 ─────────────────────────────────────────────────────
echo "──────── summary ────────"
PASS=0; FAIL=0
for r in "${RESULTS[@]}"; do
  IFS='|' read -r tag status rc elapsed calls <<< "$r"
  printf '%-4s %-5s rc=%-3s %ss llm_calls=%s\n' "$tag" "$status" "$rc" "$elapsed" "$calls"
  [ "$status" = "PASS" ] && PASS=$((PASS+1)) || FAIL=$((FAIL+1))
done
echo "PASS=$PASS FAIL=$FAIL"
echo "llm records: $LOG ($(wc -l < "$LOG") calls)"
echo "task records: $RECORDS_DIR"

{
  echo "# T1.4 L3 scenario run"
  echo
  echo "run_at: $(date -Iseconds)"
  echo "model: $MODEL"
  echo "upstream: $UPSTREAM"
  echo "timeout: ${TIMEOUT_SECS}s"
  echo
  echo "| 场景 | 判定 | rc | 耗时 | LLM 调用数 |"
  echo "|---|---|---|---|---|"
  for r in "${RESULTS[@]}"; do
    IFS='|' read -r tag status rc elapsed calls <<< "$r"
    echo "| $tag | $status | $rc | ${elapsed}s | $calls |"
  done
  echo
  echo "PASS=$PASS FAIL=$FAIL"
} > "$WORKDIR/summary.md"

[ "$FAIL" -eq 0 ]
