# Exact-revision notice copies

These published crates omit their workspace license files. `overrides.json`
records each version's published `.cargo_vcs_info.json` revision, declared
license, original GitHub raw URL and notice SHA-256. The files are unmodified
copies from those exact revisions, not from a moving default branch.

`scripts/license-audit.py` checks the crate revision, declaration and bytes
before including them in the generated package inventory. UniFFI's original
MPL license is also supplied beside its unmodified corresponding source.
Other dependency notices come directly from the Cargo source distributions.

To change a dependency version, review its source notice again and update
the matching entry; unknown/missing license data fails packaging.
