#!/usr/bin/env python3
"""Independent evaluator, never supplied to the desktop-task participant.

Verify file artifacts against the pre-task oracle. Platform-research correctness
requires a human/source review; the script only reports its saved text and URLs.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re


def verify(run):
    task = json.loads((run / "task.json").read_text())
    oracle = json.loads((run / "oracle.json").read_text())
    workspace = Path(task["workspace"])
    if not workspace.is_absolute():
        workspace = (run / workspace).resolve()
    checks = []

    def check(name, passed):
        checks.append({"assertion": name, "passed": bool(passed)})

    mapping = {
        "SPEC.md": "评审资料/SPEC.md",
        "implementation.md": "评审资料/implementation.md",
        "validation.md": "评审资料/validation.md",
        "README.md": "使用说明.md",
        "api.go": "api.go",
        "交接草稿.txt": "交接草稿.txt",
    }
    for source, target in mapping.items():
        p = workspace / target
        digest = hashlib.sha256(p.read_bytes()).hexdigest() if p.is_file() else None
        check(target + " matches original SHA-256", digest == oracle["original_hashes"][source])
        if source != target:
            check(source + " removed from root", not (workspace / source).exists())

    note = workspace / "评审交接.txt"
    try:
        # UTF-8 BOM and native CRLF do not change the required text content.
        actual = note.read_text(encoding="utf-8-sig").replace("\r\n", "\n")
        note_utf8 = True
    except (OSError, UnicodeError):
        actual, note_utf8 = None, False
    check("handoff is UTF-8 plain text", note_utf8 and not actual.startswith("{\\rtf"))
    check("handoff content matches oracle", actual == oracle["expected_note"])

    research = workspace / "平台限制核查.txt"
    try:
        research_text = research.read_text(encoding="utf-8-sig")
        research_utf8 = not research_text.startswith("{\\rtf")
    except (OSError, UnicodeError):
        research_text, research_utf8 = "", False
    check("research artifact is saved UTF-8 plain text", research_utf8)
    return {
        "run": task["run"], "baseline": task["baseline"],
        "checks": checks, "passed_assertions": sum(c["passed"] for c in checks),
        "total_assertions": len(checks),
        "files": sorted(str(p.relative_to(workspace)) for p in workspace.rglob("*") if p.is_file()),
        "research_urls": re.findall(r"https://[^\s<>]+", research_text),
        "research_text_for_manual_review": research_text,
        "research_semantics": "requires independent source review; file presence alone is not task success",
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = verify(args.run)
    text = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    print(text)
