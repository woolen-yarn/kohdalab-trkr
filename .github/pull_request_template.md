## Summary

## Verification

- [ ] `cargo fmt --manifest-path Cargo.toml --all --check`
- [ ] `cargo clippy --manifest-path Cargo.toml --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --manifest-path Cargo.toml --workspace --locked`
- [ ] `cargo build --manifest-path Cargo.toml --bin kohdalab-trkr --locked`
- [ ] `python -m pytest tests`
- [ ] Release packaging checked when changing distribution files

## Hardware impact

Describe any changes to instrument commands, coordinates, timing, limits or CSV data.
Use simulation by default; state explicitly if physical hardware was tested.

## Notes
