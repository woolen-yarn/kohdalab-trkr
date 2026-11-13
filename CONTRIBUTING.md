# Contributing

Build and test instructions are in README.md. Submit changes through a pull request
with focused Rust tests and packaging/client checks. Do not connect instruments,
home axes or move stages in CI. Keep scientific coordinate conversion, timing,
CSV schema and golden references stable. Never commit experimental CSVs or private
profiles. Changes to the GPL helper must preserve upstream notices and source.

Open a release PR with the version and release notes. After Test, Documentation
and Build release artifacts pass, merge it and promote its verified artifacts to
a **draft** by following the [release guide](docs/development/release-promotion.md).
Pushing a tag does not build packages. Before publishing the draft, check both
platform checksum manifests, packaged versions and corresponding source against
the release commit. Check the Windows installer lifecycle results and the extracted
Mac app's signature and launch. Publish the verified draft and leave published
release files intact.

## 日本語

ビルドとテストの手順は[README](README.md)を参照してください。変更は、対象を絞ったRustテストと配布・クライアント検証を添え、PRで提出してください。CIでは実機接続、原点復帰、ステージ移動を行わないでください。科学的な座標変換、タイミング、CSV形式、固定参照データを維持してください。実験CSVや私有設定はコミットしないでください。GPL helperを変更する場合は、上流の権利表記とソースを保持してください。

版番号とリリースノートを更新するPRを作成し、Test、Documentation、Build release artifactsが成功したらマージします。[リリース手順書](docs/development/release-promotion.md)に沿って、そのPRの検証済み配布物をGitHubリリースの**下書き**に昇格します。タグをpushしても配布物はビルドされません。公開前に、両OSのチェックサム、同梱版番号、対応ソースとリリース対象コミットの一致を検証してください。Windowsインストーラーのライフサイクル検証結果と、展開したMacアプリの署名・起動も確認します。検証済みの下書きを公開し、公開済みリリースのファイルは差し替えないでください。
