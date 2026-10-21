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

## 日本語

[互換性索引](python-compatibility-inventory.json)は2026-10-21時点の監査を、ハッシュを含めバイト単位で保持したものです。Pythonの参照タグとコミットは索引内に記録しています。Rustのパスは[v0.3.4](https://github.com/woolen-yarn/kohdalab-trkr/tree/v0.3.4)当時の構成を示す履歴参照であり、現在の作業ツリーのパスではありません。

現在の構成では`next/crates/`を`crates/`、`next/Cargo.toml`を`Cargo.toml`へ読み替えてください。`bindings/`、`tests/`、`notebook/`の役割は変わりません。履歴ハッシュは監査対象のバイト列を識別するもので、移動後の同一性を新たに監査したように見せるために再生成しません。現行パスは[構成ガイド](../development/repository-layout.md)を参照してください。
