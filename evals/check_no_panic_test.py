"""check_no_panic.py 自测 (ADR-007 §1 工具验证)。

验证:
- 干净代码 → exit 0
- unwrap 调用 → exit 1 + 准确报告
- #[cfg(test)] mod 内的 unwrap 被忽略
- 注释行豁免
- JSON 输出结构正确
- baseline 模式: 新增 panic 即 fail

跑: python3 evals/check_no_panic_test.py
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).parent.parent
SCRIPT = REPO / "scripts" / "check_no_panic.py"


def run(args):
    r = subprocess.run(
        ["python3", str(SCRIPT)] + args,
        capture_output=True, text=True, cwd=str(REPO),
    )
    return r.returncode, r.stdout, r.stderr


def make_src(tmp: Path, *files: tuple[str, str]) -> Path:
    """在 tmp 下建 src/ 目录并写入测试文件。"""
    src = tmp / "src"
    src.mkdir(exist_ok=True)
    for name, body in files:
        (src / name).write_text(body)
    return src


def test_clean_code_passes():
    with tempfile.TemporaryDirectory() as tmp:
        make_src(Path(tmp), ("clean.rs", "pub fn ok() -> u32 { 42 }\n"))
        rc, out, _ = run([str(Path(tmp) / "src")])
        assert rc == 0, f"clean 应 exit 0, got {rc}\n{out}"
        assert "0 个 panic 路径" in out


def test_unwrap_detected():
    with tempfile.TemporaryDirectory() as tmp:
        make_src(Path(tmp), ("bad.rs", 'pub fn bad() -> u32 { "x".parse().unwrap() }\n'))
        rc, out, _ = run([str(Path(tmp) / "src")])
        assert rc == 1, f"unwrap 应 exit 1, got {rc}"
        assert "1 个 panic 路径" in out


def test_cfg_test_ignored():
    body = (
        "pub fn good() -> u32 { 42 }\n"
        "#[cfg(test)]\nmod tests {\n"
        "    #[test]\n    fn ok() {\n"
        '        let _ = "x".parse::<u32>().unwrap();\n'
        "    }\n}\n"
    )
    with tempfile.TemporaryDirectory() as tmp:
        make_src(Path(tmp), ("t.rs", body))
        rc, out, _ = run([str(Path(tmp) / "src")])
        assert rc == 0, f"cfg(test) 内应忽略, got {rc}\n{out}"


def test_comment_line_exempt():
    body = '// let _ = "x".parse::<u32>().unwrap();\npub fn ok() -> u32 { 42 }\n'
    with tempfile.TemporaryDirectory() as tmp:
        make_src(Path(tmp), ("c.rs", body))
        rc, _, _ = run([str(Path(tmp) / "src")])
        assert rc == 0, "注释行应豁免"


def test_json_output_structure():
    with tempfile.TemporaryDirectory() as tmp:
        make_src(Path(tmp), ("bad.rs", 'pub fn bad() -> u32 { "x".parse().unwrap() }\n'))
        rc, out, _ = run([str(Path(tmp) / "src"), "--json"])
        data = json.loads(out)
        assert data["total"] == 1
        assert data["hits"][0]["kind"] == "unwrap"
        assert data["hits"][0]["file"].endswith("bad.rs")


def test_baseline_mode_detects_new_panic():
    """baseline 模式: 新增 panic 即 fail, 已删除的警告。"""
    with tempfile.TemporaryDirectory() as tmp:
        src = make_src(Path(tmp),
            ("old.rs", 'pub fn old() -> u32 { "x".parse().unwrap() }\n'),
            ("clean.rs", "pub fn clean() -> u32 { 42 }\n"))
        baseline = Path(tmp) / "baseline.json"
        # 写 baseline
        rc, _, _ = run([str(src), "--write-baseline", str(baseline)])
        assert rc == 0
        # 当前无新增 → exit 0
        rc, out, _ = run([str(src), "--baseline", str(baseline)])
        assert rc == 0, f"无新增应 exit 0, got {rc}\n{out}"
        # 新增一个 panic
        (src / "new.rs").write_text('pub fn new() -> u32 { "y".parse().unwrap() }\n')
        rc, out, _ = run([str(src), "--baseline", str(baseline)])
        assert rc == 1, f"新增 panic 应 exit 1, got {rc}"
        assert "新增 panic 路径" in out


def main() -> int:
    tests = [v for k, v in globals().items() if k.startswith("test_")]
    failed = 0
    for t in tests:
        try:
            t()
            print(f"  ✓ {t.__name__}")
        except AssertionError as e:
            print(f"  ✗ {t.__name__}: {e}")
            failed += 1
    if failed:
        print(f"\n{failed} / {len(tests)} 失败")
        return 1
    print(f"\n✓ {len(tests)} / {len(tests)} 通过")
    return 0


if __name__ == "__main__":
    sys.exit(main())
