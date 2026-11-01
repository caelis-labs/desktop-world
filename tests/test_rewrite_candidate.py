"""Candidate gates: local temporary Git regression, no Cargo/signing/network/GUI."""

import hashlib
import importlib.util
import io
import contextlib
import json
import os
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


class BrowserWorkbenchEncodingTests(unittest.TestCase):
    def test_utf16_input_events_do_not_poison_later_saves_or_attachments(self):
        spec = importlib.util.spec_from_file_location(
            "utf16_workbench", Path(__file__).parent / "fixtures/browser-workbench/server.py")
        server = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(server)
        responses = []
        fragments = ['已核对 \ud83d', '已核对 \udc26', '已核对 🐦']
        note = '第一批：已核对 🐦\n待补货'
        requests = [('/api/event', {'type':'input', 'control':'note', 'value':value})
                    for value in fragments]
        requests += [
            ('/api/save', {'sku':'DTW-0088', 'quantity':37, 'reviewed':True, 'note':note}),
            ('/api/attachment', {'name':'复核记录.txt', 'text':note}),
            # A JSON string may also retain a code unit in a saved field. Its
            # successful response must remain encodable after the disk commit.
            ('/api/save', {'sku':'DTW-0240', 'quantity':52, 'reviewed':True, 'note':'\ud83d'}),
        ]

        class HttpServer:
            def __init__(self, address, handler):
                self.handler = handler
                self.server_port = 1234
            def serve_forever(self):
                handler = self.handler.__new__(self.handler)
                handler.reply = lambda status, body, kind=None: responses.append((status, body))
                for path, value in requests:
                    data = json.dumps(value, ensure_ascii=True).encode('utf-8')
                    handler.path = path
                    handler.headers = {'Content-Length':str(len(data))}
                    handler.rfile = io.BytesIO(data)
                    handler.do_POST()
            def server_close(self):
                pass

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            with patch.object(server, 'ThreadingHTTPServer', HttpServer), \
                 patch.object(server.threading, 'Thread'), \
                 patch('sys.argv', ['server.py', '--output', str(output), '--case', 'utf16', '--file-task']), \
                 contextlib.redirect_stdout(io.StringIO()):
                server.main()
            self.assertEqual([status for status, _ in responses], [200] * len(requests))
            state = json.loads((output/'state.json').read_text(encoding='utf-8'))
            self.assertEqual([event['value'] for event in state['events']], fragments)
            self.assertEqual([saved['sku'] for saved in state['saves']], ['DTW-0088', 'DTW-0240'])
            self.assertEqual(state['items'][87]['note'], note)
            self.assertEqual(state['items'][239]['note'], '\ud83d')
            self.assertEqual(json.loads(responses[-1][1])['note'], '\ud83d')
            self.assertEqual(state['uploads'][0]['sha256'], hashlib.sha256(note.encode('utf-8')).hexdigest())
            self.assertFalse((output/'state.tmp').exists())

    def test_real_html_and_saved_unicode_are_utf8_under_cp936_defaults(self):
        spec = importlib.util.spec_from_file_location(
            "packaged_workbench", Path(__file__).parent / "fixtures/browser-workbench/server.py")
        server = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(server)
        read_text, write_text = Path.read_text, Path.write_text
        responses = []
        note = '已核对 🐦\n下一步：安排补货'

        # Simulate non-UTF-8 Windows defaults without opening a socket or
        # changing the host locale. Explicit encoding arguments remain intact.
        def read_cp936(path, encoding=None, errors=None):
            return read_text(path, encoding=encoding or 'cp936', errors=errors)
        def write_cp936(path, data, encoding=None, errors=None, newline=None):
            return write_text(path, data, encoding=encoding or 'cp936', errors=errors, newline=newline)

        class HttpServer:
            def __init__(self, address, handler):
                self.handler = handler
                self.server_port = 1234
            def serve_forever(self):
                handler = self.handler.__new__(self.handler)
                handler.reply = lambda status, body, kind=None: responses.append((status, body))
                handler.path = '/'
                handler.do_GET()
                for path, value in (
                    ('/api/save', {'sku':'DTW-0088', 'quantity':37, 'reviewed':True, 'note':note}),
                    ('/api/attachment', {'name':'复核记录.txt', 'text':note}),
                ):
                    data = json.dumps(value, ensure_ascii=False).encode('utf-8')
                    handler.path = path
                    handler.headers = {'Content-Length':str(len(data))}
                    handler.rfile = io.BytesIO(data)
                    handler.do_POST()
            def server_close(self):
                pass

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)/'中文 evidence'
            with patch.object(server, 'ThreadingHTTPServer', HttpServer), \
                 patch.object(server.threading, 'Thread'), \
                 patch('sys.argv', ['server.py', '--output', str(output), '--case', 'case-unicode', '--file-task']), \
                 patch.object(Path, 'read_text', read_cp936), patch.object(Path, 'write_text', write_cp936), \
                 contextlib.redirect_stdout(io.StringIO()):
                server.main()
            self.assertEqual([status for status, _ in responses], [200, 200, 200])
            html = responses[0][1].decode('utf-8')
            self.assertIn('DTW Workbench case-unicode', html)
            self.assertNotIn('__CASE__', html)
            state = json.loads((output/'state.json').read_bytes().decode('utf-8'))
            self.assertEqual(state['items'][87]['note'], note)
            self.assertEqual(state['saves'][0]['note'], note)
            self.assertEqual(state['uploads'][0]['name'], '复核记录.txt')
            self.assertEqual(state['uploads'][0]['sha256'], hashlib.sha256(note.encode('utf-8')).hexdigest())


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
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", "install.md", "docs/install.md",
                     "docs/install-dsh.md", "docs/install-caelis.md", "docs/install-agy.md", "docs/native-source-provenance.md",
                     "docs/native-source-provenance-cross-platform.md", "docs/background-input-port.md",
                     "docs/windows-cursor-port.md",
                     "skills/desktop-world/SKILL.md"):
            put(self.repo, name, "Original reviewed source content\n")
        put(self.repo, "docs/examples/mcp.json", '{"mcpServers":{"desktop-world":{"command":"/absolute/path/to/dtw","args":["mcp"]}}}\n')
        put(self.repo, "docs/examples/codex.toml", '[mcp_servers.desktop-world]\ncommand="/absolute/path/to/dtw"\nargs=["mcp"]\n')
        put(self.repo, "plugins/claude/.claude-plugin/plugin.json", '{"name":"desktop-world","version":"0.1.3"}\n')
        put(self.repo, "plugins/cordis/index.mjs",
            (Path(__file__).parents[1] / "plugins/cordis/index.mjs").read_bytes())
        put(self.repo, "packaging/third-party/overrides.json", "{}\n")
        put(self.repo, "tests/acceptance/windows/README.md",
            (Path(__file__).parent / "acceptance/windows/README.md").read_bytes())
        put(self.repo, "tests/acceptance/windows/smoke.ps1", "# Owned-window standard MCP smoke\n")
        for name in ("server.py", "index.html"):
            put(self.repo, "tests/fixtures/browser-workbench/" + name, "Same-source browser task 中文 🐦\n")
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
            if args[:3] in (["xcrun", "notarytool", "submit"], ["xcrun", "notarytool", "wait"], ["xcrun", "notarytool", "info"]):
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

    def test_multi_host_plugins_reuse_one_target_binary_and_canonical_skill(self):
        for target, binary in ((MAC, "dtw"), (WIN, "dtw.exe")):
            package = self.root / target
            package.mkdir()
            candidate.package_assets(self.repo, package, "0.2.0", target)
            for guide in ("install.md", "docs/install.md", "docs/install-dsh.md", "docs/install-caelis.md", "docs/install-agy.md"):
                self.assertEqual((package / guide).read_bytes(), (self.repo / guide).read_bytes())
            config = json.loads((package / ".mcp.json").read_text())
            self.assertEqual(config["mcpServers"]["desktop-world"], {"command": "${CLAUDE_PLUGIN_ROOT}/bin/" + binary, "args": ["mcp"]})
            self.assertEqual(json.loads((package / ".claude-plugin/plugin.json").read_text())["version"], "0.2.0")
            self.assertEqual((package / "skills/desktop-world/SKILL.md").read_bytes(), (self.repo / "skills/desktop-world/SKILL.md").read_bytes())
            self.assertIn("do not register both", (package / "README.md").read_text())
            manifest = json.loads((package / "plugin.json").read_text())
            self.assertEqual(manifest["name"], "desktop-world")
            self.assertEqual(manifest["version"], "0.2.0")
            self.assertEqual(manifest["$schema"], "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json")
            standard = json.loads((package / "mcp.json").read_text())
            self.assertEqual(standard["mcpServers"], {"desktop-world": {
                "type":"stdio", "command":"./bin/" + binary, "args":["mcp"]}})
            self.assertEqual(standard["$schema"], "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json")
            agy = json.loads((package / ".antigravity-plugin/plugin.json").read_text())
            self.assertEqual(agy["version"], manifest["version"])
            self.assertEqual(agy["$schema"], "https://antigravity.google/schemas/v1/plugin.json")
            self.assertEqual(json.loads((package / "mcp_config.json").read_text())["mcpServers"], {
                "desktop-world": {"command":"./bin/" + binary, "args":["mcp"], "cwd":"${PLUGIN_ROOT}"}})
            cordis = json.loads((package / "package.json").read_text())
            self.assertEqual(cordis["version"], manifest["version"])
            self.assertNotIn("dependencies", cordis)
            self.assertEqual((package / "cordis/index.mjs").read_bytes(), (self.repo / "plugins/cordis/index.mjs").read_bytes())
            if target == MAC:
                self.assertFalse((package / "docs/windows-cursor-port.md").exists())


class WorkflowTests(Fixture):
    def test_windows_sdk_metadata_handles_uppercase_environment_copy_and_plain_dict(self):
        values = {"WindowsSDKVersion":"10.0.26100.0\\", "VCToolsVersion":"14.44.35207"}
        for upper in (True, False):
            env = {key.upper() if upper else key:value for key, value in values.items()}
            fake, _ = self.fake_commands(target=WIN)
            output = self.root / ("sdk-upper" if upper else "sdk-mixed")
            with patch.dict(os.environ, env, clear=True), patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
                candidate.build(self.repo, output, WIN)
            manifest = json.loads(next(output.glob("desktop-world-*/BUILD.json")).read_text())
            self.assertEqual(manifest["toolchain"]["windows_sdk"], values)

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
        package_name = Path(result["archive"]).stem
        with zipfile.ZipFile(output / result["archive"]) as archive:
            self.assertEqual(archive.read(package_name + "/acceptance/windows/README.md"),
                             (self.repo / "tests/acceptance/windows/README.md").read_bytes())
            for name in ("server.py", "index.html"):
                self.assertEqual(archive.read(package_name + "/acceptance/browser-workbench/" + name),
                                 (self.repo / "tests/fixtures/browser-workbench" / name).read_bytes())

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

    def test_existing_external_cargo_cache_supplies_the_packaged_binary(self):
        cache = self.root / "cargo cache"
        sentinel = put(cache, "existing-dependency", b"keep")
        output = self.root / "cached-candidate"
        fake, calls = self.fake_commands()
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            result = candidate.build(self.repo, output, MAC, cargo_target_dir=cache)
        built = cache / MAC / "release/dtw"
        package = output / Path(result["archive"]).stem
        manifest = json.loads((package / "BUILD.json").read_text())
        self.assertEqual(manifest["build_environment"]["CARGO_TARGET_DIR"], str(cache.resolve()))
        self.assertEqual((package / "bin/dtw").read_bytes(), built.read_bytes())
        self.assertEqual(result["binary_sha256"], candidate.digest(built))
        self.assertFalse((output / "target").exists())
        self.assertEqual(sentinel.read_bytes(), b"keep")
        self.assertEqual(sum("build" in command for command in calls), 1)

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
        submit = next(c for c in calls if c[:3] == ["xcrun", "notarytool", "submit"])
        self.assertNotIn("--wait", submit)
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

    def test_explicit_notary_keychain_is_used_for_original_submit_and_info(self):
        fake, calls = self.fake_commands()
        keychain = put(self.root, "temporary/signing.keychain-db", "keychain fixture")
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
            candidate.build(self.repo, self.root / "keychain", MAC, "Developer ID: Test", "ci-profile", keychain)
        notary_calls = [call for call in calls if call[:2] == ["xcrun", "notarytool"]]
        self.assertEqual([call[2] for call in notary_calls], ["submit", "wait", "info"])
        for call in notary_calls:
            self.assertEqual(call[call.index("--keychain") + 1], str(keychain.resolve()))
            self.assertEqual(call[call.index("--keychain-profile") + 1], "ci-profile")
            self.assertNotIn("--password", call)

    def test_notary_wait_timeout_preserves_original_id_and_never_reuploads(self):
        fake, calls = self.fake_commands()
        output = self.root / "wait-timeout"
        def timeout(owner, args, **kwargs):
            value = fake(owner, args, **kwargs)
            if list(args[:3]) == ["xcrun", "notarytool", "wait"]:
                # The submission receipt must already exist while wait runs.
                self.assertEqual(json.loads((output/"notarization-submit.json").read_text())["id"], "original-submission")
                self.assertEqual(args[args.index("--timeout")+1], "20m")
                owner.rows[-1]["returncode"] = 75
                return json.dumps({"id":"original-submission", "status":"In Progress"}), "Timeout"
            return value
        with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", timeout):
            with self.assertRaisesRegex(ValueError, "retain/query the original"):
                candidate.build(self.repo, output, MAC, "Developer ID: Test", "profile")
        self.assertEqual(json.loads((output/"notarization-submit.json").read_text())["id"], "original-submission")
        self.assertEqual(json.loads((output/"notarization-wait.json").read_text())["status"], "In Progress")
        self.assertEqual([call[2] for call in calls if call[:2] == ["xcrun", "notarytool"]], ["submit", "wait"])
        self.assertTrue((output/"FAILURE.json").exists())
        self.assertEqual(len(list(output.glob("*.zip"))), 1)
        self.assertFalse((output/"CANDIDATE.json").exists())

    def test_keychain_without_profile_or_nonexistent_keychain_fails_before_build(self):
        keychain = put(self.root, "temporary/signing.keychain-db", "keychain fixture")
        for index, profile, path in ((1, None, keychain), (2, "profile", self.root/"missing.keychain-db")):
            fake, calls = self.fake_commands()
            with patch.object(candidate, "native_host"), patch.object(candidate.Commands, "run", fake):
                with self.assertRaises((ValueError, FileNotFoundError)):
                    candidate.build(self.repo, self.root / f"bad-keychain-{index}", MAC, "Developer ID: Test", profile, path)
            self.assertFalse(calls)


class SigningSetupTests(unittest.TestCase):
    def test_identity_must_be_unique_developer_id_for_exact_team(self):
        team = "ABC123DE45"
        identity = '  1) '+('a'*40)+' "Developer ID Application: Caelis ('+team+')"\n     1 valid identities found\n'
        self.assertEqual(candidate.macos_signing_identity(identity, team), 'A'*40)
        for output in ('0 valid identities found', identity+identity,
                       identity.replace(team, 'ZZZ123DE45'),
                       identity.replace('Developer ID Application:', 'Apple Development:'),
                       identity.replace('a'*40, 'a'*39)):
            with self.subTest(output=output), self.assertRaises(ValueError):
                candidate.macos_signing_identity(output, team)

    def test_missing_ci_secret_fails_before_build_without_logging_credentials(self):
        script = Path(__file__).parents[1]/"scripts/sign-rewrite-candidate-macos.sh"
        names = ('APPLE_DEVELOPER_ID_P12_BASE64', 'APPLE_DEVELOPER_ID_P12_PASSWORD',
                 'APPLE_NOTARY_APPLE_ID', 'APPLE_NOTARY_PASSWORD')
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)/'candidate'
            env = os.environ.copy()
            env.update({name:'private-test-sentinel-'+str(index) for index, name in enumerate(names)})
            env['APPLE_NOTARY_PASSWORD'] = ''
            env['APPLE_TEAM_ID'] = 'ABC123DE45'
            result = subprocess.run(['bash', str(script), str(output)], env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('Missing candidate signing configuration: APPLE_NOTARY_PASSWORD', result.stderr)
            self.assertNotIn('private-test-sentinel', result.stdout+result.stderr)
            self.assertFalse(output.exists())
            report = json.loads(Path(str(output)+'.signing.json').read_text())
            self.assertEqual(report['builder_invocations'], 0)
            self.assertEqual(report['keychain_cleanup'], 'not_created')
            self.assertFalse(report['raw_credentials_exported_to_builder'])


if __name__ == "__main__":
    unittest.main()
