# Release readiness follow-up / 配布前検証の追加点検

Unreleased work following the [focused reliability review](professional-quality-review.md).
This is evidence of specific checks, not a commercial-product certification.
[前回点検](professional-quality-review.md)に続く未公開の作業記録です。
個別の検証結果であり、商用製品としての認証ではありません。

This work was released in 0.5.5. / この作業は0.5.5で公開されました。

## Changes / 修正

- Separate snapshot file transactions from GUI code; classify source terminality
  once so stale GUI flags cannot retain terminal raw events in an active snapshot.
- Exercise actual `Session::alignment` through an injected protocol bus. Preserve
  move/STOP failure precedence, retain unrelated axes, and display failed STOP as Error.
- Replace direct `ttf-parser` usage with `read-fonts` in a focused font-face helper.
- Test all five Demo recording workflows and the common GUI API from the extracted
  Windows/Mac package, including raw provenance, file hashes and no-clobber behavior.
- Update the standalone Windows validation script's obsolete CSV column expectations.

- 途中保存のファイル処理をGUIから分離。元の終了状態を一度判定し、古いGUIフラグで
  途中保存にrawの完了イベントが混ざる競合を修正。
- 疑似通信を注入し、実際の`Session::alignment`を通して移動／STOPの結果優先、
  他軸の保持、STOP失敗時のError表示を検証。
- 直接の`ttf-parser`利用を`read-fonts`へ置換し、フォントface選択を分離。
- Windows/Macの展開済み配布版で5種類のDemo記録と共通GUI APIを検証し、
  raw、ハッシュ、上書き禁止を確認。
- Windowsの単独検証スクリプトに残った旧CSV列の期待値を現行仕様へ修正。

## Dependency boundary / 依存の残件

The application now uses egui/eframe 0.34.3, plot 0.35 and notify 0.22, with
Rust 1.92 as the minimum. Font selection uses `read-fonts`; the renderer uses the
maintained font stack. The rejected Workflow prototype is archived outside the
application workspace, so its old GUI dependencies are not shipped.

アプリをegui/eframe 0.34.3、plot 0.35、notify 0.22へ移行し、最低Rust版を1.92としました。
フォント選択は`read-fonts`、描画は保守中のフォント基盤を使用します。
非採用のWorkflow試作はアプリのworkspace外へ保存し、旧GUI依存を配布に含めません。

Existing drag cancellation, input validation and layout assertions are unchanged.
Native captures at default, scanner alignment, right-collapsed and minimum sizes
retain identical geometry records; font rasterization can differ.

ドラッグ取消・入力検証・配置の期待値は変更していません。
通常・Scanner Alignment・右パネル収納・最小サイズの実GUIで配置記録が一致しています。
フォントのラスタライズ結果は変わる場合があります。

TRKR/IV retain Linux AccessKit dependencies `zbus_names`, `zvariant` and
`zvariant_derive`. These are outside the Windows/Mac target graphs. No advisory is
suppressed and accessibility is preserved. Audit the exact lockfile on each release;
zero vulnerability/unsound findings do not mean zero maintenance warnings.

TRKR/IVにはLinuxのAccessKit経由の保守終了依存が残りますが、Windows/Macの依存には含まれません。
警告を隠したり、アクセシビリティを無効化したりしていません。
公開ごとに対象のCargo.lockを検査し、脆弱性／unsoundが0件でも保守終了が0件とは表現しません。

- [Font parser advisory / フォント解析の保守終了](https://rustsec.org/advisories/RUSTSEC-2026-0192.html)
- [zvariant migration / zvariantの統合先](https://rustsec.org/advisories/RUSTSEC-2026-0370.html)
- [egui migration / GUI基盤の変更](https://github.com/emilk/egui/blob/0.34.0/CHANGELOG.md)

## Physical devices / 実機

The operator explicitly waived new physical-device tests for this round, citing
existing operation history. No new hardware fault, power-loss or stopping-distance
test was performed. Software fault injection and Demo/CI results are reported
separately; prior field use is not recorded as a newly passed hardware test.

利用者の指示により、既存の運用実績を参考とし、今回の実機試験は省略しました。
新たな実機異常・電源断・停止距離の試験は実施していません。
疑似異常注入・Demo・CIの結果とは区別し、運用実績を今回の実機試験合格として記録しません。

## CI evidence / CI証跡

The `v0.5.5` tag at `e3044ce` passed [Build release artifacts (run 42736001409)](https://github.com/woolen-yarn/kohdalab-trkr/actions/runs/42736001409). / `e3044ce` のタグ `v0.5.5` で [Build release artifacts（run 42736001409）](https://github.com/woolen-yarn/kohdalab-trkr/actions/runs/42736001409) が成功しました。

Draft PRs do not imply a published application or a passed distribution build.
ドラフトPRの作成は配布版公開・ビルド成功を意味しません。

## Common SDK lease race / 共通SDKの制御権競合

The Python SDK now serializes lease transitions and rejects retired heartbeat
workers by generation. A heartbeat queued before voluntary release cannot poison
the next acquisition. Real active-lease revocation remains an error. Deterministic
regression tests cover both cases; the common SDK suite passes 16 tests.

Python SDKでは制御権の遷移を排他化し、終了したheartbeatを世代で判別します。
正常な解放の前に待機していたheartbeatが次の制御権取得を誤って失敗させる競合を修正しました。
実際の制御権喪失は引き続きエラーとし、両ケースの再現試験を含む共通SDK 16件が成功しています。

## Hidden-window processing / ウィンドウ非表示時の処理

Controller/API polling and close protection run in `App::logic`, independently of
visible drawing. Tests drive logic without UI: background progress and pending-copy
close cancellation remain active. TRKR also tests focus-loss release of an alignment
hold; IV tests active-close Stop selection, with delivery covered by client/engine tests.

制御・APIの結果処理と閉じる際の保護を`App::logic`へ移し、描画と独立して動かします。
UIを呼ばない回帰試験で進行と保存中の終了取消を確認します。TRKRではフォーカス喪失時の
長押し移動解除、IVでは測定中の終了要求がStopを選ぶことを確認し、送信経路はClient/Engine試験で検証します。

The package API verifier waits for a successful read-only discovery response rather
than the early endpoint file, within the unchanged ten-second startup deadline.

配布API検証は、接続情報ファイルの作成だけでなく読み取り専用照会への応答を待ちます。
起動待ち上限の10秒は変更していません。
