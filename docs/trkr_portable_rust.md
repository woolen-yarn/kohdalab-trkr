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
preparation is described in `docs/windows_code_signing.md`.

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
[全ファイル・関数索引](python_compatibility_inventory.json) に旧ファイルのSHA256、
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
