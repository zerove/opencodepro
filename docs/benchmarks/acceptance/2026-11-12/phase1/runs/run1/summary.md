# T1.4 L3 scenario run

run_at: 2026-11-12T10:20:25+08:00
model: qwen2.5-coder:3b
upstream: http://127.0.0.1:11434/v1/chat/completions
timeout: 900s

| 场景 | 判定 | rc | 耗时 | LLM 调用数 |
|---|---|---|---|---|
| s1 | PASS | 0 | 42s | 1 |
| s2 | PASS | 0 | 101s | 3 |
| s3 | PASS | 0 | 52s | 1 |

PASS=3 FAIL=0
