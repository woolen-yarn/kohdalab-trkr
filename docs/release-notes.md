# KohdaLab TRKR 0.4.4

## Changes / 変更内容

Add Control Protocol 1 and the common Python SDK. Publish the pending application-specific settings, standard Windows installation and explicit development shared-instrument transport. Refresh synthetic screenshots and collapse the optional rail initially on narrow screens.

## Compatibility and migration / 互換性と移行

Config is `~/.kohdalab/trkr/config.json`; Windows Setup uses `C:\Program Files\Kohdalab\KohdaLab TRKR`. Import older profiles explicitly; measurements/CSV and native continuation defaults remain. The shared instrument service is development-only, separately installed and not hardware-qualified.

## Downloads / ダウンロード

Windows x64 Setup/portable ZIP; macOS Apple-silicon ZIP/DMG; source/build kit and checksums. Windows is unsigned; macOS uses ad hoc signing, not notarization.

[Release packages](https://github.com/woolen-yarn/kohdalab-trkr/releases). Existing older releases remain available.

## Validation and provenance / 検証と出所

Local Rust workspace: 273 passed; strict clippy passed. Actual Demo GUI completed 360 synthetic points via the shared SDK. Windows/macOS/Linux tests and package/install checks are recorded in the matching CI.

The common Python SDK has nine device-free tests. Source/build identifiers, package manifests and SHA-256 evidence accompany the release. These checks do not newly qualify physical instruments, optical closure, camera calibration or scientific accuracy.

## 日本語

設定とインストール先をアプリ専用に統一し、共通GUI APIを追加しました。既存CSV・測定・停止方針は維持します。共有機器サービスは別途導入する開発機能で、実機適合確認は対象外です。
