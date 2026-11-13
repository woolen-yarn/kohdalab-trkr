# Internal maintenance map / 内部構成

| Responsibility / 責務 | Source / ソース |
| --- | --- |
| Device identities, persisted profiles, strict validation / 機器ID・保存設定・厳密検証 | `crates/trkr-desktop/src/inventory.rs` |
| Connection-only filtering of invalid edits / 接続要求だけの設定不良除外 | `inventory/connection.rs` |
| Persistent connections and selected views / 接続保持・選択ビュー | `engine/connections.rs` |
| All Connect orchestration and combined errors / 一括接続・失敗集約 | `engine/connections/batch.rs` |
| USB power worker, settings and observations / USBパワーworker・設定・取得 | `power_meter.rs` |
| Lossless raw recording and bounded file snapshots / raw保存・有界ファイルコピー | `raw_csv.rs` |

Connection snapshots never rewrite the saved profile or silently choose another measurement instrument. Save/start validation remains strict. Shared-state locks protect only state access: callbacks execute after releasing the power status mutex. Position polling retains the same sorted round-robin order without allocating a temporary ID list each tick.

接続スナップショットは保存設定を書き換えず、別の測定機器を自動選択しません。保存・開始時の厳密検証を維持します。パワー状態のロックは状態取得に限定し、通知callbackは解除後に呼びます。位置照会は従来のソート順・巡回順を保ち、毎回のID一覧生成を省きます。

Raw snapshots capture the initial file length, find the last complete quoted CSV record, then parse only that prefix. Memory scales with a CSV record rather than the whole file. Preserve multiline replies, escaped quotes, point filtering and incomplete-tail exclusion when changing this path. The acquisition calculations, precision policy, transport commands and stop policy are separate contracts and are not changed by this maintenance.

rawコピーは開始時のファイル長を固定し、引用符を考慮した最後の完結行までを二段階で読みます。メモリ使用量はファイル全体ではなくCSV行の大きさに依存します。複数行の装置応答・引用符・点数フィルター・未完行除外を維持してください。測定計算・有効桁数・通信命令・停止方針は別の契約として扱い、この整理では変更しません。

## Snapshot and transport boundaries / 途中保存・通信の境界

- `ui/snapshot.rs` owns staging and no-clobber publication of the analysis CSV,
  raw audit and run metadata. It freezes one source-state classification before
  reading the files; the GUI's idle flag alone cannot prove a run is finalized.
  It depends on filesystem/serialization helpers, not GUI widgets or instruments.
- `engine/alignment.rs` owns manual-motion outcomes and selected-axis projection.
  A final display readback is best-effort; it cannot overwrite a move or STOP
  failure. A failed STOP is an error even when cancellation was requested.
- `MotionRig::open_with_bus_factory` supplies framed transports while retaining
  the normal validation, calibration queries and initial position read. Production
  uses `MotionPortPool`; fault tests supply a scripted `Bus` without serial ports.
  The factory does not bypass motion limits or change command sequencing.
- `font_selection.rs` isolates system-font face discovery from widget geometry.
  It uses `read-fonts`; the separate renderer still has its own font dependencies.

- `ui/snapshot.rs`がmain CSV・raw・runメタデータの一時保存と上書き禁止の確定を担当します。
  コピー前に状態を一度判定し、GUIの待機フラグだけで完了とは扱いません。
  この層はファイル・シリアライズだけに依存し、ウィジェットや実機を操作しません。
- `engine/alignment.rs`が手動移動の結果と選択軸への表示反映を担当します。
  最後の表示用読取が失敗しても移動・STOPの結果を上書きせず、キャンセル要求中でも
  STOP失敗はErrorとして表示します。
- `MotionRig::open_with_bus_factory`は通信生成だけを差し替えます。設定検証・校正照会・
  初回位置読取は同じ経路を通り、本番は`MotionPortPool`、異常テストは疑似`Bus`を使用します。
  移動制限や命令順序は迂回しません。
- `font_selection.rs`でOSフォントのface探索をウィジェット座標から分離し、
  `read-fonts`を使用します。描画エンジンのフォント依存は別に残ります。

## Regression checks / 回帰検証

| Contract / 契約 | Check / 検証 |
| --- | --- |
| CSV/raw/metadata consistency / 保存状態の整合 | `ui::snapshot::tests`, existing `ui::tests` snapshot cases |
| Move/STOP result precedence and connection retention / 移動・STOP結果と接続保持 | `engine::alignment::tests` through `Session::alignment` and scripted transport |
| Protocol syntax, cancellation and shared serial ownership / 命令・停止・通信所有権 | `cargo test -p trkr-motion` |
| Real GUI drag, validation and demo acquisition / 実GUI操作・模擬取得 | `ui::gui_interaction_tests::tests` with `gui-interaction-tests` enabled |
| Packaged recording, hashes and no overwrite / 配布版の記録・ハッシュ・上書き拒否 | `scripts/verify_packaged_measurements.py` on the extracted executable |
| Packaged GUI API / 配布版GUI API | `scripts/verify_common_control.py` in isolated Demo configuration |

These checks do not measure physical stopping distance or establish electrical,
mechanical, or calibration accuracy. / これらは実機の停止距離、電気・機械安全性、校正精度を測定する試験ではありません。
