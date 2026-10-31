# Third-party notices

DTW's native macOS, Windows and experimental Linux providers and bounded app script engine include narrow adapted
source ports from MIT-licensed [Cua Driver 0.38.0](https://github.com/trycua/cua/tree/85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19),
commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. The full CUA copyright and MIT
license, donor file hashes, adaptations and excluded mechanisms are retained in
[native source provenance](docs/native-source-provenance.md) and
[Windows/Linux source provenance](docs/native-source-provenance-cross-platform.md).
The latter also retains the donor’s Victor Vannara and Francesco Bonacci MIT
notices for adapted Windows source groups. Removing the SDK
and upstream worker does not remove this source attribution.

The macOS background input group additionally ports a selected Rust input/AX
closure from CUA commit `b38360443960a6540685a9a5eb5407f10fe3d03b`. Its exact
donor hashes and deliberate adaptations are recorded in
[background input provenance](docs/background-input-port.md); the original
MIT license is included in `packaging/third-party/cua-background-input/LICENSE`.
This adds native source and small locked Rust dependencies, not the CUA SDK.

The Windows session cursor also adapts CUA's Win32 overlay mechanisms and shared
palette/DART outline from that commit. Donor hashes and the Windows ownership,
capture and rendering changes are recorded in the cross-platform provenance
record. The same retained CUA MIT license covers these source adaptations.

The script engine directly uses rquickjs 0.14.0 and its bundled QuickJS-ng
0.16.2. QuickJS-ng credits Fabrice Bellard (2017–2026), Charlie Gordon
(2017–2024), Ben Noordhuis (2023–2026) and Saúl Ibarra Corretgé (2023–2026);
rquickjs credits Mees Delzenne (2020) and the rquickjs contributors (2025).
Their original MIT license/copyright files remain in the dependency sources.
The exact-revision rquickjs-core license omitted by that crate's published
archive is also retained in `packaging/third-party/rquickjs-core-0.14.0/LICENSE`,
with its source URL and SHA-256 in `packaging/third-party/overrides.json`.

The standard MCP entry uses the official MIT-licensed rmcp 3.8.0 crate with
only server and stdio transport features, plus Tokio for its local async
runtime. It does not vendor or modify SDK source. Their license and copyright
texts remain in the locked dependency sources.

Other locked Rust dependencies retain their own licenses. `cmd/dtw/Cargo.lock`
is the rewrite's sole dependency lockfile; its resolved graph contains no CUA
SDK or upstream worker. Apple system/Swift libraries are provided by macOS.
The CUA perception extension, Spaces, browser bridge and embedded CUA assets
are not part of this source build.

The rewrite candidate builder inventories the actual target-filtered, reachable
Cargo dependency graph and copies its license/copyright files, including bundled
QuickJS and Rust runtime notices, into `notices/`. Its `DEPENDENCIES.json` records
versions, source checksums and notice hashes. Missing notices stop construction;
reviewed exact-source overrides are checked against their saved hashes. Source-only
staging is not this complete binary notice inventory. The deleted legacy audit and
package inventories do not establish license coverage for a new binary package.
Historical releases retain their accompanying notices and license terms.
