# Third-party notices

The packaged Cua Driver SDK and private worker come from
[`trycua/cua`](https://github.com/trycua/cua) at commit
`ac967b9006aafa10870aadb5a645b038cba9721a` and are MIT licensed.
Their copyright notice and full license are distributed in
`CUA-MIT-LICENSE.txt`. The Cua Driver workspace's derived-code notices are
distributed unchanged in `CUA-THIRD_PARTY_NOTICES.md`; these include its
QuickJS-ng/rquickjs and platform implementation credits.

The Rust dependencies and their exact resolved versions are recorded in
`cmd/dtw/Cargo.lock` and upstream Cua's pinned lockfile. A public binary
release must review the complete resolved dependency notices for its target
platform and include any additional required texts in its artifact.

The optional Cua perception extension and Cua Spaces are not linked or
distributed by Desktop World. Their separate licenses do not become part of
this MIT package.
