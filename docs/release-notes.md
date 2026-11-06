# KohdaLab TRKR 0.5.1

## Fix / 修正

The periodic lock-in settings refresh previously disabled Start and device controls while its read was pending. The automatic display query now runs on the same serialized device worker without reserving the UI for an operator action. Slow refreshes are coalesced, so at most one is queued or in flight. Queued automatic reads yield to operator actions; an in-flight device query finishes before the next action. Explicit API queries, measurement, movement, connection changes and selection retain their existing pending/ownership guards.

ロックイン設定の定期再取得中に、Startや装置操作欄が一時的に暗くなる問題を修正しました。表示用の自動取得は同じ装置制御処理で順番に実行し、操作ボタンを無効にしません。遅い更新要求は1件にまとめ、待ち行列に溜めません。未実行の自動取得は操作を優先して省き、通信が開始済みの場合はその完了後に次の操作を実行します。明示的なAPI照会・測定・移動・接続変更・機器選択の操作待ちと権限制限は維持します。

## Compatibility / 互換性

No changes to JSON profiles, CSV formats, calibration, measurement timing or public API parameters. Existing v0.5.0 configurations can be loaded directly.

JSON設定・CSV形式・校正・測定タイミング・公開APIの引数は変更しません。v0.5.0の設定をそのまま読み込めます。

## Validation and downloads / 検証・配布

Regression tests cover pending-state separation, queue order, coalescing, explicit query receipts, read-only mode, a closed worker and refresh failures. Release CI runs Rust/Python checks on Windows, macOS and Linux and builds Windows Setup/portable ZIP and macOS DMG/ZIP. Hardware behavior has not been retested in this patch.

操作待ちの分離・処理順序・重複抑制・明示照会の完了通知・読取専用モード・制御処理の終了・更新失敗を回帰テストで確認します。公開CIはWindows・Mac・LinuxでRust／Pythonを検証し、Windows Setup／portable ZIPとMac DMG／ZIPを生成します。今回の修正では実機を再検証していません。

Windows packages are unsigned. macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名・公証なしです。
