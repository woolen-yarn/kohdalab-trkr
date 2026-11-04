# KohdaLab TRKR 0.4.8

## Changes / 変更点

- LI5640 acquires only the selected XY or Rθ pair, without switching the display during each read. Unacquired components remain blank in CSV and null in the API; replay preserves the selected pair. Explicit mode changes are verified, with rollback or reconnection on failure. / LI5640は選択したXYまたはRθだけを取得し、点ごとの表示切替を廃止します。未取得側はCSV空欄・API nullとして維持し、再表示も対応します。明示的な変更は確認し、失敗時は設定復元または再接続を要求します。
- LI5640 normal voltage/current inputs are identified as V/A from actual input, normalization and ratio settings. Sensitivity uses the correct VSEN/ISEN and current-gain range. Saved readings check unit context before/after acquisition; a change retains that point as unknown with a flag and stops before further scan/return motion. Live units describe the last observed status; plots do not relabel data across units. / 入力・正規化・比率の実照会から通常入力のV/Aを判定し、感度はVSEN/ISENと電流利得ごとの範囲に対応します。保存点の前後で単位設定が変わった場合はunknown・flag付きで保持して以降の走査・復帰前に停止します。ライブ単位は直近状態に基づき、異なる単位のデータを混同しません。
- CONEX-AGAP arrival tolerance defaults to **0.01°**, separated from its 0.001° minimum motion. Per-axis native-unit `position_tolerance` can override it; the effective threshold is saved in run metadata. Mismatch errors include the axis, command, readback, difference and tolerance. / CONEX-AGAPの到達許容差を最小移動分解能から分離し、既定を**0.01°**とします。軸ごとの装置単位`position_tolerance`で変更でき、実効値をrun設定へ保存します。エラーには軸・指令・読戻し・差・許容差を表示します。
- Raw CSV includes native delay pulse and scanner mm/deg as explicit position rows, distinguishing native readings, calibrated delay-mm conversion and cached observations. Original exchanges/timing remain available without additional position queries. / raw CSVに遅延pulse・スキャナmm/degを明示行として保存し、生読値・遅延mm換算・キャッシュ値を区別します。生応答・観測時間は維持し、記録用の追加位置照会は行いません。
- Windows Config Browse and Output … use a foreground owner and an existing initial folder, including paths with brackets/apostrophes and not-yet-created output folders. No terminal window is opened. / WindowsのBrowse／…は前面表示用の所有ウィンドウと既存初期フォルダーを利用し、特殊文字・未作成パスにも対応します。端末画面は開きません。

## Compatibility / 互換性

Existing JSON profiles remain valid. No migration is required; omitted scanner tolerance is resolved by controller model. CONEX-CC defaults, movement quantization, calibration, compensation and stage limits/checks are unchanged. AGAP uses the larger arrival threshold described above; it remains configurable and out-of-tolerance moves still stop.

既存JSONはそのまま利用でき、設定移行は不要です。未指定の許容差は機種別に解決します。CONEX-CCの既定、指令の丸め、校正・補償・ステージの制限と確認は維持します。AGAPの到達許容差は上記へ変更しますが、設定変更と超過時停止を維持します。

Recording revision 6 adds selected-pair and unit-context quality flags; main/raw CSV headers are retained. API samples add `signal_unit` and `quality_flags`, and live events add unit/qualification metadata. Legacy signal field names ending in `_v` are retained for compatibility: use explicit `signal_unit` rather than assuming volts. Keep main CSV, raw CSV and run JSON together. Unverified normalized/ratio modes remain unknown.

記録revision 6で取得側・単位変更flagを追加し、main/rawのヘッダーは維持します。APIには単位・品質情報を追加します。互換性のため`_v`付き信号名は残しますが、Vと仮定せず`signal_unit`を使ってください。main CSV・raw CSV・run JSONは一緒に保管します。未確認の正規化・比率モードはunknownです。

## Validation and downloads / 検証・配布

Local Rust tests (307), Python/Notebook/packaging tests (63), strict Clippy and documentation checks passed. Actual macOS GUI synthetic smoke covered all five workflows (71 rows). Release CI runs Windows, macOS and Linux tests, packages Windows x64 Setup/portable ZIP and macOS Apple silicon DMG/ZIP, and verifies the isolated Windows installer/uninstaller lifecycle. Corresponding source/build kits, provenance and SHA-256 manifests accompany the packages.

ローカルのRust 307件・Python/Notebook/配布テスト63件、Clippy・文書チェックに成功しました。Macの実GUIで全5測定・合成71点を確認しました。リリースCIで3OSをテストし、Windows x64インストーラー／portable ZIP、Mac Apple silicon DMG／ZIPを作成し、隔離したWindowsのインストール・削除を検証します。対応ソース・来歴・SHA-256一覧を同梱します。

No physical-device acquisition or motion was performed for these changes. Windows tests execute the actual PowerShell path helpers without displaying a chooser; interactive foreground behavior has not been physically verified. The screenshot alone does not establish the physical cause of the original scanner mismatch.

今回の修正では実機測定・移動は行っていません。Windowsでは実際のPowerShellパス処理をテストしますが、選択画面の実画面確認は未実施です。写真だけで元の到達エラーの実機原因を確定したものではありません。

Windows artifacts remain unsigned; macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名で公証はありません。
