# Measurement integrity / 測定記録の精度

## Measurement and desktop fixes (0.4.8) / 測定・画面修正（0.4.8）

LI5640 reads only the pair selected in the left panel: XY or Rθ. The instrument display/output mapping is configured on connect or explicit mode change and verified before each read. Acquisition queries do not write DDEF/OTYP or alternate the display. The other components remain blank in main CSV and null in the API/raw signal object; they are never reconstructed. `signal_xy_only` / `signal_rtheta_only` identify the measured pair, and CSV replay preserves this omission. LI5640 mode changes are disabled while a GUI operation is pending/running; external `set_lockin("display_mode", "xy" | "r_theta")` uses the same worker. Other lock-in drivers retain all-component acquisition. Each LI5640 sample uses one DOUT? plus three read-only mode queries to prevent silent relabeling after external/front-panel changes; the former per-pair 20 ms sleeps are removed. Periodic/manual settings refresh no longer changes OTYP to fetch reference frequency, so that unqueried field stays unknown. Explicit selection failures restore/verify the old mapping or invalidate the session if restoration is unverified.

LI5640は左パネルで選んだXYまたはRθだけを取得します。接続・明示的な選択変更時に本体の表示と出力対応を設定し、読取り前に確認します。通常取得中はDDEF/OTYPを書き換えず、表示を往復しません。未取得側はmain CSVで空欄、API・raw信号オブジェクトでnullとし、計算で補いません。取得側は上記quality flagで識別し、CSV再表示でも未取得を維持します。GUI操作の実行待ち・実行中はLI5640の選択変更を無効にします。外部制御も同じワーカーで処理します。他機種の全成分取得は維持します。LI5640はDOUT? 1回と読取り専用のモード照会3回で外部・本体操作による誤ラベルを防ぎ、従来の各ペア20 ms待機は除去します。周期・手動の設定照会もOTYPを変更せず、未取得の参照周波数は不明のままです。明示的な選択変更が失敗した場合は元設定の復元を確認し、確認できなければ接続を無効化します。

Recording revision 6 retains the main/raw column schemas and adds `position_value` raw rows for observed `delay_stage_pulse` (pulse), `delay_stage_mm` (mm), `x_scanner_mm` / `y_scanner_mm` (mm), and `x_scanner_deg` / `y_scanner_deg` (deg). `device_role`, `quantity`, `value`, `unit` and point index make native values directly readable; the original responses and structured position are retained. `delay_stage_pulse` is the controller readback; `delay_stage_mm` is its conversion using effective calibration and is identified as such in `value_origin`. Missing axes remain absent, pulse 0 remains a valid value, and scalar materialization does not trigger hardware reads. Repeated reading snapshots do not create duplicate scalar rows. Cached final/last positions carry an explicit qualification with unknown observation time rather than borrowing movement completion timestamps.

記録revision 6ではmain/rawの列構成を維持し、上記の装置生座標を`position_value`行へ明示します。機器・量・値・単位・測定点番号で抽出でき、生応答と元の座標オブジェクトも保持します。遅延pulseは装置読戻し、遅延mmは実効校正によるpulseからの換算として`value_origin`で区別します。未接続軸は捏造せず、pulseの0は有効値として保持し、記録のための追加照会はしません。同じ点のreadingに再掲された座標は明示行へ二重展開しません。キャッシュされた最終・直近座標には観測時刻不明を明記し、移動完了時刻を実読取時刻として扱いません。

A scanner arrival mismatch now reports X/Y, requested sample µm, quantized native command, native readback, difference and allowed tolerance. Arrival tolerance is now separate from minimum motion resolution: CONEX-AGAP defaults to 0.01° based on the manufacturer’s specified bidirectional repeatability, while CONEX-CC retains its previous default. An optional `instruments.scanner.x.position_tolerance` / `.y.position_tolerance` overrides the threshold in native deg/mm; it must be finite, positive and no larger than the configured travel span. Normalized run configuration records the resolved tolerance. Minimum motion resolution, command rounding, travel limits and delay-stage verification are unchanged. At the exact tolerance boundary, a bound of 8 × f64 epsilon × (absolute target + absolute readback + absolute native origin + tolerance) excludes only binary conversion/subtraction roundoff; errors outside that numerical bound still fail. For example, 0.053° − 0.052° evaluates as 0.0010000000000000009° rather than exact 0.001°. The last saved point in a screenshot cannot identify the failed target; use the detailed error and raw/run files.

スキャナ到達エラーにはX/Y・要求µm・丸め後の装置指令・生読戻し・差・許容差を表示します。到達許容差を最小移動分解能から分離し、CONEX-AGAPはメーカーの両方向繰返し精度に基づく0.01°を既定にします。X/Yごとの`position_tolerance`（装置単位deg/mm）で変更でき、有限の正値かつ設定travel範囲以内が必要です。run JSONに実効許容差を保存します。CONEX-CCの既定、最小移動分解能、指令の丸め、travel limit、遅延ステージの確認は維持します。0.053°−0.052°が計算上0.0010000000000000009°になるような境界では、上記の機械精度に基づく換算・減算誤差だけを除外し、その外側は引き続き停止します。画面の最後の保存点だけでは失敗点を特定できないため、詳細エラーとraw/runを確認します。

## Stop and motion verification (0.4.1) / 停止・移動確認

Stop first stops the motion axes and verifies stationary controller states. It then
retires exactly one interrupted GPIB helper reply before accepting another command.
A healthy reply is discarded and the session stays connected; missing, malformed or
failed replies invalidate the session. This never resumes the interrupted run.
In 0.4.1–0.4.7, LI5640 display-changing reads required reconnection if cancellation prevented
restoring their settings. The 0.4.8 selected-pair reader does not perform those writes. Communication failure is distinct from a normal Stop.

Stopでは軸への停止命令と停止状態の確認を優先し、その後、中断したGPIB応答を1件だけ回収・破棄します。
正常な応答なら接続を維持し、未応答・不正応答・通信失敗なら接続を無効にします。中断した測定は再開しません。
0.4.1～0.4.7のLI5640は表示復元できない中断で再接続が必要でした。0.4.8の片側取得では通常読取りに表示変更がありません。通信異常は通常の停止と区別します。

Compensation must reach its own commanded position before the final move starts.
HOME requires a post-command BUSY/HOMING transition and a confirmed completion state;
its physical position is not assumed to equal the application's calibrated zero.
CONEX-CC OR is allowed only from NOT REFERENCED; an already referenced axis must be
initialized with the GUI **Skip HOME (already referenced)** option (0.4.2), or explicitly reset outside the application before rehoming.
The application never resets a referenced axis automatically.
See [Newport's CONEX-CC controller manual](https://www.newport.com/medias/sys_master/npresources/h42/h17/9954514534430/CONEX-CC%20-%20Controller%20Documentation/CONEX-CC-Controller-Documentation.pdf).

補償位置への実到達を確認してから本移動へ進みます。HOMEは命令後のBUSY/HOMINGと完了状態を確認し、
機械原点とアプリの校正原点を同一と仮定しません。CONEX-CCのORは未参照状態だけに許可し、
参照済みの軸はGUIの **Skip HOME (already referenced)**（0.4.2）で初期化するか、アプリ外で明示的にリセットしてから再原点復帰します。
アプリが自動でリセットすることはありません。上記メーカー資料に従います。

Live overload/reference/error status is queried at most every 500 ms independently
of saved acquisition points. A failed point status query preserves the raw evidence
and acquired point with unknown qualification, then fails the run before another
scan or return move. Overload/unlock are acquisition-quality indications, not laser
or electrical-output interlocks. STOP outcome/timing is retained in raw CSV.

測定点とは独立して最大500 msごとにライブ異常状態を照会します。点の状態照会が失敗した場合は、
raw証跡と取得済みの点を品質不明として保持し、次の走査・原点復帰へ進む前に測定を失敗で終了します。
過負荷・参照ロック外れは測定品質の表示であり、レーザーや電源出力のインターロックではありません。
STOPの結果と時間窓もraw CSVに記録します。

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
| `quality_flags` | `simulated`, `ok`, `overload`, `reference_unlock`, `reference_status_unknown`, `status_unknown`, `unit_unknown`; revision 6 (0.4.8) adds `signal_xy_only` / `signal_rtheta_only` and `unit_context_changed`; combined with `\|` / 状態・不明・合成を区別 |
| `measurement`, `fast_axis`, `slow_axis`, `line_index` | Workflow, axes and zero-based 2D line index; slow/line empty for 1D / 測定・走査軸と0始まり2D行番号。1Dはslow・line空欄 |
| `target_elapsed_s` | Monitor scheduled seconds / モニター予定経過秒 |
| `target_t_cor_ps`, `target_x_cor_um`, `target_y_cor_um` | Canonical commanded measurement coordinates, even when input uses mm/pulse / 入力がmm・pulseでも換算済みの測定座標指令 |
| `target_u_cor_um`, `target_v_cor_um` | Rotated commands; orthogonal zero is explicit / 回転座標の指令。直交回転軸の0も明示 |
| `t_ps`, `x_um`, `y_um` | Available actual instrument coordinates, including fixed axes / 固定軸を含む取得済み機器座標 |
| `t_cor_ps`, `x_cor_um`, `y_cor_um`, `u_cor_um`, `v_cor_um` | Actual corrected and rotated readbacks / 実読取の原点補正・回転座標 |
| `spatial_theta_deg` | Spatial rotation / 空間回転角 |
| `X`, `Y`, `R`, `Theta_deg`, `signal_unit` | Original instrument components and qualified unit / 装置由来の各成分と確認できた単位 |

Do not assume V if `signal_unit` is `unknown`. SR7265 voltage input
(IMODE 0) is qualified from the point status query. LI5640 checks ISRC/NORM/RAT
before and after each saved reading: normal voltage inputs are V and current inputs
are A; normalized, ratio and unsupported modes remain unknown. A changed context
preserves that point as unknown with a quality flag and stops before the next
scan/return move. Raw records retain both query windows without claiming atomic
instrument acquisition. Simulation is synthetic V. A status read follows the signal read and is
not simultaneous with it; supported overload/reference bits are saved, while
unsupported information remains unknown. Status/IMODE reads add read-only I/O,
so total point duration includes their cost. They do not configure the instrument.
Start settings are freshly queried using the existing settings commands; controls
not queried (including unimplemented phase/reference controls) remain unknown.

`signal_unit=unknown`をVと仮定しません。点ごとの状態照会で確認したSR7265 IMODE 0をVとします。LI5640は保存用信号の前後でISRC/NORM/RATを照会し、通常のA/A−B入力はV、電流入力はAと確認します。正規化・比率表示や未対応モードは不明を維持します。取得前後で設定が変わった点はunknown・flag付きで保持して、次の走査・復帰前に停止します。rawには前後の時間窓を保存しますが、装置側の同時刻取得を保証するものではありません。合成値は合成Vです。
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


## Reference settings / 参照設定

Version 0.5.2 records `context.lockin_settings.value.reference` in run JSON and the corresponding `settings_snapshot` exchange in raw CSV. `phase_deg` is the instrument reference offset, **not** the measured signal `Theta_deg`; `source` preserves the numeric code and its model-specific meaning; `harmonic` is the reference multiplier. These are read-only queries once at run start, separate from the periodic GUI refresh. They are not simultaneous readings or a continuous trace of front-panel changes. A failed settings snapshot records `query_failed` and its error; no default reference value is invented. Raw responses retain native formatting, including the SR5210 quadrant/millidegree pair. Demo does not claim a hardware readback.

0.5.2ではrun JSONの上記項目とraw CSVの`settings_snapshot`に参照設定を保存します。`phase_deg`は装置の参照位相で、測定信号の`Theta_deg`ではありません。参照源は数値コードと機種ごとの意味、高調波は倍率を残します。開始時に一度だけ読取り専用で照会し、周期ライブ更新には追加しません。同時取得や測定中の本体操作の連続記録ではありません。失敗時は`query_failed`とエラーを保存し、既定値で補いません。SR5210の象限・ミリ度を含む生応答の表記も保持します。Demoは実機の読戻しとして扱いません。

| Model / 機種 | Phase / 位相 | Source / 参照源 | Harmonic / 高調波 |
| --- | --- | --- | --- |
| SR830 | `PHAS?`, degrees / 度 | `FMOD?`: 0 external, 1 internal | `HARM?` |
| SR7265 | `REFP.`, degrees / 度 | `IE`: 0 internal, 1 external TTL, 2 external analog | `REFN` |
| LI5640 | `PHAS?`, degrees / 度 | `RSRC?`: 0 external, 1 internal, 2 signal | `HARM?` |
| SR5210 | `P`: quadrant × 90 + millidegrees / 1000, no wrapping / 折返しなし | `IE`: 0 external, 1 internal | `F2F`: 0 fundamental, 1 second harmonic |

Command definitions / コマンド根拠: [SRS SR830 manual](https://www.thinksrs.com/downloads/PDFs/Manuals/SR830m.pdf), [AMETEK 7265 AN1002](https://www.ameteksi.com/-/media/ameteksi/download_links/documentations/7225/an1002_using_the_model_7225_and_7265_lock-in_amplifiers_with_software_written_for_the_sr830.pdf), [NF LI5640 manual](https://www.nfcorp.co.jp/files/LI5640_InstructionManual_Jp-1.pdf), [EG&G 5210 manual §6.4.03](https://www.artisantg.com/info/Princeton_Applied_Research_EG_G_5210_Manual_201972510325.pdf). Protocol tests use simulated replies; this change is not a new physical-instrument qualification. / テストは模擬応答を使用し、新たな実機検証ではありません。

## Stored digits / 保存する桁

Starting with 0.5.2 (recording revision 7), main CSV `t_ps` and `t_cor_ps`
use fixed decimal places derived from the **observed effective ps per pulse**.
The decimal grid is at most one tenth of a pulse, so formatting error is at
most one twentieth of a pulse. The controller microstep, configured calibration
and optical round trips determine that scale; it is not a claim of physical
accuracy. For example, a 1 µm pulse and one optical round trip give approximately
0.00667 ps/pulse: `-49.501718235949715` is written as `-49.5017`.

`target_t_cor_ps` also preserves the decimal precision of the requested range
and Step (converting mm input precision to ps). Finer commanded points therefore
remain distinct even if the stage cannot resolve that motion. Measurement-unit
targets are saved directly rather than adding and subtracting the zero first.
Without observed calibration, or if fixed formatting would require more than
15 decimal places, the corresponding field retains shortest round-trip text.

The raw CSV reading event keeps exact unrounded delay values in
`details_json.unrounded_delay`, in addition to native pulse/mm readbacks and
transport responses. Run JSON records the actual formatting parameters at
`context.delay_csv_format`. Calculations, motion, API samples and plots keep
their unrounded values. Existing CSVs and Save Now snapshots are not reformatted.

0.5.2（記録revision 7）から、main CSVの`t_ps`・`t_cor_ps`は**接続機器で確認した
実効ps/pulse**を基準に小数桁数を決めます。出力の最小桁は1パルスの1/10以下、
丸め誤差は1/20以下です。マイクロステップ・校正・光路の往復数を反映しますが、
装置の確度を保証する桁数ではありません。例えば1 µm/pulse・光路1往復では
約0.00667 ps/pulseとなり、`-49.501718235949715`は`-49.5017`と保存します。

`target_t_cor_ps`は入力した範囲・Stepの小数桁も維持します（mm入力はpsへ換算）。
装置の分解能より細かい指令点も区別し、測定座標の指令値はゼロの加減算をせず
そのまま使います。実効校正が未取得、または小数点以下15桁を超える精度が必要な
項目は、従来の最短往復表記で保存します。

丸め前の遅延値はraw CSVのreadingイベントの`details_json.unrounded_delay`へ
保持し、pulse・mm・通信応答も従来通り残します。実際の出力桁数とps/pulseは
run JSONの`context.delay_csv_format`で確認できます。計算・移動・API・プロットの
内部値は丸めません。過去CSVやSave Nowのコピーを再整形することもありません。

Other fields retain their previous precision: X/Y/R use shortest scientific text
that reconstructs the acquired finite f64 exactly, while spatial coordinates,
phase and host elapsed time use shortest Python-compatible round-trip text.
These stored digits are not a measurement-uncertainty estimate. CSV columns stay
unchanged.

その他の列の保存精度は維持します。X/Y/Rは取得f64を復元できる最短指数表記、
空間座標・位相・ホスト経過時間はPython互換の最短往復表記です。保存桁を測定の
不確かさと同一視しません。CSV列名は変更しません。

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


### LI5640 units and Windows selection dialogs / LI5640の単位・Windows選択画面

Normal LI5640 input is qualified by `ISRC?` (0/1: V; 2/3: A), with `NORM?=0` and `RAT?=0`. Current sensitivity uses `ISEN?` / `ISEN`: gain 10⁶ permits indices 4–26 (50 fA–1 µA), and gain 10⁸ permits indices 1–20 (5 fA–10 nA); voltage sensitivity uses `VSEN?` / `VSEN`. The GUI displays A/V with the corresponding metric prefix. An idle live unit is the last observed status (up to 500 ms cadence), not an atomic ADC-unit snapshot. A changed input mode suppresses stale sensitivity until fresh settings arrive. Historical line/heatmap traces use their own recorded unit and omit points in other units from that view; source CSV still retains every original row/unit.

LI5640は通常の電圧入力をV、電流入力をAと実照会で判定し、感度もVSEN／ISENに対応させます。電流利得10⁶は50 fA–1 µA（index 4–26）、10⁸は5 fA–10 nA（index 1–20）の範囲に制限します。ライブ単位は最大500 ms周期で確認した直近状態で、ADCと同時の照会ではありません。入力モードが変わった直後は古い感度を新しい単位で表示せず、設定再取得まで非表示にします。過去の線・heatmapは各記録の単位を維持し、異なる単位の点を同一単位で描きません。元CSVの全行・単位は保持します。

Windows Config **Browse** and Output **…** open native choosers with a centered topmost owner; the initial location falls back to the nearest existing folder for a not-yet-created output path. Literal brackets/apostrophes are preserved, filenames are separate from directories, PowerShell remains STA with no terminal window, and cancellation is distinct from failure. A Windows-only nonvisual test executes the actual PowerShell path helpers; interactive foreground behavior still needs a Windows desktop check.

WindowsのConfig **Browse**・Output **…** は中央・前面の所有ウィンドウを使う選択画面にし、保存先が未作成でも最寄りの既存フォルダーから開きます。角括弧・引用符をパスとして保持し、ファイル名とディレクトリを分離します。端末画面は出さず、キャンセルとエラーを区別します。Windows専用テストで実際のPowerShellパス処理を実行しますが、前面表示の実機確認とは区別します。

In 0.4.9, Windows Output uses the same native Explorer-style `rfd` folder picker as IV, titled **Output directory**, within the TRKR process. Config, snapshot-save and replay dialogs have explicit task-specific titles on all platforms. macOS and Windows preserve nearest-existing-folder fallback for not-yet-created paths; cancellation is logged and does not change the selected path. Config/file selection on Windows retains the hidden-console PowerShell implementation; native folder cancellation follows IV's picker behavior. Interactive Windows appearance remains unverified locally.

0.4.9ではWindowsのOutputをIVと同じ`rfd`のエクスプローラー形式・**Output directory**タイトルへ変更し、TRKRプロセス内で開きます。各OSのConfig・スナップショット保存・再表示ダイアログにも用途別タイトルを付けます。Mac・Windowsは未作成パスの最寄りの既存フォルダーから開き、キャンセルはログに表示して選択パスを変更しません。WindowsのConfig・ファイル選択は端末非表示PowerShell実装を維持し、フォルダー選択のキャンセルはIVと同じ扱いです。Windowsでの実画面確認はローカル未実施です。

Sources: [Newport AG-M100D datasheet](https://www.newport.com.cn/medias/sys_master/images/images/hcf/hf6/9122779496478/CONEX-AG-M100D-Data-Sheet.pdf), [NF LI5640 GPIB manual](https://www.nfcorp.co.jp/english/sup/dl/manual/mi/e_li5640-4.pdf).
