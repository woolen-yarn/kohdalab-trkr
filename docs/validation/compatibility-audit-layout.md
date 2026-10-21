# Compatibility audit source layout

[The compatibility inventory](python-compatibility-inventory.json) is a frozen
audit from 2026-10-21, preserved byte-for-byte, including its file hashes.
The Python source tag and commit are recorded inside the inventory. Its Rust
paths describe the layout used by [v0.3.4](https://github.com/woolen-yarn/kohdalab-trkr/tree/v0.3.4).
They are historical audit references, not current checkout paths.

For the current workspace, map `next/crates/` to `crates/` and
`next/Cargo.toml` to `Cargo.toml`. Paths under `bindings/`, `tests/`, and
`notebook/` retain their roles. Historical hashes identify the audited bytes;
they are not regenerated to claim a new equivalence audit after a source move.
See [repository layout](../development/repository-layout.md) for current paths.
