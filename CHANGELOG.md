# Changelog

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
