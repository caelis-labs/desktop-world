"""Owned child fixture for the std-only transport tests; no GUI or network."""
import pathlib
import sys
import time

mode, ready_path = sys.argv[1:3]


def ready():
    pathlib.Path(ready_path).write_text("ready", encoding="utf-8")


if mode == "no_read":
    ready()
    while True:
        time.sleep(60)

if mode == "idle_exit":
    ready()
    sys.exit(0)

for line in sys.stdin.buffer:
    ready()
    if mode == "hang":
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
