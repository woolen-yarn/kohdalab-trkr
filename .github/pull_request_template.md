## Summary / 概要

## Verification / 確認

- [ ] `cargo fmt --manifest-path Cargo.toml --all --check`
- [ ] `cargo clippy --manifest-path Cargo.toml --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --manifest-path Cargo.toml --workspace --locked`
- [ ] `cargo build --manifest-path Cargo.toml --bin kohdalab-trkr --locked`
- [ ] `python -m pytest tests`
- [ ] Release packaging checked when changing distribution files / 配布関連を変更した場合はパッケージ生成を確認

## Hardware impact / 機器への影響

Describe any changes to instrument commands, coordinates, timing, limits or CSV data. / 機器命令・座標・タイミング・制限・CSVの変更を記載してください。
Use simulation by default; state explicitly if physical hardware was tested. / 原則シミュレーションを使い、実機検証した場合は明記してください。

## Notes / 補足
