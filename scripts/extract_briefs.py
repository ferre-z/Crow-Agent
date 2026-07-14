#!/usr/bin/env python3
"""Extract one task brief per markdown file from a wave plan."""
import re, sys, pathlib

src = pathlib.Path(sys.argv[1]).read_text()
pattern = re.compile(r"^### Task ([\d.]+) \u2014 (.+?)$", re.MULTILINE)
matches = list(pattern.finditer(src))
out_dir = pathlib.Path(sys.argv[2])
out_dir.mkdir(parents=True, exist_ok=True)

for i, m in enumerate(matches):
    task_id = m.group(1)
    title = m.group(2)
    start = m.start()
    end = matches[i + 1].start() if i + 1 < len(matches) else len(src)
    chunk = src[start:end].rstrip() + "\n"
    out = out_dir / f"task-{task_id.replace('.', '-')}.md"
    out.write_text(chunk)
    print(f"wrote {out}  ({len(chunk)} bytes)")
