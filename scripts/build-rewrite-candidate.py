#!/usr/bin/env python3
"""Build an unpublished, native-host DTW rewrite candidate from a clean commit.

Requires Python 3.11+, Git, rustup with 1.98.0 and rust-docs, and the platform's
native compiler/SDK. This is an explicit release-stage command, not a PR check.
It neither installs software nor creates tags, releases, or Agent configuration.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import struct
import subprocess
import tarfile
import time
import tomllib
import zipfile


RUST = "1.98.0"
TARGETS = {
    "aarch64-apple-darwin": ("Darwin", {"arm64", "aarch64"}, "dtw"),
    "x86_64-pc-windows-msvc": ("Windows", {"amd64", "x86_64"}, "dtw.exe"),
}
NOTICE_PREFIXES = ("license", "licence", "notice", "copying", "copyright")

_plugin_spec = importlib.util.spec_from_file_location("dtw_plugin_package", Path(__file__).with_name("plugin_package.py"))
plugin_package = importlib.util.module_from_spec(_plugin_spec)
_plugin_spec.loader.exec_module(plugin_package)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


class Commands:
    """Retain every attempted command and its stdout/stderr, including failures."""

    def __init__(self, output):
        self.output = output
        self.rows = []
        (output / "logs").mkdir()

    def run(self, args, *, cwd, env=None, check=True):
        args = [str(arg) for arg in args]
        stem = f"{len(self.rows) + 1:03d}"
        row = {"argv": args, "cwd": str(cwd), "stdout": f"logs/{stem}.out",
               "stderr": f"logs/{stem}.err", "returncode": None}
        self.rows.append(row)
        write_json(self.output / "commands.json", self.rows)
        with (self.output / row["stdout"]).open("wb") as out, (self.output / row["stderr"]).open("wb") as err:
            completed = subprocess.run(args, cwd=cwd, env=env, stdout=out, stderr=err, check=False)
        row["returncode"] = completed.returncode
        write_json(self.output / "commands.json", self.rows)
        stdout = (self.output / row["stdout"]).read_bytes()
        stderr = (self.output / row["stderr"]).read_bytes()
        require(not check or completed.returncode == 0,
                f"Command failed ({completed.returncode}): {args[0]}; inspect {row['stderr']}")
        return stdout.decode("utf-8", errors="replace"), stderr.decode("utf-8", errors="replace")


def reserve_output(repo, output):
    require(".." not in output.parts, "--output must not contain '..'")
    output = output.absolute()
    require(output.parent.is_dir(), "--output parent must already exist")
    require(not output.exists() and not output.is_symlink(), "--output must be a new path")
    output = output.parent.resolve(strict=True) / output.name
    require(not output.is_relative_to(repo.resolve()), "--output must be outside the source checkout")
    output.mkdir()  # Atomic reservation; never clean up another invocation's path.
    return output


def native_host(target):
    expected, machines, _ = TARGETS[target]
    require(platform.system() == expected and platform.machine().lower() in machines,
            f"{target} must be built on its native {expected} host; cross-builds are not candidates")


def macos_signing_identity(output, team):
    """Accept exactly one valid imported identity belonging to the expected team."""
    require(re.fullmatch(r"[A-Z0-9]{10}", team), "Expected a ten-character Apple Team ID")
    identities = re.findall(r'^\s*\d+\)\s+([A-Fa-f0-9]{40})\s+"([^"\n]+)"\s*$', output, re.MULTILINE)
    require(len(identities) == 1, "Temporary keychain must contain exactly one valid signing identity")
    fingerprint, name = identities[0]
    require(name.startswith("Developer ID Application: ") and name.endswith(f" ({team})"),
            "Imported signing identity is not Developer ID Application for the expected team")
    return fingerprint.upper()


def source_identity(repo, run):
    def git(*args):
        return run(["git", *args], cwd=repo)[0].strip()
    require(Path(git("rev-parse", "--show-toplevel")).resolve() == repo.resolve(),
            "Source directory must be the Git checkout root")
    require(not git("status", "--porcelain=v1", "--untracked-files=all"),
            "Candidate source must be clean, including untracked files; commit reviewed work first")
    head, tree = git("rev-parse", "HEAD"), git("rev-parse", "HEAD^{tree}")
    require(re.fullmatch(r"[0-9a-f]{40,64}", head) and re.fullmatch(r"[0-9a-f]{40,64}", tree),
            "Invalid Git commit/tree identity")
    # Git archive exports the committed LF blob, even on a clean CRLF checkout.
    lock_blob, _ = run(["git", "show", f"{head}:cmd/dtw/Cargo.lock"], cwd=repo)
    return {"commit": head, "tree": tree, "commit_unix_seconds": int(git("show", "-s", "--format=%ct", "HEAD")),
            "cargo_lock_sha256": hashlib.sha256(lock_blob.encode("utf-8")).hexdigest()}


def export_source(repo, archive, commit, run):
    # git archive also applies core.autocrlf. Export committed text consistently
    # on Windows without rewriting intentionally binary/-text notice files.
    run(["git", "-c", "core.autocrlf=false", "-c", "core.eol=lf", "archive",
         "--format=tar", f"--output={archive}", commit], cwd=repo)


def extract_source(archive, destination):
    destination.mkdir()
    with tarfile.open(archive) as tar:
        for item in tar:
            name = PurePosixPath(item.name)
            require(not name.is_absolute() and ".." not in name.parts,
                    "Unsafe source archive path")
            require(item.isdir() or item.isfile(), "Source archive links/special files are not supported")
            path = destination.joinpath(*name.parts)
            if item.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with tar.extractfile(item) as src, path.open("xb") as dst:
                    shutil.copyfileobj(src, dst)
                path.chmod(0o755 if item.mode & 0o111 else 0o644)


def check_version(source):
    package = tomllib.loads((source / "cmd/dtw/Cargo.toml").read_text(encoding="utf-8"))["package"]
    version = package["version"]
    require(re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version), "Invalid package version")
    numeric = tuple(int(n) for n in version.split("-")[0].split("."))
    require(numeric >= (0, 2, 0), "Rewrite candidates require version 0.2.0 or newer; legacy versions are refused")
    require((source / "version.txt").read_text().strip() == version, "version.txt differs from Cargo.toml")
    locked = tomllib.loads((source / "cmd/dtw/Cargo.lock").read_text(encoding="utf-8"))["package"]
    roots = [p for p in locked if p["name"] == package["name"] and not p.get("source")]
    require(len(roots) == 1 and roots[0]["version"] == version, "Cargo.lock root version differs from Cargo.toml")
    return version, locked


def dependency_packages(metadata, manifest):
    """Walk only the already target-filtered normal/build resolve graph, not packages[]."""
    require(metadata.get("resolve") is not None, "Cargo metadata has no resolved graph")
    roots = [p for p in metadata["packages"] if Path(p["manifest_path"]).resolve() == manifest.resolve()]
    require(len(roots) == 1, "Cannot identify the actual DTW package in Cargo metadata")
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = roots[0]["id"]
    require(metadata["resolve"].get("root") in (None, root), "Cargo metadata root differs from DTW")
    seen, pending = set(), [root]
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        require(current in nodes and current in packages, "Incomplete resolved dependency graph")
        seen.add(current)
        for dep in nodes[current]["deps"]:
            kinds = dep.get("dep_kinds")
            require(isinstance(kinds, list) and kinds, "Missing dependency kind in Cargo metadata")
            if any(k.get("kind") in (None, "normal", "build") for k in kinds):
                pending.append(dep["pkg"])
    return [packages[key] for key in sorted(seen - {root})]


def notice_files(root):
    return sorted(p for p in root.rglob("*") if p.name.lower().startswith(NOTICE_PREFIXES)
                  and (p.is_file() or p.is_symlink()))


def copy_notice(src, dst, root):
    require(src.is_file() and not src.is_symlink() and src.resolve().is_relative_to(root.resolve()),
            f"Notice must be a regular file within its source: {src}")
    require(0 < src.stat().st_size <= 32 * 1024 * 1024, f"Empty or unexpectedly large notice: {src}")
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dst)
    return {"file": dst.name, "sha256": digest(dst), "bytes": dst.stat().st_size}


def collect_notices(source, package, dependencies, locked, sysroot):
    base = package / "notices"
    overrides_root = source / "packaging/third-party"
    overrides = json.loads((overrides_root / "overrides.json").read_text(encoding="utf-8"))
    inventory = []
    for dep in dependencies:
        root = Path(dep["manifest_path"]).parent
        name = f"{dep['name']}-{dep['version']}"
        require(re.fullmatch(r"[A-Za-z0-9_.+-]+", name), "Unsafe package name/version")
        matches = [p for p in locked if p["name"] == dep["name"] and p["version"] == dep["version"]
                   and p.get("source") == dep.get("source")]
        require(len(matches) == 1, f"Dependency source does not match lockfile: {name}")
        require(dep.get("source") or root.resolve().is_relative_to(source.resolve()),
                f"External path dependency is not in the fixed source snapshot: {name}")
        dest = base / "crates" / name
        require(not dest.exists(), f"Same name/version has multiple sources: {name}")
        notices = notice_files(root)
        if dep.get("license_file"):
            declared = Path(dep["license_file"])
            if not declared.is_absolute():
                declared = root / declared
            require(declared.is_file() and declared.resolve().is_relative_to(root.resolve()),
                    f"Declared license file is missing/outside package: {name}")
            if declared not in notices:
                notices.append(declared)
        files = []
        for path in notices:
            relative = path.relative_to(root)
            info = copy_notice(path, dest / relative, root)
            info["file"] = relative.as_posix()
            files.append(info)
        # The bundled C library is not a separate Cargo package.
        if dep["name"] == "rquickjs-sys":
            require(any(p.relative_to(root).as_posix() == "quickjs/LICENSE" for p in notices),
                    "rquickjs-sys is missing quickjs/LICENSE")
        override = None
        if not files:
            override = overrides.get(name)
            require(override and override.get("declared_license") == dep.get("license"),
                    f"No license/notice files for {name}; add a reviewed exact-source override")
            for item in override["notices"]:
                rel = PurePosixPath(item["file"])
                require(not rel.is_absolute() and ".." not in rel.parts and "\\" not in item["file"] and ":" not in item["file"],
                        "Unsafe notice override path")
                src = overrides_root.joinpath(*rel.parts)
                require(not src.is_symlink() and src.resolve().is_relative_to(overrides_root.resolve()),
                        "Notice override escapes its source directory")
                require(digest(src) == item["sha256"], f"Notice override hash mismatch: {name}")
                info = copy_notice(src, dest / src.name, overrides_root)
                info["upstream_url"] = item["url"]
                files.append(info)
            require(files, f"Empty notice override: {name}")
        inventory.append({"name": dep["name"], "version": dep["version"], "source": dep.get("source"),
                          "checksum": matches[0].get("checksum"), "license": dep.get("license"),
                          "repository": dep.get("repository"), "files": files, "override": override})
    rust_root = sysroot / "share/doc/rust"
    required = [rust_root / "COPYRIGHT.html", rust_root / "COPYRIGHT-library.html"]
    require(all(p.is_file() for p in required) and (rust_root / "licenses").is_dir(),
            f"Rust runtime notices missing; install rust-docs for toolchain {RUST} and retry in a new output")
    rust_notices = []
    for src in required + sorted(p for p in (rust_root / "licenses").rglob("*") if p.is_file()):
        relative = src.relative_to(rust_root)
        info = copy_notice(src, base / "rust" / relative, rust_root)
        info["file"] = relative.as_posix()
        rust_notices.append(info)
    require(len(rust_notices) > 2, "Rust licenses directory is empty; install rust-docs")
    report = {"selection": "root-reachable normal/build edges from cargo metadata --locked --filter-platform",
              "crates": inventory, "rust_toolchain": RUST, "rust_notices": rust_notices}
    write_json(base / "DEPENDENCIES.json", report)
    return report


def binary_format(path, target):
    data = path.read_bytes()
    if target == "aarch64-apple-darwin":
        require(len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe", "Expected a thin 64-bit Mach-O")
        _, cpu, _, kind, count, size, _, _ = struct.unpack_from("<8I", data)
        require(cpu == 0x0100000C and kind == 2, "Expected an arm64 Mach-O executable")
        require(size <= len(data) - 32, "Truncated Mach-O commands")
        offset, minimum, rpaths = 32, None, []
        for _ in range(count):
            require(offset + 8 <= 32 + size, "Invalid Mach-O load command")
            cmd, length = struct.unpack_from("<II", data, offset)
            require(length >= 8 and offset + length <= 32 + size, "Invalid Mach-O load command size")
            if cmd == 0x32:
                require(length >= 24, "Truncated LC_BUILD_VERSION")
                plat, encoded = struct.unpack_from("<II", data, offset + 8)
                require(plat == 1, "Expected macOS build platform")
                minimum = f"{encoded >> 16}.{(encoded >> 8) & 255}.{encoded & 255}"
            if cmd == 0x8000001C:
                require(length >= 12, "Truncated LC_RPATH")
                start = struct.unpack_from("<I", data, offset + 8)[0]
                require(12 <= start < length, "Invalid LC_RPATH string")
                end = data.find(b"\0", offset + start, offset + length)
                require(end != -1, "Unterminated LC_RPATH")
                rpaths.append(data[offset + start:end].decode("utf-8"))
            offset += length
        require(minimum == "14.2.0", f"Mach-O deployment target must be 14.2.0, found {minimum}")
        return {"format": "Mach-O", "architecture": "arm64", "deployment_target": minimum, "rpaths": rpaths}
    require(len(data) >= 64 and data[:2] == b"MZ", "Expected a PE executable")
    offset = struct.unpack_from("<I", data, 0x3C)[0]
    require(offset + 24 <= len(data) and data[offset:offset + 4] == b"PE\0\0", "Invalid PE header")
    machine, sections = struct.unpack_from("<HH", data, offset + 4)
    optional_size = struct.unpack_from("<H", data, offset + 20)[0]
    characteristics = struct.unpack_from("<H", data, offset + 22)[0]
    opt = offset + 24
    require(machine == 0x8664 and optional_size >= 128 and opt + optional_size <= len(data),
            "Expected an x86_64 PE32+ executable")
    require(characteristics & 2 and not characteristics & 0x2000, "Expected an executable PE, not a DLL")
    require(struct.unpack_from("<H", data, opt)[0] == 0x20B, "Expected PE32+ optional header")
    require(struct.unpack_from("<H", data, opt + 68)[0] == 3, "Expected a Windows console executable")
    table = opt + optional_size
    require(table + sections * 40 <= len(data), "Truncated PE section table")
    def rva(value):
        for i in range(sections):
            vsize, va, rawsize, raw = struct.unpack_from("<4I", data, table + i * 40 + 8)
            if va <= value < va + max(vsize, rawsize):
                require(value - va < rawsize and raw + value - va < len(data), "Unmapped PE import RVA")
                return raw + value - va
        raise ValueError("Unmapped PE import RVA")
    imports = []
    import_rva, import_size = struct.unpack_from("<II", data, opt + 120)
    require(import_rva and import_size >= 20, "PE has no inspectable import directory")
    start = rva(import_rva)
    for index in range(min(import_size // 20, 4096)):
        pos = start + index * 20
        require(pos + 20 <= len(data), "Truncated PE imports")
        fields = struct.unpack_from("<5I", data, pos)
        if not any(fields):
            break
        name_offset = rva(fields[3])
        end = data.find(b"\0", name_offset, name_offset + 256)
        require(end != -1, "Invalid PE import name")
        imports.append(data[name_offset:end].decode("ascii").lower())
    else:
        raise ValueError("Unterminated PE imports")
    require(imports, "PE imports cannot be empty")
    require(not any(n.startswith(("vcruntime", "msvcp", "ucrtbase", "api-ms-win-crt", "msvcr"))
                    for n in imports), "Dynamic CRT dependency found despite +crt-static")
    return {"format": "PE32+", "architecture": "x86_64", "subsystem": "console", "imports": sorted(imports),
            "static_crt_import_check": "passed", "minimum_windows_runtime": "unverified"}


def archive_package(package, archive, epoch):
    files = sorted(p for p in package.rglob("*") if p.is_file())
    require(files and all(not p.is_symlink() for p in package.rglob("*")), "Package must contain only regular files/directories")
    require(not (package / "SHA256SUMS").exists(), "Package manifest already exists")
    sums = "".join(f"{digest(p)}  {p.relative_to(package).as_posix()}\n" for p in files)
    (package / "SHA256SUMS").write_text(sums, encoding="utf-8")
    stamp = time.gmtime(max(315532800, min(epoch, 4354819198)))[:6]
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
        for path in sorted(p for p in package.rglob("*") if p.is_file()):
            relative = path.relative_to(package).as_posix()
            info = zipfile.ZipInfo(f"{package.name}/{relative}", stamp)
            info.create_system = 3
            mode = 0o755 if relative in ("bin/dtw", "bin/dtw.exe") else 0o644
            info.external_attr = (stat.S_IFREG | mode) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            out.writestr(info, path.read_bytes())
    verify_archive(archive, package.name)
    (archive.parent / (archive.name + ".sha256")).write_text(f"{digest(archive)}  {archive.name}\n", encoding="utf-8")


def verify_archive(archive, package_name):
    with zipfile.ZipFile(archive) as src:
        names = src.namelist()
        require(len(names) == len(set(names)), "Duplicate archive entry")
        prefix = package_name + "/"
        require(all(n.startswith(prefix) and ".." not in PurePosixPath(n).parts and "\\" not in n and ":" not in n
                    for n in names), "Unsafe archive path")
        sums_name = prefix + "SHA256SUMS"
        require(sums_name in names, "Archive lacks SHA256SUMS")
        require(all(stat.S_ISREG(src.getinfo(n).external_attr >> 16) for n in names), "Archive contains a link or special file")
        expected = {}
        for line in src.read(sums_name).decode("utf-8").splitlines():
            require(re.fullmatch(r"[0-9a-f]{64}  .+", line), "Invalid SHA256SUMS line")
            checksum, name = line.split("  ", 1)
            require(name not in expected, "Duplicate checksum entry")
            expected[name] = checksum
        require(set(expected) == {n[len(prefix):] for n in names if n != sums_name}, "Archive inventory differs from SHA256SUMS")
        for name, checksum in expected.items():
            require(hashlib.sha256(src.read(prefix + name)).hexdigest() == checksum, f"Archive hash mismatch: {name}")
            mode = src.getinfo(prefix + name).external_attr >> 16
            require(stat.S_ISREG(mode), "Archive contains a link or special file")
            if name in ("bin/dtw", "bin/dtw.exe"):
                require(mode & 0o111 == 0o111, "Archive lost executable mode")
        require(src.testzip() is None, "Archive CRC validation failed")


def package_readme(target):
    executable = TARGETS[target][2]
    readme = f"""# DTW rewrite candidate

This native package is built from fixed source. Automatic release CI verifies
the complete immutable artifacts before publication. Keep BUILD.json,
SHA256SUMS and the outer CANDIDATE.json together as construction evidence.
The outer archive .sha256 covers the immutable ZIP. Notarization receipts, when
requested, are external because they are obtained after this ZIP is fixed.

Extract to a stable location. Run `bin/{executable} --version`, then use the
absolute extracted binary path in examples/mcp.json or examples/codex.toml.
Use MCP arguments ["mcp"]. DTW manages input routing and foreground access
when an operation requires it; the Agent needs no extra input-mode argument.
This same directory includes Agent Plugins 1.0/Caelis, Claude Code, Antigravity
and DSH Cordis entries around the same executable and Skill. Follow install.md
and docs/install-caelis.md, docs/install-dsh.md or docs/install-agy.md. Claude
Code can load it with `claude --plugin-dir /absolute/path/to/extracted/package`.
Choose the plugin route or ordinary MCP/Skill registration; do not register both
copies simultaneously. Examples are templates; this archive does not install
anything, edit Agent configuration, or grant OS permissions.

On Windows, in PowerShell: `& 'C:\\path with spaces\\bin\\dtw.exe' --version`.
Use escaped backslashes or forward slashes in the JSON/TOML path.
Windows builds use the static CRT. CI runs native contracts and final-package
MCP metadata/EOF checks without desktop input. Optional UI diagnostics are in
the self-contained PowerShell smoke and manual matrix at acceptance/windows/README.md.
They use the installed DTW through standard MCP and require no Rust or CUA.
The same-source Chrome workbench server and HTML are in
acceptance/browser-workbench/; its startup and UTF-8 state readback commands
are in the Windows acceptance README. Only this optional browser task requires
Python 3; the installed DTW runtime and WinForms smoke do not.
On macOS, the binary deployment target is 14.2, arm64. The user grants
Accessibility and Screen Recording permissions when needed. CI verifies signing
and notarization; interactive GUI checks are optional diagnostics.
A standalone ZIP is not stapled; an Accepted online notarization submission is
not proof of offline Gatekeeper or GUI acceptance. Linux is not packaged here.

Ordinary operations use stable app IDs and exec({{app,source}}). Harness approvals
remain with the Agent; no custom Host is required. See the canonical Skill for
pagination, input, reference lifetime, unknown-effect and no-replay behavior.
Full dependency notices are under notices/; adapted source notices and hashes
are in THIRD_PARTY_NOTICES.md, docs/native-source-provenance*.md and
docs/background-input-port.md.
"""
    readme += """
Caelis integration uses the root Agent Plugins 1.0 plugin.json and
mcp.json. Install this extracted directory with `caelis plugin install <path>
--json`, then check `caelis plugin status desktop-world --json` and start a new
Session. The stable plugin ID is desktop-world regardless of this directory's
name. Caelis chooses the standard manifest; Claude continues to use its own
metadata. Both launch the same target executable and bundled Skill. If an older Claude
registration used the directory name as its ID, remove that old registration
before installing this format; preserve the package and other plugins.
"""
    return readme


def package_assets(source, package, version, target):
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", "install.md"):
        shutil.copy2(source / name, package / name)
    shutil.copytree(source / "skills/desktop-world", package / "skills/desktop-world")
    for relative in ("docs/native-source-provenance.md", "docs/native-source-provenance-cross-platform.md",
                     "docs/background-input-port.md", "docs/install.md", "docs/install-dsh.md",
                     "docs/install-caelis.md", "docs/install-agy.md"):
        dest = package / relative
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source / relative, dest)
    shutil.copytree(source / "packaging/third-party", package / "packaging/third-party")
    if target.endswith("msvc"):
        shutil.copy2(source / "docs/windows-cursor-port.md", package / "docs/windows-cursor-port.md")
        shutil.copytree(source / "tests/acceptance/windows", package / "acceptance/windows")
        shutil.copytree(source / "tests/fixtures/browser-workbench", package / "acceptance/browser-workbench")
    (package / "examples").mkdir()
    for name in ("mcp.json", "codex.toml"):
        shutil.copy2(source / "docs/examples" / name, package / "examples" / name)
    (package / "README.md").write_text(package_readme(target), encoding="utf-8")
    return plugin_package.write_plugin_entries(source, package, version, target)


def build(repo, output, target, signing_identity=None, notary_profile=None, notary_keychain=None, cargo_target_dir=None):
    output = reserve_output(repo, output)
    commands = Commands(output)
    try:
        native_host(target)
        cargo_target_dir = Path(cargo_target_dir).resolve() if cargo_target_dir is not None else output / "target"
        require(target == "aarch64-apple-darwin" or not (signing_identity or notary_profile or notary_keychain),
                "macOS signing/notarization options cannot be used for Windows")
        require(not notary_profile or signing_identity, "--notary-profile requires --signing-identity")
        require(not notary_keychain or notary_profile, "--notary-keychain requires --notary-profile")
        keychain_args = []
        if notary_keychain:
            notary_keychain = Path(notary_keychain).resolve(strict=True)
            require(notary_keychain.is_file(), "--notary-keychain must be an existing keychain file")
            keychain_args = ["--keychain", str(notary_keychain)]
        require(signing_identity != "-", "An ad-hoc identity cannot establish a signed distribution candidate")
        identity = source_identity(repo, commands.run)
        source_tar = output / "source.tar"
        export_source(repo, source_tar, identity["commit"], commands.run)
        source = output / "source"
        extract_source(source_tar, source)
        version, locked = check_version(source)
        require(digest(source / "cmd/dtw/Cargo.lock") == identity["cargo_lock_sha256"], "Exported lockfile differs from source identity")
        toolchain = {}
        for tool in ("rustc", "cargo"):
            text, _ = commands.run(["rustup", "run", RUST, tool, "--version", "--verbose"], cwd=source)
            require(re.search(rf"^{tool} {re.escape(RUST)}(?:\s|$)", text), f"Expected {tool} {RUST}")
            toolchain[tool] = text.strip()
        require(f"host: {target}" in toolchain["rustc"], "Rust host must equal the native candidate target")
        sysroot_text, _ = commands.run(["rustup", "run", RUST, "rustc", "--print", "sysroot"], cwd=source)
        sysroot = Path(sysroot_text.strip())
        env = os.environ.copy()
        for key in list(env):
            if key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN", "CARGO_BUILD_RUSTC") or (
                    key.startswith("CARGO_TARGET_") and key.endswith(("_RUSTFLAGS", "_LINKER"))):
                env.pop(key)
        build_env = {"CARGO_TARGET_DIR": str(cargo_target_dir), "SOURCE_DATE_EPOCH": str(identity["commit_unix_seconds"]),
                     "RUSTC": str(sysroot / "bin" / ("rustc.exe" if target.endswith("msvc") else "rustc")),
                     "RUSTC_WRAPPER": "", "RUSTC_WORKSPACE_WRAPPER": "",
                     "RUSTFLAGS": "-C target-feature=+crt-static" if target.endswith("msvc") else ""}
        if target.endswith("darwin"):
            build_env["MACOSX_DEPLOYMENT_TARGET"] = "14.2"
            for args in (["xcodebuild", "-version"], ["xcrun", "swiftc", "--version"], ["xcrun", "--show-sdk-version"]):
                text, err = commands.run(args, cwd=source)
                toolchain[" ".join(args)] = (text + err).strip()
        else:
            for args in (["where.exe", "cl.exe"], ["where.exe", "link.exe"]):
                text, _ = commands.run(args, cwd=source)
                toolchain[" ".join(args)] = text.strip()
            text, err = commands.run(["cl.exe", "/Bv"], cwd=source, check=False)
            require("Compiler Version" in text + err or "编译器" in text + err, "Cannot establish native MSVC compiler version; use a Developer Command Prompt")
            toolchain["cl.exe /Bv"] = (text + err).strip()
            # Windows os.environ is case-insensitive, but its dict copy has
            # uppercase keys. Keep the original labels in recorded metadata.
            toolchain["windows_sdk"] = {k: env.get(k.upper(), env.get(k)) for k in ("WindowsSDKVersion", "VCToolsVersion")}
        env.update(build_env)
        manifest = source / "cmd/dtw/Cargo.toml"
        cargo = ["rustup", "run", RUST, "cargo"]
        metadata_text, _ = commands.run(cargo + ["metadata", "--locked", "--format-version", "1", "--filter-platform", target,
                                               "--manifest-path", str(manifest)], cwd=source, env=env)
        metadata = json.loads(metadata_text)
        write_json(output / "cargo-metadata.json", metadata)
        dependencies = dependency_packages(metadata, manifest)
        package = output / f"desktop-world-{version}-{target}"
        package.mkdir()
        notices = collect_notices(source, package, dependencies, locked, sysroot)
        commands.run(cargo + ["build", "--release", "--locked", "--target", target, "--manifest-path", str(manifest), "--bin", "dtw"], cwd=source, env=env)
        binary = package / "bin" / TARGETS[target][2]
        binary.parent.mkdir()
        shutil.copy2(cargo_target_dir / target / "release" / binary.name, binary)
        binary.chmod(0o755)
        fmt = binary_format(binary, target)
        signing = {"status": "not_requested", "installation_verification": "unverified"}
        if target.endswith("darwin"):
            text, _ = commands.run(["otool", "-L", str(binary)], cwd=source)
            dylibs = [line.strip().split(" (", 1)[0] for line in text.splitlines()[1:] if line.strip()]
            require(dylibs and all(p.startswith(("/usr/lib/", "/System/Library/", "@rpath/libswift")) for p in dylibs),
                    "Unexpected non-system macOS dynamic dependency; inspect otool log")
            if any(p.startswith("@rpath/") for p in dylibs):
                require(set(fmt["rpaths"]) == {"/usr/lib/swift"}, "Swift @rpath is not restricted to the system Swift runtime")
            fmt["dynamic_dependencies"] = dylibs
            if signing_identity:
                commands.run(["codesign", "--force", "--options", "runtime", "--timestamp", "--identifier", "dev.caelis.dtw",
                              "--sign", signing_identity, str(binary)], cwd=source)
                commands.run(["codesign", "--verify", "--strict", "--verbose=2", str(binary)], cwd=source)
                text, err = commands.run(["codesign", "-d", "--verbose=4", str(binary)], cwd=source)
                details = text + err
                require("Identifier=dev.caelis.dtw" in details and re.search(r"flags=.*\bruntime\b", details),
                        "Signed binary lacks the requested identifier/hardened runtime")
                if notary_profile:
                    require("Authority=Developer ID Application:" in details, "Notarization requires Developer ID Application signing")
                signing = {"status": "verified", "identifier": "dev.caelis.dtw", "hardened_runtime": True,
                           "details": details, "installation_verification": "unverified"}
        text, _ = commands.run([str(binary), "--version"], cwd=output)
        require(text.strip() == f"dtw {version}", "Built CLI version does not match source")
        plugin_entries = package_assets(source, package, version, target)
        require(source_identity(repo, commands.run) == identity, "Source changed during candidate build")
        require(digest(source / "cmd/dtw/Cargo.lock") == identity["cargo_lock_sha256"], "Build changed the exported lockfile")
        build_info = {"schema": 1, "kind": "unpublished-rewrite-candidate", "version": version, "target": target,
                      "source": identity, "source_archive_sha256": digest(source_tar), "toolchain": toolchain,
                      "build_environment": build_env, "host": {"system": platform.system(), "release": platform.release(), "machine": platform.machine()},
                      "binary": {"path": f"bin/{binary.name}", "sha256": digest(binary), **fmt},
                      "skill_sha256": digest(package / "skills/desktop-world/SKILL.md"),
                      "plugin_package": plugin_entries,
                      "dependency_count": len(notices["crates"]), "dependencies_sha256": digest(package / "notices/DEPENDENCIES.json"),
                      "signing": signing, "notarization": {"requested": bool(notary_profile), "status": "unverified",
                          "meaning": "Verdict is obtained only after this immutable archive; see outer CANDIDATE.json and notarization receipts"},
                      "runtime_evidence": {"cli_version": text.strip(), "gui": "unverified", "clean_install": "unverified", "minimum_os_runtime": "unverified"},
                      "commands": commands.rows}
        write_json(package / "BUILD.json", build_info)
        archive = output / (package.name + ".zip")
        archive_package(package, archive, identity["commit_unix_seconds"])
        archive_hash = digest(archive)
        notarization = {"status": "unverified", "requested": bool(notary_profile), "stapled": False}
        if notary_profile:
            # Persist the original upload ID before waiting. A timeout must be
            # recoverable by querying this ID, never by uploading another ZIP.
            text, _ = commands.run(["xcrun", "notarytool", "submit", str(archive), "--keychain-profile", notary_profile,
                                   "--output-format", "json"] + keychain_args, cwd=output, check=False)
            receipt = json.loads(text)
            write_json(output / "notarization-submit.json", receipt)
            require(commands.rows[-1]["returncode"] == 0 and isinstance(receipt, dict)
                    and isinstance(receipt.get("id"), str) and receipt["id"].strip(),
                    "Notarization upload did not return a successful submission ID; inspect the original receipt/log")
            text, _ = commands.run(["xcrun", "notarytool", "wait", receipt["id"], "--keychain-profile", notary_profile,
                                   "--timeout", "20m", "--output-format", "json"] + keychain_args, cwd=output, check=False)
            waited = json.loads(text)
            write_json(output / "notarization-wait.json", waited)
            require(commands.rows[-1]["returncode"] == 0 and isinstance(waited, dict)
                    and waited.get("id") == receipt["id"] and waited.get("status") == "Accepted",
                    "Notarization wait did not return Accepted; retain/query the original submission ID without resubmitting")
            text, _ = commands.run(["xcrun", "notarytool", "info", receipt["id"], "--keychain-profile", notary_profile,
                                   "--output-format", "json"] + keychain_args, cwd=output)
            verified = json.loads(text)
            write_json(output / "notarization-info.json", verified)
            require(verified.get("status") == "Accepted" and verified.get("id") == receipt["id"], "Notarization info did not confirm the original Accepted submission")
            notarization.update(status="Accepted", submission_id=receipt["id"],
                                receipts=["notarization-submit.json", "notarization-wait.json", "notarization-info.json"])
        require(digest(archive) == archive_hash, "Archive changed after validation/submission")
        require(digest(binary) == build_info["binary"]["sha256"], "Packaged binary changed after archival")
        verify_archive(archive, package.name)
        summary = {"schema": 1, "kind": "unpublished-rewrite-candidate", "version": version, "target": target,
                   "source": identity, "archive": archive.name, "archive_sha256": archive_hash,
                   "binary_sha256": digest(binary), "build_manifest_sha256": digest(package / "BUILD.json"),
                   "notarization": notarization, "signing": signing["status"], "gui": "unverified", "clean_install": "unverified"}
        write_json(output / "CANDIDATE.json", summary)
        return summary
    except BaseException as error:
        write_json(output / "FAILURE.json", {"type": type(error).__name__, "message": str(error), "partial_output_retained": str(output)})
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--output", required=True, type=Path, help="New directory outside the source checkout; parent must exist")
    parser.add_argument("--cargo-target-dir", type=Path,
                        help="Reusable Cargo build directory; defaults to OUTPUT/target. Keep caches separate from candidate/signing output")
    parser.add_argument("--signing-identity", help="macOS codesign identity; no entitlements are inferred")
    parser.add_argument("--notary-profile", help="Existing macOS notarytool keychain profile; requires signing identity")
    parser.add_argument("--notary-keychain", type=Path,
                        help="Existing keychain containing the profile; requires --notary-profile")
    args = parser.parse_args()
    try:
        summary = build(Path(__file__).resolve().parent.parent, args.output, args.target,
                        args.signing_identity, args.notary_profile, args.notary_keychain, args.cargo_target_dir)
    except (OSError, ValueError, KeyError, RuntimeError) as error:
        parser.exit(1, f"Candidate build failed; inspect the retained output: {error}\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
