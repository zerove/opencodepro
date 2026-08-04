#!/usr/bin/env python3
"""扫描生产路径（测试模块之外）的 unwrap/expect 调用。"""
import os
import re
import sys

pattern = re.compile(r'\.(unwrap|expect)\(')
test_pattern = re.compile(r'#\[cfg\(test\)\]|#\[test\]|#\[tokio::test\]')

for root, dirs, files in os.walk('crates'):
    for f in files:
        if not f.endswith('.rs'):
            continue
        path = os.path.join(root, f)
        in_test = False
        line_no = 0
        hits = []
        with open(path, 'r', errors='ignore') as fh:
            for line in fh:
                line_no += 1
                if test_pattern.search(line):
                    in_test = True
                if not in_test and pattern.search(line):
                    stripped = line.strip()
                    if stripped.startswith('//') or 'allow(' in stripped:
                        continue
                    hits.append(line_no)
        if hits:
            print(f"{path}: {len(hits)} hits: {hits[:6]}{'...' if len(hits) > 6 else ''}")
