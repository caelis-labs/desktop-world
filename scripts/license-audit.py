#!/usr/bin/env python3
"""Collect exact locked Cargo dependency notices and MPL source for binary packages.

Includes normal and build dependencies (including proc macros) of both shipped
executables on Windows x64 and macOS arm64, excluding dev-only/unused workspace
members. Runs metadata/tree only; never edits the pinned CUA checkout.
"""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tomllib

PIN = "ac967b9006aafa10870aadb5a645b038cba9721a"
TARGETS = ("x86_64-pc-windows-msvc", "aarch64-apple-darwin")
ALLOWED = {"MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
           "Unicode-3.0", "MPL-2.0", "Unlicense", "0BSD", "MIT-0", "BSL-1.0",
           "CDLA-Permissive-2.0", "LLVM-exception"}


def run(*args):
    return subprocess.check_output([str(arg) for arg in args]).decode("utf-8")


def notice_files(directory):
    return sorted(path for path in directory.rglob("*") if path.is_file()
                  and not any(part in {"target", ".git", "node_modules"} for part in path.relative_to(directory).parts)
                  and re.search(r"(^|[._-])(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|AUTHORS|OFL)($|[._-])", path.name.upper()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cua-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    source = args.cua_source.resolve()
    assert run("git", "-C", source, "rev-parse", "HEAD").strip() == PIN
    assert not run("git", "-C", source, "status", "--porcelain").strip(), "CUA source must be clean"
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    entries = {}
    source_crates = {}
    overrides_root = repo / "packaging/third-party"
    overrides = json.loads((overrides_root / "overrides.json").read_text(encoding="utf-8"))
    for kind, manifest, package in (
        ("adapter", repo / "cmd/dtw/Cargo.toml", "desktop-world"),
        ("worker", source / "libs/cua-driver/rust/Cargo.toml", "cua-driver"),
    ):
        lockfile = manifest.parent / "Cargo.lock"
        lock = tomllib.loads(lockfile.read_text(encoding="utf-8"))
        checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
        # metadata's platform filter omits some host-side proc-macro/build
        # packages that cargo tree retains. Both release targets cover both
        # supported build hosts; union their package metadata by canonical ID.
        all_packages = {}
        for target in TARGETS:
            metadata = json.loads(run("cargo", "+1.98.0", "metadata", "--manifest-path", manifest,
                "--format-version", "1", "--locked", "--filter-platform", target))
            all_packages.update({p["id"]: p for p in metadata["packages"]})
        for target in TARGETS:
            tree = run("cargo", "+1.98.0", "tree", "--manifest-path", manifest, "--package", package,
                "--target", target, "--edges", "normal,build", "--prefix", "none", "--format", "{p}",
                "--no-dedupe", "--locked")
            selected = set()
            for line in tree.splitlines():
                match = re.match(r"^(\S+) v(\S+)", line)
                assert match, line
                selected.add(match.groups())
            packages = [p for p in all_packages.values() if (p["name"], p["version"]) in selected]
            assert len(packages) == len(selected), {
                "root": kind, "target": target, "metadata_count": len(packages), "tree_count": len(selected),
                "missing": sorted(selected - {(p['name'], p['version']) for p in packages}),
                "duplicates": [p['id'] for p in packages if sum(q['name']==p['name'] and q['version']==p['version'] for q in packages)>1]}
            for p in packages:
                key = p["name"] + "-" + p["version"]
                declared = p.get("license")
                assert declared, f"Missing license: {key}"
                tokens = set(re.findall(r"[A-Za-z0-9][A-Za-z0-9.-]*", declared.replace("/", " OR ")))
                assert tokens - {"OR", "AND", "WITH"} <= ALLOWED, f"Unreviewed license: {key}: {declared}"
                origin = p.get("source") or ("this repository" if p["name"] == "desktop-world" else "git+https://github.com/trycua/cua#" + PIN)
                if origin.startswith("git+https://github.com/trycua/cua"):
                    origin = "git+https://github.com/trycua/cua#" + PIN
                if key in entries:
                    assert entries[key]["license"] == declared and entries[key]["source"] == origin, key
                    assert entries[key]["registry_checksum"] == checksums.get((p["name"], p["version"])), key
                    entries[key]["uses"].add(kind + ":" + target)
                    continue
                directory = Path(p["manifest_path"]).parent
                if origin.startswith("registry+"):
                    crate_archive = directory.parent.parent.parent / "cache" / directory.parent.name / (key + ".crate")
                    assert hashlib.sha256(crate_archive.read_bytes()).hexdigest() == checksums[(p["name"], p["version"])], key
                files = notice_files(directory)
                if p.get("license_file"):
                    files.append(directory / p["license_file"])
                if origin == "this repository":
                    files.append(repo / "LICENSE")
                elif origin.startswith("git+https://github.com/trycua/cua"):
                    files.append(source / "LICENSE.md")
                if not files:
                    if origin.startswith("registry+"):
                        override = overrides[key]
                        vcs = json.loads((directory / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
                        assert override["revision"] == vcs["git"]["sha1"] and override["declared_license"] == declared, key
                        for notice in override["notices"]:
                            file = overrides_root / notice["file"]
                            assert hashlib.sha256(file.read_bytes()).hexdigest() == notice["sha256"], str(file)
                            files.append(file)
                    else:
                        # Workspace crates inherit the unmodified repository MIT license.
                        files = [repo / "LICENSE"] if origin == "this repository" else [source / "LICENSE.md"]
                notices = []
                for file in sorted(set(files)):
                    relative = file.relative_to(directory).as_posix() if file.is_relative_to(directory) else (
                        "upstream/" + file.relative_to(overrides_root / key).as_posix() if file.is_relative_to(overrides_root) else file.name)
                    data = file.read_bytes()
                    assert data.strip(), str(file)
                    data.decode("utf-8")  # No silent loss of attribution bytes.
                    destination = output / "licenses" / key / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.write_bytes(data)
                    notice = {"file": destination.relative_to(output).as_posix(), "sha256": hashlib.sha256(data).hexdigest()}
                    if file.is_relative_to(overrides_root):
                        notice["source_url"] = next(n["url"] for n in overrides[key]["notices"] if overrides_root / n["file"] == file)
                    notices.append(notice)
                entries[key] = {"name": p["name"], "version": p["version"], "license": declared,
                    "source": origin, "registry_checksum": checksums.get((p["name"], p["version"])),
                    "repository": p.get("repository"), "authors": p.get("authors", []),
                    "uses": {kind + ":" + target}, "notices": notices}
                if "MPL-2.0" in declared:
                    assert origin.startswith("registry+"), "Review non-registry MPL source separately"
                    source_crates[key] = crate_archive
    # Unmodified corresponding source accompanies the MPL binary components.
    source_archive = output / "MPL-SOURCES.tar.gz"
    with source_archive.open("wb") as raw, gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode="w", format=tarfile.PAX_FORMAT) as archive:
            for key, crate_archive in sorted(source_crates.items()):
                with tarfile.open(crate_archive) as published:
                    for member in sorted(published.getmembers(), key=lambda item: item.name):
                        if not member.isfile():
                            continue
                        path = Path(member.name)
                        assert not path.is_absolute() and ".." not in path.parts and path.parts[0] == key, member.name
                        data = published.extractfile(member).read()
                        info = tarfile.TarInfo(member.name)
                        info.size = len(data)
                        info.mode = 0o644
                        archive.addfile(info, io.BytesIO(data))
                # Some published UniFFI crates omit the workspace license file.
                # Add the byte-verified license from their exact published VCS revision.
                if key in overrides:
                    for notice in overrides[key]["notices"]:
                        data = (overrides_root / notice["file"]).read_bytes()
                        info = tarfile.TarInfo(key + "/upstream-notices/" + Path(notice["file"]).name)
                        info.size = len(data)
                        info.mode = 0o644
                        archive.addfile(info, io.BytesIO(data))
    # Rust's statically linked standard-library/runtime attribution catalog.
    sysroot = Path(run("rustc", "+1.98.0", "--print", "sysroot").strip())
    rust_docs = sysroot / "share/doc/rust"
    assert (rust_docs / "COPYRIGHT-library.html").is_file(), "Missing Rust runtime copyright catalog"
    shutil.copyfile(rust_docs / "COPYRIGHT-library.html", output / "RUST-COPYRIGHT-library.html")
    shutil.copytree(rust_docs / "licenses", output / "rust-runtime-licenses", dirs_exist_ok=True)
    for entry in entries.values():
        entry["uses"] = sorted(entry["uses"])
    inventory = {"cua_revision": PIN, "rust_toolchain": "1.98.0", "targets": list(TARGETS),
        "scope": "Locked normal and build dependencies of adapter and worker; dev-only/unused members excluded",
        "packages": [entries[key] for key in sorted(entries)],
        "mpl_source_packages": sorted(source_crates),
        "mpl_source_sha256": hashlib.sha256(source_archive.read_bytes()).hexdigest()}
    assets = source / "libs/cua-driver/rust/crates/cursor-overlay/assets"
    inventory["embedded_components"] = [
        {"name": "Inter font", "license": "OFL-1.1", "source_path": "cursor-overlay/assets/Inter.ttf",
         "source_sha256": hashlib.sha256((assets / "Inter.ttf").read_bytes()).hexdigest(),
         "notice": "licenses/cursor-overlay-0.34.0/assets/Inter-OFL.txt"},
        {"name": "CUA default cursor theme", "license": "MIT", "source_path": "cursor-overlay/assets/cua.default.cua-theme",
         "source_sha256": hashlib.sha256((assets / "cua.default.cua-theme").read_bytes()).hexdigest(),
         "notice": "licenses/cursor-overlay-0.34.0/LICENSE.md"},
    ]
    assert all((output / item["notice"]).is_file() for item in inventory["embedded_components"])
    (output / "DEPENDENCIES.json").write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    text = ["# Binary dependency notices", "", "Exact locked versions for Windows x64 and macOS arm64.",
            "Normal and build dependencies are included; dev-only and unused optional workspace members are excluded.",
            "Full original notices are in licenses/. Rust runtime notices are in RUST-COPYRIGHT-library.html",
            "and rust-runtime-licenses/. Unmodified MPL corresponding source is in MPL-SOURCES.tar.gz.",
            "CUA derived-code attribution is also preserved in the package's CUA-THIRD_PARTY_NOTICES.md.",
            "System Windows/Apple libraries are supplied by the OS and are not redistributed in this archive.",
            "Node, perception models and Cua Spaces are not bundled.", "",
            "Embedded Inter font: OFL-1.1, original notice at licenses/cursor-overlay-0.34.0/assets/Inter-OFL.txt.",
            "Embedded default cursor theme: CUA MIT. QuickJS-ng/native crypto and other vendored notices are",
            "preserved recursively in their respective crate notice directories.", "",
            "| Dependency | Declared license | Original notices |", "| --- | --- | --- |"]
    for key in sorted(entries):
        entry = entries[key]
        links = ", ".join("[" + n["file"].split("/")[-1] + "](" + n["file"] + ")" for n in entry["notices"])
        text.append(f"| {entry['name']} {entry['version']} | {entry['license']} | {links} |")
    (output / "THIRD-PARTY.md").write_text("\n".join(text) + "\n", encoding="utf-8")
    print(json.dumps({"packages": len(entries), "mpl_source_packages": sorted(source_crates), "output": str(output)}))


if __name__ == "__main__":
    main()
