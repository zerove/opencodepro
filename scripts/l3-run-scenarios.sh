#!/usr/bin/env bash
# T1.4 L3 真实闭环 —— 可复跑场景脚本
#
# 三个场景（每个都是「真实仓库任务」，不是 mock）：
#   s1 诊断修复：注入编译错误（缺分号）→ 要求修复，判定 `cargo check` 通过
#   s2 质量门  ：注入 clippy 告警（bool_comparison/needless_bool）→ 要求消除，
#                判定 `cargo clippy -- -D warnings` 通过
#   s3 功能新增：只有 add() → 要求新增 multiply() + 单元测试，判定 `cargo test`
#                通过且确实运行了 ≥1 个测试、multiply 存在
#
# 依赖：
#   - 一个 OpenAI 兼容的真实 LLM 端点（默认走本机 ollama）
#   - 可选：`scripts/l3-llm-proxy.py` 记录请求/响应（T1.4 证据要求）
#
# 用法：
#   ./scripts/l3-run-scenarios.sh [s1] [s2] [s3] [--model M] [--timeout-secs N]
#                               [--upstream URL] [--workdir DIR]
#
# 退出码：0 = 所有被选场景判定 PASS；1 = 有场景未通过（属正常结果，需看结论）。

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="$REPO_ROOT/target/debug/sisyphus"
MODEL="qwen2.5-coder:3b"
UPSTREAM="http://127.0.0.1:11434/v1/chat/completions"
TIMEOUT_SECS=900
WORKDIR="${TMPDIR:-/tmp}/opencode/l3"
SCENARIOS=()

while [ $# -gt 0 ]; do
  case "$1" in
    s1|s2|s3) SCENARIOS+=("$1"); shift ;;
    --model) MODEL="$2"; shift 2 ;;
    --timeout-secs) TIMEOUT_SECS="$2"; shift 2 ;;
    --upstream) UPSTREAM="$2"; shift 2 ;;
    --workdir) WORKDIR="$2"; shift 2 ;;
    --cli) CLI="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done
[ ${#SCENARIOS[@]} -eq 0 ] && SCENARIOS=(s1 s2 s3)

mkdir -p "$WORKDIR"
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
  local out
  out="$( cd "$WORKDIR/s3" && env -u RUSTUP_TOOLCHAIN cargo test 2>&1 )" || return 1
  # 必须真的跑了 ≥1 个测试，而不是「0 passed」的空跑
  printf '%s' "$out" | grep -qE "test result: ok\. [1-9][0-9]* passed"
}

run_case() {
  local tag="$1"; shift
  local dir="$WORKDIR/$tag"
  local goal="$1"; shift
  local ctx="$1"; shift
  local verify="$1"; shift
  local log="$WORKDIR/$tag-run.log"

  echo "──────── $tag ────────"
  echo "goal: $goal"
  echo "verify: $verify"

  local before=0
  [ -f "$LOG" ] && before=$(wc -l < "$LOG")

  ( cd "$dir" && local start; start=$(date +%s); \
    timeout $((TIMEOUT_SECS + 60)) "$CLI" --config "$CFG" dev "$goal" \
      --context "$ctx" --timeout-secs "$TIMEOUT_SECS" \
      --verify-cmd "$verify" >"$log" 2>&1; \
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
