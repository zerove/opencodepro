#!/usr/bin/env python3
"""审计 GitHub Actions workflow 的引用完整性。

背景：workflow 里的引用写错**不会让 CI 变红**，而是各自以不同方式静默失效。

**第一类：无来源的 step 引用**

GitHub 对无法解析的 `${{ }}` 引用一律展开成空字符串，于是：

    📊 **Bench Dashboard** ([详情]())
    跑  大小

这段注释曾真实存在于 `.github/workflows/ci.yml`——它引用了
`steps.upload-artifact.outputs.artifact-url`，而那个 step 从未声明 `id`。
GitHub 只为显式声明 id 的 step 暴露 outputs，于是链接渲染成空链接、
句子缺了数字，PR 上天天如此，却没有任何报警。

**第二类：无来源的 needs 引用**

`needs` 只能引用**同一 workflow 内**定义的 job，跨 workflow 没有依赖机制。
`.github/workflows/mdbook.yml` 曾写着 `needs: [bench-core]`——那是
**ci.yml** 的 job 名。后果比第一类严重得多：GitHub 直接把整个 workflow 判为
Invalid 并拒绝运行，mdbook 部署从未真正执行过，而且只有去 Actions 页面看
才会发现。

两类都由本脚本覆盖。

**不覆盖的**：`hashFiles()` 被用来判断**同 job 内刚生成**的文件——它评估的是
job 开始时的文件状态，对这种文件不作保证。这属于语义问题，无法静态判定，
只在 workflow 注释里留了说明。

**已知的自检盲区**（变异验证时逐条试过，据实记录）：
`count_needs()` 只用于汇报「检查了多少条 needs 引用」，把它的返回值改成恒 0
不会被自检抓住。这只影响进度输出的数字，不影响「是否报缺陷」的结论——判据是
孤儿列表本身。属显示层盲区，可接受。

退出码：0 = 全部引用有来源，2 = 发现无来源引用，1 = 用法/环境错误。
"""

import glob
import os
import re
import sys

# 匹配 steps.<id>. 形式的引用。GitHub 的 step id 只允许字母数字、- 和 _。
STEP_REF = re.compile(r"steps\.([A-Za-z0-9_-]+)\.")

# 只扫 workflow 目录，不递归进 target/ 等目录。
WORKFLOW_GLOB = ".github/workflows/*.yml"


def live_code_lines(path):
    """返回去掉整行注释后的内容。

    只剔除**整行**都是注释的行。行尾注释不剔除：YAML 里 `#` 只有在
    值之外才是注释，同一行里出现 `#` 的情况（如 `${{ }}` 表达式含
    `#`）语义复杂，宁可多扫也不漏——漏扫会制造假绿灯，那比误报糟。
    注释里提到 `steps.foo.` 是正常的（记录历史缺陷），必须排除，否则
    本脚本会把自己写的说明当成缺陷。
    """
    with open(path, encoding="utf-8") as fh:
        return "".join(
            line for line in fh if not line.lstrip().startswith("#")
        )


def defined_step_ids(doc):
    """收集所有声明了 id 的 step，返回 {step_id: job_name}。"""
    defined = {}
    for job_name, job in (doc.get("jobs") or {}).items():
        for step in job.get("steps") or []:
            step_id = step.get("id")
            if step_id:
                defined[step_id] = job_name
    return defined


def _normalize_needs(raw):
    """把 needs 的各种写法归一成 job 名列表。

    GitHub 接受三种形式：单个字符串、字符串列表、以及
    `{job: [outputs...]}` 形式的依赖对象。
    """
    if raw is None:
        return []
    if isinstance(raw, str):
        return [raw]
    if isinstance(raw, list):
        return [x for x in raw if isinstance(x, str)]
    if isinstance(raw, dict):
        return [k for k in raw if isinstance(k, str)]
    raise ValueError("needs 字段类型异常：%r" % (raw,))


def count_needs(doc):
    """workflow 内 needs 引用的总条数（不论有无来源），用于汇报规模。"""
    jobs = doc.get("jobs") or {}
    return sum(len(_normalize_needs(job.get("needs"))) for job in jobs.values())


def orphan_needs(doc):
    """返回 [(引用方 job, 被引用但不存在的 job 名)]。

    needs 只能引用同一 workflow 内定义的 job。跨 workflow 引用不会报错，
    而是把整个 workflow 判为 Invalid——所以漏报代价极高。
    """
    jobs = doc.get("jobs") or {}
    orphans = []
    for job_name, job in jobs.items():
        for needed in _normalize_needs(job.get("needs")):
            if needed not in jobs:
                orphans.append((job_name, needed))
    return orphans


def check_file(path):
    """检查单个 workflow。

    返回 (step_references, {step_id: job_name}, [无来源的 step 引用],
            [(引用方, 无来源的 needs)], needs 引用总条数)。
    """
    import yaml  # 延迟导入，让 --help 在缺依赖时也能给出清晰提示

    with open(path, encoding="utf-8") as fh:
        doc = yaml.safe_load(fh)
    if not isinstance(doc, dict):
        raise ValueError("%s: 顶层不是映射，可能不是合法 workflow" % path)

    defined = defined_step_ids(doc)
    refs = sorted(set(STEP_REF.findall(live_code_lines(path))))
    orphans = [r for r in refs if r not in defined]
    return refs, defined, orphans, orphan_needs(doc), count_needs(doc)


def _self_check():
    """用内联的最小样本验证 check_file 的判别力。

    直接调用生产函数，而不是另写一份判定逻辑——否则自检验证的是副本，
    生产代码改了它照样通过。前几轮已经吃过这个亏。
    """
    import tempfile

    import yaml

    cases = [
        # (说明, workflow YAML 文本, 期望的无来源引用列表)
        (
            "引用了未声明 id 的 step",
            """
jobs:
  a:
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - name: comment
        run: echo ${{ steps.upload.outputs.artifact-id }}
""",
            ["upload"],
        ),
        (
            "引用了已声明 id 的 step",
            """
jobs:
  a:
    steps:
      - name: upload
        id: up
        uses: actions/upload-artifact@v4
      - name: comment
        run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
        ),
        (
            "跨 job 的引用同样按全局 id 解析",
            """
jobs:
  producer:
    steps:
      - id: gen
        run: echo hi
  consumer:
    steps:
      - run: echo ${{ steps.gen.outputs.x }}
""",
            [],
        ),
        (
            "注释里提到无来源引用不算缺陷",
            """
jobs:
  a:
    steps:
      - name: upload
        id: up
        uses: actions/upload-artifact@v4
      # 曾错误地写成 steps.upload.outputs.artifact-id
      - run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
        ),
        (
            "conclusion 引用同样受检",
            """
jobs:
  a:
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - if: steps.upload.conclusion == 'success'
        run: echo ok
""",
            ["upload"],
        ),
        (
            "结尾的点必须存在：steps.up 后面没有点就不是引用",
            # `${{ steps.up }}` 不是合法引用——step 引用至少要带一个属性
            # （outputs/conclusion/outcome）。若正则去掉结尾的 `\\.`，它会
            # 凭空认出一个 id 为 `up` 的引用；而该 step 刻意**不**声明 id，
            # 于是这个变异就会误报 `up`，与期望的 [] 不符而被自检抓住。
            #
            # 注意早先两版构造都无效：
            #   · `steps_context.value`：steps 后面是 `_` 而非 `.`，两种正则
            #     都不匹配，构造不出差异。
            #   · 让该 step 声明 `id: up`：无点版匹配到它也不算孤儿，一样过。
            """
jobs:
  a:
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - run: echo ${{ steps.up }}
""",
            [],
        ),
        (
            "非映射顶层应报错而非静默放过",
            "[]\n",
            "ERROR",
        ),
    ]

    # (说明, YAML, 期望的无来源 step 引用, 期望的无来源 needs)
    cases = [
        (c[0], c[1], c[2], []) for c in cases
    ] + [
        (
            "needs 引用了别的 workflow 的 job（mdbook.yml 的真实缺陷）",
            """
jobs:
  build-and-deploy:
    runs-on: ubuntu-latest
    needs: [bench-core]
    steps:
      - run: echo hi
""",
            [],
            [("build-and-deploy", "bench-core")],
        ),
        (
            "needs 引用同 workflow 内的 job 合法",
            """
jobs:
  producer:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
  consumer:
    runs-on: ubuntu-latest
    needs: [producer]
    steps:
      - run: echo hi
""",
            [],
            [],
        ),
        (
            "needs 的单字符串写法也要校验",
            """
jobs:
  consumer:
    runs-on: ubuntu-latest
    needs: ghost
    steps:
      - run: echo hi
""",
            [],
            [("consumer", "ghost")],
        ),
        (
            "needs 的依赖对象写法（job: [outputs]）也要校验",
            """
jobs:
  consumer:
    runs-on: ubuntu-latest
    needs:
      ghost: [result]
    steps:
      - run: echo hi
""",
            [],
            [("consumer", "ghost")],
        ),
        (
            "没有任何 needs 不算缺陷",
            """
jobs:
  solo:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
""",
            [],
            [],
        ),
    ]

    failures = []
    for desc, text, expected_refs, expected_needs in cases:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".yml", delete=False, encoding="utf-8"
        ) as tmp:
            tmp.write(text.lstrip())
            tmp_path = tmp.name
        # 「非映射顶层」这类用例的期望值直接是字符串 "ERROR"，必须**在**
        # 构造 expected 元组之前判断，否则判断永远发生在元组上、对
        # "ERROR" 用例恒为假，会把「用例本就期望报错」误判成意外异常。
        expects_error = expected_refs == "ERROR"

        try:
            _refs, _defined, orphans, bad_needs, _n = check_file(tmp_path)
            actual = (orphans, bad_needs)
            expected = (expected_refs, expected_needs)
        except ValueError:
            # 必须区分「用例本就期望报错」与「生产代码意外抛错」。
            # 早先两者都记成字符串 "ERROR"，于是变异把 _normalize_needs 改成
            # 只处理 list 后，单字符串/字典写法抛出的 ValueError 恰好等于
            # 期望值，自检误判为通过——变异 10 因此漏网。
            if expects_error:
                actual = expected = "ERROR"
            else:
                actual = "UNEXPECTED ValueError"
                expected = None
        except Exception as exc:  # noqa: BLE001
            actual = "RAISED %r" % (exc,)
            expected = None
        finally:
            os.unlink(tmp_path)

        if actual != expected:
            failures.append(
                "%s: 期望 %r，实际 %r" % (desc, expected, actual)
            )

    # yaml 确实可用（否则上面每个用例都会变成异常，掩盖真实问题）
    assert yaml is not None

    if failures:
        print("自检失败：", file=sys.stderr)
        for f in failures:
            print("  - %s" % f, file=sys.stderr)
        return 2
    return 0


def main() -> int:
    if "--self-check" in sys.argv:
        return _self_check()

    paths = sorted(glob.glob(WORKFLOW_GLOB))
    if not paths:
        print("未找到任何 workflow（%s）" % WORKFLOW_GLOB, file=sys.stderr)
        return 1

    try:
        import yaml  # noqa: F401
    except ImportError:
        print("需要 PyYAML：pip install pyyaml", file=sys.stderr)
        return 1

    total_refs = 0
    total_needs = 0
    problems = []
    for path in paths:
        try:
            refs, _defined, orphans, bad_needs, n_needs = check_file(path)
        except Exception as exc:  # noqa: BLE001
            problems.append("%s: 解析失败 %r" % (path, exc))
            continue
        total_refs += len(refs)
        total_needs += n_needs
        rel = os.path.relpath(path)
        if orphans or bad_needs:
            if orphans:
                problems.append(
                    "%s: %d 个 step 引用无来源（未声明 id）：%s"
                    % (rel, len(orphans), ", ".join(orphans))
                )
            for src_job, missing in bad_needs:
                problems.append(
                    "%s: job `%s` 的 needs 引用了不存在的 job `%s`"
                    % (rel, src_job, missing)
                )
        else:
            print("✅ %s: %d 个 step 引用、%d 个 needs 引用全部有来源"
                  % (rel, len(refs), n_needs))

    if problems:
        print("", file=sys.stderr)
        for p in problems:
            print("❌ %s" % p, file=sys.stderr)
        print(
            "\n提示：GitHub 只为显式声明 id 的 step 暴露 outputs；"
            "无来源的引用会静默展开为空字符串——不会让 CI 变红，"
            "只会在渲染结果里表现为空链接或缺值。",
            file=sys.stderr,
        )
        print(
            "needs 只能引用同一 workflow 内的 job，跨 workflow 引用"
            "会让整个 workflow 被判为 Invalid 而完全无法运行。",
            file=sys.stderr,
        )
        return 2

    print("\n✅ 共检查 %d 个 workflow，%d 个 step 引用与 %d 个 needs 引用全部有来源"
          % (len(paths), total_refs, total_needs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
