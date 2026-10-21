# Security Policy

This project is laboratory-control software. A security issue may include any
bug that allows unintended motion, unsafe remote operation, leaked local
configuration, or corrupted measurement files.

## Reporting

Please report sensitive issues through GitHub private security advisories when
available. If that is not available, contact the repository maintainers through
the KohdaLab organization before publishing details.

Include:

- affected repository and version or commit
- operating system and instrument connection type
- steps to reproduce
- whether any stage or scanner was moving
- suggested mitigation if known

## Scope

In scope:

- unsafe motion or missing stop behavior
- command injection or unsafe parsing
- leaked local configuration
- vulnerabilities in PC-side control tools

Out of scope:

- unsupported hardware modifications
- failures caused by bypassing documented safety checks
- third-party instrument firmware vulnerabilities

## 日本語

本プロジェクトは実験装置を制御するソフトウェアです。意図しない移動、安全でない遠隔操作、ローカル設定の漏えい、測定ファイルの破損につながる不具合もセキュリティ問題に含みます。

### 報告

利用可能な場合はGitHubの非公開セキュリティアドバイザリーで報告してください。利用できない場合は、詳細を公開する前にKohdaLab組織を通じて保守担当者へ連絡してください。

以下を含めてください。

- 対象リポジトリとバージョンまたはコミット
- OSと機器の接続方式
- 再現手順
- ステージやスキャナが移動中だったか
- 分かっている場合は回避策

### 対象範囲

対象：危険な移動、停止処理の欠落、コマンドインジェクション、安全でない解析、ローカル設定の漏えい、PC側制御ツールの脆弱性。

対象外：未対応のハードウェア改造、文書化された安全確認の回避による故障、第三者の機器ファームウェアの脆弱性。
