# Third-party notices

The packaged Cua Driver SDK and private worker come from
[`trycua/cua`](https://github.com/trycua/cua) at commit
`ac967b9006aafa10870aadb5a645b038cba9721a` and are MIT licensed.
Their copyright notice and full license are distributed in
`CUA-MIT-LICENSE.txt`. The Cua Driver workspace's derived-code notices are
distributed unchanged in `CUA-THIRD_PARTY_NOTICES.md`; these include its
QuickJS-ng/rquickjs and platform implementation credits.

Binary packages include `LICENSING/DEPENDENCIES.json` with the exact locked
normal/build dependency inventory for the adapter and worker on Windows x64
and macOS arm64. `LICENSING/THIRD-PARTY.md` indexes the original license and
copyright texts in `LICENSING/licenses/`. Their byte hashes are recorded in
the inventory and included in the package's complete `SHA256SUMS`.

These dependencies retain their own licenses, including MIT, Apache-2.0,
BSD, ISC, Zlib, Unicode-3.0, BSL-1.0, CDLA-Permissive-2.0 and MPL-2.0.
UniFFI 0.31.0's six MPL components are unmodified; their corresponding source
and exact-revision workspace license accompany the binaries in
`LICENSING/MPL-SOURCES.tar.gz`. DTW's MIT license does not replace these terms.
The embedded Inter font retains its OFL-1.1 copyright and full license in
`LICENSING/licenses/cursor-overlay-0.34.0/assets/Inter-OFL.txt`. CUA's embedded
default cursor theme remains MIT licensed. Rust's linked standard-library
copyright catalog and license texts are also included in `LICENSING/`.

Some published crates omit their workspace license files. Reviewed copies
from the exact crate VCS revision are kept in `packaging/third-party/`; their
source URLs, revisions and SHA-256 values are verified during packaging.
The two Cargo lockfiles and `scripts/license-audit.py` define the reproducible
audit scope. Dev-only and unused optional workspace members are excluded.
Windows VC++ and Apple system/Swift runtime libraries are supplied by the
target OS/runtime installation and are not redistributed in the package.

The optional Cua perception extension and Cua Spaces are not linked or
distributed by Desktop World. Their separate licenses do not become part of
this MIT package.
