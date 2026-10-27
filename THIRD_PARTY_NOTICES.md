# Third-party notices

DTW's native macOS provider and bounded app script engine include narrow adapted
source ports from MIT-licensed [Cua Driver 0.38.0](https://github.com/trycua/cua/tree/85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19),
commit `85e2b1318bd77ccf3cfa88ccdf875a4bb7eaed19`. The full CUA copyright and MIT
license, donor file hashes, adaptations and excluded mechanisms are retained in
[native source provenance](docs/native-source-provenance.md). Removing the SDK
and upstream worker does not remove this source attribution.

The script engine directly uses rquickjs 0.14.0 and its bundled QuickJS-ng
0.16.2. QuickJS-ng credits Fabrice Bellard (2017–2026), Charlie Gordon
(2017–2024), Ben Noordhuis (2023–2026) and Saúl Ibarra Corretgé (2023–2026);
rquickjs credits Mees Delzenne (2020) and the rquickjs contributors (2025).
Their original MIT license/copyright files remain in the dependency sources.
The exact-revision rquickjs-core license omitted by that crate's published
archive is also retained in `packaging/third-party/rquickjs-core-0.14.0/LICENSE`,
with its source URL and SHA-256 in `packaging/third-party/overrides.json`.

Other locked Rust dependencies retain their own licenses. `cmd/dtw/Cargo.lock`
is the rewrite's sole dependency lockfile; its resolved graph contains no CUA
SDK or upstream worker. Apple system/Swift libraries are provided by macOS.
The CUA perception extension, Spaces, browser bridge and embedded CUA assets
are not part of this source build.

There is no rewritten release package yet. Before publishing one, inventory its
actual target-specific dependencies and redistribute their required license and
copyright texts, including Rust runtime notices. The deleted legacy audit and
package inventories do not establish license coverage for a new binary package.
Historical releases retain their accompanying notices and license terms.
