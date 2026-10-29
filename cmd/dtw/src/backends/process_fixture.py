"""Owned child fixture for the std-only transport tests; no GUI or network."""
import pathlib
import os
import signal
import sys
import time

mode, ready_path = sys.argv[1:3]
stop_requested = False


def terminate_requested(_signal, _frame):
    global stop_requested
    stop_requested = True


if mode == "graceful_input":
    signal.signal(signal.SIGTERM, terminate_requested)
elif mode == "ignore_term":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)


def ready():
    pathlib.Path(ready_path).write_text("ready", encoding="utf-8")


if mode == "no_read":
    ready()
    while True:
        time.sleep(60)

if mode == "idle_exit":
    ready()
    sys.exit(0)

if mode == "cleanup_failed":
    ready()
    sys.exit(74)

if mode == "idle_nonzero":
    ready()
    os._exit(17)

if mode in ("gated_nonzero", "gated_exit_one"):
    ready()
    while not pathlib.Path(ready_path).with_name("release").exists():
        time.sleep(0.002)
    os._exit(17 if mode == "gated_nonzero" else 1)

for line in sys.stdin.buffer:
    if mode == "graceful_input":
        root = pathlib.Path(ready_path).parent
        (root / "events").write_text("down\n", encoding="utf-8")
        ready()
        while not stop_requested:
            time.sleep(0.002)
        (root / "closing").write_text("closing", encoding="utf-8")
        # The test explicitly releases the admitted operation. SIGTERM alone
        # must not skip its sole cleanup event or cause the request to replay.
        while not (root / "release").exists():
            time.sleep(0.002)
        with (root / "events").open("a", encoding="utf-8") as events:
            events.write("up\n")
        sys.exit(0)
    ready()
    if mode in ("nonzero", "nonzero_one"):
        os._exit(17 if mode == "nonzero" else 1)
    elif mode in ("hang", "ignore_term"):
        while True:
            time.sleep(60)
    elif mode == "oversize":
        sys.stdout.buffer.write(b"x" * (24 * 1024 * 1024 + 1))
        sys.stdout.buffer.flush()
    elif mode == "empty":
        sys.stdout.buffer.write(b"\n")
        sys.stdout.buffer.flush()
    elif mode == "invalid_utf8":
        sys.stdout.buffer.write(b"\xff\n")
        sys.stdout.buffer.flush()
    elif mode == "multiple_frames":
        sys.stdout.buffer.write(b"{}\n{}\n")
        sys.stdout.buffer.flush()
    elif mode == "partial_eof":
        sys.stdout.buffer.write(b'{"incomplete":')
        sys.stdout.buffer.flush()
        break
    elif mode == "eof":
        break
    else:
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
