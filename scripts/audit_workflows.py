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

**第三类：引用尚未执行的 step（自我引用 / 前向引用）**

GitHub 文档对 `steps` context 的定义里有一句限定词容易被跳过：

> The `steps` context contains a list of steps **that have an id specified and
> have already run** in the current job.

只有**已经跑完**的 step 才会出现在 context 里。`.github/workflows/mdbook.yml`
曾真实存在这样一处：

    - name: Deploy to GitHub Pages
      id: deployment
      uses: actions/deploy-pages@v4
      environment:
        name: github-pages
        url: ${{ steps.deployment.outputs.page_url }}   # 引用自己

`page_url` 由 deploy-pages 在 action 跑完之后才写入，而 `environment.url`
恰在这个 step 执行**之前**求值，于是恒为空字符串——同样不报错、不变红，只是
environment 页面上永远没有站点链接。

**只检查 step 内部**是有意的收窄：job 级的 `environment.url` 前向引用部署 step
是**合法**的（environment 的 URL 要等 job 跑完才登记，那时 step 早已执行），
这正是官方 Pages starter workflow 的标准写法。

四类都由本脚本覆盖。

**第四类：未设 timeout-minutes 的 job**

GitHub 对单个 job 的默认上限是 360 分钟（6 小时）。不写 `timeout-minutes`
不会报错、也不会变红——卡住的 job 会静静占着 runner 跑满 6 小时，期间 PR
一直挂着「检查进行中」。补 timeout 前本仓库 48 个 job 里有 43 个没写。

这与第三类同源：判据没错，但执行环境没有边界。

**不覆盖的**：`hashFiles()` 被用来判断**同 job 内刚生成**的文件——它评估的是
job 开始时的文件状态，对这种文件不作保证。这属于语义问题，无法静态判定，
只在 workflow 注释里留了说明。

**已知的自检盲区**（变异验证时逐条试过，据实记录）：

- `count_needs()` 只用于汇报「检查了多少条 needs 引用」，把它的返回值改成恒 0
  不会被自检抓住。这只影响进度输出的数字，不影响「是否报缺陷」的结论——判据是
  孤儿列表本身。属显示层盲区，可接受。
- `job_of_line()` 里 `end` 写成 `end - 1` 不会被自检抓住，且**这不是缺陷**：
  PyYAML 的 `end_mark.line` 指向块结束之后那一行的行首，job 的最后一行内容
  落在 `end - 1` 处，于是 `start <= line <= end` 与 `start <= line <= end - 1`
  覆盖完全相同的行集合。曾试过三种构造去区分它们（引用放最后一行、把 job 值
  收敛成单行 flow mapping、缩小 end 偏移），都无法产生差异；随后对 7 个真实
  workflow 逐行核对，`end - 1` 下「无归属」的 6–40 行**全部**位于 `jobs:`
  之前（`name:`、`on:` 等顶层配置），没有一行是 job 内部内容。据此确认它是等价
  变换，不需要专门用例，也不值得为一个无害写法加测试。
- `WORKFLOW_GLOBS` 的断言只覆盖「扩展名齐不齐」和「目录对不对」两个维度。
  若有人把模式改成某个仍然存在、但恰好为空目录下的 `*.yml`，自检仍会绿灯。
  这类配置错误在真实仓库里表现为 exit 1「未找到任何 workflow」而非静默
  漏扫，所以可接受；但它意味着**自检保证的是模式形状，不是扫描结果非空**。

退出码：0 = 全部引用有来源，2 = 发现无来源引用，1 = 用法/环境错误。
"""

import glob
import os
import re
import sys

# 匹配 steps.<id>. 形式的引用。GitHub 的 step id 只允许字母数字、- 和 _。
STEP_REF = re.compile(r"steps\.([A-Za-z0-9_-]+)\.")

# 只扫 workflow 目录，不递归进 target/ 等目录。
#
# 必须同时覆盖 `.yml` 与 `.yaml`：GitHub 两者都接受，只扫 `.yml` 时，
# 新建的 `foo.yaml` 会被静默跳过——门禁显示全绿，而新 workflow 里
# 的引用缺陷一次也没被查过。这正是本脚本要防的那类假绿灯。
WORKFLOW_GLOBS = (".github/workflows/*.yml", ".github/workflows/*.yaml")

# 审计脚本目录：用于「门禁自身是否被接线」的自检。只认 audit_* 与 scan_*，
# 不匹配 fix_*——后者是一次性迁移脚本（fix_error_wrapping 等），跑完即弃，
# 本就不该在 CI 里常驻执行，拿它们当门禁检查对象只会制造永久误报。
AUDIT_SCRIPT_DIR = "scripts"


def live_code_lines(text):
    """返回去掉整行注释后的内容。

    只剔除**整行**都是注释的行。行尾注释不剔除：YAML 里 `#` 只有在
    值之外才是注释，同一行里出现 `#` 的情况（如 `${{ }}` 表达式含
    `#`）语义复杂，宁可多扫也不漏——漏扫会制造假绿灯，那比误报糟。
    注释里提到 `steps.foo.` 是正常的（记录历史缺陷），必须排除，否则
    本脚本会把自己写的说明当成缺陷。
    """
    return "".join(
        line for line in text.splitlines(keepends=True)
        if not line.lstrip().startswith("#")
    )


def count_step_references(text):
    """活代码里 steps.<id>. 形式的引用条数（去重后），仅用于汇报规模。

    与 step_reference_orphans() 同样基于 live_code_lines()，两处口径必须
    一致，否则「检查了 N 条引用」与实际检查到的会不符。
    """
    return len(set(STEP_REF.findall(live_code_lines(text))))


def job_spans(text):
    """返回 [(job 名, 起始行, 结束行)]，行号为 0 基。

    用 yaml.compose 而不是 safe_load 拿**位置信息**：只有节点对象才带
    start_mark/end_mark，safe_load 只给出纯数据，行号无从谈起。

    作用域是必须的：GitHub 官方文档对 steps context 的定义是
    "information about the steps that have an id specified and have
    already run in the **current job**"——跨 job 引用一律解析为空字符串。
    早先的实现把所有 step id 收成整个 workflow 的全局集合，于是
    「A job 声明、B job 引用」这种真实缺陷能蒙混过关。
    """
    import yaml

    node = yaml.compose(text)
    if node is None:
        return []
    spans = []
    for key, value in node.value:
        if key.value != "jobs" or not isinstance(value, yaml.MappingNode):
            continue
        for job_key, job_value in value.value:
            spans.append(
                (job_key.value, job_value.start_mark.line, job_value.end_mark.line)
            )
    return spans


def job_of_line(spans, line):
    """行号落在哪个 job 的范围内；不在任何 job 内则返回 None。"""
    for name, start, end in spans:
        if start <= line <= end:
            return name
    return None


def defined_step_ids_by_job(doc):
    """返回 {job 名: {该 job 内声明了 id 的 step}}。

    必须按 job 分组而非拍平成一个集合——见 job_spans() 的说明。
    同一 job 内重复的 id 会被收集成列表，交给 duplicate_step_ids() 单独判。
    """
    out = {}
    for job_name, job in (doc.get("jobs") or {}).items():
        ids = {}
        for step in job.get("steps") or []:
            step_id = step.get("id")
            if step_id:
                ids.setdefault(step_id, 0)
                ids[step_id] += 1
        out[job_name] = ids
    return out


def duplicate_step_ids(doc):
    """返回 [(job 名, 重复的 step id, 出现次数)]。

    同一 job 内两个 step 用同一个 id 时，steps context 里只会留下后者的值，
    引用它的那个 step 会**静默**读到错误的结果——不报错，也不会有任何提示。
    """
    dups = []
    for job_name, ids in defined_step_ids_by_job(doc).items():
        for step_id, count in sorted(ids.items()):
            if count > 1:
                dups.append((job_name, step_id, count))
    return dups


def step_order_by_job(text, doc):
    """返回 {job 名: [(step_id 或 None, 起始行, 结束行)]}，按声明顺序。

    需要顺序而非集合，是因为 steps context 只含「已执行」的 step：引用一个
    排在自己后面的 step，求值时它还没跑，输出恒为空。
    """
    import yaml

    node = yaml.compose(text)
    order = {}
    if node is None:
        return order
    for key, value in node.value:
        if key.value != "jobs" or not isinstance(value, yaml.MappingNode):
            continue
        for job_key, job_value in value.value:
            steps = None
            for jk, jv in job_value.value:
                if jk.value == "steps":
                    steps = jv
            if steps is None or not isinstance(steps, yaml.SequenceNode):
                order[job_key.value] = []
                continue
            entries = []
            # steps 是 SequenceNode，元素直接就是 step 节点本身，
            # 不是 MappingNode 那种 key-value 对——按对解包会抛
            # "cannot unpack non-iterable MappingNode object"
            for step_node in steps.value:
                if not isinstance(step_node, yaml.MappingNode):
                    continue
                step_id = None
                for pk, pv in step_node.value:
                    if pk.value == "id":
                        step_id = pv.value
                entries.append((step_id, step_node.start_mark.line,
                                step_node.end_mark.line))
            order[job_key.value] = entries
    return order


def forward_reference_orphans(text, doc):
    """返回 [(行号, job 名, step_id, 引用位置)]：引用了尚未执行的后续 step。

    `.github/workflows/mdbook.yml` 曾真实存在这样一处：

        - name: Deploy to GitHub Pages
          id: deployment
          uses: actions/deploy-pages@v4
          environment:
            url: ${{ steps.deployment.outputs.page_url }}   # 引用自己

    steps context 按官方文档只包含 "have already run" 的 step，而
    environment.url 恰在该 step **自身执行前**求值，于是恒为空字符串——
    不报错、不让 CI 变红，只是部署完的 environment 页面永远没有站点链接。

    判定口径：同一 job 内，引用的 id 出现在引用者自身之后即前向引用。**自我
    引用**（id 与所在 step 相同）同样算前向——它是最容易被忽略的一种。
    """
    spans = job_spans(text)
    order = step_order_by_job(text, doc)
    orphans = []
    for line_no, line in enumerate(text.split("\n")):
        if line.lstrip().startswith("#"):
            continue
        job_name = job_of_line(spans, line_no)
        if job_name is None:
            continue
        entries = order.get(job_name) or []
        for ref in STEP_REF.findall(line):
            # step 的行区间是 [start_mark.line, end_mark.line) 半开区间。
            # end_mark.line 是块内容**之后**那一行的 0-based 序号，而不是
            # 最后一行内容的序号——所以必须用 `< end` 而不是 `<= end`：
            # 否则下一个 step 的首行会被算进上一个 step，进而把
            # 「引用前面已执行的 step」误判成自我引用。
            # 同时下界要用 `<= start`，因为 start_mark 正指向 `- name:` 那一行。
            for idx, (step_id, start, end) in enumerate(entries):
                if not (start <= line_no < end):
                    continue
                already_run = {
                    sid for sid, _s, _e in entries[:idx] if sid
                }
                # 自我引用：所在 step 自己就声明了该 id
                if step_id == ref:
                    orphans.append((line_no + 1, job_name, ref, "自我引用"))
                elif ref not in already_run:
                    if any(sid == ref for sid, _s, _e in entries[idx + 1:]
                           if sid):
                        orphans.append((line_no + 1, job_name, ref,
                                        "引用了尚未执行的后续 step"))
                break
    return orphans


def step_reference_orphans(text, doc):
    """返回 [(引用所在行号, 所属 job 名或 None, 无来源的 step id)]。

    逐行扫描而非全文扫描，因为要按行判断它属于哪个 job。整行注释仍被排除
    （注释里提到 steps.foo. 是正常的，本脚本自己就写了不少）。

    所属 job 为 None 表示引用出现在 jobs 之外（workflow 级 env/concurrency/
    run-name 等）。那里的 steps context 同样不可用，官方 Context availability
    表里这些位置允许的 context 不含 steps，所以照样算缺陷。
    """
    spans = job_spans(text)
    defined = defined_step_ids_by_job(doc)
    orphans = []
    for line_no, line in enumerate(text.split("\n")):
        if line.lstrip().startswith("#"):
            continue
        job_name = job_of_line(spans, line_no)
        # 顶层（job_name is None）必须**显式**判为空 dict，不能写成
        # defined.get(job_name, {})：两者对 None 都返回 {}，写法差异被
        # 完全掩盖，一处日后被改成"取全局并集"也不会有自检用例报警。
        job_ids = {} if job_name is None else defined.get(job_name, {})
        for step_id in STEP_REF.findall(line):
            if step_id not in job_ids:
                orphans.append((line_no + 1, job_name, step_id))
    return orphans


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


def jobs_without_timeout(doc):
    """返回 [未设 timeout-minutes 的 job 名]。

    GitHub 对单个 job 的默认上限是 360 分钟（6 小时）。不写 timeout-minutes
    不会报错、也不会变红——一个卡住的 job 会**静静占着 runner 跑满 6 小时**，
    期间 PR 一直挂着「检查进行中」，看起来像在正常跑。

    这正是本脚本要防的那一类：门禁本身没坏，是它的运行环境没有边界。
    修复前 48 个 job 里有 43 个没写 timeout（另 5 个早就有）；补齐后 48 个
    job 全部有超时，分档 5–45 分钟。这个检查的价值就在于此——它防的不是
    「写错 timeout」，是「新增 job 时忘了写」。

    判断条件必须用 `is None` 而非真值：`timeout-minutes: 0` 在 YAML 里
    解析成整数 0，是 False，按真值判断会把这个**已显式设了超时**的 job
    误报成缺超时。
    """
    jobs = doc.get("jobs") or {}
    return [
        name
        for name, job in jobs.items()
        if isinstance(job, dict) and job.get("timeout-minutes") is None
    ]


def audit_scripts_wired(scripts, workflow_texts):
    """返回 [(脚本路径, 说明)]：存在却没有任何 workflow 执行的审计脚本。

    这类缺陷最隐蔽的地方在于——门禁**代码本身完全正确**，自检也能全过，
    但它从来没在 CI 里跑过，于是「防护」是零。本项目已经踩过一次同源的坑：
    scan_flaky.py 的文档写着「便于 CI 定期拦截」，而当时没有任何 workflow
    调用它；审计脚本若重演一遍，排查成本极高，因为 CI 全绿恰恰说明它没跑。

    两种「没接线」都要覆盖：
      1. 整个 workflow 里找不到脚本名的 run 行——完全没调用；
      2. 调用了，但所在 job 的 if 条件恒为假——写了等于没写。

    只认 `run:` 里出现的脚本路径，而不是全文包含：注释里提到脚本名
    （例如「scan_flaky.py 原本从未被调用」）会制造假阳性。取子串匹配是为了
    兼容 `python3 scripts/x.py --flag` 这类带参数的调用行。

    workflow_texts 为 {路径: 原文}。
    """
    missing = []
    for script in scripts:
        holders = [
            (wf_path, text)
            for wf_path, text in workflow_texts.items()
            if script in text
        ]
        if not holders:
            missing.append((script, "没有任何 workflow 执行它"))
            continue

        # 找到调用行后，确认所在 job 的 if 不会恒假
        executed = False
        for _wf_path, text in holders:
            try:
                import yaml

                doc = yaml.safe_load(text)
            except Exception:  # noqa: BLE001
                break
            if not isinstance(doc, dict):
                continue
            for job_name, job in (doc.get("jobs") or {}).items():
                if not isinstance(job, dict):
                    continue
                steps = job.get("steps") or []
                if not any(
                    isinstance(s, dict) and script in str(s.get("run", ""))
                    for s in steps
                ):
                    continue
                cond = job.get("if")
                # 必须同时处理布尔与字符串两种形态：YAML 里裸写 `if: false`
                # 解析成的是布尔 False（不是字符串 "false"），只判 str 会漏检。
                # 这个漏检是变异测试逼出来的：注入 `if: false` 后门禁曾报 exit 0，
                # 正是「探针能跑通不等于探针正确」的实例。
                is_false_literal = cond is False or (
                    isinstance(cond, str) and cond.strip().lower() == "false"
                )
                if is_false_literal:
                    missing.append(
                        (script, "job `%s` 的 if 恒为假，脚本永不执行" % job_name)
                    )
                else:
                    executed = True
        # 全文提到但没有任何 run 行调用 —— 靠注释提到也算没接
        if not executed and not any(m[0] == script and "if 恒为假" in m[1] for m in missing):
            missing.append((script, "仅在注释中提及，无任何 run 步骤真正执行它"))
    return missing


# 门禁命令：除了审计脚本，还包括以命令形式调用的门禁工具。
# 只按 *.py 发现会漏掉它们——check-jsonschema 与 cargo deny 同样是让 CI
# 变红的关键检查，被 continue-on-error 吞掉的后果一样严重。
GATE_COMMANDS = (
    "check-jsonschema",
    "cargo deny",
)


def _gate_markers(gate_scripts):
    """返回需要防 continue-on-error 吞掉的标记列表。"""
    return list(gate_scripts) + list(GATE_COMMANDS)


def _swallowed_gates(scripts, doc):
    """返回被 continue-on-error 吞掉的门禁脚本名。

    `continue-on-error: true` 让 step 失败时 job 仍为绿色。这是给「辅助产物」
    用的正确设计（ci.yml 的 bench dashboard 就是这么用的，有注释说明）。
    但如果有人不慎把它加到**门禁 step** 上，那道门禁就再也不会让 CI 变红——
    它会一直显示绿色，而实际什么都没拦住。

    这类缺陷最难被发现：改动本身在 schema 上完全合法（变异验证：
    check-jsonschema exit 0），接线检查也看不出来（脚本确实被调用了），
    只有“把失败吞掉”这一层语义会静默反转门禁的效果。
    """
    swallowed = []
    for job in (doc.get("jobs") or {}).values():
        if not isinstance(job, dict):
            continue
        job_coe = job.get("continue-on-error")
        for step in job.get("steps") or []:
            if not isinstance(step, dict):
                continue
            if not (step.get("continue-on-error") or job_coe):
                continue
            run = str(step.get("run", ""))
            for script in scripts:
                if script in run:
                    swallowed.append(script)
    return swallowed


def check_file(path):
    """检查单个 workflow。

    返回 (step 引用总数, [(行号, job 名或 None, 无来源的 step id)],
            [(job 名, 重复 id, 次数)], [(行号, job 名, id, 原因)],
            [(引用方, 无来源的 needs)], needs 总条数,
            [未设 timeout 的 job 名])。
    """
    import yaml  # 延迟导入，让 --help 在缺依赖时也能给出清晰提示

    with open(path, encoding="utf-8") as fh:
        text = fh.read()
    doc = yaml.safe_load(text)
    if not isinstance(doc, dict):
        raise ValueError("%s: 顶层不是映射，可能不是合法 workflow" % path)

    n_refs = count_step_references(text)
    return (
        n_refs,
        step_reference_orphans(text, doc),
        duplicate_step_ids(doc),
        forward_reference_orphans(text, doc),
        orphan_needs(doc),
        count_needs(doc),
        jobs_without_timeout(doc),
    )


def _self_check():
    """用内联的最小样本验证 check_file 的判别力。

    直接调用生产函数，而不是另写一份判定逻辑——否则自检验证的是副本，
    生产代码改了它照样通过。前几轮已经吃过这个亏。
    """
    import tempfile

    import yaml

    cases = [
        # (说明, workflow YAML 文本, 期望的无来源 step 引用, 期望的重复 id, 期望的无来源 needs)
        (
            "引用了未声明 id 的 step",
            """
jobs:
  a:
    timeout-minutes: 10
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - name: comment
        run: echo ${{ steps.upload.outputs.artifact-id }}
""",
            [(8, "a", "upload")],
            [],
            [],
        ),
        (
            "引用了已声明 id 的 step",
            """
jobs:
  a:
    timeout-minutes: 10
    steps:
      - name: upload
        id: up
        uses: actions/upload-artifact@v4
      - name: comment
        run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
            [],
            [],
        ),
        (
            "跨 job 引用是缺陷：steps context 只在当前 job 内可见",
            # 早先把 step id 收成整个 workflow 的全局集合，于是这个用例被
            # 断言为「合法」，把真实缺陷当成了正确行为。GitHub 文档对 steps
            # context 的定义明确是 "in the current job"，此处必然解析为空。
            # 期望值带上行号（1 基）与 job 名，用来确认定位没写错。
            """
jobs:
  producer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: gen
        run: echo hi
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - run: echo ${{ steps.gen.outputs.x }}
""",
            [(12, "consumer", "gen")],
            [],
            [],
        ),
        (
            "同一 id 在不同 job 各声明一次互不冲突",
            """
jobs:
  producer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: gen
        run: echo hi
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: gen
        run: echo hi
      - run: echo ${{ steps.gen.outputs.x }}
""",
            [],
            [],
            [],
        ),
        (
            "job 末行之后的空行不属于该 job（end_mark 是包含式边界）",
            # 早先的跨 job 用例把引用放在倒数第二行，于是 end 与 end-1 两种实现
            # 都判为同一归属——变异「边界写 end-1」能蒙混过关。这里把引用放在
            # **最后一行**上：end 属于该 job，end-1 就会掉出范围、变成「顶层
            # 无来源」，两种实现才会产生可观察的差异。
            """
jobs:
  producer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: gen
        run: echo hi
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: gen
        run: echo hi
      - run: echo ${{ steps.gen.outputs.x }}
""",
            [],
            [],
            [],
        ),
        (
            "jobs 之外的 steps 引用同样是缺陷（该作用域下 context 不可用）",
            # id 刻意与某个 job 内声明的**同名**——若实现把 `if job_name else {}`
            # 误写成 `defined.get(job_name, {})`，去掉 `if` 之后仍会拿到
            # 某个 job 的 id 集合而误判为合法。变量名取 dashboard，对应的是
            # job a 声明的 dashboard。
            """
env:
  REPORT: ${{ steps.dashboard.outputs.artifact-id }}
jobs:
  a:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: dashboard
        run: echo hi
""",
            [(2, None, "dashboard")],
            [],
            [],
        ),
        (
            "job outputs 引用本 job 的 steps 合法",
            # 官方 Context availability 表里 `jobs.<job_id>.outputs.<output_id>`
            # 明确允许 steps——不能因为做了作用域收紧就误伤这类正常写法。
            """
jobs:
  a:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    outputs:
      artifact: ${{ steps.up.outputs.artifact-id }}
    steps:
      - id: up
        run: echo hi
      - run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
            [],
            [],
        ),
        (
            "同一 job 内重复 step id 是缺陷（引用会读到后一个的值）",
            """
jobs:
  a:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - id: up
        run: echo one
      - id: up
        run: echo two
      - run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
            [("a", "up", 2)],
            [],
        ),
        (
            "注释里提到无来源引用不算缺陷",
            """
jobs:
  a:
    timeout-minutes: 10
    steps:
      - name: upload
        id: up
        uses: actions/upload-artifact@v4
      # 曾错误地写成 steps.upload.outputs.artifact-id
      - run: echo ${{ steps.up.outputs.artifact-id }}
""",
            [],
            [],
            [],
        ),
        (
            "conclusion 引用同样受检",
            """
jobs:
  a:
    timeout-minutes: 10
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - if: steps.upload.conclusion == 'success'
        run: echo ok
""",
            [(7, "a", "upload")],
            [],
            [],
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
    timeout-minutes: 10
    steps:
      - name: upload
        uses: actions/upload-artifact@v4
      - run: echo ${{ steps.up }}
""",
            [],
            [],
            [],
        ),
        (
            "非映射顶层应报错而非静默放过",
            "[]\n",
            "ERROR",
            None,
            None,
        ),
    ]

    # (说明, YAML, 期望的无来源 step 引用, 期望的重复 id, 期望的前向引用,
    #  期望的无来源 needs)
    cases = [
        (c[0], c[1], c[2], c[3], [], c[4], []) for c in cases
    ] + [
        (
            "needs 引用了别的 workflow 的 job（mdbook.yml 的真实缺陷）",
            """
jobs:
  build-and-deploy:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    needs: [bench-core]
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [("build-and-deploy", "bench-core")],
            [],
        ),
        (
            "needs 引用同 workflow 内的 job 合法",
            """
jobs:
  producer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    needs: [producer]
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [],
            [],
        ),
        (
            "needs 的单字符串写法也要校验",
            """
jobs:
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    needs: ghost
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [("consumer", "ghost")],
            [],
        ),
        (
            "needs 的依赖对象写法（job: [outputs]）也要校验",
            """
jobs:
  consumer:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    needs:
      ghost: [result]
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [("consumer", "ghost")],
            [],
        ),
        (
            "没有任何 needs 不算缺陷",
            """
jobs:
  solo:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [],
            [],
        ),
        (
            "step 引用尚未执行的后续 step（mdbook.yml 同类真实缺陷）",
            """
jobs:
  deploy:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - name: Announce
        id: announce
        run: echo ${{ steps.build.outputs.page_url }}
      - name: Build
        id: build
        run: echo built
""",
            [],
            [],
            [(8, "deploy", "build", "引用了尚未执行的后续 step")],
            [],
            [],
        ),
        (
            "step 引用自己（mdbook.yml 的真实形态）",
            """
jobs:
  deploy:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
        environment:
          name: github-pages
          url: ${{ steps.deployment.outputs.page_url }}
""",
            [],
            [],
            [(11, "deploy", "deployment", "自我引用")],
            [],
            [],
        ),
        (
            "引用前面已执行的 step 是合法的",
            """
jobs:
  deploy:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    steps:
      - name: Build
        id: build
        run: echo built
      - name: Announce
        run: echo ${{ steps.build.outputs.page_url }}
""",
            [],
            [],
            [],
            [],
            [],
        ),
        (
            "job 级 environment.url 前向引用部署 step 合法：环境 URL 要等 job 跑完才登记",
            """
jobs:
  deploy:
    timeout-minutes: 10
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
""",
            [],
            [],
            [],
            [],
            [],
        ),
    ]

    # timeout 专项用例：第 7 项非空。前面 19 条用例的 YAML 已显式带上
    # timeout-minutes，所以它们期望的缺 timeout 列表是空的。
    cases += [
        (
            "未设 timeout-minutes 的 job 会被报出（本仓库 46 个 job 里 40 个裸奔）",
            """
jobs:
  no-timeout:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [],
            ["no-timeout"],
        ),
        (
            "多个 job 缺 timeout 时要全部报出，不能只报第一个",
            """
jobs:
  a:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
  b:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
  c:
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [],
            ["a", "b"],
        ),
        (
            "timeout-minutes: 0 算已设（YAML 里的 0 是 False，真值判断会误报）",
            """
jobs:
  zero:
    runs-on: ubuntu-latest
    timeout-minutes: 0
    steps:
      - run: echo hi
""",
            [],
            [],
            [],
            [],
            [],
        ),
    ]

    failures = []
    for desc, text, exp_refs, exp_dups, exp_fwd, exp_needs, exp_to in cases:
        with tempfile.NamedTemporaryFile(
            "w", suffix=".yml", delete=False, encoding="utf-8"
        ) as tmp:
            tmp.write(text.lstrip())
            tmp_path = tmp.name
        # 「非映射顶层」这类用例的期望值直接是字符串 "ERROR"，必须**在**
        # 构造 expected 元组之前判断，否则判断永远发生在元组上、对
        # "ERROR" 用例恒为假，会把「用例本就期望报错」误判成意外异常。
        expects_error = exp_refs == "ERROR"

        try:
            _n, orphans, dups, fwd, bad_needs, _m, no_to = check_file(tmp_path)
            actual = (orphans, dups, fwd, bad_needs, no_to)
            expected = (exp_refs, exp_dups, exp_fwd, exp_needs, exp_to)
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

    # glob 必须同时覆盖 .yml 与 .yaml。这条不能靠上面的用例表验证——
    # 那些用例直接把 YAML 文本喂给 check_file()，走不到「按文件名发现
    # 文件」这一步，glob 退化时它们仍会全绿。
    glob_failures = []
    for ext in (".yml", ".yaml"):
        if not any(g.endswith(ext) for g in WORKFLOW_GLOBS):
            glob_failures.append("WORKFLOW_GLOBS 不含 `%s`" % ext)
    # 扩展名对了不等于目录对了。变异「把 .github/workflows/*.yml 误写成
    # .github/*.yml」时，上面的扩展名检查仍全部通过、自检绿灯，而实际扫描
    # 会一个文件都找不到——所以必须连目录一起断言。
    if not all(".github/workflows/" in g for g in WORKFLOW_GLOBS):
        glob_failures.append(
            "WORKFLOW_GLOBS 存在不在 `.github/workflows/` 下的模式：%r"
            % (WORKFLOW_GLOBS,)
        )
    failures.extend(glob_failures)

    # audit_scripts_wired 的用例。直接调用生产函数，不另写判定逻辑。
    wired_cases = [
        (
            "run 步骤里正常调用 -> 已接线",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: python3 scripts/audit_x.py\n"
                )
            },
            ["scripts/audit_x.py"],
            [],
        ),
        (
            "只在注释里提及 -> 未接线",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: echo hi\n"
                    "# scripts/audit_x.py 原本从未被调用\n"
                )
            },
            ["scripts/audit_x.py"],
            ["scripts/audit_x.py"],
        ),
        (
            "if 为布尔 false -> 写了等于没写",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    if: false\n"
                    "    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: python3 scripts/audit_x.py\n"
                )
            },
            ["scripts/audit_x.py"],
            ["scripts/audit_x.py"],
        ),
        (
            "if 为字符串 'false' -> 同样漏跑",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    if: 'false'\n"
                    "    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: python3 scripts/audit_x.py\n"
                )
            },
            ["scripts/audit_x.py"],
            ["scripts/audit_x.py"],
        ),
        (
            "if 为正常条件表达式 -> 不误报",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    if: github.ref == 'refs/heads/main'\n"
                    "    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: python3 scripts/audit_x.py\n"
                )
            },
            ["scripts/audit_x.py"],
            [],
        ),
        (
            "带参数的调用行 -> 仍识别为已接线",
            {
                "wf.yml": (
                    "name: w\non: push\njobs:\n  j:\n    runs-on: ubuntu-latest\n"
                    "    steps:\n      - run: |\n"
                    "          python3 scripts/audit_x.py --self-check\n"
                )
            },
            ["scripts/audit_x.py"],
            [],
        ),
    ]
    for desc, wfs, scripts, expect_ids in wired_cases:
        got = audit_scripts_wired(scripts, wfs)
        got_ids = sorted({s for s, _ in got})
        if got_ids != sorted(expect_ids):
            failures.append(
                "audit_scripts_wired(%s): 期望 %r，实际 %r（%r）"
                % (desc, sorted(expect_ids), got_ids, got)
            )

    # gate_scripts 的 glob 不能退化。这条不能靠 wired_cases 验证——那些用例
    # 直接把脚本名传给 audit_scripts_wired()，走不到「从磁盘发现脚本」这一步，
    # glob 写错时它们仍会全绿。这与 WORKFLOW_GLOBS 的退化是同一类假绿灯：
    # 检查静默空转、报告显示「0 个问题」，而实际什么都没查。
    # 断言直接打 _gate_script_globs()，不另写一份字面量——否则验证的就是副本。
    gate_globs = _gate_script_globs()
    for suffix in ("audit_*.py", "scan_*.py"):
        if not any(g.endswith(suffix) for g in gate_globs):
            failures.append("gate_scripts 的 glob 不匹配任何 `%s`" % suffix)
    if not all(g.startswith(AUDIT_SCRIPT_DIR.rstrip(os.sep) + os.sep) for g in gate_globs):
        failures.append(
            "gate_scripts 的 glob 不在 `%s/` 下：%r" % (AUDIT_SCRIPT_DIR, gate_globs)
        )

    # _swallowed_gates 的用例。直接调用生产函数。
    gate_markers = _gate_markers(["scripts/audit_x.py"])
    for desc, text, expect in [
        (
            "门禁 step 无 continue-on-error -> 未被吞掉",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: python3 scripts/audit_x.py\n",
            [],
        ),
        (
            "step 级 continue-on-error -> 吞掉",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: python3 scripts/audit_x.py\n        continue-on-error: true\n",
            ["scripts/audit_x.py"],
        ),
        (
            "job 级 continue-on-error -> 同样吞掉",
            "name: w\non: push\njobs:\n  j:\n    continue-on-error: true\n"
            "    steps:\n      - run: python3 scripts/audit_x.py\n",
            ["scripts/audit_x.py"],
        ),
        (
            "continue-on-error: false -> 不算吞掉",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: python3 scripts/audit_x.py\n        continue-on-error: false\n",
            [],
        ),
        (
            "辅助产物用 continue-on-error 但不涉及门禁 -> 不误报",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: ./scripts/bench-dashboard.sh\n        continue-on-error: true\n",
            [],
        ),
        (
            "命令式门禁 check-jsonschema 被吞 -> 也算",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: check-jsonschema --builtin-schema x .github/workflows/*.yml\n"
            "        continue-on-error: true\n",
            ["check-jsonschema"],
        ),
        (
            "命令式门禁 cargo deny 被吞 -> 也算",
            "name: w\non: push\njobs:\n  j:\n    steps:\n"
            "      - run: cargo deny check\n        continue-on-error: true\n",
            ["cargo deny"],
        ),
    ]:
        doc = yaml.safe_load(text)
        got = _swallowed_gates(gate_markers, doc)
        if got != expect:
            failures.append(
                "_swallowed_gates(%s): 期望 %r，实际 %r" % (desc, expect, got)
            )

    # 工具链来源一致性：验证判别力，而不是验证副本
    with tempfile.TemporaryDirectory() as td:
        action_path = os.path.join(".github", "actions", "setup-rust", "action.yml")
        good_tc = '[toolchain]\nchannel = "nightly-2026-08-08"\nprofile = "default"\n'
        tc_path = os.path.join(td, "rust-toolchain.toml")
        wf_path = os.path.join(td, "w.yml")

        def run_pin(tc_text, wf_text):
            with open(tc_path, "w", encoding="utf-8") as fh:
                fh.write(tc_text)
            return toolchain_pin_problems(
                {wf_path: wf_text}, tc_path=tc_path, action_path=action_path
            )

        clean = run_pin(good_tc, "steps:\n  - uses: ./.github/actions/setup-rust\n")
        if clean:
            failures.append(
                "toolchain_pin_problems(合法配置): 期望 []，实际 %r" % (clean,)
            )
        got = run_pin(
            '[toolchain]\nchannel = "nightly"\n',
            "steps:\n  - uses: ./.github/actions/setup-rust\n",
        )
        if not any("浮动" in g for g in got):
            failures.append(
                "toolchain_pin_problems(浮动 channel): 未检出，实际 %r" % (got,)
            )
        got = run_pin(
            'channel = "nightly-2026-08-08"\nprofile = "stable"\n',
            "steps:\n  - uses: ./.github/actions/setup-rust\n",
        )
        if not any("[toolchain]" in g for g in got):
            failures.append(
                "toolchain_pin_problems(缺 [toolchain] 表头): 未检出，实际 %r" % (got,)
            )
        got = run_pin(good_tc, "steps:\n  - uses: dtolnay/rust-toolchain@nightly\n")
        if not any("setup-rust" in g for g in got):
            failures.append(
                "toolchain_pin_problems(直接写 dtolnay@nightly): 未检出，实际 %r"
                % (got,)
            )

    # 产物名：包名 != target 名，作业只 cp 后者。判别力直接打生产函数。
    with tempfile.TemporaryDirectory() as td:
        os.makedirs(os.path.join(td, "crates", "cli"), exist_ok=True)
        with open(os.path.join(td, "Cargo.toml"), "w", encoding="utf-8") as fh:
            fh.write('[workspace]\nmembers = ["crates/cli"]\n')
        with open(
            os.path.join(td, "crates", "cli", "Cargo.toml"), "w", encoding="utf-8"
        ) as fh:
            fh.write(
                '[package]\nname = "my-cli"\nversion = "0.2.0"\n\n'
                '[[bin]]\nname = "mybin"\npath = "src/main.rs"\n'
            )
        wf = os.path.join(td, "w.yml")

        good = artifact_path_problems(
            {wf: "run: cp target/release/mybin out\n"}, root=td
        )
        if good:
            failures.append(
                "artifact_path_problems(正确产物名): 期望 []，实际 %r" % (good,)
            )
        # 真实事故的形态：用包名当产物名
        got = artifact_path_problems(
            {wf: "run: cp target/release/my-cli out\n"}, root=td
        )
        if not any("my-cli" in g for g in got):
            failures.append(
                "artifact_path_problems(包名当产物名): 未检出，实际 %r" % (got,)
            )
        # 带 target triple 的路径（macOS 交叉编译那条）
        got = artifact_path_problems(
            {wf: "path: target/aarch64-apple-darwin/release/my-cli\n"}, root=td
        )
        if not any("my-cli" in g for g in got):
            failures.append(
                "artifact_path_problems(带 triple 的错误产物名): 未检出，实际 %r"
                % (got,)
            )
        # Windows 的 .exe 后缀不应被当成不同名字
        ok_exe = artifact_path_problems(
            {wf: "run: cp target/release/mybin.exe out.exe\n"}, root=td
        )
        if ok_exe:
            failures.append(
                "artifact_path_problems(.exe 后缀): 期望 []，实际 %r" % (ok_exe,)
            )
        # 注释里的旧名字不该报错（否则注释一提到历史就误报）
        ok_comment = artifact_path_problems(
            {wf: "#  曾写 cp target/release/my-cli ...\nrun: cp target/release/mybin o\n"},
            root=td,
        )
        if ok_comment:
            failures.append(
                "artifact_path_problems(注释): 期望 []，实际 %r" % (ok_comment,)
            )

    if failures:
        print("自检失败：", file=sys.stderr)
        for f in failures:
            print("  - %s" % f, file=sys.stderr)
        return 2
    return 0


def _gate_script_globs():
    """返回发现门禁脚本用的 glob 模式。

    抽成函数是为了让 --self-check 能直接断言这些模式——自检里另写一份
    同样的字面量，就等于验证副本，生产代码改了照样全绿。
    """
    return (
        os.path.join(AUDIT_SCRIPT_DIR, "audit_*.py"),
        os.path.join(AUDIT_SCRIPT_DIR, "scan_*.py"),
    )


def toolchain_pin_problems(
    wf_texts, tc_path="rust-toolchain.toml", action_path=None
):
    """检查「CI 装的工具链」与「cargo 实际解析到的工具链」是否同一个。

    两次真实事故，都在本地完全无法复现：

    1. `rust-toolchain.toml` 曾把 `channel` / `profile` 写在顶层、且缺
       `[toolchain]` 表头（`profile = "stable"` 也不是合法 profile，合法值
       只有 minimal/default/complete）。rustup 直接拒绝解析
       （`missing field 'toolchain'`），CI 里**每一个** cargo step 全红
       ——连续 40 次 run 0 成功。本地因为 shell 设了 `RUSTUP_TOOLCHAIN`
       **完全绕过**该文件，所以 fmt/clippy/test 一直绿。

    2. workflow 里写 `dtolnay/rust-toolchain@nightly` 装的是浮动的
       `nightly`，而仓库 pin 的是 `nightly-YYYY-MM-DD`。两个名字不是同一个
       工具链：clippy / rustfmt / target 全装在用不到的那个上，cargo 报
       `not installed for the toolchain 'nightly-...'` 或
       `can't find crate for core`。release 的四个平台构建就是这样挂的。

    因此强制：工具链只有一个来源（rust-toolchain.toml），workflow 一律
    `uses: ./.github/actions/setup-rust`，由它把文件里的 channel 交给 action。
    """
    problems = []
    valid_profiles = {"minimal", "default", "complete"}

    if action_path is None:
        action_path = os.path.join(".github", "actions", "setup-rust", "action.yml")

    if not os.path.isfile(tc_path):
        problems.append(
            "%s 不存在：CI 与本地会各自解析到不同工具链（fmt 结果会漂移）" % tc_path
        )
    else:
        with open(tc_path, encoding="utf-8") as fh:
            text = fh.read()
        # 顶层写了 channel/profile 却没有表头 —— 正是事故 1 的形态。
        # 单独判一次，因为 tomllib 能"成功"解析这种文件（当成顶层键）。
        has_table = re.search(r"(?m)^\[toolchain\]\s*$", text) is not None
        if not has_table:
            problems.append(
                "%s 缺少 [toolchain] 表头：rustup 会报 missing field `toolchain`，"
                "CI 所有 cargo step 全红（本地因 RUSTUP_TOOLCHAIN 无感）" % tc_path
            )
        try:
            import tomllib
        except ImportError:  # pragma: no cover - Python < 3.11
            tomllib = None
        if tomllib is not None:
            try:
                doc = tomllib.loads(text)
            except Exception as exc:  # noqa: BLE001
                problems.append("%s 无法被 TOML 解析：%s" % (tc_path, exc))
                doc = None
            if doc is not None and has_table:
                section = doc.get("toolchain")
                channel = str((section or {}).get("channel") or "").strip()
                if not channel:
                    problems.append("%s 的 [toolchain] 没有 channel" % tc_path)
                elif channel in ("nightly", "stable", "beta"):
                    problems.append(
                        "%s 的 channel 是浮动的 %r：rustfmt 换行启发式随 nightly "
                        "漂移，会让「本地 fmt 绿、CI fmt 红」。固定到具体日期"
                        "（如 nightly-2026-08-08）" % (tc_path, channel)
                    )
                profile = (section or {}).get("profile")
                if profile is not None and profile not in valid_profiles:
                    problems.append(
                        "%s 的 profile=%r 非法（合法值只有 %s）"
                        % (tc_path, profile, "/".join(sorted(valid_profiles)))
                    )

    if not os.path.isfile(action_path):
        problems.append(
            "%s 不存在：workflow 无处从 rust-toolchain.toml 取工具链" % action_path
        )
    else:
        with open(action_path, encoding="utf-8") as fh:
            body = fh.read()
        for needle, why in (
            ("rust-toolchain.toml", "未从 rust-toolchain.toml 读取 channel"),
            (
                "steps.channel.outputs.channel",
                "未把读到的 channel 交给 dtolnay/rust-toolchain 安装",
            ),
        ):
            if needle not in body:
                problems.append("%s %s" % (action_path, why))

    for path, text in sorted(wf_texts.items()):
        for line_no, line in enumerate(text.splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            if "dtolnay/rust-toolchain@" in line:
                problems.append(
                    "%s:%d: 直接使用 dtolnay/rust-toolchain@...；应改为 "
                    "`uses: ./.github/actions/setup-rust`（否则装的是浮动 nightly，"
                    "与 rust-toolchain.toml 的 pin 不是同一个工具链）"
                    % (os.path.relpath(path), line_no)
                )
    return problems


# `target/<triple>/release/<bin>` 或 `target/release/<bin>`。
# 只认 release/ 后面的部分：那才是真正被 cp / upload 的产物名。
ARTIFACT_PATH = re.compile(r"target/(?:[^/\s]+/)?release/([A-Za-z0-9_.+-]+)")


def workspace_bin_names(root="."):
    """返回 workspace 里所有**可执行产物**的名字。

    cargo 的产物名是 target 名（`[[bin]] name`），**不是**包名。两者可以
    不同——`sisyphus-cli` 这个包的 `[[bin]] name = "sisyphus"`，于是
    `cargo build -p sisyphus-cli` 产出的是 `target/release/sisyphus`。
    """
    names = set()
    manifests = [os.path.join(root, "Cargo.toml")]
    for pattern in ("crates/*/Cargo.toml", "*/Cargo.toml"):
        manifests.extend(sorted(glob.glob(os.path.join(root, pattern))))

    try:
        import tomllib
    except ImportError:  # pragma: no cover - Python < 3.11
        tomllib = None

    for path in manifests:
        if not os.path.isfile(path):
            continue
        if tomllib is None:  # pragma: no cover
            break
        try:
            with open(path, "rb") as fh:
                doc = tomllib.load(fh)
        except Exception:  # noqa: BLE001
            continue
        package = doc.get("package") or {}
        pkg_name = package.get("name")
        bins = doc.get("bin")
        if isinstance(bins, dict):
            bins = [bins]
        explicit = [b.get("name") for b in (bins or []) if isinstance(b, dict)]
        explicit = [b for b in explicit if isinstance(b, str)]
        if explicit:
            names.update(explicit)
            continue
        # 没写 [[bin]] 时，cargo 只在 src/main.rs 存在时产出一个与包同名的 bin
        if isinstance(pkg_name, str) and os.path.isfile(
            os.path.join(os.path.dirname(path), "src", "main.rs")
        ):
            names.add(pkg_name)
        # `src/bin/*.rs` 会被自动发现（名为文件名）
        bindir = os.path.join(os.path.dirname(path), "src", "bin")
        for src in glob.glob(os.path.join(bindir, "*.rs")):
            names.add(os.path.splitext(os.path.basename(src))[0])
    return names


def artifact_path_problems(texts, root="."):
    """workflow 里 cp / upload 的 `target/.../release/<x>` 必须真的存在。

    真实事故：release.yml 三个平台都写 `cp target/release/sisyphus-cli ...`，
    而 `sisyphus-cli` 包的 `[[bin]] name = "sisyphus"` —— 产物根本叫
    `sisyphus`。`cp: ... No such file or directory`，构建本身**成功**却仍判红。
    这类错误在本地同样不会暴露：本地只会 `cargo build`，不会去 cp 那个名字。
    """
    bins = workspace_bin_names(root)
    if not bins:  # pragma: no cover - 仓库结构异常
        return []

    problems = []
    for path, text in sorted(texts.items()):
        for line_no, line in enumerate(text.splitlines(), 1):
            stripped = line.lstrip()
            if stripped.startswith("#"):
                continue
            for match in ARTIFACT_PATH.finditer(line):
                name = match.group(1)
                # Windows 产物带 .exe；cargo 在 Linux/macOS 上不带
                stem = name[:-4] if name.endswith(".exe") else name
                if stem in bins:
                    continue
                problems.append(
                    "%s:%d: 引用了不存在的产物 `%s`（cargo 的产物名是 target 名，"
                    "不是包名；当前 workspace 的 bin：%s）"
                    % (
                        os.path.relpath(path),
                        line_no,
                        name,
                        ", ".join(sorted(bins)) or "（无）",
                    )
                )
    return problems


def main() -> int:
    if "--self-check" in sys.argv:
        return _self_check()

    paths = sorted(p for g in WORKFLOW_GLOBS for p in glob.glob(g))
    if not paths:
        print(
            "未找到任何 workflow（%s）" % " 或 ".join(WORKFLOW_GLOBS),
            file=sys.stderr,
        )
        return 1

    try:
        import yaml  # noqa: F401
    except ImportError:
        print("需要 PyYAML：pip install pyyaml", file=sys.stderr)
        return 1

    total_refs = 0
    total_needs = 0
    problems = []
    wf_texts = {}
    for path in paths:
        try:
            with open(path, encoding="utf-8") as fh:
                wf_texts[path] = fh.read()
        except OSError:
            pass
    for path in paths:
        try:
            n_refs, orphans, dups, fwd, bad_needs, n_needs, no_timeout = check_file(path)
        except Exception as exc:  # noqa: BLE001
            problems.append("%s: 解析失败 %r" % (path, exc))
            continue
        total_refs += n_refs
        total_needs += n_needs
        rel = os.path.relpath(path)
        if orphans or dups or fwd or bad_needs or no_timeout:
            for line_no, job_name, step_id in orphans:
                where = "workflow 级" if job_name is None else "job `%s`" % job_name
                problems.append(
                    "%s:%d: %s 引用了无来源的 step `%s`"
                    % (rel, line_no, where, step_id)
                )
            for job_name, step_id, count in dups:
                problems.append(
                    "%s: job `%s` 内 step id `%s` 重复 %d 次"
                    % (rel, job_name, step_id, count)
                )
            for line_no, job_name, step_id, reason in fwd:
                problems.append(
                    "%s:%d: job `%s` 的 step `%s` %s"
                    % (rel, line_no, job_name, step_id, reason)
                )
            for src_job, missing in bad_needs:
                problems.append(
                    "%s: job `%s` 的 needs 引用了不存在的 job `%s`"
                    % (rel, src_job, missing)
                )
            for job_name in no_timeout:
                problems.append(
                    "%s: job `%s` 未设 timeout-minutes，将占用 runner 至多 360 分钟"
                    % (rel, job_name)
                )
        else:
            print("✅ %s: %d 个 step 引用、%d 个 needs 引用全部有来源"
                  % (rel, n_refs, n_needs))

    # 门禁自身的接线检查：审计脚本存在却没人执行，等于没有防护。
    gate_scripts = []
    for pattern in _gate_script_globs():
        gate_scripts.extend(sorted(glob.glob(pattern)))
    if gate_scripts:
        unwired = audit_scripts_wired(gate_scripts, wf_texts)
        for script, why in unwired:
            problems.append("%s: %s" % (script, why))
        # 被 continue-on-error 吞掉的门禁：脚本确实被调用了，但失败不再变红。
        for path in paths:
            try:
                import yaml

                doc = yaml.safe_load(open(path, encoding="utf-8").read())
            except Exception:  # noqa: BLE001
                continue
            if not isinstance(doc, dict):
                continue
            for script in _swallowed_gates(_gate_markers(gate_scripts), doc):
                problems.append(
                    "%s: %s 所在 step 设了 continue-on-error，该门禁失败不会让 CI 变红"
                    % (script, os.path.relpath(path))
                )

    # 工具链来源一致性：CI 装的工具链必须与 cargo 实际解析到的是同一个。
    for p in toolchain_pin_problems(wf_texts):
        problems.append(p)

    # 产物名必须与 cargo 的 target 名一致（包名 != 产物名）。
    for p in artifact_path_problems(wf_texts):
        problems.append(p)

    if problems:
        print("", file=sys.stderr)
        for p in problems:
            print("❌ %s" % p, file=sys.stderr)
        print(
            "\n提示：steps context 只在**当前 job** 内可见（GitHub 文档："
            "「steps that have an id specified and have already run in the "
            "current job」），且只为显式声明 id 的 step 暴露数据。"
            "无来源或跨 job 的引用会静默展开为空字符串——不会让 CI 变红，"
            "只会在渲染结果里表现为空链接或缺值。",
            file=sys.stderr,
        )
        print(
            "同一 job 内重复的 step id 会让引用读到后一个 step 的结果；"
            "引用尚未执行的后续 step（含自我引用）则恒为空——"
            "steps context 只含已跑完的 step，两者都不会有提示。",
            file=sys.stderr,
        )
        print(
            "needs 只能引用同一 workflow 内的 job，跨 workflow 引用"
            "会让整个 workflow 被判为 Invalid 而完全无法运行。",
            file=sys.stderr,
        )
        print(
            "未设 timeout-minutes 的 job 不会报错也不会变红，但卡住时会"
            "静默占用 runner 达 GitHub 默认的 360 分钟上限，期间 PR 一"
            "直显示「检查进行中」。",
            file=sys.stderr,
        )
        print(
            "工具链必须只有一个来源：workflow 一律 `uses: ./.github/actions/setup-rust`，"
            "它从 rust-toolchain.toml 读 channel 再交给 dtolnay/rust-toolchain 安装。"
            "直接写 `dtolnay/rust-toolchain@nightly` 装的是浮动 nightly，而 cargo 按"
            "仓库 pin 找 `nightly-YYYY-MM-DD` —— 两者不是同一个工具链，"
            "clippy/rustfmt/target 会全装在用不到的那一个上。",
            file=sys.stderr,
        )
        return 2

    print("\n✅ 共检查 %d 个 workflow，%d 个 step 引用与 %d 个 needs 引用全部有来源"
          % (len(paths), total_refs, total_needs))
    return 0


if __name__ == "__main__":
    sys.exit(main())
