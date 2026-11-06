# KohdaLab TRKR 0.5.2

## Fix / 修正

Delay values in the analysis CSV no longer show long binary conversion tails when the connected stage calibration is known. `t_ps` and `t_cor_ps` use decimal places derived from the observed ps/pulse, including microstep and optical round trips, with one guard decimal. `target_t_cor_ps` also retains the requested range/Step precision. The change applies to every workflow that records a connected delay stage, including fixed-axis SRKR and Signal Monitor.

接続ステージの実効校正が分かる場合、解析CSVの`t_ps`・`t_cor_ps`に長い換算端数を出さず、マイクロステップと光路往復数を反映したps/pulseに予備1桁を加えて保存します。`target_t_cor_ps`は入力範囲・Stepの細かさも維持します。TRKRだけでなく、固定tを保存するSRKR・Signal Monitorなど全測定に適用します。

## Data preservation / データ保持

The run-start settings snapshot now retains lock-in reference phase, source and harmonic, distinct from the measured signal phase. Exact replies go to raw CSV and parsed settings to run JSON. These read-only queries run once at measurement start; periodic live refresh is unchanged. / 測定開始時に信号位相とは別の参照位相・参照源・高調波次数を記録します。生応答はraw CSV、解釈した設定はrun JSONへ保存します。開始時の読取り専用照会であり、周期ライブ更新は変更しません。

The raw CSV retains native readbacks and unrounded delay coordinates; run JSON records formatting parameters (recording revision 7). Unknown calibration and unsupported decimal precision retain round-trip output. This is an output-format policy, not an accuracy claim. Motion, calculations, API values, plots, signal columns and existing files are unchanged. Existing JSON profiles remain compatible.

raw CSVには装置の生読取と丸め前の遅延座標を残し、run JSONに出力桁数を記録します（recording revision 7）。校正不明・対応桁数を超える場合は最短往復表記を維持します。これは保存表記の方針であり、確度の保証ではありません。移動・計算・API値・プロット・信号列・既存ファイルは変更しません。既存JSONもそのまま利用できます。

## Validation and downloads / 検証・配布

Tests cover controller-scale-dependent formatting, fine commanded targets, all measurement workflows and preservation of unrounded raw data. Release CI verifies Windows/macOS/Linux and builds Windows Setup/portable ZIP and macOS DMG/ZIP. This patch has not been physically retested on instruments.

実効校正に応じた桁数、細かい指令値、各測定とraw値の保持をテストします。公開CIはWindows・Mac・Linuxを検証し、Windows Setup／portable ZIPとMac DMG／ZIPを作成します。今回の修正では実機を再検証していません。

Windows packages are unsigned. macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名・公証なしです。
