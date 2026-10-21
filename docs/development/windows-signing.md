# Windows code-signing preparation

Windows release artifacts are currently unsigned. The release workflow rebuilds
them on GitHub-hosted Windows runners, uploads checksummed unsigned artifacts,
and stages a narrow SignPath input archive. It does not contain a SignPath token
or signing submission step.

The only proposed first-party signing inputs are `kohdalab-trkr.exe`,
`kohdalab-trkr-demo.exe`, and `kohdalab-visa-helper.exe`. The workflow fails if
the GPL GPIB helper enters that input. The installer is a separate provider
review sample because NSIS generates an uninstaller during compilation.

After SignPath accepts the project, a protected release workflow should submit
the workflow artifact ID to SignPath, wait for manual approval, download the
signed result, verify the publisher and timestamp with
`Get-AuthenticodeSignature`, and package checksums from the signed bytes. The
installer and generated uninstaller need an approved signing integration before
they can be advertised as signed.

Do not disable SmartScreen, Defender, or Windows security controls to work
around an unsigned-download warning. Obtain releases from GitHub and verify the
published SHA-256 sums.

See [the policy](../../CODE_SIGNING_POLICY.md),
[application configuration](../../.signpath/application-pe.xml), and the official
[SignPath GitHub documentation](https://docs.signpath.io/trusted-build-systems/github).

## 日本語

現在のWindows成果物は未署名です。リリースワークフローはGitHub提供のWindows runnerで再ビルドし、チェックサム付き未署名成果物と、対象を限定したSignPath入力をアップロードします。SignPathトークンや署名申請処理は含みません。

申請する第一者ファイルは`kohdalab-trkr.exe`、`kohdalab-trkr-demo.exe`、`kohdalab-visa-helper.exe`だけです。GPLのGPIB helperが入力に混入するとワークフローは失敗します。NSISはコンパイル時にアンインストーラーを生成するため、Setupは提供者の審査用サンプルとして分離します。

SignPathがプロジェクトを承認した後、保護されたリリースワークフローから成果物IDを送信し、手動承認を待ち、署名済み成果物を取得する構成にします。`Get-AuthenticodeSignature`で公開者とタイムスタンプを検証し、署名後のバイト列からチェックサムを作成します。インストーラーと生成アンインストーラーにも承認済み連携が必要で、それまでは署名済みと表示しません。

未署名ダウンロードの警告を避ける目的でSmartScreen、Defender、Windowsの安全機能を無効にしないでください。GitHubから取得し、公開SHA-256を確認してください。

[ポリシー](../../CODE_SIGNING_POLICY.md)、[対象設定](../../.signpath/application-pe.xml)、[SignPath公式GitHub文書](https://docs.signpath.io/trusted-build-systems/github)を参照してください。
