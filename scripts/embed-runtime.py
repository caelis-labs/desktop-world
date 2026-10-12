#!/usr/bin/env python3
"""Create the deterministic runtime payload compiled into the standalone DTW CLI."""
import gzip
from pathlib import Path
import sys
import tarfile

root = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
required = [root / "libexec" / ("cua-driver.exe" if sys.platform == "win32" else "cua-driver"),
            root / "skills/desktop-world/SKILL.md", root / "LICENSING/DEPENDENCIES.json",
            root / "LICENSING/MPL-SOURCES.tar.gz", root / "mcp.json"]
for path in required:
    if not path.is_file():
        raise SystemExit(f"incomplete embedded payload: {path.relative_to(root)}")
with out.open("wb") as raw, gzip.GzipFile(fileobj=raw, mode="wb", mtime=0) as compressed:
    with tarfile.open(fileobj=compressed, mode="w") as archive:
        for path in sorted(root.rglob("*")):
            if not path.is_file() or path.name == "SHA256SUMS" or path.parts[-2:] == ("bin", "dtw"):
                continue
            relative = path.relative_to(root).as_posix()
            info = archive.gettarinfo(str(path), arcname=relative)
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mtime = 0
            with path.open("rb") as contents:
                archive.addfile(info, contents)
