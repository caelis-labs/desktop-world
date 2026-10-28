#!/usr/bin/env python3
"""Check real macOS worker shutdown without AX, SCK, or desktop input.

Run after building dtw; pass the exact binary path. The only native request is
release of a never-created binding. Admitted input drain itself is covered by
the independent Swift gate tests and the opt-in GUI cleanup acceptance.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import uuid


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def wait_until(predicate):
    end = time.monotonic() + 3
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(0.005)
    raise AssertionError("owned worker did not exit before deadline")


def gone(pid):
    try:
        os.kill(pid, 0)
        return False
    except ProcessLookupError:
        return True


def read_output(stream, messages):
    try:
        for line in stream:
            messages.put(json.loads(line))
    except BaseException as error:
        messages.put(error)
    finally:
        messages.put(EOFError("worker closed output"))


def check(binary, mode, directory):
    pid_file = directory / "worker.pid"
    command = [str(binary), "--dtw-native-worker"]
    if mode == "parent_death":
        # Inherited pipes stay open in this controller when the intermediate
        # parent is killed. This isolates the actual getppid watchdog from EOF.
        command = [sys.executable, "-c", """
from pathlib import Path
import subprocess, sys
worker = subprocess.Popen([sys.argv[1], '--dtw-native-worker'])
Path(sys.argv[2]).write_text(str(worker.pid))
raise SystemExit(worker.wait())
""", str(binary), str(pid_file)]
    with (directory / "stderr.log").open("w") as error:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=error, text=True, bufsize=1)
        messages = queue.Queue()
        reader = threading.Thread(target=read_output, args=(process.stdout, messages), daemon=True)
        reader.start()
        worker = process.pid
        try:
            if mode == "parent_death":
                wait_until(lambda: pid_file.exists() and pid_file.read_text().strip())
                worker = int(pid_file.read_text())
            request_id = str(uuid.uuid4())
            request = {"request_id": request_id, "operation": "release",
                       "binding": "never-created-" + str(uuid.uuid4())}
            process.stdin.write(json.dumps(request) + "\n")
            process.stdin.flush()
            reply = messages.get(timeout=3)
            require(reply == {"request_id": request_id, "result": {"released": False}}, reply)
            started = time.monotonic()
            if mode == "sigterm":
                process.terminate()
            elif mode == "eof":
                process.stdin.close()
            elif mode == "malformed":
                process.stdin.write("{not-json}\n")
                process.stdin.flush()
            elif mode == "parent_death":
                process.kill()
            else:
                raise AssertionError(mode)
            code = process.wait(timeout=2)
            require(code == (-9 if mode == "parent_death" else 0), {"mode": mode, "exit_code": code})
            if mode == "parent_death":
                require(not process.stdin.closed, "parent-death evidence accidentally closed stdin")
                wait_until(lambda: gone(worker))
            require(isinstance(messages.get(timeout=2), EOFError), "unexpected extra worker reply")
            elapsed = time.monotonic() - started
            require(elapsed < 2, {"mode": mode, "elapsed": elapsed})
            return {"mode": mode, "parent_pid": process.pid, "worker_pid": worker,
                    "exit_code": code, "elapsed_seconds": elapsed,
                    "worker_gone": gone(worker), "native_calls": ["release_unknown_binding"]}
        finally:
            if not process.stdin.closed:
                process.stdin.close()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=2)
            # Closing the still-owned pipe also lets a broken parent watchdog
            # fixture exit through EOF; this is cleanup, never passing evidence.
            if mode == "parent_death":
                wait_until(lambda: gone(worker))
            reader.join(timeout=2)
            process.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        print("Native worker controls skipped: macOS worker required")
        return
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="dtw-native-worker-controls-") as scratch:
        cases = []
        for mode in ("sigterm", "eof", "malformed", "parent_death"):
            directory = Path(scratch) / mode
            directory.mkdir()
            cases.append(check(binary, mode, directory))
        print(json.dumps({"native_worker_controls": cases, "desktop_access": False}))


if __name__ == "__main__":
    main()
