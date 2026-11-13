# Code signing policy

## Current status

KohdaLab TRKR has not been accepted by SignPath Foundation and current Windows
artifacts are unsigned. The files in [`.signpath`](.signpath) and the release
workflow prepare an application only; they do not claim sponsorship,
certification, an active signing service, or a signed release. macOS ad-hoc
signing, if present, is not Apple Developer ID signing or notarization.

## Responsibility and approval

The public source and application-review repository is
[woolen-yarn/kohdalab-trkr](https://github.com/woolen-yarn/kohdalab-trkr).
The maintainer listed in [`.github/CODEOWNERS`](.github/CODEOWNERS) is the
proposed signing approver. Before production signing is enabled, the repository
owner must confirm its reviewer and approver roles in SignPath and enable MFA
for all repository and signing-team accounts.

Every production request requires source review, successful CI, and manual
approval by the designated approver. Signing credentials remain in SignPath and
a protected GitHub environment. Pull requests, forks, and untrusted workflows
must never receive them. A release must distribute the same signed artifacts
and checksums that were produced by the approved workflow.

## Signing boundaries

The proposed first-party PE allowlist is deliberately exact:

- `kohdalab-trkr.exe`
- `kohdalab-trkr-demo.exe`
- `kohdalab-visa-helper.exe`

The GPIB helper contains GPL-licensed upstream-derived code and is excluded
from the allowlist. It must not be signed under the project identity unless
SignPath explicitly approves that boundary. No recursive `*.exe` or `*.dll`
rule is permitted.

The NSIS Setup executable is staged separately for provider review. Its
generated uninstaller must have an approved signing path before the installer
can be claimed as a fully signed distribution. Signing the outer installer alone
does not sign the installed application binaries or uninstaller.

## After acceptance

Only after acceptance and activation, add the required attribution:
“Free code signing provided by [SignPath.io](https://about.signpath.io/),
certificate by [SignPath Foundation](https://signpath.org/).”

Each release must state its actual signer and signature status. A signature
identifies a publisher and detects modification; it does not certify instrument
accuracy or guarantee the absence of Windows warnings.

See [privacy](PRIVACY.md), [redistribution terms](REDISTRIBUTION.md), and the
[SignPath GitHub integration](https://docs.signpath.io/trusted-build-systems/github).

## 日本語

### 現在の状態

KohdaLab TRKRはSignPath Foundationに承認されておらず、現在のWindows成果物は未署名です。[`.signpath`](.signpath)とリリースワークフローは申請準備のためのもので、支援、認証、稼働中の署名サービス、署名済みリリースを示しません。macOSのアドホック署名があっても、Apple Developer ID署名や公証ではありません。

### 責任と承認

公開ソースと申請審査用リポジトリは[woolen-yarn/kohdalab-trkr](https://github.com/woolen-yarn/kohdalab-trkr)です。[`.github/CODEOWNERS`](.github/CODEOWNERS)の保守担当者を署名承認者の候補とします。本番署名前に、所有者はSignPathで審査・承認の役割を確認し、すべてのリポジトリ・署名チームアカウントでMFAを有効にする必要があります。

本番の各署名要求にはソースレビュー、CI成功、指定承認者の手動承認が必要です。署名認証情報はSignPathと保護されたGitHub環境に保持し、PR・fork・信頼できないワークフローへ渡しません。承認済みワークフローで生成したものと同一の署名済み成果物とチェックサムを配布します。

### 署名の境界

第一者PEファイルの申請用許可リストは、`kohdalab-trkr.exe`、`kohdalab-trkr-demo.exe`、`kohdalab-visa-helper.exe`の3つに限定します。

GPLの上流由来コードを含むGPIB helperは対象外です。SignPathが境界を明示的に承認しない限り、プロジェクトの名義で署名しません。再帰的な`*.exe`・`*.dll`指定は禁止します。

NSIS Setupは提供者の審査用に別途準備します。インストーラー全体を署名済みと表示するには、生成されるアンインストーラーにも承認済みの署名経路が必要です。外側のSetupだけを署名しても、導入後の実行ファイルやアンインストーラーには署名されません。

### 承認後

承認・有効化後に限り、次のクレジットを追加します。

“Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).”

各リリースで実際の署名者と署名状態を明記します。署名は公開者を識別し改変を検知しますが、測定精度やWindows警告が出ないことを保証しません。

[プライバシー](PRIVACY.md)、[再配布条件](REDISTRIBUTION.md)、[SignPathのGitHub連携](https://docs.signpath.io/trusted-build-systems/github)も参照してください。
