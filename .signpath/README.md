# SignPath application preparation

Status: preparation only. This repository has no SignPath organization ID,
project, policy, GitHub App installation, API token, certificate, or accepted
Foundation sponsorship configured. The CI workflow never submits a signing
request and never exposes a signing secret.

The public application and review repository is
[woolen-yarn/kohdalab-trkr](https://github.com/woolen-yarn/kohdalab-trkr).
`release.yml` rebuilds native helpers and Rust binaries from this source on
GitHub-hosted Windows and macOS runners. It uploads unsigned release artifacts
and a separate Windows `signpath-input-*` workflow artifact. That artifact has
only the three first-party PE files in `application-pe.xml`, their SHA-256
record, and GitHub-provided artifact origin metadata.

The GPL GPIB helper is intentionally excluded. The NSIS Setup executable is
uploaded separately as a review sample because the installer/uninstaller
boundary must be approved before an installer policy is enabled.

## Steps after SignPath acceptance

1. Confirm the public repository, maintainers, signing approvers, and MFA in
   SignPath. Install the SignPath GitHub App and link the predefined GitHub.com
   trusted build system to the project.
2. Create a release signing policy that requires manual approval and
   GitHub-hosted runners. Review [`application-pe.xml`](application-pe.xml) and
   the separate installer boundary before enabling either policy.
3. Add the SignPath organization ID, project slug, signing-policy slug, and
   restricted token to a protected GitHub environment. Use the official
   `signpath/github-action-submit-signing-request@v3` only on the artifact ID
   produced by the same trusted workflow; do not upload a locally built archive.
4. Sign the exact allowlisted files, verify the signer and timestamp, then build
   the final portable archive and Setup executable from those signed bytes.
   The generated NSIS uninstaller needs an approved signing hook before Setup
   can be represented as a fully signed distribution.
5. Update the release notes with the actual signature status and publisher.
   Do not claim SignPath sponsorship or publish its credit before acceptance.

SignPath requires a GitHub Actions artifact before a trusted signing submission
and evaluates GitHub origin information for the workflow. See the official
[GitHub trusted-build integration](https://docs.signpath.io/trusted-build-systems/github)
and [artifact configuration reference](https://docs.signpath.io/artifact-configuration/reference).

## 日本語

現在は準備段階です。SignPathの組織ID、プロジェクト、ポリシー、GitHub App、APIトークン、証明書、Foundationの支援承認はいずれも設定されていません。CIは署名要求を送信せず、署名シークレットを公開しません。

公開ソースと審査用リポジトリは[woolen-yarn/kohdalab-trkr](https://github.com/woolen-yarn/kohdalab-trkr)です。`release.yml`はGitHubのWindows・macOS runnerでソースからhelperとRustバイナリを再ビルドし、未署名成果物と独立したWindows用`signpath-input-*`をアップロードします。署名用成果物には`application-pe.xml`の第一者PE 3点、SHA-256記録、GitHub提供の由来情報だけを含めます。

GPLのGPIB helperは除外します。Setupはインストーラーとアンインストーラーの境界を承認してもらう必要があるため、審査サンプルとして別にアップロードします。

### SignPath承認後の手順

1. 公開リポジトリ、保守担当者、署名承認者、MFAを確認し、SignPath GitHub Appを導入してGitHub.comのtrusted build systemと連携します。
2. 手動承認とGitHub提供runnerを必須とするリリース署名ポリシーを作成します。有効化前に[`application-pe.xml`](application-pe.xml)とインストーラーの境界を審査します。
3. 組織ID、project slug、signing-policy slug、制限付きtokenを保護されたGitHub環境に追加します。公式`signpath/github-action-submit-signing-request@v3`には、同じ信頼済みワークフローの成果物IDだけを渡し、ローカルビルドのアーカイブは送りません。
4. 許可リストの正確なファイルを署名し、署名者とタイムスタンプを検証してから、署名後のバイト列で最終portableとSetupを生成します。完全な署名済み配布と表示するにはNSISが生成するアンインストーラーにも承認済みhookが必要です。
5. リリースノートに実際の署名状態と公開者を記載します。承認前にSignPathの支援を主張したりクレジットを公開したりしません。

trusted signingの送信前にGitHub Actions成果物が必要で、SignPathはGitHub由来情報を評価します。[公式連携手順](https://docs.signpath.io/trusted-build-systems/github)と[成果物設定](https://docs.signpath.io/artifact-configuration/reference)を参照してください。
