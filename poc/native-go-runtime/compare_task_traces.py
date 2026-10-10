#!/usr/bin/env python3
"""Compare fixed model-visible MCP result payloads; bytes are not tokens."""

import json
import re
import sys
from pathlib import Path


def read_trace(path: Path):
    result = {}
    for section in re.split(r"^## ", path.read_text(), flags=re.MULTILINE)[1:]:
        execution_id = section.split(" | ", 1)[0].strip()
        text_match = re.search(
            r"content\.text \((\d+) UTF-8 bytes\):\n(.*?)\nstructuredContent ",
            section,
            flags=re.DOTALL,
        )
        structured_match = re.search(
            r"structuredContent .*?:\n(\{.*\})\s*$", section, flags=re.DOTALL
        )
        if not text_match or not structured_match:
            raise ValueError(f"cannot parse {path}: {execution_id}")
        text_bytes = int(text_match.group(1))
        if len(text_match.group(2).encode()) != text_bytes:
            raise ValueError(f"text byte mismatch: {execution_id}")
        structured = json.loads(structured_match.group(1))
        structured_bytes = len(
            json.dumps(structured, ensure_ascii=False, separators=(",", ":")).encode()
        )
        result[execution_id] = {
            "text": text_bytes,
            "structured": structured_bytes,
            "sum": text_bytes + structured_bytes,
            "state": structured.get("state"),
        }
    return result


def main():
    baseline = read_trace(Path(sys.argv[1]))
    compact = read_trace(Path(sys.argv[2]))
    common = [execution_id for execution_id in baseline if execution_id in compact]
    print("execution_id\tbaseline_text\tbaseline_structured\tcompact_text\tcompact_structured\tbaseline_sum\tcompact_sum")
    for execution_id in common:
        a, b = baseline[execution_id], compact[execution_id]
        print(f"{execution_id}\t{a['text']}\t{a['structured']}\t{b['text']}\t{b['structured']}\t{a['sum']}\t{b['sum']}")
    before = sum(baseline[execution_id]["sum"] for execution_id in common)
    after = sum(compact[execution_id]["sum"] for execution_id in common)
    print(f"COMMON_TOTAL\t\t\t\t\t{before}\t{after}")
    print(f"extra_compact_calls={','.join(sorted(set(compact) - set(baseline)))}")
    print("metric=UTF-8 text bytes plus compact structured JSON bytes, without MCP envelope, tool arguments, model tokenizer or cache")


if __name__ == "__main__":
    main()
