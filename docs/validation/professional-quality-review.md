# Focused reliability review / 信頼性・保守性の重点点検

Unreleased local work based on `9bf80cf` on `codex/oss-evaluation`, macOS arm64. This records source/software checks, not certification, a hardware safety qualification or a published release.
`codex/oss-evaluation` の `9bf80cf` を基点とする未公開のローカル修正です。ソース・ソフトウェアの確認記録であり、認証・実機安全性の認定・公開済みリリースではありません。

This work was released in 0.5.5. / この作業は0.5.5で公開されました。

## Scope and decisions / 対象・判断

- Trace production Stop/hold/initialization/ownership and command outcomes; preserve acquisition, calibration, compensation, CSV precision and device protocols.
- Preserve failed-read evidence in raw audits. Active snapshots must not inherit a source-run terminal event that races the snapshot copy. A typed `SnapshotState` distinguishes active, finalized and failed copies.
- Preserve accepted finite input values when an increment would overflow. Bound overlay text as well as notification count; keep the complete log.
- Resolve the invariant signal scale once per plotted trace, avoiding repeated unit-string generation per point. This changes no samples or plot coordinates. No overall FPS/speedup claim is made.
- Add actual-App device-free interaction tests and a pinned RustSec auditor to CI, without replacing existing platform, packaging or measurement tests.

- 本番の停止・Hold・初期化・制御権・操作結果の経路を確認。取得処理、校正、補償、CSV精度、装置プロトコルは維持します。
- rawの失敗読取情報を保持。途中保存には競合して追加された完了イベントを入れません。`SnapshotState` で保存状態を明示し、矛盾するbooleanの組合せを避けます。
- 増減操作がオーバーフローする場合は直前の有効値を保持。通知の件数に加えて文字量も制限し、全文はログに残します。
- グラフ全体で共通の倍率を各点で求め直す処理を除去。測定値・座標は不変です。アプリ全体のFPSや速度倍率は主張しません。
- 実Appの実機不要操作テストと版固定のRustSec検査をCIへ追加。従来のOS・配布・測定テストは維持します。

## Dependency evidence / 依存関係

`cargo-audit 0.22.2`, RustSec database commit `899077d9d471b9fd3dbf4b2faea8ef26e83692e8`.

- TRKR: `time 0.3.45 → 0.3.47` addresses [RUSTSEC-2026-0009](https://rustsec.org/advisories/RUSTSEC-2026-0009.html). The affected upstream path parses RFC 2822; TRKR's inspected timestamp paths use RFC 3339. A reported dependency advisory is not evidence that this app was exploited.
- IV and Camera: a separate minimal `tokio 1.53.2 → 1.53.3` lockfile change addresses the `unsound` advisory [RUSTSEC-2026-0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html). PLE/Shutter's inspected locks already use a patched version.
- TRKR post-update: zero vulnerability entries and zero unsound entries. Four unmaintained warnings remain: `ttf-parser`, `zbus_names`, `zvariant`, `zvariant_derive`. These are retained in evidence, not suppressed. Replacing their upstream GUI/font stack needs a separate cross-platform compatibility review.

- TRKRの`time`を修正版へ限定更新。問題の対象はRFC 2822解析で、確認したTRKR経路はRFC 3339です。依存指摘をアプリへの攻撃実証とは扱いません。
- IV/Cameraは別ブランチで`tokio`の限定更新。確認したPLE/Shutterは修正版でした。
- TRKR更新後は脆弱性・unsound項目とも0件。保守終了4件は隠さず記録します。上流GUI・フォント基盤の置換には別途OS横断検証が必要です。

## Release boundary / 公開前に必要な確認

Local software tests do not prove electrical/mechanical safety, controller fault behavior, emergency stopping distance or recovery after power loss. Windows/Mac artifact CI, physical-device fault/stop tests, dependency maintenance decisions, signing and release review remain separate gates. No real device was operated in this review.
ローカルのソフトウェア試験だけでは電気・機械安全性、装置異常、緊急停止距離、停電復旧を証明できません。Windows/Mac配布CI、実機異常・停止試験、依存保守判断、署名・公開レビューは別に必要です。この点検では実機を動かしていません。


## Results / 結果

[Machine-readable evidence / 機械可読の検証記録](professional-quality-evidence.json)

- TRKR Rust workspace: 461 passed; actual-App interactions: 5 passed; strict workspace Clippy passed.
- Python/packaging: 65 passed, then all 3 initially skipped native-notebook integrations passed using the newly built binary; common Python API: 14 passed.
- Independent OSS probes: 20 passed, 1 manual benchmark intentionally ignored. Release build passed; 90 native-layout captures passed and recorded geometry matched the preceding revision.
- IV lockfile update: 304 passed, 3 ignored. Camera lockfile update: 196 passed, 1 ignored; its existing compiler warnings remain.
- Independent final code review found no material issue in the changed paths. This does not establish whole-program correctness.

- TRKRのRust全体461件、実App操作5件、警告禁止Clippyに成功。
- Python・配布65件に成功。初回はバイナリ未準備で未実行だったNotebook統合3件も、新バイナリで全件成功。共通Python API14件成功。
- 独立OSS検証20件成功、手動ベンチマーク1件は意図的に除外。Releaseビルドと90画面の描画検査に成功し、記録した座標は直前版と一致。
- IVの依存更新は304件成功・3件除外、Cameraは196件成功・1件除外。Cameraの既存コンパイラ警告は残っています。
- 最終独立レビューで今回の変更範囲に重大な問題は見つかりませんでした。プログラム全体の正しさを保証するものではありません。

Alignment now projects only the operated device's confirmed readback into selected axes; it does not poll another selected device and convert its unrelated failure into a failure of a completed move. Operation/Stop failures retain precedence; the final display readback remains best-effort. Regression tests check Session axis projection. A complete pseudo-serial Session test was not used because macOS rejects Python PTYs at serial DTR setup; physical serial fault injection remains unverified.

Alignmentは操作した装置の確認済み座標だけを対応軸へ反映し、別装置の読取失敗で完了した移動を失敗扱いにしません。移動・Stopの失敗はそのまま返し、表示用最終読戻しは従来どおり可能な範囲で更新します。Sessionの軸反映を回帰検証しました。macOSのserialportはPython疑似端末のDTR設定を拒否するため、疑似シリアルによる全経路試験は採用していません。実機への異常注入は未検証です。

Common API receipt/result cleanup classification now shares one helper: completed/stopped = verified; error = unknown; active = pending. The CI checks are added to source; no GitHub CI run or release was performed in this local review.

共通APIのcleanup判定を一箇所に集約し、完了・停止済みはverified、エラーはunknown、処理中はpendingに統一しました。CI検査はソースへ追加しましたが、この点検ではGitHub上のCI実行・公開は行っていません。
