# Measurement integrity / 測定記録の精度

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
