# Measurement integrity / 測定記録の精度

## Three-file recording (0.4.0) / 3ファイル保存

New runs use `<name>.csv` (analysis), `<name>.raw.csv` (raw evidence), and
`<name>.run.json` (conditions and outcome). Recording revision 5 declares
`trkr_analysis_v1`, `raw_csv_v1`, and metadata schema 3. This intentionally changes
new CSV column names; old CSV and JSONL-backed runs remain readable/copyable.
Historical descriptions below apply to their stated versions, not the new writer.

新規測定はmain CSV・raw CSV・run JSONの3ファイルです。記録revision 5で
`trkr_analysis_v1`・`raw_csv_v1`・metadata schema 3を宣言します。
新規CSVの列名は変更しますが、旧CSVの読込と旧JSONL付き測定のコピーは維持します。
以下の旧版の説明は、その版での保存形式を記述した履歴です。

| Main columns / main列 | Meaning / 意味 |
| --- | --- |
| `run_id`, `point_index` | Run UUID and one-based saved point number / 測定UUIDと1始まりの点番号 |
| `timestamp`, `elapsed_s` | Signal-read completion on host UTC and monotonic run seconds / ホスト側信号読取完了のUTC・単調経過秒 |
| `quality_flags` | `simulated`, `ok`, `overload`, `reference_unlock`, `reference_status_unknown`, `status_unknown`, `unit_unknown`; combined with `\|` / 状態・不明・合成を区別 |
| `measurement`, `fast_axis`, `slow_axis`, `line_index` | Workflow, axes and zero-based 2D line index; slow/line empty for 1D / 測定・走査軸と0始まり2D行番号。1Dはslow・line空欄 |
| `target_elapsed_s` | Monitor scheduled seconds / モニター予定経過秒 |
| `target_t_cor_ps`, `target_x_cor_um`, `target_y_cor_um` | Canonical commanded measurement coordinates, even when input uses mm/pulse / 入力がmm・pulseでも換算済みの測定座標指令 |
| `target_u_cor_um`, `target_v_cor_um` | Rotated commands; orthogonal zero is explicit / 回転座標の指令。直交回転軸の0も明示 |
| `t_ps`, `x_um`, `y_um` | Available actual instrument coordinates, including fixed axes / 固定軸を含む取得済み機器座標 |
| `t_cor_ps`, `x_cor_um`, `y_cor_um`, `u_cor_um`, `v_cor_um` | Actual corrected and rotated readbacks / 実読取の原点補正・回転座標 |
| `spatial_theta_deg` | Spatial rotation / 空間回転角 |
| `X`, `Y`, `R`, `Theta_deg`, `signal_unit` | Original instrument components and qualified unit / 装置由来の各成分と確認できた単位 |

Do not assume V if `signal_unit` is `unknown`. Currently SR7265 voltage input
(IMODE 0) is qualified from the point status query; other unverified modes remain
unknown. Simulation is synthetic V. A status read follows the signal read and is
not simultaneous with it; supported overload/reference bits are saved, while
unsupported information remains unknown. Status/IMODE reads add read-only I/O,
so total point duration includes their cost. They do not configure the instrument.
Start settings are freshly queried using the existing settings commands; controls
not queried (including unimplemented phase/reference controls) remain unknown.

`signal_unit=unknown`をVと仮定しません。現時点では点ごとの状態照会で確認した
SR7265 IMODE 0をVとし、未検証のモードは不明を維持します。合成値は合成Vです。
状態照会は信号読取後で同時ではありません。対応する過負荷・参照異常ビットを保存し、
未対応情報は不明です。状態・IMODE照会による読取時間は点の所要時間に含まれますが、
装置設定は変更しません。開始時の設定は既存の設定照会を使い再取得し、未照会の位相・
参照制御などを確認済みとして扱いません。

Raw CSV uses these shared columns: `run_id`, `event_index`, `point_index`,
`reading_index`, `event_type`, `device_role`, `quantity`, `command`, `raw_response`,
`value`, `unit`, `host_started_at_utc`, `host_completed_at_utc`,
`started_elapsed_s`, `completed_elapsed_s`, `result`, `error`, `details_json`.
Quoted cells preserve CR/LF, commas, quotes and trailing zeroes. `details_json`
retains structured coordinates/settings/return outcomes not representable in one
scalar. Main CSV rows are not copied as another string array. No new JSONL mirror
is written. Lock-in and point-position transport responses are retained; idle live
polling is excluded. Grouped queries inherit a logical-operation time window,
explicitly not individual-command or device ADC timestamps. This is decoded valid
text evidence, not a binary bus capture.

rawの列は上記共通18列です。引用符付きセルに改行・カンマ・引用符・末尾ゼロを保持し、
単一値に収まらない座標・設定・終了状態は`details_json`へ保存します。mainの行全体の
文字列配列コピーや、新規JSONLへの二重保存はしません。ロックインと測定点位置の
通信応答を保持し、待機ライブ表示は除外します。まとめて取得した照会の時刻は
論理操作全体の時間窓であり、個別コマンドや装置ADC時刻ではありません。
これは有効な文字列の記録で、バイナリバスの完全記録ではありません。

Run JSON includes the plan/optional `experiment` object, configured hardware,
normalized motion settings with effective compensation defaults, runtime delay
microstep and scale, observed start lock-in settings, software version/build
commit when available, completion state and main/raw SHA-256 hashes. Use an
`experiment` object in a profile or Notebook plan for sample ID, conditions,
units and notes; external conditions are user-supplied, never automatically
invented. Save copies retain the run ID, update filenames and recompute hashes for
the actual snapshot. Checksums detect mismatches, not malicious tampering.

run JSONには計画・任意の`experiment`、設定上の機器、既定値を解決した移動設定・補償、
実効マイクロステップ・換算係数、開始時ロックイン設定、ソフトウェア版・取得可能な
ビルドコミット、終了結果、main/raw SHA-256を保存します。設定ファイルまたはNotebook
計画の`experiment`へ試料ID・外部条件・単位・メモを指定できます。外部条件は入力値で、
自動測定した値と捏造しません。Saveコピーは測定IDを維持し、コピー先名と実際の内容の
チェックサムを更新します。チェックサムは取り違え検出用で、改ざんへの署名ではありません。

Migration: `X_V/Y_V/R_V` become `X/Y/R` plus `signal_unit`;
`acquisition_elapsed_s` is merged into `elapsed_s`; controller mm/degree/pulse
fields move to raw position evidence. Coordinate type and immutable settings live
in run JSON. Readers still accept the original old column names. Do not delete
old sidecars or rewrite historical runs to make them resemble a new acquisition.

移行では信号列を`X/Y/R`と`signal_unit`へ変更し、経過秒を`elapsed_s`へ統一します。
コントローラー単位はraw、座標種別・固定設定はrun JSONへ移します。旧列名の読込は
維持します。過去の測定を新規取得のように書き換えたり、旧付属ファイルを削除しません。


## Stored digits / 保存する桁

Starting with 0.3.5 (recording revision 2), X_V/Y_V/R_V use shortest scientific
text that reconstructs the acquired finite f64 exactly. Other numeric fields
retain shortest Python-compatible round-trip text. CSV columns are unchanged.
GUI labels may use fewer digits; display formatting never rounds the saved value.
This is numeric preservation, not a claim that every digit is physically significant.

0.3.5（記録revision 2）から、X_V/Y_V/R_Vは取得した有限f64値を正確に復元できる最短の指数表記で保存します。他の数値列はPython互換の最短往復表記を維持し、CSV列は変更しません。GUI表示は短くても、保存値は表示用に丸めません。これは数値の保持であり、全桁が物理的な有効数字だという保証ではありません。

## Coordinate recording (0.3.6, revision 3) / 座標の保存

Every available fresh connected-axis readback is saved, even when that axis is
fixed during SRKR, TRKR or 2D acquisition. `t_ps`/`x_um`/`y_um` are instrument
coordinates; `t_cor_ps`/`x_cor_um`/`y_cor_um` subtract `plan.zero`. Controller
mm/degree/pulse readings are retained when available. Disconnected/unknown axes
stay empty. No extra connections, motion or position queries are introduced.
Signal Monitor follows the same availability rule.

SRKR・TRKR・2D測定で固定されている軸も、接続済み機器から取得できた読取を保存します。`t_ps`・`x_um`・`y_um`は機器座標、`t_cor_ps`・`x_cor_um`・`y_cor_um`は`plan.zero`を差し引いた座標です。取得できるmm・degree・pulseも保存します。未接続・不明な軸は空欄で、追加の接続・移動・位置照会は行いません。Signal Monitorも同じ基準です。

For rotated scans, `target_u_cor_um`/`target_v_cor_um` hold commanded U/V values;
the unscanned rotated axis is explicitly zero because a rotated move commands
both X and Y. `u_cor_um`/`v_cor_um` hold the inverse rotation of actual corrected
X/Y readings: `u=x_cor*cos(theta)+y_cor*sin(theta)`,
`v=-x_cor*sin(theta)+y_cor*cos(theta)`. Both X/Y readings are required; never
replace a missing readback with a target. Cartesian scans do not invent U/V
targets, but may record measured U/V when both X/Y readings exist.
`spatial_theta_deg` records the angle; `plan.zero` remains in `.run.json`.
U/V replay groups scan lines by explicit targets while retaining actual X/Y.
In 0.3.5 and earlier, `u_cor_um`/`v_cor_um` were derived from targets, not readbacks;
existing CSVs are not rewritten. CSV column names stay unchanged.

U/Vスキャンでは`target_u_cor_um`・`target_v_cor_um`が指令値です。回転移動はX/Y両軸を指令するため、走査しない回転軸の目標も明示的に0とします。`u_cor_um`・`v_cor_um`には実際の補正済みX/Y読取を上式で逆回転した値を保存します。両方の読取が必要で、欠けた読取を指令値で埋めません。X/Y走査ではU/V指令値を捏造しませんが、両軸を読めれば実測U/Vを記録します。角度は`spatial_theta_deg`、原点は`.run.json`の`plan.zero`で確認できます。U/V再生は指令値で行を区別し、実際のX/Y読取も保持します。0.3.5以前の`u_cor_um`・`v_cor_um`は指令値由来でした。既存CSVの書換えと列名の変更は行いません。

## Replay grids and acquisition times (0.3.7, revision 4) / 再生グリッドと取得時刻

All 2D workflows prefer explicit commanded targets for both scan axes, including
T/X/Y and U/V. Equal targets form one scan line even when actual positions drift;
distinct targets are retained even below 1e-9. Actual physical readbacks remain
available separately. Older CSVs without targets use the previous best-effort
readback grouping, so their intended grid cannot always be recovered.

全2D測定はT/X/Y・U/Vとも両走査軸の指令値を優先してグリッドを復元します。実読取が揺れても同じ指令値は同じ行となり、1e-9未満の異なる指令値も区別します。実際の機器読取は別に保持します。指令値のない旧CSVは従来の読取による推定に戻るため、意図したグリッドを必ず復元できるわけではありません。

CSV `timestamp` now labels signal-read completion on the host, captured before
provenance serialization and storage. `acquisition_elapsed_s` uses the matching
monotonic completion observation. UTC and monotonic clocks are read adjacently
before formatting; these are host observations, not simultaneous device clocks.
Earlier releases labeled CSV formatting time.
Unknown acquisition timestamps stay empty. Audit schema 2 adds `timing.position`
and `timing.signal` windows to reading events and a separate `csv_write` event
per attempted row. Each window has `host_started_at_utc`, `host_completed_at_utc`,
`host_started_elapsed_s`, `host_completed_elapsed_s` and null
`device_timestamp_utc`. Position timing covers the existing aggregate read
before signal acquisition; it does not imply simultaneous position/signal
sampling. Signal timing covers the logical lock-in read, which can contain
multiple commands. CSV-write completion follows a successful buffer flush,
not a per-row disk sync. Missing position windows mean no position read occurred.
Simulation timings are host timings for synthetic signals, never device clocks.

CSVの`timestamp`は監査の整形・保存前に取得したホスト側の信号読取完了時刻です。`acquisition_elapsed_s`も同じ完了点の単調時計を使います。従来版はCSV整形時刻でした。取得時刻が不明なら空欄です。監査schema 2では読取イベントに`timing.position`・`timing.signal`、各行の書込試行に`csv_write`を追加し、上記のUTC開始・完了と単調経過秒を記録します。`device_timestamp_utc`は不明を表すnullです。位置の時間窓は信号前の既存の一括読取を表し、信号との同時取得を意味しません。信号の時間窓は複数コマンドを含み得るロックイン読取全体です。CSV完了はバッファー排出後で、各行のディスク同期ではありません。位置窓がない場合は位置照会をしていません。シミュレーションも合成信号のホスト時刻として扱います。

## Keep three files / 3ファイルを保管

Keep `name.csv`, `name.run.json`, and `name.readings.jsonl` together. The JSONL
retains the exact valid text responses returned by the existing instrument reads,
including trailing zeroes, conversion inputs, errors and point identifiers.
Transport decoding errors are recorded as errors; this is not a binary bus capture.
Metadata records the configured model, transport, motion calibration and plan;
a configured model name is not a fresh instrument identification query.
Resolution and uncertainty that are not obtained remain unknown. Simulation
is explicitly synthetic and does not fabricate hardware response strings.

`name.csv`、`name.run.json`、`name.readings.jsonl`を一緒に保管してください。JSONLには既存の装置読取で返された有効な文字列を末尾のゼロも含めて保持し、換算入力、エラー、点番号を残します。文字コード変換の失敗はエラーとして記録し、バイナリ通信の記録ではありません。メタデータは設定上の機種、通信、移動校正と計画を記録します。設定した機種名は新たに照会した識別結果ではありません。取得していない分解能・不確かさは不明です。シミュレーションは明示し、実機応答を捏造しません。

## Failure and compatibility / 失敗と互換性

Files are created without overwriting existing files. Each acquired point is
flushed; finalization syncs both streams. This does not guarantee survival of
an abrupt power loss. Retain partial files after an error: a raw successful
reading may exist even if writing its CSV row failed. Save Now copies only
complete CSV lines and matching audit points into a new snapshot, with sidecars.
Copies of finalized failed runs also retain raw attempts whose CSV write failed.
No extra device query or change to scaling, motion, waits or limits is introduced.
Audit I/O adds storage work and is included in measured elapsed time.

既存ファイルは上書きしません。各点で書込バッファーを排出し、終了時に両ファイルを同期しますが、突然の停電に対する完全な保証ではありません。エラー時は部分ファイルを保持してください。CSV行の書込が失敗しても、その前の正常な生読取が残る場合があります。Save Nowは完全なCSV行と対応する監査点を付属ファイルごと新たなスナップショットへコピーします。終了済みの失敗runでは、CSV書込が失敗した読取の生記録も保持します。機器への追加照会、換算、移動、待機、制限の変更はありません。監査用の保存処理は実測経過時間に含まれます。

Existing profiles, Notebook commands and old CSV playback remain supported.
Old fixed-seven-digit CSV exports cannot recover discarded digits. Calibration,
noise, input mode and instrument specifications determine physical uncertainty;
CSV exponent notation alone cannot determine significant figures.

既存設定、Notebook命令と旧CSV再生は維持します。旧有効7桁CSVで失われた桁は復元できません。物理的な不確かさは校正、ノイズ、入力モード、装置仕様で決まり、CSVの指数表記だけでは有効数字を決められません。
