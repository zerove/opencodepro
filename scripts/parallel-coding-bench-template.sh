#!/usr/bin/env bash
# M0.3（草案）：并行编码场景基准/证据采集模板
#
# 用途：为 docs/plans/parallel-coding-scenarios.md 中的场景提供统一的
#       证据目录结构与元信息记录；具体“执行场景”的命令由调用方注入。
#
# 设计原则：
# - 不内置假数据/假结论；只记录实际执行的命令与退出码、耗时与环境快照。
# - 计数与结果以实跑输出为准（ADR-021）。
#
# 用法示例：
#   ./scripts/parallel-coding-bench-template.sh \
#     --scenario S1 \
#     --out docs/benchmarks/parallel-coding/2026-11-18-s1-smoke \
#     -- bash -lc 'echo "TODO: run scenario command here"; exit 0'
#
# 说明：
# - `--` 之后的命令为被测命令（可选）；若省略，则仅生成证据骨架与环境信息。
# - 建议在干净工作区或隔离工作树中运行，避免与正在进行的修改混淆。

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  scripts/parallel-coding-bench-template.sh --scenario <ID> --out <DIR> [--note <TEXT>] [--] [COMMAND...]

Options:
  --scenario <ID>   场景编号（如 S1/S2/...），用于 summary.md 记录
  --out <DIR>       证据输出目录（将创建）
  --note <TEXT>     可选备注（环境说明、限制、BLOCKED 原因等）
  -h, --help        显示帮助

Behavior:
  - 生成 <DIR>/summary.md、<DIR>/env.txt 与 <DIR>/summary.json（机器可读字段映射）
  - 若提供 COMMAND，则执行并将 stdout/stderr/exit code/耗时写入 <DIR>/run.log、summary.md 与 summary.json
EOF
}

SCENARIO=""
OUT_DIR=""
NOTE=""
CMD=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scenario)
      SCENARIO="${2:-}"; shift 2 ;;
    --out)
      OUT_DIR="${2:-}"; shift 2 ;;
    --note)
      NOTE="${2:-}"; shift 2 ;;
    -h|--help)
      usage; exit 0 ;;
    --)
      shift; CMD=("$@"); break ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2 ;;
  esac
done

if [[ -z "$SCENARIO" || -z "$OUT_DIR" ]]; then
  echo "Error: --scenario and --out are required." >&2
  usage >&2
  exit 2
fi

mkdir -p "$OUT_DIR"

TS_UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
HOST="$(hostname 2>/dev/null || echo unknown)"
KERNEL="$(uname -a 2>/dev/null || echo unknown)"
GIT_HEAD="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
GIT_DIRTY="$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ' || echo unknown)"
PWD_NOW="$(pwd)"
RUSTC_VER="$(rustc --version 2>/dev/null || echo n/a)"
CARGO_VER="$(cargo --version 2>/dev/null || echo n/a)"

{
  echo "timestamp_utc=$TS_UTC"
  echo "scenario=$SCENARIO"
  echo "host=$HOST"
  echo "kernel=$KERNEL"
  echo "git_head=$GIT_HEAD"
  echo "git_dirty_files=$GIT_DIRTY"
  echo "pwd=$PWD_NOW"
  echo "rustc=$RUSTC_VER"
  echo "cargo=$CARGO_VER"
} > "$OUT_DIR/env.txt"

{
  echo "# 并行编码场景证据（模板生成）"
  echo
  echo "- scenario: \`$SCENARIO\`"
  echo "- timestamp_utc: \`$TS_UTC\`"
  echo "- git_head: \`$GIT_HEAD\`"
  echo "- git_dirty_files: \`$GIT_DIRTY\`"
  echo "- output_dir: \`$OUT_DIR\`"
  if [[ -n "$NOTE" ]]; then
    echo "- note: $NOTE"
  fi
  echo
  echo "## 环境"
  echo
  echo '```text'
  cat "$OUT_DIR/env.txt"
  echo '```'
  echo
} > "$OUT_DIR/summary.md"


write_summary_json() {
  # 生成机器可读 summary.json（M0.2 字段映射，值均来自本次实跑/环境快照；不伪造指标）
  local status="$1"      # SKIPPED | COMPLETED
  local exit_code="$2"   # 数字或 null
  local elapsed_ms="$3"  # 数字或 null
  local cmd_json="$4"    # JSON 数组字符串
  SCENARIO="$SCENARIO" TS_UTC="$TS_UTC" GIT_HEAD="$GIT_HEAD" GIT_DIRTY="$GIT_DIRTY" \
  OUT_DIR="$OUT_DIR" NOTE="$NOTE" HOST="$HOST" KERNEL="$KERNEL" PWD_NOW="$PWD_NOW" \
  RUSTC_VER="$RUSTC_VER" CARGO_VER="$CARGO_VER" STATUS="$status" EXIT_CODE="$exit_code" \
  ELAPSED_MS="$elapsed_ms" CMD_JSON="$cmd_json" \
  python3 - "$OUT_DIR/summary.json" <<'PY'
import json, os, sys
out = sys.argv[1]

def maybe_int(name):
    v = os.environ.get(name, "")
    if v in ("", "n/a", "null", "unknown"):
        return None
    try:
        return int(v)
    except Exception:
        return None

exit_code = maybe_int("EXIT_CODE")
elapsed_ms = maybe_int("ELAPSED_MS")
git_dirty = maybe_int("GIT_DIRTY")
cmd = json.loads(os.environ.get("CMD_JSON", "[]"))

status = os.environ.get("STATUS", "UNKNOWN")
scenario_pass = None
if status == "COMPLETED" and exit_code is not None:
    scenario_pass = (exit_code == 0)

doc = {
    "schema_version": 1,
    "scenario": os.environ.get("SCENARIO", ""),
    "timestamp_utc": os.environ.get("TS_UTC", ""),
    "git": {"head": os.environ.get("GIT_HEAD", ""), "dirty_files": git_dirty},
    "output_dir": os.environ.get("OUT_DIR", ""),
    "note": (os.environ.get("NOTE") or None),
    "environment": {
        "host": os.environ.get("HOST", ""),
        "kernel": os.environ.get("KERNEL", ""),
        "pwd": os.environ.get("PWD_NOW", ""),
        "rustc": os.environ.get("RUSTC_VER", ""),
        "cargo": os.environ.get("CARGO_VER", ""),
        "env_file": "env.txt",
    },
    "execution": {
        "status": status,
        "command": cmd,
        "exit_code": exit_code,
        "elapsed_ms": elapsed_ms,
        "log": "run.log" if status == "COMPLETED" else None,
    },
    "metrics": {
        "scenario_pass": scenario_pass,
        "wall_time_ms": elapsed_ms,
        "command_exit_code": exit_code,
    },
}
with open(out, "w", encoding="utf-8") as f:
    json.dump(doc, f, ensure_ascii=False, indent=2)
    f.write("\n")
PY
}

if [[ ${#CMD[@]} -eq 0 ]]; then
  {
    echo "## 执行"
    echo
    echo "- status: SKIPPED（未提供 COMMAND；仅生成证据骨架）"
  } >> "$OUT_DIR/summary.md"
  write_summary_json "SKIPPED" "null" "null" "[]"
  echo "[parallel-coding-bench-template] Evidence skeleton created at: $OUT_DIR"
  exit 0
fi

{
  echo "## 执行"
  echo
  echo "### Command"
  echo
  echo '```bash'
  printf '%q ' "${CMD[@]}"
  echo
  echo '```'
  echo
} >> "$OUT_DIR/summary.md"

START_NS="$(date +%s%N 2>/dev/null || echo 0)"
set +e
"${CMD[@]}" >"$OUT_DIR/run.log" 2>&1
RC=$?
set -e
END_NS="$(date +%s%N 2>/dev/null || echo 0)"

ELAPSED_MS="n/a"
if [[ "$START_NS" =~ ^[0-9]+$ && "$END_NS" =~ ^[0-9]+$ && "$END_NS" -ge "$START_NS" && "$START_NS" -gt 0 ]]; then
  ELAPSED_MS="$(( (END_NS - START_NS) / 1000000 ))"
fi

{
  echo "### Result"
  echo
  echo "- exit_code: \`$RC\`"
  echo "- elapsed_ms: \`$ELAPSED_MS\`"
  echo "- log: \`run.log\`"
  echo "- summary_json: \`summary.json\`"
  echo
  echo "### 结论"
  echo
  echo "- 请基于 run.log 与实际验收命令填写 PASS/FAIL/BLOCKED（不得伪造）。"
} >> "$OUT_DIR/summary.md"

CMD_JSON="$(python3 - "${CMD[@]}" <<'PY'
import json, sys
print(json.dumps(sys.argv[1:]))
PY
)"
JSON_ELAPSED="$ELAPSED_MS"
if [[ "$JSON_ELAPSED" == "n/a" ]]; then JSON_ELAPSED="null"; fi
write_summary_json "COMPLETED" "$RC" "$JSON_ELAPSED" "$CMD_JSON"

echo "[parallel-coding-bench-template] Command finished: exit_code=$RC elapsed_ms=$ELAPSED_MS"
echo "[parallel-coding-bench-template] Evidence written to: $OUT_DIR"
exit "$RC"
