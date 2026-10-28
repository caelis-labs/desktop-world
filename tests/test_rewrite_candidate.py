"""Candidate gates: local temporary Git regression, no Cargo/signing/network/GUI."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import stat
import struct
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


SPEC = importlib.util.spec_from_file_location("rewrite_candidate", Path(__file__).parents[1] / "scripts/build-rewrite-candidate.py")
candidate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(candidate)
MAC = "aarch64-apple-darwin"
WIN = "x86_64-pc-windows-msvc"


def put(root, name, data):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data if isinstance(data, bytes) else data.encode())
    return path


def macho(minimum=(14 << 16) | (2 << 8), cpu=0x0100000C):
    return struct.pack("<8I", 0xFEEDFACF, cpu, 0, 2, 1, 24, 0, 0) + struct.pack("<6I", 0x32, 24, 1, minimum, minimum, 0)


def pe(import_name="kernel32.dll", machine=0x8664):
    data = bytearray(1024)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<HH", data, 68, machine, 1)
    struct.pack_into("<HH", data, 84, 240, 2)
    struct.pack_into("<H", data, 88, 0x20B)
    struct.pack_into("<H", data, 88 + 68, 3)
    struct.pack_into("<II", data, 88 + 120, 0x1000, 40)
    struct.pack_into("<4I", data, 88 + 240 + 8, 512, 0x1000, 512, 512)
    struct.pack_into("<5I", data, 512, 0, 0, 0, 0x1080, 0)
    encoded = import_name.encode() + b"\0"
    data[640:640 + len(encoded)] = encoded
    return bytes(data)


class Fixture(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        put(self.repo, "version.txt", "0.2.0\n")
        put(self.repo, "cmd/dtw/Cargo.toml", '[package]\nname="desktop-world"\nversion="0.2.0"\n')
        put(self.repo, "cmd/dtw/Cargo.lock", 'version=4\n[[package]]\nname="desktop-world"\nversion="0.2.0"\n')
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", "docs/native-source-provenance.md",
                     "docs/native-source-provenance-cross-platform.md", "skills/desktop-world/SKILL.md"):
            put(self.repo, name, "Original reviewed source content\n")
        put(self.repo, "docs/examples/mcp.json", '{"mcpServers":{"desktop-world":{"command":"/absolute/path/to/dtw","args":["mcp"]}}}\n')
        put(self.repo, "docs/examples/codex.toml", '[mcp_servers.desktop-world]\ncommand="/absolute/path/to/dtw"\nargs=["mcp"]\n')
        put(self.repo, "plugins/claude/.claude-plugin/plugin.json", '{"name":"desktop-world","version":"0.1.3"}\n')
        put(self.repo, "packaging/third-party/overrides.json", "{}\n")
        put(self.repo, "tests/acceptance/windows/README.md", "Windows independent acceptance\n")
        put(self.repo, "tests/acceptance/windows/smoke.ps1", "# Owned-window standard MCP smoke\n")
        self.sysroot = self.root / "rust"
        for name in ("COPYRIGHT.html", "COPYRIGHT-library.html", "licenses/MIT.txt", "licenses/Apache.txt"):
            put(self.sysroot, "share/doc/rust/" + name, "Rust runtime notice\n")

    def dep(self, name="foo", version="1.0.0", notices=("LICENSE",)):
        directory = self.root / "registry" / (name + "-" + version)
        manifest = put(directory, "Cargo.toml", "[package]\n")
        for path in notices:
            put(directory, path, "Copyright original authors\nMIT license text\n")
        package = {"id": name, "name": name, "version": version, "manifest_path": str(manifest),
                   "source": "registry+https://example.invalid/index", "license": "MIT", "repository": "https://example.invalid/project"}
        locked = {key: package[key] for key in ("name", "version", "source")}
        locked["checksum"] = "b" * 64
        return package, locked

    def fake_commands(self, *, dirty=False, binary_version="0.2.0", notary_status="Accepted", target=MAC):
        calls = []
        lock_blob = (self.repo / "cmd/dtw/Cargo.lock").read_bytes().replace(b"\r\n", b"\n")
        def run(owner, args, *, cwd, env=None, check=True):
            args = list(map(str, args))
            calls.append(args)
            while args[:2] == ["git", "-c"]:
                args = ["git"] + args[3:]
            owner.rows.append({"argv": args, "cwd": str(cwd), "returncode": 0})
            if args[:2] == ["git", "rev-parse"]:
                return ({"--show-toplevel": str(self.repo), "HEAD": "a" * 40, "HEAD^{tree}": "b" * 40}[args[2]], "")
            if args[:2] == ["git", "status"]:
                return (" M skills/desktop-world/SKILL.md\n" if dirty else "", "")
            if args[:2] == ["git", "show"]:
                if args[-1].endswith(":cmd/dtw/Cargo.lock"):
                    return lock_blob.decode("utf-8"), ""
                return "1700000000\n", ""
            if args[:2] == ["git", "archive"]:
                path = Path(next(a.split("=", 1)[1] for a in args if a.startswith("--output=")))
                with tarfile.open(path, "w") as archive:
                    for p in sorted(self.repo.rglob("*")):
                        if p.is_file():
                            name = p.relative_to(self.repo).as_posix()
                            if name == "cmd/dtw/Cargo.lock":
                                member = tarfile.TarInfo(name)
                                member.size = len(lock_blob)
                                archive.addfile(member, io.BytesIO(lock_blob))
                            else:
                                archive.add(p, arcname=name, recursive=False)
                return "", ""
            if args[:3] == ["rustup", "run", candidate.RUST]:
                if args[3:5] == ["rustc", "--version"]:
                    return f"rustc {candidate.RUST} (fixture)\nhost: {target}\n", ""
                if args[3:5] == ["cargo", "--version"]:
                    return f"cargo {candidate.RUST} (fixture)\n", ""
                if args[3:5] == ["rustc", "--print"]:
                    return str(self.sysroot) + "\n", ""
                if args[3:5] == ["cargo", "metadata"]:
                    manifest = args[args.index("--manifest-path") + 1]
                    return json.dumps({"packages": [{"id": "root", "manifest_path": manifest}],
                                       "resolve": {"root": "root", "nodes": [{"id": "root", "deps": []}]}}), ""
                if args[3:5] == ["cargo", "build"]:
                    put(Path(env["CARGO_TARGET_DIR"]), target + "/release/" + candidate.TARGETS[target][2], macho() if target == MAC else pe())
                    if target == MAC:
                        self.assertEqual(env["MACOSX_DEPLOYMENT_TARGET"], "14.2")
                    else:
                        self.assertEqual(env["RUSTFLAGS"], "-C target-feature=+crt-static")
                    self.assertEqual(env["RUSTC_WRAPPER"], "")
                    self.assertEqual(env["RUSTC"], str(self.sysroot / "bin" / ("rustc.exe" if target == WIN else "rustc")))
                    self.assertNotIn("CARGO_BUILD_RUSTC", env)
                    return "", ""
            if args[:2] == ["otool", "-L"]:
                return args[2] + ":\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n", ""
            if args[0] == "codesign":
                return "", "Identifier=dev.caelis.dtw\nflags=0x10000(runtime)\nAuthority=Developer ID Application: Test\n"
            if args[:3] in (["xcrun", "notarytool", "submit"], ["xcrun", "notarytool", "info"]):
                return json.dumps({"status": notary_status, "id": "original-submission"}), ""
            if args[-1] == "--version" and Path(args[0]).name in ("dtw", "dtw.exe"):
                return f"dtw {binary_version}\n", ""
            if args[0] == "where.exe":
                return "C:/MSVC/" + args[1] + "\n", ""
            if args == ["cl.exe", "/Bv"]:
                return "Microsoft C/C++ Compiler Version 19.0 (fixture)\n", ""
            if args[0] in ("xcrun", "xcodebuild"):
                return "Fixture SDK/compiler\n", ""
            self.fail(f"Unexpected command in pure test: {args}")
        return run, calls


class SourceAndOutputTests(Fixture):
    def test_real_git_archive_is_independent_of_autocrlf(self):
        def run(args, *, cwd):
            result = subprocess.run(args, cwd=cwd, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            return result.stdout.decode("utf-8"), result.stderr.decode("utf-8")
        hooks = self.root / "empty-hooks"
        hooks.mkdir()
        run(["git", "init", "--quiet"], cwd=self.repo)
        run(["git", "config", "core.autocrlf", "true"], cwd=self.repo)
        put(self.repo, ".gitattributes", "preserved-notice -text\n")
        put(self.repo, "preserved-notice", b"Original notice\r\nMust stay byte-identical\r\n")
        run(["git", "add", "."], cwd=self.repo)
        run(["git", "-c", "commit.gpgsign=false", "-c", f"core.hooksPath={hooks}",
             "-c", "user.name=Package test", "-c", "user.email=package@example.invalid",
             "commit", "--quiet", "-m", "local archive fixture"], cwd=self.repo)
        archive = self.root / "fixed-source.tar"
        candidate.export_source(self.repo, archive, "HEAD", run)
        with tarfile.open(archive) as exported:
            for name in ("cmd/dtw/Cargo.lock", "preserved-notice"):
                blob = subprocess.check_output(["git", "show", "HEAD:" + name], cwd=self.repo)
                self.assertEqual(exported.extractfile(name).read(), blob)

    def test_clean_crlf_checkout_hashes_committed_lf_blob(self):
        lock = self.repo / "cmd/dtw/Cargo.lock"
        canonical = lock.read_bytes()
        lock.write_bytes(canonical.replace(b"\n", b"\r\n"))
        fake, _ = self.fake_commands()
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            result = candidate.build(self.repo, self.root / "crlf-candidate", MAC)
        self.assertEqual(result["source"]["cargo_lock_sha256"], hashlib.sha256(canonical).hexdigest())
        self.assertIn(b"\r\n", lock.read_bytes())
        self.assertEqual((self.root / "crlf-candidate/source/cmd/dtw/Cargo.lock").read_bytes(), canonical)

    def test_dirty_source_stops_before_export_build_and_retains_failure(self):
        fake, calls = self.fake_commands(dirty=True)
        output = self.root / "dirty-candidate"
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            with self.assertRaisesRegex(ValueError, "must be clean"):
                candidate.build(self.repo, output, MAC)
        self.assertTrue((output / "FAILURE.json").exists())
        self.assertFalse(any("archive" in c or "build" in c for c in calls))

    def test_versions_must_match_and_legacy_release_is_refused(self):
        self.assertEqual(candidate.check_version(self.repo)[0], "0.2.0")
        put(self.repo, "version.txt", "0.2.1")
        with self.assertRaisesRegex(ValueError, "version.txt"):
            candidate.check_version(self.repo)
        put(self.repo, "version.txt", "0.2.0")
        put(self.repo, "cmd/dtw/Cargo.lock", '[[package]]\nname="desktop-world"\nversion="0.1.3"\n')
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            candidate.check_version(self.repo)
        put(self.repo, "cmd/dtw/Cargo.toml", '[package]\nname="desktop-world"\nversion="0.1.3"\n')
        with self.assertRaisesRegex(ValueError, "legacy"):
            candidate.check_version(self.repo)

    def test_existing_inside_source_and_traversal_output_are_refused_without_removal(self):
        existing = self.root / "candidate"
        put(existing, "owned.txt", "keep me")
        for output in (existing, self.repo / "new", self.root / "x/../new"):
            with self.subTest(output=output), self.assertRaises(ValueError):
                candidate.reserve_output(self.repo, output)
        self.assertEqual((existing / "owned.txt").read_text(), "keep me")
        reserved = candidate.reserve_output(self.repo, self.root / "fresh space 中文")
        self.assertTrue(reserved.is_dir())

    def test_wrong_native_platform_is_refused(self):
        with patch.object(candidate.platform, "system", return_value="Linux"), patch.object(candidate.platform, "machine", return_value="x86_64"):
            with self.assertRaisesRegex(ValueError, "native"):
                candidate.native_host(WIN)


class DependencyTests(Fixture):
    def test_target_filtered_reachable_graph_keeps_build_deps_but_not_dev_or_other_target(self):
        manifest = self.repo / "cmd/dtw/Cargo.toml"
        packages = [{"id": "root", "manifest_path": str(manifest)}] + [
            {"id": name, "manifest_path": str(self.root / name / "Cargo.toml")} for name in ("win", "build", "shared", "dev", "linux")]
        def edge(name, kind=None):
            return {"pkg": name, "dep_kinds": [{"kind": kind, "target": None}]}
        metadata = {"packages": packages, "resolve": {"root": "root", "nodes": [
            {"id": "root", "deps": [edge("win"), edge("build", "build"), edge("dev", "dev")]},
            {"id": "win", "deps": [edge("shared")]}, {"id": "build", "deps": []},
            {"id": "shared", "deps": []}, {"id": "dev", "deps": []}, {"id": "linux", "deps": []}]}}
        self.assertEqual({p["id"] for p in candidate.dependency_packages(metadata, manifest)}, {"win", "build", "shared"})

    def test_no_license_fails_and_missing_sysroot_notices_requires_rust_docs(self):
        dep, lock = self.dep(notices=())
        with self.assertRaisesRegex(ValueError, "No license"):
            candidate.collect_notices(self.repo, self.root / "package", [dep], [lock], self.sysroot)
        dep, lock = self.dep(name="licensed")
        (self.sysroot / "share/doc/rust/COPYRIGHT-library.html").unlink()
        with self.assertRaisesRegex(ValueError, "rust-docs"):
            candidate.collect_notices(self.repo, self.root / "package2", [dep], [lock], self.sysroot)

    def test_bundled_quickjs_notice_is_required_and_retained(self):
        dep, lock = self.dep(name="rquickjs-sys", notices=("LICENSE",))
        with self.assertRaisesRegex(ValueError, "quickjs/LICENSE"):
            candidate.collect_notices(self.repo, self.root / "bad", [dep], [lock], self.sysroot)
        root = Path(dep["manifest_path"]).parent
        put(root, "quickjs/LICENSE", "QuickJS authors and license\n")
        report = candidate.collect_notices(self.repo, self.root / "good", [dep], [lock], self.sysroot)
        self.assertEqual({r["file"] for r in report["crates"][0]["files"]}, {"LICENSE", "quickjs/LICENSE"})
        self.assertEqual((self.root / "good/notices/crates/rquickjs-sys-1.0.0/quickjs/LICENSE").read_text(), "QuickJS authors and license\n")

    def test_override_requires_reviewed_hash_and_declared_license(self):
        dep, lock = self.dep(name="rquickjs-core", version="0.14.0", notices=())
        content = b"Exact upstream license bytes\n"
        put(self.repo, "packaging/third-party/rquickjs-core-0.14.0/LICENSE", content)
        override = {"rquickjs-core-0.14.0": {"declared_license": "MIT", "revision": "fixed-upstream-revision", "notices": [
            {"file": "rquickjs-core-0.14.0/LICENSE", "sha256": hashlib.sha256(content).hexdigest(), "url": "https://example.invalid/LICENSE"}]}}
        put(self.repo, "packaging/third-party/overrides.json", json.dumps(override))
        report = candidate.collect_notices(self.repo, self.root / "good", [dep], [lock], self.sysroot)
        self.assertEqual(report["crates"][0]["override"]["revision"], "fixed-upstream-revision")
        put(self.repo, "packaging/third-party/rquickjs-core-0.14.0/LICENSE", "modified text")
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            candidate.collect_notices(self.repo, self.root / "bad", [dep], [lock], self.sysroot)

    def test_declared_license_file_with_nonstandard_name_is_not_dropped(self):
        dep, lock = self.dep(notices=("legal/Terms.txt",))
        dep["license_file"] = "legal/Terms.txt"
        result = candidate.collect_notices(self.repo, self.root / "package", [dep], [lock], self.sysroot)
        self.assertEqual(result["crates"][0]["files"][0]["file"], "legal/Terms.txt")


class BinaryAndArchiveTests(Fixture):
    def test_macho_architecture_and_actual_deployment_target(self):
        path = put(self.root, "dtw", macho())
        self.assertEqual(candidate.binary_format(path, MAC)["deployment_target"], "14.2.0")
        path.write_bytes(macho(minimum=14 << 16))
        with self.assertRaisesRegex(ValueError, "14.2"):
            candidate.binary_format(path, MAC)
        path.write_bytes(macho(cpu=0x01000007))
        with self.assertRaisesRegex(ValueError, "arm64"):
            candidate.binary_format(path, MAC)

    def test_pe_rejects_wrong_arch_and_dynamic_crt(self):
        path = put(self.root, "dtw.exe", pe())
        self.assertEqual(candidate.binary_format(path, WIN)["imports"], ["kernel32.dll"])
        for name in ("VCRUNTIME140.dll", "ucrtbase.dll", "api-ms-win-crt-runtime-l1-1-0.dll"):
            path.write_bytes(pe(name))
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, "Dynamic CRT"):
                candidate.binary_format(path, WIN)
        path.write_bytes(pe(machine=0xAA64))
        with self.assertRaisesRegex(ValueError, "x86_64"):
            candidate.binary_format(path, WIN)

    def test_archive_has_exact_checksums_modes_and_rejects_tampering(self):
        package = self.root / "package"
        put(package, "bin/dtw", b"executable")
        put(package, "skills/desktop-world/SKILL.md", "canonical skill")
        archive = self.root / "candidate.zip"
        candidate.archive_package(package, archive, 1700000000)
        self.assertEqual((self.root / "candidate.zip.sha256").read_text().split()[0], candidate.digest(archive))
        with zipfile.ZipFile(archive) as src:
            entries = [(info, src.read(info)) for info in src.infolist()]
            mode = src.getinfo("package/bin/dtw").external_attr >> 16
            self.assertTrue(stat.S_ISREG(mode) and mode & 0o111 == 0o111)
        bad = self.root / "tampered.zip"
        with zipfile.ZipFile(bad, "w") as out:
            for info, data in entries:
                out.writestr(info, b"tampered" if info.filename.endswith("bin/dtw") else data)
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            candidate.verify_archive(bad, "package")
        unlisted = self.root / "unlisted.zip"
        with zipfile.ZipFile(unlisted, "w") as out:
            for info, data in entries:
                out.writestr(info, data)
            extra = zipfile.ZipInfo("package/unlisted.txt")
            extra.external_attr = (stat.S_IFREG | 0o644) << 16
            out.writestr(extra, b"unlisted")
        with self.assertRaisesRegex(ValueError, "inventory differs"):
            candidate.verify_archive(unlisted, "package")

    def test_claude_optional_wrapper_reuses_one_target_binary_and_canonical_skill(self):
        for target, binary in ((MAC, "dtw"), (WIN, "dtw.exe")):
            package = self.root / target
            package.mkdir()
            candidate.package_assets(self.repo, package, "0.2.0", target)
            config = json.loads((package / ".mcp.json").read_text())
            self.assertEqual(config["mcpServers"]["desktop-world"], {"command": "${CLAUDE_PLUGIN_ROOT}/bin/" + binary, "args": ["mcp"]})
            self.assertEqual(json.loads((package / ".claude-plugin/plugin.json").read_text())["version"], "0.2.0")
            self.assertEqual((package / "skills/desktop-world/SKILL.md").read_bytes(), (self.repo / "skills/desktop-world/SKILL.md").read_bytes())
            self.assertIn("do not register both", (package / "README.md").read_text())


class WorkflowTests(Fixture):
    def test_windows_uses_static_crt_and_target_specific_graph_and_executable(self):
        fake, calls = self.fake_commands(target=WIN)
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            result = candidate.build(self.repo, self.root / "windows", WIN)
        output = self.root / "windows"
        manifest = json.loads(next(output.glob("desktop-world-*/BUILD.json")).read_text())
        self.assertEqual(manifest["build_environment"]["RUSTFLAGS"], "-C target-feature=+crt-static")
        self.assertEqual(manifest["binary"]["path"], "bin/dtw.exe")
        self.assertEqual(manifest["binary"]["static_crt_import_check"], "passed")
        self.assertEqual(manifest["runtime_evidence"]["minimum_os_runtime"], "unverified")
        metadata = next(c for c in calls if "metadata" in c)
        self.assertEqual(metadata[metadata.index("--filter-platform") + 1], WIN)
        candidate.verify_archive(output / result["archive"], Path(result["archive"]).stem)

    def test_native_candidate_is_immutable_unverified_archive_not_a_release(self):
        fake, calls = self.fake_commands()
        with patch.dict("os.environ", {"CARGO_BUILD_RUSTC": "/untrusted/compiler"}), patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            result = candidate.build(self.repo, self.root / "output", MAC)
        output = self.root / "output"
        manifest = json.loads(next(output.glob("desktop-world-*/BUILD.json")).read_text())
        self.assertEqual(result["kind"], "unpublished-rewrite-candidate")
        self.assertEqual(result["notarization"]["status"], "unverified")
        self.assertEqual(manifest["runtime_evidence"]["cli_version"], "dtw 0.2.0")
        self.assertEqual(manifest["runtime_evidence"]["gui"], "unverified")
        metadata = next(c for c in calls if "metadata" in c)
        self.assertEqual(metadata[metadata.index("--filter-platform") + 1], MAC)
        build = next(c for c in calls if "build" in c)
        self.assertIn("--release", build)
        self.assertIn("--locked", build)
        self.assertEqual(build[build.index("--target") + 1], MAC)
        self.assertFalse(any(c[0] in ("codesign", "claude", "agy") or "notarytool" in c for c in calls))
        candidate.verify_archive(output / result["archive"], Path(result["archive"]).stem)

    def test_binary_version_mismatch_preserves_failed_output_without_candidate_manifest(self):
        fake, _ = self.fake_commands(binary_version="0.1.3")
        output = self.root / "wrong-version"
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            with self.assertRaisesRegex(ValueError, "Built CLI version"):
                candidate.build(self.repo, output, MAC)
        self.assertTrue((output / "FAILURE.json").exists())
        self.assertTrue((output / "source.tar").exists())
        self.assertFalse((output / "CANDIDATE.json").exists())

    def test_notary_acceptance_is_external_and_original_archive_stays_fixed(self):
        fake, calls = self.fake_commands()
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            result = candidate.build(self.repo, self.root / "signed", MAC, "Developer ID: Test", "existing-profile")
        self.assertEqual(result["notarization"]["status"], "Accepted")
        self.assertFalse(result["notarization"]["stapled"])
        self.assertEqual(sum(c[:3] == ["xcrun", "notarytool", "submit"] for c in calls), 1)
        with zipfile.ZipFile(self.root / "signed" / result["archive"]) as archive:
            manifest = json.loads(archive.read(Path(result["archive"]).stem + "/BUILD.json"))
        self.assertEqual(manifest["notarization"]["status"], "unverified")
        self.assertEqual(candidate.digest(self.root / "signed" / result["archive"]), result["archive_sha256"])

    def test_notary_rejection_keeps_original_receipt_and_archive(self):
        fake, _ = self.fake_commands(notary_status="Invalid")
        output = self.root / "rejected"
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            with self.assertRaisesRegex(ValueError, "did not return Accepted"):
                candidate.build(self.repo, output, MAC, "Developer ID: Test", "existing-profile")
        self.assertEqual(json.loads((output / "notarization-submit.json").read_text())["status"], "Invalid")
        self.assertEqual(len(list(output.glob("*.zip"))), 1)
        self.assertTrue((output / "FAILURE.json").exists())
        self.assertFalse((output / "CANDIDATE.json").exists())


if __name__ == "__main__":
    unittest.main()
