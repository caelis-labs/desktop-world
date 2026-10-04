#!/usr/bin/env python3
"""Opt-in, per-feature real-desktop acceptance. Every effect uses dtw/host SDK.
Each scenario gets new apps and app-owned logs; OS permission is never requested.
Windows F4/F5 must be run on a logged-in Windows 11 amd64 machine.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("features", nargs="*", choices=["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9"])
parser.add_argument("--helper", type=Path, help="validate an existing packaged dtw instead of rebuilding it")
args = parser.parse_args()
windows = platform.system() == "Windows"
features = args.features or (["F4", "F5"] if windows else ["F1", "F2", "F3", "F6", "F7", "F8", "F9"])
if platform.system() not in ["Darwin", "Windows"]:
    parser.error("a real interactive macOS or Windows desktop is required")
if any(f in ["F4", "F5"] for f in features) and not windows:
    parser.error("F4/F5 require Windows, not cross-build or mocked evidence")
if any(f in ["F2", "F3", "F6", "F7", "F8", "F9"] for f in features) and windows:
    parser.error("these app-side state/getter fixtures are macOS-specific; Windows acceptance is deferred")
run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:6]
out = ROOT / "artifacts" / ("feature-acceptance-" + run_id)
out.mkdir(parents=True)
env = dict(os.environ, GOWORK="off")
exe = ".exe" if windows else ""
helper = args.helper.resolve() if args.helper else ROOT / "bin" / ("dtw" + exe)
test = ROOT / "bin" / ("features.test" + exe)
def run(command, **options):
    return subprocess.run(command, cwd=ROOT, env=env, check=True, **options)
if not args.helper:
    run(["go", "build", "-o", str(helper), "./cmd/dtw"])
run(["go", "test", "-c", "-o", str(test), "./tests/acceptance"])
if windows:
    fixture = ROOT / "bin" / "DWNativeFixture.exe"
    run(["go", "build", "-o", str(fixture), "./tests/native-fixtures/windows"])
else:
    run(["./script/build_and_run.sh", "--build-only"])
    fixture = ROOT / "bin" / "DWNativeFixture.app"
    human = ROOT / "bin" / "DTWHumanFixture.app"
    if human.exists():
        shutil.rmtree(human)
    shutil.copytree(fixture, human)
    info_path = human / "Contents" / "Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    info["CFBundleIdentifier"] = "dev.caelis.desktop-world.human-fixture"
    info["CFBundleName"] = "DTWHumanFixture"
    info_path.write_bytes(plistlib.dumps(info))
    run(["codesign", "--force", "--sign", "-", str(human)])
files = run(["git", "ls-files", "-c", "-o", "--exclude-standard", "-z"], capture_output=True).stdout.decode().split("\0")
source = {}
for name in sorted(set(files)):
    path = ROOT / name
    if path.is_file() and path.suffix in [".go", ".mod", ".sum", ".m", ".h", ".swift", ".mjs", ".py", ".sh", ".yml"]:
        source[name] = hashlib.sha256(path.read_bytes()).hexdigest()
(out / "source-sha256.json").write_text(json.dumps(source, indent=2) + "\n")
summary = {
    "run": run_id, "platform": platform.platform(),
    "base_commit": run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip(),
    "helper_version": json.loads(run([str(helper), "version"], capture_output=True, text=True).stdout),
    "helper_sha256": hashlib.sha256(helper.read_bytes()).hexdigest(),
    "go_version": run(["go", "version"], capture_output=True, text=True).stdout.strip(),
    "source_manifest": "source-sha256.json", "features": {},
}
active = []
def launch(label, *, front=False, background=False, slow=0, rows=0, semantic=""):
    title = f"DTW {label} {run_id}"
    log = out / (label.lower() + ".jsonl")
    if windows:
        child = subprocess.Popen([str(fixture), "-title", title, "-log", str(log), "-rows", str(rows)], cwd=ROOT)
        active.append(child)
    else:
        bundle = human if front else fixture
        command = ["/usr/bin/open", "-n"]
        if background:
            command.append("-g")
        run(command + [str(bundle), "--args", "--title", title, "--log", str(log), "--slow-count", str(slow), "--trace-reads", "1", "--background", "1" if background else "0", "--semantic-case", semantic])
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if log.exists() and log.stat().st_size:
            rows_read = [json.loads(line) for line in log.read_text().splitlines()]
            if not windows and rows_read[0]["pid"] not in active:
                active.append(rows_read[0]["pid"])
            if any(row.get("event") == "ready" for row in rows_read):
                return title, log
        time.sleep(.05)
    raise RuntimeError("fixture did not become ready")
def cleanup():
    for child in active:
        if windows:
            child.terminate()
            child.wait(timeout=5)
        else:
            try:
                os.kill(child, 15)
            except ProcessLookupError:
                pass
    active.clear()
try:
    for feature in features:
        case_env = dict(env, DTW_NATIVE_HELPER=str(helper))
        if feature in ["F1", "F6", "F7", "F8", "F9"]:
            front_title, front_log = launch(feature + "-human", front=True)
            # Only fixture startup sets up the simulated human's desktop.
            title, log = launch(feature + "-background", background=True, semantic={"F6": "selection", "F7": "check", "F8": "scroll", "F9": "capture"}.get(feature, ""))
            case_env.update(DTW_FOREGROUND_TITLE=front_title, DTW_FOREGROUND_LOG=str(front_log))
            test_name = {"F1": "TestNativeNoSharedInput", "F6": "TestNativeSetSelected", "F7": "TestNativeSetChecked", "F8": "TestNativeScrollIntoView", "F9": "TestNativeWindowCapture"}[feature]
        else:
            title, log = launch(feature, slow=12 if feature == "F3" else 0, rows=1000 if feature == "F4" else 0)
            test_name = {"F2": "TestNativeSetExpanded", "F3": "TestNativeFieldPlan", "F4": "TestNativeWindowsContinuation", "F5": "TestNativeWindowsManagedControl"}[feature]
        if feature == "F9":
            case_env["DTW_CAPTURE_ASSETS"] = str(out / "f9-images")
        case_env.update({f"DTW_{feature}_TITLE": title, f"DTW_{feature}_LOG": str(log)})
        completed = subprocess.run([str(test), "-test.v", "-test.run", f"^{test_name}$"], cwd=ROOT, env=case_env, capture_output=True, text=True, timeout=75)
        (out / (feature.lower() + "-test.txt")).write_text(completed.stdout + completed.stderr)
        summary["features"][feature] = {"passed": completed.returncode == 0, "test": test_name, "log": log.name}
        print(completed.stdout + completed.stderr, end="", flush=True)
        if completed.returncode:
            raise RuntimeError(feature + " native acceptance failed")
        cleanup()
finally:
    cleanup()
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(out)
