# Changelog

## 1.0.1

- The common Python SDK can stop a specified job and confirm its cleanup. / 共通Python SDKで測定を指定し、停止後の処理を確認できます。

## 1.0.0

### English

- Lock-in settings follow the connected model. LI5640 reports measured reference frequency in settings and the third field of raw `DOUT?` replies. Its main CSV still contains only the selected signal pair.
- SR830 voltage/current units are based on instrument readback. Display selection checks the analog-output source before changing the front panel. SR5210 current signals use amperes and automatic settings require a serial-poll-capable VISA helper.
- See [hardware verification](docs/validation/v1-hardware-results.md) for the tested connection paths.

### 日本語

- 接続機種に応じてロックインの設定を表示します。LI5640の参照周波数は設定情報とrawの `DOUT?` 応答の第3フィールドに記録されます。main CSVには引き続き選択したペアだけを記録します。
- SR830の電圧・電流単位を実機から判断し、表示切り替え前にアナログ出力の設定を確認します。SR5210の電流信号はAに換算し、自動調整にはシリアルポーリング対応のVISAヘルパーが必要です。
- 確認済みの接続経路は[実機検証表](docs/validation/v1-hardware-results.md)を参照してください。

## 0.5.11 — 2026-11-12

- Build line plot segments in one pass to reduce the cost of preparing long traces. / 折れ線グラフの点列を一度の走査で作り、長いトレースの描画準備を軽くしました。

## 0.5.10 — 2026-11-12

- Add device cards using the configuration of an existing card of the same type. / 同じ種類の既存カードの設定を使って、新しい機器カードを追加できます。
- Report existing measurement output files before opening instrument connections. / 測定の保存先に既存ファイルがある場合、機器へ接続する前に通知します。

## 0.5.9 — 2026-11-12

- Require both corresponding-source ZIPs to contain the complete expected file set and match the release tree. / 両OSの対応ソースZIPに必要なファイルがすべてあり、リリースのソースツリーと一致することを確認します。
- Require the latest matching Test and Documentation runs for both PR and manual builds, and record them in the release evidence. / PR・手動ビルドとも対象の最新TestとDocumentationの成功を必須とし、その結果をリリース証跡に記録します。
- Document how to recover when build artifacts are missing, expired, or split across attempts. / 配布物の欠落・失効や、異なる試行の成果物が混在した場合の復旧手順を明記しました。

## 0.5.8 — 2026-11-12

- Update `rustix` from 1.1.5 to 1.1.6 to address [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html). / `rustix` を1.1.5から1.1.6に更新し、RUSTSEC-2026-0486に対応します。
- Clarify the power-meter guide and retain the conclusions of the development tool evaluation. / パワーメータの手順を明確にし、開発ツール評価の結論を文書に残しました。
- Verify and promote CI-built packages to a draft release using their source tree and SHA-256 manifests. / CIで作成した配布物をソースツリーとSHA-256で検証し、リリースの下書きに昇格する手順を追加しました。

## 0.5.7 — 2026-11-10

- Save analysis CSV, raw CSV, lifecycle log and conditions JSON as a linked, hashed measurement bundle; retain old data and configuration compatibility. / 解析CSV・raw CSV・ライフサイクルlog・条件JSONを参照・ハッシュで結び、既存データ・設定の互換性を維持します。
- Clarify stored units, native versus corrected readings, timestamps and original-data references without changing measurement control. / 保存単位・生値と補正後値・時刻・元データ参照を明確化し、測定制御は維持します。
- Support installed Thorlabs TLPMX/TLPM drivers, limit Refresh discovery to the selected backend, and unify the power-meter live display with lock-in cards. / Thorlabs TLPMX/TLPMドライバーに対応し、Refreshを選択中の通信方式に限定。パワーメーターのライブ表示をロックインと統一します。
- New CSV headers are X_V/Y_V/R_V, timestamp_utc and sample_power_W. Recording revision is 9; replay accepts older headers and does not guess V for unitless legacy signals. / 新CSV列はX_V/Y_V/R_V・timestamp_utc・sample_power_W、記録revisionは9です。旧列名も再生可能で、単位不明をVと推測しません。

## 0.5.6 — 2026-11-10

- Align transport, motion and common control crates with the existing Rust 1.92 workspace requirement so normal dependency updates are not constrained by stale member MSRVs. / 通信・移動・共通制御クレートの最低Rust版を既存の1.92へ統一し、古い宣言による依存更新の制限を解消。
- Apply equivalent transport condition cleanups required by newly enabled Clippy lints; preserve dependencies, measurement behavior, settings and CSV formats. / 新たに有効となったClippyに合わせ通信部の条件式を等価に整理。依存版・測定動作・設定・CSV形式は維持。

## 0.5.5 — 2026-11-10

- Separate snapshot transactions, preserve alignment STOP failures and keep background processing independent of painting. / 途中保存の責務分離、Alignmentの停止失敗保持、描画と独立した背景処理。
- Fix stale Python heartbeat interference with ownership reacquisition; maintain GUI dependencies and improve input/alignment feedback. / 古いPython heartbeatによる制御権再取得の競合を修正し、GUI依存と入力・Alignment表示を改善。
- Validate extracted/installed Windows/Mac packages, five Demo workflows and GUI API; retain scientific formats and stop-policy defaults. / Windows/Mac配布版・5種類のDemo・GUI APIを検証し、測定形式と停止方針の既定値を維持。

## 0.5.4 — 2026-11-09

- Separate connection filtering, persisted inventory validation and batch connection orchestration; document their invariants. / 接続時の設定除外・保存設定検証・一括接続の責務を分離し、維持すべき条件を文書化。
- Avoid temporary device-ID copies during position polling and bound raw snapshot memory to CSV records instead of the full audit file. Preserve multiline replies and incomplete-tail filtering. / 位置照会の一時IDコピーを削減し、rawコピーのメモリ使用をファイル全体からCSV行単位へ変更。複数行応答・未完行の除外は維持。
- Release the power status mutex before callbacks, avoiding deadlock when notifications read status again. / パワー状態通知前にロックを解除し、通知先からの状態再取得による停止を防止。
- Fix chronological comparison of variable-precision timestamps in tests and isolate CI toolchain installations per job. GUI, measurement calculations, recording revision, numeric precision, API and stop policies are unchanged. / 時刻比較テストとCIツールチェーンの分離を修正。GUI・測定計算・記録revision・有効桁数・API・停止方針は維持。

## 0.5.3 — 2026-11-09

- Continue All Connect after an invalid device configuration or connection failure, retain successful connections, and report each skipped device. GUI and GUI API share this behavior. / All Connectで設定不良・接続失敗の機器を飛ばして残りを接続し、成功済みの接続を保持して失敗した機器を報告します。GUI・GUI APIで同じ動作です。
- Add optional Thorlabs PM100USB/PM100D power-meter cards, with wavelength and sample correction, a live card placed at the bottom by default, and an independent per-workflow assignment. A missing or failed selected meter leaves `power_W` blank, records `power_unavailable`, and does not stop the scan. Physical USB access is Windows x64 VISA USBTMC only; this is not a physical-hardware qualification. / Thorlabs PM100USB/PM100Dの任意パワーメータカード、波長・試料補正、既定で最下部のライブカード、測定ごとの独立割当を追加。選択したメータが未接続・読取失敗の場合は`power_W`を空欄、`power_unavailable`を記録し、scanは継続します。実機USBアクセスはWindows x64 VISA USBTMCのみで、実機適合の保証ではありません。
- Record corrected main-CSV power in four-significant-digit scientific notation while retaining unrounded power values and transport replies in raw CSV. Record the format policy in run JSON; provenance JSON rereads finite f64 values with round-trip precision. Recording revision is 8; profile and API revisions remain 1. / 補正済みmain CSVパワーは有効4桁の指数表記にし、raw CSVには丸め前のパワー値・通信応答を保持。run JSONへ表記規則を記録し、provenance JSONの有限f64はround-trip精度で再読込みします。記録revisionは8、profile・API revisionは1のままです。
- Restore the most recently successfully loaded or saved Config during normal startup, retain Demo separation and safe fallback, and display the tail of Config/Output paths without changing field width. / 通常起動時に直前に読込・保存に成功したConfigを復元し、Demoの分離・安全なフォールバックを維持。Config／Outputの入力幅を変えずにパス末尾を表示します。
- Fix Alignment Options scanner Direction selection for both normal (`0`) and reverse (`1`), share confirmed feedback with the Device editor, and accept controller aliases for Initialize. / Alignment OptionsのScanner Directionでnormal（`0`）・reverse（`1`）を正しく選択し、Device欄と確認済みの更新結果を共有。Initializeではcontroller aliasも受け付けます。

## 0.5.2 — 2026-11-06

- Format `t_ps` and `t_cor_ps` from the observed ps/pulse, including microsteps and optical round trips, with one guard decimal. Keep the requested range and Step precision for `target_t_cor_ps`. This applies to every workflow that records a connected delay stage. / 実効ps/pulse（マイクロステップ・光路往復数を含む）に予備1桁を加えて`t_ps`・`t_cor_ps`を表記し、`target_t_cor_ps`には入力範囲とStepの細かさも保持します。接続済み遅延ステージを記録する全測定が対象です。
- Read lock-in reference phase, source and harmonic once at run start, separately from signal phase. Save exact replies in raw CSV and parsed settings in run JSON. Periodic live refresh is unchanged. / 測定開始時に信号位相とは別の参照位相・参照源・高調波次数を一度読み、生応答をraw CSV、解釈した設定をrun JSONへ保存します。周期ライブ更新は従来どおりです。
- Retain native readbacks and unrounded delay coordinates in raw CSV, and formatting parameters in run JSON (recording revision 7). Keep round-trip output for unknown calibration or unsupported precision. The policy applies to saved formatting only. Motion, calculations, API values, plots, signal columns and existing files are unchanged. Existing JSON profiles remain compatible. / raw CSVに装置の生読取と丸め前の遅延座標、run JSONに出力桁数の設定を保持します（記録revision 7）。校正不明・対応桁数を超える場合は往復表記を維持します。保存表記だけを変更し、移動・計算・API値・プロット・信号列・既存ファイルは維持します。既存JSONもそのまま利用できます。

## 0.5.1 — 2026-11-06

- Keep Start and device controls available during automatic lock-in refresh. The same serialized worker keeps at most one automatic query queued or in flight. Queued refreshes yield to operator actions, and an in-flight query finishes first. Existing pending and ownership guards remain for explicit API queries, measurement, motion, connection changes and device selection. / ロックインの自動更新中もStartや装置操作を有効にします。同じ装置制御処理で自動照会を待機中・実行中合わせて最大1件とし、未実行の照会より手動操作を優先し、実行中の照会は完了を待ちます。明示的なAPI照会・測定・移動・接続変更・機器選択の操作待ちと権限制限は維持します。
- Keep JSON profiles, CSV formats, calibration, measurement timing and public API parameters unchanged. Existing 0.5.0 profiles load directly. / JSON設定・CSV形式・校正・測定タイミング・公開APIの引数を維持し、0.5.0の設定をそのまま読み込めます。

## 0.5.0 — 2026-11-05

- Connect multiple lock-ins, delay stages and paired X/Y scanners. Each measurement remembers its selection and highlights the selected devices without changing which cards are open. Rename, reorder, add or delete registered devices. The TOML actuator catalog supplies CONEX-CC with TRA12CC and CONEX-AGAP with AG-M100D. / 複数のロックイン・遅延ステージ・X/Y一組のスキャナを接続し、測定ごとの選択を記憶します。カードの開閉を変えずに対象を強調し、登録機器の名前変更・並べ替え・追加・削除に対応します。Actuator候補はTOMLから取得し、CONEX-CC＋TRA12CCとCONEX-AGAP＋AG-M100Dを選択できます。
- Add an independent Alignment tab with up to two Delay and four Scanner panels. Panel changes retain empty selections and unique device assignments, and Config saves the layout, unit and Step. Click to step, hold for bounded movement, and release to stop. Delay also supports end moves. Step is nonnegative, and zero disables motion. / 独立したAlignmentタブにDelay最大2枚・Scanner最大4枚を配置できます。パネルの変更では空欄と機器割当の重複防止を維持し、配置・単位・StepをConfigへ保存します。クリックでStep移動、長押しで制限内の連続移動、解放で停止します。Delayは端移動にも対応し、Stepは負数を受け付けず、0で移動を無効にします。
- Configure optical round trips and reverse the delay axis while preserving displayed coordinates. GUI, motion and saved calibration share the same conversion. / 光路往復数を設定し、表示座標を維持したまま遅延軸を反転できます。GUI・移動処理・保存する校正には同じ換算を適用します。
- Expose Alignment layout, Step, Jog, End, Initialize and Direction through the GUI-owned Python/common API. Jog requires keepalives and stops on expiry or owner release. Ordinary measurement stop policies remain unchanged. / GUI所有のPython・共通APIからパネル設定・Step・Jog・端移動・Initialize・Directionを操作できます。Jogは継続通知の失効または制御権の解放で停止し、通常測定の停止方針は維持します。
- Start the selected measurement with Ctrl+Enter when Start is available. Dialogs, Alignment, active jobs and Python ownership block the shortcut. / Startが有効な測定をCtrl+Enterで開始できます。ダイアログ表示中、Alignment、処理中、Python制御中は開始しません。
- Import existing JSON profile devices as first entries. Save explicitly to retain the new inventory and panel layout. Old files are not overwritten automatically. Measurement CSV retains the selected signal and coordinates, raw CSV retains native readbacks, and run metadata records device IDs and effective calibration. Update the Python client with the app. Each measurement uses one device of each required kind, with no simultaneous multi-lock-in acquisition. / 既存JSONの機器を各種類の1台目として読み込みます。新しい機器一覧・パネル配置は明示的に保存し、旧ファイルは自動上書きしません。測定CSVは選択した信号と座標、raw CSVは装置の生読取、run記録は機器IDと実効校正を保持します。Pythonクライアントはアプリと一緒に更新してください。1測定で使う機器は必要な種類ごとに1台で、複数ロックインの同時取得は含みません。

## 0.4.10 — 2026-11-05

- Ignore empty home-directory environment values and use the valid platform fallback consistently in the GUI and common Python client. / GUIと共通Pythonクライアントで、空のホーム環境変数を除外して有効な代替先を使います。
- Search only newly received bytes for the common API response delimiter, preserving existing size limits and error handling. / 共通APIの受信済みデータを繰り返し走査せず、新しい断片だけで区切りを検出します。サイズ制限とエラー処理は維持します。
- Save CSV snapshots in a background worker with pending/completion/error feedback; prevent normal window closure during the copy. / CSVスナップショット保存をバックグラウンド化し、保存中・完了・失敗を表示します。コピー中の通常終了を防ぎます。
- Preserve whitespace in selected paths and show full Config/Output paths on hover without changing field widths. / 選択パスの空白を保持し、入力欄の幅を変えずにConfig・Outputの全文をホバー表示します。

## 0.4.9

- Use consistent Config, Output, snapshot-save and replay dialog titles across platforms; report cancellation rather than leaving an opening notice. / 各OSのConfig・Output・スナップショット保存・再表示ダイアログのタイトルを統一し、キャンセル完了をログへ表示。
- Fall back to the nearest existing initial directory on macOS as on Windows when a nested output path has not yet been created. / Macでも未作成の入れ子パスを指定したときは最寄りの既存フォルダーから開きます。

- Match IV’s Windows Output folder picker: use the native Explorer-style dialog titled `Output directory`, with nearest-existing-folder fallback. / WindowsのOutput選択画面をIVと同じエクスプローラー形式・`Output directory`タイトルへ統一し、未作成パスは最寄りの既存フォルダーから開きます。

## 0.4.8

- Keep Windows Config/Output choosers foregrounded with an owned dialog and use a literal-safe existing initial folder; preserve paths with brackets/apostrophes and hidden console startup. / WindowsのConfig・Output選択画面に前面表示用の所有ウィンドウを設け、未作成・特殊文字を含むパスも扱える初期フォルダー処理に修正。
- Qualify LI5640 normal voltage/current units from ISRC/NORM/RAT readback, use VSEN/ISEN sensitivity accordingly, and preserve ambiguous normalized/ratio units as unknown. Compare unit context before/after each saved signal; flag/retain unknown and stop if it changes. Keep mixed-unit plot samples separate. / LI5640の電圧・電流単位と感度を実照会で判定。取得前後で単位設定が変わった点はunknownで保持して停止し、単位の異なるデータを同一単位として描画しません。
- Separate scanner arrival tolerance from resolution: CONEX-AGAP defaults to 0.01°; optional native-unit `position_tolerance` overrides it per axis. CONEX-CC defaults and motion quantization remain unchanged. / CONEX-AGAPの到達許容差を分解能から分離し既定0.01°へ変更。軸ごとの`position_tolerance`で指定でき、CONEX-CCの既定と移動指令の丸めは維持。

- Read only the selected XY or Rθ pair from LI5640; configure the display at connect/explicit mode change, never switch it per sample. Unacquired components remain blank in CSV and null in the API, with no reconstruction. / LI5640は選択した片側だけを取得し、接続・明示的な選択変更時のみ表示設定。未取得側はCSV空欄・API nullで計算補完しません。
- Expose native delay pulse/mm and scanner mm/deg values as explicit raw CSV rows without extra instrument queries. / 遅延ステージのpulse・mm、スキャナのmm・degをraw CSVの明示行へ保存。追加の実機照会は行いません。
- Report the scanner axis, commanded/read-back position, difference and configured tolerance when arrival verification fails; avoid false boundary failures caused solely by binary arithmetic roundoff. / 到達確認エラーに軸・装置指令値・読戻し値・差・設定許容差を表示し、境界の浮動小数点計算誤差だけで失敗しないよう修正。

## 0.4.7

- Open both side panels at startup, including on narrower displays; retain manual collapse controls and align the initial window width with the expanded layout. / 狭い画面でも起動時に左右パネルを表示し、手動の収納操作を維持。初期ウィンドウ幅を展開時のレイアウトに統一。

## 0.4.6

- Use the common app-owned directory layout: config/config.json, run, results and logs beneath ~/.kohdalab/trkr.
- Align normal startup, file selection, external API/Notebook discovery and documentation. Preserve operator-selected paths and all old files without automatic migration.
- Common control SDK 1.0.1 discovers app-owned run directories; wire protocol and instrument safety are unchanged.


## 0.4.5

- Automatically create the missing application-specific default configuration on first normal GUI launch.
- Preserve existing/corrupt profiles with atomic no-clobber creation and report initialization errors. Do not connect hardware or enable outputs.


## 0.4.4 — 2026-10-29

- Add the additive Control Protocol 1 GUI API, shared Python SDK, canonical discovery and verified receipt/ownership tests.

- Keep normal and Demo configuration in `~/.kohdalab/trkr/config.json` and `demo.json`; validate application identity and write settings atomically. Import older JSON explicitly with Browse → Load → Save. / 本番・Demo設定をTRKR専用フォルダーに分離し、アプリ識別と安全な保存を追加。旧JSONは明示的に読込・保存。
- Standardize Windows Setup to `C:\Program Files\Kohdalab\KohdaLab TRKR` (administrator installation), machine registry and shared shortcuts. Match notebook discovery to the installed location. / Windowsの配置先・レジストリ・ショートカット・Notebook検出を統一。
- Add explicit opt-in development transport for the separate shared instrument service, with no fallback to direct native/VISA access. The service is not bundled or hardware-qualified. / 別途導入する共有GPIBサービスの開発用接続に対応。通常接続への自動フォールバックは行わず、サービス同梱・実機適合確認は対象外。
- Keep existing measurement, Stop policies and CSV schemas unchanged. / 測定・停止条件・CSV schemaは維持。

## 0.4.3 — 2026-10-28

- Add GUI-owned Python automation with exclusive control, profile launch/attach, device operations and measurement receipts. / GUI所有のPython外部制御、排他制御、設定指定起動・既存GUI接続、装置操作と処理結果を追加。
- Keep continuation as the default; independently select disconnect, Python context exception and wait-timeout Stop conditions. Add confirmed `stop_and_wait()` and typed errors. / 継続を既定とし、通信断・Python例外・待機時間切れの停止条件、停止完了待ち、例外分類を追加。
- Bound concurrent network requests while preserving serialized device operations; keep CSV formats and existing device protection unchanged. / 通信受付を上限付き並行処理にし、装置操作の直列処理・CSV形式・既存保護を維持。

## 0.4.2 — 2026-10-23

- Include the 0.4.1 protection fixes with a GUI Skip HOME option for already referenced CONEX-CC axes. / 0.4.1の保護修正に、参照済みCONEX-CC向けのGUI「Skip HOME」を追加。
- The v0.4.1 tag build was cancelled before publication so the referenced-axis initialization path could be included. / 参照済み軸の初期化操作を揃えるため、v0.4.1のタグビルドは公開前に中止。

## 0.4.1 — 2026-10-23

- Stop motion before retiring a cancelled GPIB reply; keep a healthy session and never reuse its late response. Real transport/recovery failures still disconnect. / Stop時は軸の停止を優先し、中断した応答を回収・破棄して正常なGPIB接続を維持。通信・回復失敗は切断。
- Abort on acquisition status failure before further scan/return motion; update live overload/reference/error indicators. / 状態取得失敗時は次の走査・原点復帰へ進まず中止し、ライブ異常表示も更新。
- Verify compensation arrival, HOME transitions and actual stationary states after STOP; reject unsafe or unsupported HOME preconditions and block ordinary writes after serial faults. / 補償位置、HOME遷移、STOP後の停止を確認し、不正なHOMEと通信異常後の通常書込みを拒否。
- Keep the revision-5 three-file recording format, calibration and scan/return compensation distances unchanged. / 3ファイル保存形式・校正・走査と帰還の補償距離は維持。

## 0.4.0 — 2026-10-23

- Split new recordings into 29-column analysis CSV, multiline-safe raw CSV and run JSON, with UUID and SHA-256 associations. / 新規保存を29列main CSV・raw CSV・run JSONへ分離し、UUIDとSHA-256で対応付け。
- Keep controller units and exact valid text exchanges in raw; preserve fixed/rotated coordinates in main and correct raw-input target units. / コントローラー単位と有効な元応答をraw、固定・回転座標をmainに保存し、raw指定時の指令単位を修正。
- Snapshot queried start settings and resolved motion calibration; log point status, motion and file-write windows without recording idle live samples. / 開始時に照会した設定と実効移動校正を保存し、点の状態・移動・保存時間窓を記録。待機ライブ表示は保存対象外。
- Recording revision 5 / metadata 3 / trkr_analysis_v1 / raw_csv_v1. Old CSV/JSONL readers and safe Save copies remain supported. / 保存schema更新。旧形式の読込と安全なSaveコピーを維持。

## 0.3.7 — 2026-10-22

- Reconstruct every 2D replay grid from commanded targets when available, retaining actual readbacks and distinct fine steps. / 全2D再生を取得済みの指令値で復元し、実読取と微細なステップを保持。
- Timestamp CSV rows at host-observed signal-read completion; separate position, signal and flushed CSV-write windows in the audit. Recording revision 4, audit schema 2; CSV columns and profiles stay compatible. / CSV時刻をホストで観測した信号読取完了に修正し、位置・信号・CSV排出完了の時間窓を監査に分離。記録revision 4・監査schema 2。CSV列と設定は維持。

## 0.3.6 — 2026-10-22

- Save every available axis readback, including fixed delay/orthogonal axes in SRKR, with raw controller units. / SRKRの固定遅延・直交軸など、取得できる全軸の読取とコントローラー単位を保存。
- Distinguish commanded U/V targets from U/V calculated from actual X/Y readbacks and the measurement origin/rotation; unknown readings stay empty. / U/V指令値と、実際のX/Y読取・測定原点・角度から求めるU/Vを分離し、未取得値は空欄を維持。
- Preserve U/V replay line grouping using explicit targets even when readbacks drift. Recording revision 3 keeps CSV columns and profile/API revision 1. / 読取がずれても指令値でU/V再生の行構造を保持。記録revision 3でもCSV列、設定・API revision 1を維持。

## 0.3.5 — 2026-10-22

- Preserve acquired f64 values in X/Y/R CSV fields instead of fixed seven-digit rounding; keep CSV columns and settings compatible. / X/Y/Rの固定7桁丸めを廃止し、取得したf64値を保持。CSV列と設定は維持。
- Add raw-response JSONL provenance, explicit unknown resolution/uncertainty, failure records and consistent Save snapshots. / 生応答JSONL、分解能・不確かさの不明表記、失敗記録、整合する保存コピーを追加。
- Estimate acquisition ETA from observed point timing including motion and reads; retain unknown/returning states. / 移動・読取を含む実測点間時間からETAを算出し、未確定・復帰中を区別。

## 0.3.4 — 2026-10-21

- Make the fake-device homing progress test tolerate an additional time-driven position read on slow CI runners, and cover a delayed-ready response without changing production motion code.

- Preserve existing measurement, GUI, JSON, CSV and Notebook behavior while separating recording, sequencing, profile compatibility and tests into focused modules.
- Share workflow names, ordering, axis rules, output names and plot counts between core validation and the GUI.
- Centralize scanner capabilities, actuator defaults, lock-in setting options and auto commands; use one compensation plan for preflight and execution.
- Represent worker phases and fixed control/transport failures with internal types while preserving public strings and error precedence.
- Freeze unversioned profile/API/recording revision-1 contracts with additional fake-device and compatibility tests; document extension points.
- Add native demo GUI screenshots and supported instrument, stage/actuator preset and transport tables to the README. Keep documentation captures independent of operator profiles and paths.

## 0.3.3 — 2026-10-21

- Make manual-position polling verification deterministic under slow CI scheduling, including a delayed-ready case.
- Include the motion/live-display/configuration fixes listed below. Version 0.3.2 did not publish binaries because its timing-dependent test failed on CI.

## 0.3.2 — 2026-10-21

- Audit all 50 legacy runtime source files and index 219 legacy test functions; document remaining GUI, profile, plotting, maintenance and Notebook API gaps.
- Preserve singular `measurement` profile ranges/zeros and validate manual-move device references before connection.
- Enable configured spatial compensation for GUI/Notebook standalone Move and the post-initialization instrument-zero move; preflight the entire approach before positioning or homing.
- Restore return-to-zero hysteresis for enabled CONEX-CC scanners; use the common scan/return distance (100 µm default), preflight limits and preserve Stop cancellation.
- Compensate each 2D spatial fast-axis line before acquisition; omit redundant uncompensated line-start visits and never return to experiment zero between lines.
- Restore Cartesian 2D slow-before-fast motion, fast-before-slow completion returns and measurement-zero returns after raw-coordinate scans.
- Restore GUI initialization's calibrated instrument-zero move, with explicit action wording and opt-in native command compatibility.
- Accept legacy coordinate/output aliases; reject unsupported named-instrument selection before connecting instead of silently selecting a different device.
- Preserve legacy hardware aliases, device omissions and calibration values through GUI profile load/save; canonicalize known output aliases while retaining unknown metadata.
- Match frozen Python CSV common columns and numeric text; retain all scan timing in the additive `acquisition_elapsed_s` column.

- Retain the latest plot per SRKR axis and ordered STRKR/SRKR 2D axis pair; restore original calibration, progress and recorded CSV when switching back.

- Keep the XY/Rθ live monitor updating at approximately 100 ms during motion and scan settling, independently of recorded points.

- Show device-local Moving status and actual encoder positions during manual moves (approximately 100 ms updates).
- Make Initialize explicitly confirm mechanical homing for delay stages/CONEX-CC; CONEX-AGAP enables closed-loop operation.
- Report initialization completion, refresh delay calibration, and preserve cancellation/STOP on motion errors.

## 0.3.1 — 2026-10-19

- Automatically discover installed Windows/macOS applications from the Notebook client.
- Share executable/profile discovery across six notebooks; prefer GUI-saved profiles over source examples.
- Add `prepare_profile()` and path-free `Client()`, with UTF-8 native communication.
- Prefer the bundled client over an older imported client in portable/installed notebooks.
- Include the updated notebooks, client and migration guide in Setup, portable ZIP and Mac distributions.

## 0.3.0 — 2026-10-19

- Replace the Python GUI, drivers and measurement engine with the shared Rust application.
- Windows portable and per-user Setup with desktop shortcut; macOS native app and DMG.
- Preserve Signal Monitor, TRKR, SRKR, STRKR and SRKR 2D, JSON profiles and CSV schema.
- Live XY/Rθ monitor, TRKR log display, Python-compatible heatmaps and paired u/v validation.
- Device-local progress status, acquired/total Step, fixed sidebars and compact dark interface.
- Read-only Rust CSV demo and six Rust-backed Notebook examples.
- Source-built release CI, checksums, origin metadata and SignPath application preparation.

Earlier Python versions are preserved in Git tags and GitHub Releases.

## 日本語

### 0.3.4 — 2026-10-21

- 遅いCIでも原点復帰テストが時刻に基づく追加位置読取を許容するよう修正し、ready応答が遅れるケースを追加。製品の移動コードは変更しない。
- 測定・GUI・JSON・CSV・Notebookの挙動を保ち、記録、実行順序、設定互換、テストを目的別モジュールへ分割。
- 測定名、順序、軸制約、保存名、プロット数をcore検証とGUIで共有。
- スキャナ機能、アクチュエータ既定値、ロックイン選択肢とauto命令を集約し、同じ補償計画を事前検証と実行に使用。
- worker状態と固定の制御・通信エラーを内部型にし、公開文字列とエラー優先順位を維持。
- バージョンフィールドを持たない設定・API・記録のrevision 1を疑似機器・互換性テストで固定し、拡張箇所を文書化。
- DemoのGUI画像、対応機器・ステージ・アクチュエータ・通信表をREADMEに追加。撮影に操作者の設定やパスを使わない。

### 0.3.3 — 2026-10-21

- 遅いCIスケジュールでも手動位置読取の検証が決定的になるよう修正し、ready遅延ケースを追加。
- 下記の移動・ライブ表示・設定の修正を収録。0.3.2は時間依存テストがCIで失敗したため、バイナリ未公開。

### 0.3.2 — 2026-10-21

- 旧実行ソース全50ファイルを監査し、旧テスト219関数の索引と、GUI・設定・描画・保守・Notebook APIの残件を文書化。
- 単数形`measurement`の範囲と原点を保持し、手動Moveの機器参照を接続前に検証。
- GUI・Notebookの単独Moveと初期化後の機器原点移動に設定済み補償を適用。移動・原点復帰前に全接近経路を検証。
- 有効なCONEX-CCで原点復帰補償を復元し、共通距離（既定100 µm）、制限の事前確認、Stopを維持。
- 2Dの各空間fast行の開始を補償し、未補償の行始点への不要な移動と、行間の実験原点復帰を除外。
- 直交2Dのslow→fast移動、終了時fast→slow復帰、raw座標スキャン後の測定原点復帰を復元。
- GUI初期化後の校正済み機器原点への移動を復元し、操作を明示。低レベル命令の変更は明示選択式にする。
- 旧座標・保存先の別名を受け入れ、未対応の機器名指定を勝手な別機器選択に置き換えず接続前に拒否。
- GUIの設定読込・保存で旧機器別名、機器省略、校正値を保持し、既知の保存先別名を正規化。未知メタデータを保持。
- 固定Python参照の共通CSV列と数値表記を一致させ、`acquisition_elapsed_s`追加列に全測定時間を保持。
- SRKRの各軸、STRKR・SRKR 2Dの順序付き軸ペアごとに最新プロットを保持し、戻った際に校正・進捗・記録CSVを復元。
- 移動・待機中も記録点と独立してXY/Rθモニターを約100 msで更新。
- 手動Move中に機器欄のMovingと実際のエンコーダー位置を約100 msで更新。
- 遅延ステージ・CONEX-CCはInitializeで機械原点復帰を明示確認。CONEX-AGAPは閉ループを有効化。
- 初期化完了を通知し、遅延校正を更新。移動エラー時のキャンセルとSTOPを保持。

### 0.3.1 — 2026-10-19

- Notebookから導入済みWindows・macOSアプリを自動探索。
- 6つのNotebookで実行ファイル・設定探索を共通化し、ソース例よりGUI保存設定を優先。
- `prepare_profile()`、パス指定不要の`Client()`、UTF-8通信を追加。
- portable・導入済みNotebookで、古いimport済みクライアントより同梱版を優先。
- 更新Notebook、クライアント、移行ガイドをSetup・portable ZIP・Mac配布に同梱。

### 0.3.0 — 2026-10-19

- Python GUI・ドライバー・測定エンジンを共有Rustアプリへ置換。
- Windows portableとデスクトップショートカット付きユーザー単位Setup、macOSアプリとDMGを提供。
- Signal Monitor・TRKR・SRKR・STRKR・SRKR 2D、JSON設定とCSV形式を維持。
- XY/Rθライブ、TRKR log、Python互換ヒートマップ、u/v軸ペア検証を提供。
- 機器欄の進捗、取得数/総数Step、固定サイドパネル、コンパクトなダーク画面を提供。
- 読取専用のRust CSV Demoと、Rustを利用するNotebook 6本を追加。
- ソースからのリリースCI、チェックサム、由来情報、SignPath申請準備を追加。

以前のPython版はGitタグとGitHub Releasesに保持しています。
