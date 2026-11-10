#!/usr/bin/env bash
# 验证 docs/examples/sop-templates 下模板 JSON 的基本结构
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="${1:-$ROOT/docs/examples/sop-templates}"
FAIL=0

if ! command -v python3 >/dev/null 2>&1; then
  echo "[FAIL] python3 required" >&2
  exit 1
fi

for f in "$DIR"/*.json; do
  [ -e "$f" ] || continue
  echo "-- $f"
  if python3 - "$f" <<'PY'
import json,sys
p=sys.argv[1]
with open(p,'r',encoding='utf-8') as fh:
    d=json.load(fh)
for k in ('id','name','config'):
    if k not in d:
        raise SystemExit(f'missing key: {k}')
cfg=d['config']
if 'nodes' not in cfg or 'edges' not in cfg:
    raise SystemExit('config must contain nodes and edges')
if not isinstance(cfg['nodes'], list) or not isinstance(cfg['edges'], list):
    raise SystemExit('nodes/edges must be lists')
ids=set()
for n in cfg['nodes']:
    if 'name' not in n or 'kind' not in n:
        raise SystemExit('node missing name/kind')
print(f"  ok: id={d['id']} nodes={len(cfg['nodes'])} edges={len(cfg['edges'])}")
PY
  then :; else
    echo "  [FAIL] $f" >&2
    FAIL=1
  fi
done

if [ "$FAIL" -ne 0 ]; then exit 1; fi
echo "== all template validations passed =="
