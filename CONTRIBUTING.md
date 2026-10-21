# Contributing

Build and test instructions are in README.md. Submit changes through a pull request
with focused Rust tests and packaging/client checks. Do not connect instruments,
home axes or move stages in CI. Keep scientific coordinate conversion, timing,
CSV schema and golden references stable. Never commit experimental CSVs or private
profiles. Changes to the GPL helper must preserve upstream notices and source.

## 日本語

ビルドとテストの手順は[README](README.md)を参照してください。変更は、対象を絞ったRustテストと配布・クライアント検証を添え、PRで提出してください。CIでは実機接続、原点復帰、ステージ移動を行わないでください。科学的な座標変換、タイミング、CSV形式、固定参照データを維持してください。実験CSVや私有設定はコミットしないでください。GPL helperを変更する場合は、上流の権利表記とソースを保持してください。
