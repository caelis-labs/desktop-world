#!/usr/bin/env python3
"""Freeze a participant packet and a separate evaluator packet. Does not run UI."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--output", type=Path, required=True)
p.add_argument("--baseline", default="6185e89")
p.add_argument("--helper", type=Path, default=Path("bin/desktop-world"))
a = p.parse_args()
root = Path(__file__).resolve().parent.parent
out = a.output.resolve()
out.mkdir(parents=True, exist_ok=False)
participant, evaluator = out/"participant", out/"evaluator"
for d in [participant/"workspace", participant/"harness", participant/"bin", participant/"docs", participant/"skills/desktop-world", evaluator]:
    d.mkdir(parents=True)
workspace = participant/"workspace"
for name, source in [("SPEC.md","SPEC.md"),("README.md","README.md"),("api.go","api.go"),("implementation.md","docs/implementation.md"),("validation.md","docs/validation.md")]:
    (workspace/name).write_bytes(subprocess.check_output(["git","show",f"{a.baseline}:{source}"],cwd=root))
expected = f"Desktop World 基线交接\n\n基线：{a.baseline}\n状态：实验版，尚未正式发布。\n已验证：macOS AppKit 和 Chrome 的真实输入、截图、去重与控件生命周期。\nWindows：后端已实现，通过交叉构建与静态检查，尚未实机验收。\n同步：目前采用刷新与 Watch 轮询。\n下一步：以真实任务衡量陌生 Agent 的上手成本、效率和恢复能力。\n"
(workspace/"交接草稿.txt").write_text(expected.replace("尚未实机验收。","已完成实机验收。"))
def write(path, obj):
    path.write_text(json.dumps(obj,ensure_ascii=False,indent=2)+"\n")
task={"run":out.name,"baseline":a.baseline,"workspace":"workspace","sources":["https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput","https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-eventsoverview"],"tasks":"See PROMPT.md; Finder package, TextEdit revision, official browser research"}
write(participant/"task.json",task)
write(evaluator/"task.json",dict(task,workspace="../participant/workspace"))
write(evaluator/"oracle.json",{"expected_note":expected,"original_hashes":{f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in workspace.iterdir()}})
for src,dst in [(root/"tests/usability/PROMPT.md",participant/"PROMPT.md"),(root/"tests/usability/RUBRIC.md",evaluator/"RUBRIC.md"),(root/"scripts/verify-usability.py",evaluator/"verify-usability.py"),(root/"docs/helper.md",participant/"docs/helper.md"),(root/"skills/desktop-world/SKILL.md",participant/"skills/desktop-world/SKILL.md"),(a.helper.resolve(),participant/"bin/desktop-world")]:
    shutil.copy2(src,dst)
write(participant/"host.json",{"helper":"bin/desktop-world","args":["serve","--write-app","访达","--write-app","文本编辑","--audit","harness/audit.jsonl"],"host_setup_required":"Before the run, trusted host must authorize the selected Chrome instance with --write-app or --write-app-window; see evaluator/RUBRIC.md. Do not let the participant enlarge scope."})
write(participant/"result-template.json",{"status":"not_run","agent":None,"model":None,"host_version":None,"started_utc":None,"stopped_utc":None,"first_observe_seconds":None,"first_action_seconds":None,"tasks":{},"calls":None,"request_bytes":None,"response_bytes":None,"model_input_tokens":None,"model_output_tokens":None,"compactions":None,"human_hints":None,"adapter_lines":None,"builds":None,"restarts":None,"remaining_processes":None,"notes":[]})
manifest={"platform":"darwin-arm64 local development build; unsigned distribution","baseline":a.baseline,"source_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=root,text=True).strip(),"source_dirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=root,text=True).strip()),"files":{str(f.relative_to(participant)):hashlib.sha256(f.read_bytes()).hexdigest() for f in participant.rglob("*") if f.is_file()}}
write(participant/"manifest.json",manifest)
(out/"START-HERE.md").write_text("先由你阅读 evaluator/RUBRIC.md，准备干净桌面与 host.json 中的 Chrome 授权。\n\n给外部 Agent 仅提供 participant 目录，在全新对话中粘贴 participant/PROMPT.md。不要给它 evaluator 或本轮内部盲测报告。\n\n结束后确认 helper 已退出，再运行 evaluator/verify-usability.py；结果模板尚未执行，不能视为验收通过。\n\n压缩包分别包含 participant 和 evaluator，解压时保持这两个目录为同级。当前二进制为本机开发构建；Antigravity 宿主权限、其他机器的签名/公证/Gatekeeper 尚未验收。\n")
for folder in [participant,evaluator]:
    with zipfile.ZipFile(out/(folder.name+".zip"),"w",zipfile.ZIP_DEFLATED) as z:
        for f in folder.rglob("*"):
            if f.is_file():z.write(f,f.relative_to(out))
print(out/"START-HERE.md")
