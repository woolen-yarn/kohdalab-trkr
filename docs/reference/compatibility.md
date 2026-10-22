# KohdaLab TRKR native application

Version 0.3.0 replaces the Python GUI and measurement backend with Rust.
Python is optional and used only by the thin Notebook client. The external
USB-GPIB/VISA helper processes are C/C++ and disclosed in REDISTRIBUTION.md.

## Devices and transport

- Lock-ins: SR5210, SR7265, SR830 and LI5640; GPIB primary addresses 1–30.
- Delay controllers: SHOT302GS and GSC01/GSC01A.
- Scanners: CONEX-CC (including TRA12CC) and CONEX-AGAP.
- Windows uses a system 64-bit VISA runtime when it enumerates the selected
  resource. Enumeration does not scan GPIB instrument addresses. Helpers run
  without a console window. A failed VISA operation is never retried on USB-GPIB.
- The separate native helper supports NI USB-GPIB-HS+ and already initialized
  Agilent 82357B adapters. Authorized initialization and appropriate device
  access/drivers are prerequisites; no firmware or driver installer is bundled.
- On Mac, choose the actual `/dev/cu.*` serial ports in Device settings.

Connecting does not home or move axes. Set controller, actuator, scale,
software hysteresis, direction, origin and limits for the real apparatus.
Unreferenced axes cannot start a scan until the operator resolves their state.
Initialize opens an action dialog: delay stages and CONEX-CC use **Home & initialize**,
which changes the optical position; CONEX-AGAP enables closed-loop operation.
Both GUI actions then move to the calibrated **instrument zero**, as the Python
GUI did (delay-stage `t_ps=0`, scanner `x_um/y_um=0`, not the saved experiment zero).
Cancel leaves the apparatus untouched. Homing establishes the mechanical reference; the following zero move does not
change the saved measurement zero. Low-level JSON Initialize commands retain
`move_to_origin=false` unless explicitly requested. Manual Move shows Moving on the
affected device and refreshes actual coordinates approximately every 100 ms (subject
to serial response time); Stop cancels the operation.

Effective delay-stage step is read from its microstep division after connection;
before connection only an explicitly marked full-step estimate is shown.

## Measurement and interface

Config remains above a device-only floating-scroll area. The left chevron hides
or restores the setup panel. The right panel is 300 logical pixels wide, and
only the centre grows during window resizing. All tabs share the same widths.
Output and Run frames have 12-pixel outer and between-frame margins.

Device headers display `----` while disconnected; connected names carry relevant
Moving/Waiting/Reading status without replacing numeric readouts. Step displays
acquired/total, including when the display retains only a limited plot history.
TRKR's plot toolbar selects Linear or Log (positive values only); raw signals
and CSVs are unaffected. Heatmaps use the frozen 256-colour Python RdBu reference.

SRKR keeps the latest X/Y (or U/V) scans visible together. STRKR and SRKR 2D
retain the latest result for each ordered fast/slow axis pair; selecting an earlier
pair restores its plot, Step and recorded CSV for Save Now. Repeating a scan
replaces only that axis/pair. Plots use the zero and theta saved with each result;
U/V results are shown only when their saved theta matches the selected angle.
Changing theta does not accumulate unlimited histories: each axis/pair retains
its latest result. Clear plot clears retained results in that tab.

SRKR 2D permits only x/y or u/v pairs, with a single shared theta editor beside u.
STRKR pairs t with one of x/y/u/v in either order. Targets and return movements
are validated before acquisition. Settling deadlines, motion-before-read order
and per-point CSV flush are preserved. The XY/Rθ monitor reads fresh signals roughly
every 100 ms during motion and settling as well as while idle, subject to device
response time. Display-only reads never add CSV rows or advance Step; saved signals
are still read after settling. Near the end of a wait, the measurement read takes
priority over an additional display read. Stop can interrupt a pending live-read
response; an interrupted wire transaction disconnects the lock-in and requires
reconnection. For LI5640, an interrupted display-selection sequence also marks
the connection invalid and asks the operator to verify display/output settings.

## Read-only demo

Windows: run `kohdalab-trkr-demo.exe`, or drop one CSV onto it.
Mac: open KohdaLab TRKR Demo.app. Load a CSV, Connect All (virtual), then Start.
The playback loops until Stop; Wait=0 gives fast replay. It cannot open hardware,
move an instrument, record a new CSV or overwrite its source. Only synthetic
example files are distributed.

## Notebook and automation

The stdio API accepts one JSON command per line. See `bindings/python` for the
standard-library Client and `notebook/` for six simulation-first examples.
`--profile-json FILE` validates a profile and exports hardware/plans without I/O.
`--stdio` emits state, phase, sample, live, error and finished events. Advisory
phase events are nonblocking; live events never count as acquisition samples.
Run requests are serialized, and Stop may be sent from another Python thread.

## Validation and release

CI builds all application/helper binaries from checked-in source on GitHub-hosted
runners. Windows portable/Setup and Mac application ZIP/DMG include source and
licenses. SHA256SUMS and BUILD-ORIGIN files accompany each platform. Signing
preparation is described in `docs/development/windows-signing.md`.

Simulation, cancellation, CSV replay and read-only instrument checks were run
during migration. Motion of the laboratory's unreferenced X axis was deliberately
not initiated; full physical scan validation remains an apparatus commissioning task.
No real movement is needed for repository tests or release builds.


## Python compatibility audit — 2026-10-21

Reference: Python tag `v0.2.0`, commit `2ed1ed32ec141fd5eff49340aaad8a02b05e1ac0`.
The audit compared acquisition, range planning, device protocols, profile parsing,
CSV export, GUI initialization and the optional Notebook interface. Frozen
reference data in `tests/fixtures/python_measurement_reference.json` contains
15 range cases and six measurement-row cases generated from the archived pure
Python modules, including its actual `output_row` CSV formatter. Reference
values were kept unchanged while Rust was corrected.

| Area | Result |
| --- | --- |
| Scan ranges | Ascending/descending, nonaligned endpoints and nine-decimal rounding match the frozen Python cases. |
| Cartesian 2D order | Slow axis completes before the spatial fast-axis approach and scan. Each spatial fast-axis line now receives compensation as requested, extending the old first-point-only behavior. No experiment-zero return occurs between lines. Optional T-fast line reset is retained. Completion returns fast, then slow. |
| Completion approach | Enabled CONEX-CC compensation now applies to return-to-zero, including preflight of the entire approach. |
| Nonmeasurement scans | mm/degree/pulse input does not change the meaning of experiment-zero return. |
| Initialization | GUI references/enables the axis and moves to calibrated instrument zero; low-level no-home/no-origin requests remain nonmoving. |
| Profiles | Legacy output-directory/filename/suffix and coordinate-unit aliases are read with nested-value priority and device-unit checks. Hardware controller/scale/hysteresis aliases survive GUI load/save without duplicate fields or replacement by GUI defaults; unknown metadata is retained. Measurement-coordinate profiles do not require unused scanners. |
| CSV | Common Python columns, missing-value semantics and numeric text match frozen rows; additional U/V, theta and acquisition timing columns are Rust extensions. |
| Stop/error | Stops motion and does not automatically return or restart it, as in Python. Stop during an approach also prevents the subsequent zero move. |

For spatial scans and completion returns, the scanner's existing software-hysteresis
`enabled` and `direction` settings apply. A single
`software_hysteresis.distance_um` is shared by scan starts, each spatial fast-axis
line start, and enabled completion returns. Its default is **100 µm**; explicit
existing profile values are retained. For a negative approach, each line runs
`line_start - distance_um → line_start → scan`, and a completion return runs
`experiment_zero - distance_um → experiment_zero`. No experiment-zero return is
inserted between lines. Spatial fast axes do not first visit the uncompensated
line start before this approach. The slow axis is compensated on its initial
move, then follows the planned line positions. A U/V fast-axis approach remains
a coupled X/Y movement. CONEX-AGAP compensation remains excluded just as in Python.
All line approaches and completion returns are preflighted against travel limits.
The short-lived development-only `return_distance_um` option has been removed;
profile normalization drops it in favor of the common distance. Private profiles
and calibrated zeros are not rewritten by loading or testing.

Spatial entry-point audit: GUI standalone Move, Notebook `Client.move()` and
native Move commands with an omitted `approach` mask now request configured
X/Y compensation. An explicit native/Notebook mask remains available as a low-level
override. TRKR/SRKR/STRKR/SRKR 2D scan entries are covered for every supported
axis pair, including T-fast and U/V. Compensation occurs at scan entries and
spatial fast-axis line starts, not at every sample. Post-initialization movement
to instrument zero also requests compensation; its entire approach is validated
before homing begins. Mechanical homing itself remains the controller's dedicated
operation. Scanner compensation still requires the profile's enabled CONEX-CC
configuration; disabled scanners and CONEX-AGAP remain unaffected.

Remaining explicit interface differences:

- The portable GUI exposes one lock-in (`main`), one stage (`t`) and X/Y scanners.
  Per-measurement arbitrary device names/multiple-device selection are not yet
  supported. Such profiles are rejected before connection; no first-map-entry
  fallback is used. This limitation is now detected instead of silently ignored.
- Notebook access is through `kohdalab_trkr.Client` and the native JSON protocol,
  not a drop-in replacement for the old `kohdalab.Experiment` imports/callback API.
  Native `run(output=None)` intentionally creates no CSV; pass an output path to save.
- U/V maps are a Rust extension. Their two physical X/Y targets remain coupled;
  Cartesian scan ordering is not applied as two independent motor moves.
- A failed GPIB transport is invalidated rather than retried with other lock-in
  read commands. LI5640 requires a complete saved display/output state before
  temporarily selecting channels. These retain restoration and transport safety.
- Delay position parsing retains validated comma-separated controller replies.
  Undocumented variants accepted by the old permissive regex have not been
  claimed compatible without captured responses. No hardware motion was used
  for this audit; physical motion/limit behavior still needs apparatus validation.


## 全ソース再監査 — 2026-10-21

**結論：現時点でPython版の完全互換には達していない。** 主要測定の移植と、
旧API・設定・GUIの全機能の互換性は別である。以下の残件があるため、
「移植漏れなし」「実機の全動作を確認済み」とは扱わない。

対象は `v0.2.0` の `src/kohdalab` にある全50ファイル（Python 48、TOML 2）、
同梱Notebook 6本、旧テスト35ファイル・219テスト関数の項目索引。
[全ファイル・関数索引](../validation/python-compatibility-inventory.json) に旧ファイルのSHA256、
クラス／関数名と行番号、Rustの対応先、分類を保存した。索引は関数ごとの
動作同一性を自動証明するものではない。旧テスト219本をRustに対してそのまま
実行したという意味でもない。現在の疑似機器テスト・固定参照データ・ソース照合で
確認できる範囲と、実機が必要な範囲を区別する。

### 今回新たに修正した設定の漏れ

| 項目 | 再現と対応 |
| --- | --- |
| 単数形 `measurement` | Pythonは受け付けるがRustは無視していた。合成設定で範囲17→23、step 3、zero=[123,45,67]が、範囲−50→300、step 5、zero=[0,0,0]へ変わることを実行確認。読込時にcanonical `measurements`へ移行するよう修正。両方ある場合はcanonicalを優先し、不正型は拒否する。 |
| 手動Moveの機器参照 | 機器選択の検証が測定5モードのみで、`move_abs`が漏れていた。`delay_stage_key`／`scanner_keys`等で別名の機器を選んでもcanonical機器へ進む可能性があった。手動Moveも検証し、未対応の選択は接続前に拒否する。任意名の複数機器を扱う機能そのものは未移植。 |

### 未移植・未互換の残件

| 優先度 | 機能 | 旧Pythonの根拠／現在のRustの状態 |
| --- | --- | --- |
| 高 | 旧Advanced／さらに古いJSONの取り込み | `apps/trkr_gui_config.py::_extract_legacy_config`は`lockin`、`xy_scanner`、`scanner1/2`、`delayline`等の旧root形式を扱う。Rustはcanonical `instruments`形式のみ。Advancedの`srkr.zero`、`x/y.minimum/maximum`、`corrected_origin/offset/center`、mm表記のzero等のfallbackも移植されていない。未知項目を保持しても実行に使う保証にはならない。 |
| 高 | 任意名・複数機器と測定ごとの選択 | `api/session.py`、`device_requirements.py`は名前付き機器registry、ref alias、各測定の機器選択を持つ。Rustはmain/t/x/yの固定構成。未対応の明示選択はエラーにするが、同等のregistry機能はない。 |
| 高 | GUIの原点復帰ON/OFF | 旧通常／Advanced GUIのTRKR `Return to t zero`、SRKR `Return to origin` checkboxがRust GUIにない。JSON／native Planの`return_to_zero`は実装済みで読込・保存・実行に使うが、GUIでは変更できない。 |
| 高 | 大きな測定のプロット保持 | Rust `ui.rs::MAX_TRACE=50_000`を超えると表示用traceの前半を捨てる。CSVは残るが、長い1Dや大きな2Dの画面・履歴が全測定点を保持するとはいえない。Python GUIは測定rowsを保持していた。 |
| 中 | GUIでの座標系切替 | 旧Advanced GUIにはMove/TRKR/SRKRのmeasurement/interface/instrument切替がある。RustのPlan／JSONは1Dのraw座標を扱うが、同等のGUI切替はない。raw設定を読み込んだ際のGUI単位表記も専用の受入確認が必要。 |
| 中 | 旧NotebookのAPI | `kohdalab.api.Experiment`、`DeviceSession`、typed Position/MeasurementPoint、`on_point`／`on_status`／`should_continue`、`required_devices`／`missing_devices`、`auto_connect=False`等は新Clientの同名APIとして存在しない。Rustのイベント／Stopで代替できるものもあるが、旧Notebookをそのまま実行できるわけではない。 |
| 中 | 非等間隔の点列 | 旧 `run_trkr/run_srkr(scan_points=..., target_points=...)`は任意の点列を指定できる。RustのPlanはmin/max/stepによる等間隔範囲であり、この入口がない。 |
| 中 | Notebookのライブプロットhelper | 旧 `api/notebook.py::make_*_live_update`とtyped point callbackは未移植。同梱新Notebookはスレッド実行と結果プロットを備えるが、このhelperとの互換性はない。Clientの生イベントを購読するコードは別途書ける。 |
| 中 | ステージの保守API | SHOT302/GSC01の`execute_drive`、`set_logical_zero`、`set_excitation`、`jog`、速度・加速度の`set_speed`、`query_internal/write_internal`、`query_sensor_status`、デバッグ通信がnative公開APIにない。通常の絶対移動・home・stopとは別。 |
| 中 | GSC01Aの緊急停止API | 旧`gsc01a.py::immediate_stop()`の`L:E`と、通常の軸停止`L:{axis}`は別物。Rustは通常Stopを実装するが、独立したimmediate_stop命令を公開していない。 |
| 中 | 相対移動・プリセット列挙API | 旧driver/interfaceの相対移動や`list_stages/list_actuators`と同等の公開入口がない。内部に校正・presetはあり、現在位置＋絶対移動で相当操作は可能だが、旧API互換ではない。 |
| 中 | 旧CLIと診断GUI | 旧`kohdalab-cli --config ... trkr/srkr/.../move-abs`と独立した`trkr_gui_test.py`診断ウィンドウは未移植。native stdio／本体GUIで一部代替できるが、旧コマンド・画面との互換はない。 |
| 中 | 設定探索・前回パスの記憶 | 旧`KOHDALAB_CONFIG`、`KOHDALAB_DEFAULT_CONFIG`、`KOHDALAB_STATE_DIR`、`KOHDALAB_LAST_CONFIG_STATE_PATH`と`last_config.json`の探索は同じではない。新Clientは`TRKR_PROFILE/TRKR_BINARY`、GUIは専用ユーザー設定パスを使う。 |
| 中 | プロット画像Exportと標準操作 | 旧pyqtgraphの標準context menuによるExportや個別表示設定に相当するUIが不足。Rustはzoom/pan、TRKR log、Clear、CSV再生を備えるが、ユーザー向け画像Exportはない。旧ライブラリの暗黙機能と、旧リポジトリで明示実装した機能は区別する。 |
| 中 | Stage direction=-1 | 旧通常GUIは−1..1を入力可能、変換は0以外を同じ符号として扱う。Rustは0/1のみ受け付け、−1設定は検証エラーになる。合成profileで拒否を実行確認。勝手な符号変更はしていない。 |
| 中 | CSVの厳密な同一性 | 新CSVは追加列と固定headerを持ち、時刻はUTC RFC3339（旧はlocal naive ISO millisecond）。Signal Monitorの旧`elapsed_s`は予定時刻、新は実測経過時間。1Dのzero未指定時も旧は補正値を空欄にできるが、新Planは数値zero=0を前提にする。既存の固定6行比較だけではこれらを網羅しない。 |
| 中 | 長い範囲の点数 | 旧範囲生成はfloatの逐次加算、新はindex×step。例えば−30→30, step=.001では旧60000点、新式60001点。旧0→100, step=.001は100000点だが、新式100001点はRustの軸上限を超えて拒否する。15個の既存固定ケースが一致しても全範囲の同一性は保証しない。旧の丸め誤差を再現するかは別途決定する。 |
| 中 | native Replay命令 | GUIのReplay CSVは専用Demoを開き、`replay_data.rs`を使う。一方、stdioのnative `engine.rs::replay`には別parserが残り、Y-onlyのfast列、2D line、U/V等の復元能力が異なる。これはPython由来機能とは別のRust内の残件として統一が必要。 |
| 低 | Read Live・共有ポート・actuator候補UI | 手動Read Liveは自動更新に置換。共有XYポートtoggleはないが、同じportを設定するとMotionRigは共通serialを使うため、共有通信自体が未実装という意味ではない。Actuatorは候補ComboBoxから自由入力へ変わり、入力支援が減っている。 |

### 意図的な変更と確認範囲

- Portableの接続対象はユーザー指定どおりGPIBベース。従来VISA任意resourceとの完全互換ではない。
- 機器設定・単位換算・通常測定で使う操作は、ロックイン4機種（SR830/SR7265/SR5210/LI5640）、
  delay controller 3種（SHOT302GS/GSC01/GSC01A）、scanner 2種（CONEX-CC/CONEX-AGAP）で照合した。
  これは全保守APIの公開や、全機種の実機検証を意味しない。
- Stage 4 preset／actuator 2 presetの共通校正値は旧TOMLと一致。
  テキストhashはコメント・書式等で異なり、stageの旧`pos_unit="pulse"`は現assetでは省略される。
- 各行の空間fast補償、U/V、Moving中の読取、100 msライブ更新、キャンセル時の通信無効化などは
  新仕様。原点復帰と補償は共通距離を使い、行間の実験原点復帰はしない。
- CSV同名上書きは旧Pythonの上書きから新Rustの拒否へ変更。`.run.json`は追加情報。
  新native `run(output=None)`が自動保存しないことも旧runの既定保存と異なる。
- 途中停止は後続の原点復帰を始めない。範囲検証、GPIB異常時の無効化、LI5640の保存復元に
  関する厳格化は維持する。
- 実機の移動・原点復帰・センサー・travel limit・各機種の実応答は今回検証していない。
  コード照合と疑似機器の成功を、そのまま装置の動作保証にはしない。

監査JSON内の旧 `next/` パスは監査時点の記録です。[現行レイアウトへの対応表](../validation/compatibility-audit-layout.md)を参照してください。

## 日本語 — アプリ・測定・互換性監査

### ネイティブアプリと通信

0.3.0でPython GUIと測定バックエンドをRustへ置き換えました。Pythonは任意の薄いNotebookクライアントだけに使います。外部USB-GPIB/VISA helperはC/C++で、REDISTRIBUTION.mdに記載しています。

- ロックイン：SR5210・SR7265・SR830・LI5640。GPIB一次アドレス1–30。
- 遅延コントローラー：SHOT302GS・GSC01/GSC01A。
- スキャナ：CONEX-CC（TRA12CCを含む）・CONEX-AGAP。
- Windowsでは選択リソースが列挙されればシステム64-bit VISAを使います。列挙はGPIB機器アドレスの走査ではありません。helperはコンソールを表示しません。VISAで失敗した操作をUSB-GPIBで再試行しません。
- 独立helperはNI USB-GPIB-HS+と初期化済みAgilent 82357Bに対応します。許可された初期化と適切なアクセス・ドライバーが必要です。ファームウェアやドライバー導入プログラムは同梱しません。
- MacではDevice設定で実際の`/dev/cu.*`を選びます。

接続だけでは原点復帰・移動しません。実機に合わせてコントローラー、アクチュエータ、換算、ソフトウェアヒステリシス、方向、原点、制限を設定します。基準位置が確立されていない軸は、操作者が状態を解決するまでスキャンを開始できません。

Initializeは操作ダイアログを開きます。遅延ステージとCONEX-CCは**Home & initialize**で機械原点復帰し、光学位置が変わります。CONEX-AGAPは閉ループを有効にします。その後どちらもPython GUIと同様、校正済み**機器原点**（遅延`t_ps=0`、スキャナ`x_um/y_um=0`）へ移動し、保存済みの実験原点には移動しません。Cancelでは機器を変更しません。機械原点復帰で基準を確立した後のゼロ移動は、保存済み測定原点を変更しません。低レベルJSON Initializeの`move_to_origin`は、明示指定がなければ`false`です。手動Moveは対象機器にMovingを表示し、実座標を約100 msごとに更新します（シリアル応答時間に依存）。Stopで中断できます。

接続後はmicrostep分割から実効遅延ステップを読み取ります。接続前はfull-stepの推定値であると明示して表示します。

### 測定と画面

Configは機器欄のスクロール領域の上に固定します。左の矢印で設定パネルを収納・復元できます。右パネルは300論理pixelで固定し、ウィンドウを広げた際は中央だけが伸びます。タブ間で横幅は変わりません。Output・Run枠の外側と枠間は12 pixelです。

未接続の機器見出しは`----`です。接続名のそばにMoving/Waiting/Readingを表示し、数値を置換しません。Stepは表示履歴の点数ではなく取得数/総数です。TRKRのツールバーでLinear・Log（正値のみ）を切り替えます。生信号とCSVは変わりません。ヒートマップは固定のPython RdBu 256色参照を使います。

SRKRは最新X/YまたはU/Vを同時保持します。STRKRとSRKR 2Dは順序付きfast/slowペアごとに最新結果を保持し、過去のペアを選ぶとプロット、Step、Save Now用の記録CSVを復元します。再測定で置き換わるのは対象軸・ペアだけです。各結果の原点とthetaを使い、U/Vは保存thetaが選択角度と一致するときだけ表示します。theta変更で履歴を無制限に増やしません。Clear plotはそのタブの保持結果を消します。

SRKR 2Dはx/yまたはu/vのみで、thetaはuの横に1つです。STRKRはtとx/y/u/vのいずれかを両順序で組み合わせます。取得前に目標と復帰経路を検証します。待機期限、移動後の取得、点ごとのCSV flushを維持します。XY/Rθモニターは、待機・移動・アイドルのいずれでも機器応答に応じて約100 msごとに新規取得します。表示用読取はCSVやStepに加算しません。保存用信号は待機後に取得し、待機終了間際は追加表示読取より測定を優先します。Stopは進行中のライブ応答を中断でき、中断した通信はロックイン接続を無効化して再接続を要求します。LI5640の表示選択途中の中断では、表示・出力設定の確認も要求します。

### 読取専用Demo

Windowsは`kohdalab-trkr-demo.exe`を起動するか、CSVを1つドロップします。MacはKohdaLab TRKR Demo.appを開きます。CSVを読み込み、Connect All（仮想）、Startを押します。Stopまで繰り返し、Wait=0で高速再生します。実機接続、移動、新規CSV記録、元CSV上書きはできません。配布例は合成データだけです。

### Notebook・自動化

stdio APIは1行1JSON命令です。標準ライブラリーだけのClientは`bindings/python`、シミュレーションを先に行う6つの例は`notebook/`にあります。`--profile-json FILE`はI/Oなしで設定を検証し機器と計画を出力します。`--stdio`はstate・phase・sample・live・error・finishedイベントを出力します。通知用phaseは非同期でブロックせず、liveは取得点に数えません。runは直列化され、別PythonスレッドからStopを送れます。

### 検証と配布

CIはGitHub runner上で管理ソースからアプリとhelperをビルドします。Windows portable・Setup、Mac ZIP・DMGにはソースとライセンスを同梱し、各環境にSHA256SUMSとBUILD-ORIGINを付けます。署名準備は`docs/development/windows-signing.md`を参照してください。

移行中にシミュレーション、キャンセル、CSV再生、読取専用実機確認を行いました。基準未確立の実験室X軸は意図的に移動させていません。完全な実機スキャン検証は装置立上げ時に行う必要があります。リポジトリのテスト・配布ビルドに実機移動は不要です。

### Python互換性監査 — 2026-10-21

参照はPythonタグ`v0.2.0`、コミット`2ed1ed32ec141fd5eff49340aaad8a02b05e1ac0`です。取得、範囲計画、機器通信、設定解析、CSV、GUI初期化、任意のNotebook連携を比較しました。`tests/fixtures/python_measurement_reference.json`は、保存した純Pythonモジュールと実際の`output_row`から生成した範囲15ケースと測定行6ケースです。参照値を変えずRust側を修正しました。

| 項目 | 結果 |
| --- | --- |
| 範囲 | 昇順・降順、端点がstepに一致しない場合、9桁丸めが固定Pythonケースと一致。 |
| 直交2D順序 | slow移動完了後に空間fastの補償とスキャンを行う。各行を補償し、旧版の最初の点だけの挙動を拡張。行間の実験原点復帰はせず、任意のT-fast行リセットは保持。終了時はfast→slowで復帰。 |
| 終了接近 | 有効なCONEX-CC補償を原点復帰にも適用し、全接近経路を事前検証。 |
| 測定座標以外 | mm・degree・pulse入力でも実験原点復帰の意味を変えない。 |
| 初期化 | GUIは基準確立・有効化後に校正済み機器原点へ移動。低レベルのno-home・no-origin指定では移動しない。 |
| 設定 | 旧保存先・名前・suffix・単位別名をnested優先と機器単位確認で読み込む。機器・換算・補償別名をGUIの読込・保存で保持し、重複やGUI既定値への置換をしない。未知メタデータを保持し、測定座標設定で未使用スキャナを必須にしない。 |
| CSV | 共通列、欠損値、数値表記が固定行と一致。U/V・theta・取得時間はRustの追加列。 |
| Stop・エラー | Python同様に移動を停止し、自動復帰・再開しない。接近途中のStopでは後続ゼロ移動もしない。 |

空間スキャンと終了復帰は、既存の`enabled`・`direction`に従います。`software_hysteresis.distance_um`1つをスキャン開始、空間fastの各行開始、許可された終了復帰に共用します。既定は**100 µm**で、既存設定の明示値は維持します。負方向接近は`line_start - distance_um → line_start → scan`、復帰は`experiment_zero - distance_um → experiment_zero`です。行間で実験原点には戻らず、補償前に未補償の行始点へ寄りません。slowは最初の移動を補償し、以後は行位置に従います。U/Vのfast接近はX/Y連動です。Python同様CONEX-AGAPは補償対象外です。全行の接近・復帰でtravel limitを事前確認します。開発中だけの`return_distance_um`は除去し、設定正規化で共通距離に集約します。私有設定や校正原点は読込・テストで書き換えません。

GUI単独Move、Notebookの`Client.move()`、`approach`省略のnative Moveは設定済みX/Y補償を要求します。明示マスクは低レベルの上書きとして残します。TRKR・SRKR・STRKR・SRKR 2Dの全対応ペア（T-fast・U/Vを含む）の開始を確認しています。補償はスキャン・空間fast行の開始時で、各サンプルではありません。初期化後の機器原点移動も補償を要求し、機械原点復帰前に全接近経路を検証します。機械原点復帰自体は専用操作です。補償は有効なCONEX-CC設定が前提で、無効スキャナとCONEX-AGAPは変わりません。

### 明示的なインターフェース差

- GUIはロックイン`main`、ステージ`t`、X/Yスキャナの固定構成です。測定ごとの任意名・複数機器選択は未対応で、該当設定は接続前に拒否し、最初の要素へ勝手に切り替えません。
- Notebookは`kohdalab_trkr.Client`とJSON経由です。旧`kohdalab.Experiment`・callback APIの差替え互換ではありません。`run(output=None)`では意図的にCSVを作らず、保存には出力パスが必要です。
- U/VはRust拡張でX/Yが連動し、直交2Dの独立したモーター移動順序は適用しません。
- GPIB失敗を別の読取命令で再試行せず接続無効とします。LI5640は表示を一時選択する前に全表示・出力状態の保存を必要とし、復元と通信安全性を保持します。
- 遅延位置は検証済みのカンマ区切り応答を解析します。旧正規表現が許容した未文書化形式は、実応答なしで互換とは主張しません。監査で実機移動は行っておらず、移動・制限は装置ごとの検証が必要です。

## English — full-source re-audit, 2026-10-21

**Conclusion: complete compatibility with the Python version has not been reached.** Porting the main measurements is distinct from compatibility with every legacy API, profile, and GUI feature. The remaining items below prevent claims of “no missing ports” or “all hardware behavior verified.”

Scope: all 50 files under `src/kohdalab` at `v0.2.0` (48 Python and 2 TOML), six notebooks, and an index of 35 legacy test files / 219 test functions. The [file/function inventory](../validation/python-compatibility-inventory.json) records Python SHA256 hashes, classes/functions and lines, Rust replacements, and classifications. It is not an automated proof of function-level equivalence, nor evidence that all 219 old tests were run unchanged against Rust. Distinguish current fake-device tests, frozen references, and source comparisons from checks requiring hardware.

### Newly corrected profile omissions

| Item | Reproduction and correction |
| --- | --- |
| Singular `measurement` | Python accepted this, but Rust ignored it. A synthetic profile with range17→23, step3 and zero=[123,45,67] became −50→300, step5 and zero=[0,0,0]. Loading now migrates it to canonical `measurements`; canonical wins if both exist, and malformed types are rejected. |
| Manual Move device references | Device selection had been checked only for the five measurements, omitting `move_abs`. Alternate `delay_stage_key` / `scanner_keys` could silently use canonical devices. Manual Move now validates these and rejects unsupported selections before connection. An arbitrary-name/multiple-device registry itself is not ported. |

### Remaining unported or incompatible items

| Priority | Feature | Legacy evidence / current Rust behavior |
| --- | --- | --- |
| High | Old Advanced and older JSON import | `apps/trkr_gui_config.py::_extract_legacy_config` accepts root-level `lockin`, `xy_scanner`, `scanner1/2`, `delayline`, etc. Rust uses canonical `instruments`. Advanced fallbacks such as `srkr.zero`, `x/y.minimum/maximum`, `corrected_origin/offset/center`, and mm-based zero are not ported. Preserving unknown keys does not guarantee execution uses them. |
| High | Arbitrary names, multiple devices, per-measurement selection | `api/session.py` and `device_requirements.py` provide named registries, reference aliases, and per-measurement selection. Rust fixes main/t/x/y. Unsupported explicit selections error rather than silently choosing another device; there is no equivalent registry. |
| High | GUI return-to-zero toggles | Legacy standard/Advanced GUI had TRKR `Return to t zero` and SRKR `Return to origin` checkboxes. Rust GUI lacks them. JSON/native Plan `return_to_zero` is loaded, saved, and executed but cannot be edited in the GUI. |
| High | Large-run plot retention | After `ui.rs::MAX_TRACE=50_000`, Rust drops early display points. CSV remains, but long 1D/large 2D plots and retained histories do not retain every acquired point. Python retained measurement rows. |
| Medium | GUI coordinate-system switching | Advanced GUI supported measurement/interface/instrument for Move/TRKR/SRKR. Rust Plan/JSON handles 1D raw coordinates without the equivalent GUI switch. GUI unit labels for loaded raw profiles need dedicated acceptance checks. |
| Medium | Old Notebook API | `kohdalab.api.Experiment`, `DeviceSession`, typed Position/MeasurementPoint, `on_point`/`on_status`/`should_continue`, `required_devices`/`missing_devices`, and `auto_connect=False` do not exist as the same Client API. Events/Stop replace some uses, but old notebooks do not run unchanged. |
| Medium | Nonuniform points | Legacy `run_trkr/run_srkr(scan_points=..., target_points=...)` accepts arbitrary point sequences. Rust Plan provides uniform min/max/step ranges without that entry point. |
| Medium | Notebook live-plot helpers | `api/notebook.py::make_*_live_update` and typed point callbacks are not ported. New notebooks provide background threads and result plots, without compatibility with those helpers. Raw Client event subscriptions can be implemented separately. |
| Medium | Stage maintenance API | SHOT302/GSC01 `execute_drive`, `set_logical_zero`, `set_excitation`, `jog`, speed/acceleration `set_speed`, `query_internal/write_internal`, `query_sensor_status`, and debug communication lack native public APIs. They are distinct from normal absolute move/home/stop. |
| Medium | GSC01A emergency-stop API | Old `gsc01a.py::immediate_stop()` uses `L:E`, distinct from normal `L:{axis}`. Rust implements normal Stop without a separate immediate_stop command. |
| Medium | Relative moves and preset enumeration | No public equivalent of legacy driver/interface relative moves or `list_stages/list_actuators`. Calibration/presets exist internally, and current position plus absolute move can approximate the operation, but this is not old-API compatibility. |
| Medium | Old CLI and diagnostic GUI | `kohdalab-cli --config ... trkr/srkr/.../move-abs` and separate `trkr_gui_test.py` are not ported. Native stdio and the main GUI replace some functions without command/window compatibility. |
| Medium | Profile search and last-path state | Search for `KOHDALAB_CONFIG`, `KOHDALAB_DEFAULT_CONFIG`, `KOHDALAB_STATE_DIR`, `KOHDALAB_LAST_CONFIG_STATE_PATH`, and `last_config.json` differs. Client uses `TRKR_PROFILE/TRKR_BINARY`; GUI uses its dedicated user settings path. |
| Medium | Plot-image export and standard controls | Equivalent pyqtgraph context-menu Export and per-plot settings are missing. Rust supports zoom/pan, TRKR log, Clear, CSV replay but no user-facing image export. Distinguish library-provided features from explicitly implemented legacy features. |
| Medium | Stage direction=-1 | Legacy standard GUI accepts −1..1; conversion treats nonzero values with the same sign. Rust accepts only0/1 and rejects −1. Rejection was reproduced on a synthetic profile; no silent sign change was made. |
| Medium | Exact CSV identity | New CSV adds columns/fixed header and uses UTC RFC3339 timestamps rather than local naive ISO milliseconds. Signal Monitor `elapsed_s` was scheduled time, now actual elapsed time. Old 1D with omitted zero could leave corrected fields empty; new Plan assumes numeric zero=0. Six frozen rows do not cover these differences. |
| Medium | Point counts in long ranges | Python accumulates floats; Rust uses index×step. −30→30 at .001 gives60000 old versus60001 new points. Old0→100 at .001 gives100000; new100001 exceeds Rust's axis limit and is rejected. Matching15 frozen cases does not prove all ranges identical. Whether to reproduce legacy rounding error is a separate decision. |
| Medium | Native Replay command | GUI Replay CSV opens Demo using `replay_data.rs`. Stdio `engine.rs::replay` has a separate parser differing for Y-only fast columns, 2D lines, U/V, etc. Unifying these is a Rust-internal issue, distinct from Python parity. |
| Low | Read Live, shared-port and actuator UI | Automatic updates replace manual Read Live. No shared-XY-port toggle exists, but MotionRig shares serial communication when ports match. Actuator candidates changed from ComboBox to free text, reducing input assistance. |

### Intentional changes and validation scope

- Portable connections are GPIB-based as requested, not fully compatible with arbitrary old VISA resources.
- Settings, units, and normal measurement operations were compared for four lock-ins (SR830/SR7265/SR5210/LI5640), three delay controllers (SHOT302GS/GSC01/GSC01A), and two scanners (CONEX-CC/CONEX-AGAP). This does not imply all maintenance APIs or physical testing of every model.
- Shared calibration for four stage and two actuator presets matches legacy TOML. Text hashes differ due to comments/formatting; old stage `pos_unit="pulse"` is omitted in current assets.
- Per-line spatial-fast compensation, U/V, readback during Moving,100 ms live reads, and invalidation after interrupted communication are new behavior. Returns and compensation share one distance; no experiment-zero return occurs between lines.
- CSV overwrite changed from Python overwrite to Rust refusal; `.run.json` is additional metadata. Native `run(output=None)` does not autosave, unlike the old default.
- Stop does not start a subsequent return. Range checks, GPIB invalidation, and stricter LI5640 save/restore remain.
- Physical movement, homing, sensors, travel limits, and actual model replies were not tested in this re-audit. Source comparisons and fake-device success do not guarantee apparatus behavior.

The old `next/` paths in the JSON identify the historical audit layout. See [current-path mapping](../validation/compatibility-audit-layout.md).


## Recording revision 2 / 記録revision 2 (0.3.5)

The historical audit above describes the releases examined at that time.
Version 0.3.5 intentionally removes Python-compatible seven-significant-digit
X/Y/R rounding while retaining CSV columns, profile/API revision 1 and legacy
CSV loading. New records include raw-response evidence. See
[measurement integrity](measurement-integrity.md). Old CSV digits cannot be restored.

上記は当時のリリースを対象とした監査です。0.3.5ではCSV列、設定・API revision 1と旧CSV読込を維持しつつ、Python由来のX/Y/Rの有効7桁丸めを意図的に廃止します。新しい記録には生応答を添付します。[測定記録の精度](measurement-integrity.md)を参照してください。旧CSVで失われた桁は復元できません。
