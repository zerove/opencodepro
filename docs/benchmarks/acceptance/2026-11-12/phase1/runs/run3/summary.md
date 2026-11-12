# T1.4 L3 scenario run

run_at: 2026-11-12T10:26:04+08:00
model: qwen2.5-coder:3b
upstream: http://127.0.0.1:11434/v1/chat/completions
timeout: 900s

| 场景 | 判定 | rc | 耗时 | LLM 调用数 |
|---|---|---|---|---|
| s1 | PASS | 0 | 52s | 2 |
| s2 | PASS | 0 | 39s | 2 |
| s3 | PASS | 0 | 51s | 1 |

PASS=3 FAIL=0
