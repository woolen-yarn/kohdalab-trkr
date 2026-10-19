# Native Rust workspace

The maintained TRKR application lives in this workspace. Run from the repository root:

```sh
cargo run --manifest-path next/Cargo.toml --locked --bin kohdalab-trkr
cargo test --manifest-path next/Cargo.toml --locked
```

Crates separate the experiment plan, device transport, motion safety and desktop GUI.
See the root README for source-built releases and Notebook access.
