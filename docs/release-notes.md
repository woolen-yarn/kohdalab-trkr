# KohdaLab TRKR 0.5.0

## Changes / 変更点

- Connect multiple lock-ins, delay stages and paired X/Y scanners. Each measurement remembers its device selection in Settings below Wait. Selected devices are highlighted without opening or closing left-panel cards. / 複数のロックイン・遅延ステージ・X/Y一組のスキャナを接続できます。Wait下のSettingsで測定ごとの機器選択を記憶し、左カードの開閉を変えずに対象機器を強調します。
- Rename, reorder, add and delete registered devices. Device fields share a consistent order and show edit/confirmation feedback. The TOML actuator catalog supplies the selection list: CONEX-CC with TRA12CC, CONEX-AGAP with AG-M100D. / 登録機器の名前変更・並べ替え・追加・削除に対応。Device設定の順序と変更表示を統一し、Actuator候補はTOMLから生成します。現在はCONEX-CC＋TRA12CC、CONEX-AGAP＋AG-M100Dです。
- Add an independent Alignment tab with up to two delay and four scanner panels. Add/Delete affects the panel only; empty selections and unique device assignments are retained. Config saves the panel layout, unit and Step. / 独立したAlignmentタブにDelay最大2枚・Scanner最大4枚を配置できます。Add／Deleteはパネルのみを変更し、空欄と機器割当の重複防止を維持します。配置・単位・StepをConfigへ保存します。
- Compact alignment controls place a square scanner graph on the left, X/Y values above the direction pad on the right, and Options at the top. Click for a step, hold for bounded continuous movement, release to stop. Delay also provides end moves. Step cannot be negative; zero disables movement. / Scannerは左に正方形グラフ、右上にX/Y値、その下に十字操作、上端にOptionsを配置。クリックでStep移動、長押しで連続移動、解放で停止します。Delayは端移動にも対応し、Stepは負数を受け付けず、0では移動を無効にします。
- Configure delay optical round trips and reverse axis direction while preserving the current displayed coordinates. The GUI, motion engine and saved calibration share the same conversion. / 遅延光路の往復数を設定でき、現在の表示座標を維持してDirectionを反転できます。GUI・移動処理・保存する校正に同じ換算を適用します。
- Expose Alignment layout, Step/Jog/End/Initialize and Direction through the GUI-owned Python/common control API. Jog requires explicit keepalives and stops on expiry or owner release; ordinary measurement stop policies remain unchanged. Shared validation and reduced per-frame copying keep configuration and display code simpler. / GUI所有のPython・共通APIからパネル設定・Step／Jog／端移動／Initialize／Directionを操作できます。Jogは専用継続通知の失効・操作権限解放で停止し、通常測定の停止方針は維持します。設定検証を共通化し、画面更新時の不要なコピーを削減しました。
- Ctrl+Enter starts the selected measurement when Start is available; dialogs, Alignment, active jobs and Python ownership prevent accidental starts. / Startが有効な測定をCtrl+Enterで開始できます。ダイアログ・Alignment・処理中・Python制御中には開始しません。

## Compatibility / 互換性

Existing JSON profiles import their devices as the first entries. Save explicitly to retain the new inventory and panel layout; old files are not overwritten automatically. Measurement CSV retains the selected signal and coordinates, raw CSV retains native readbacks, and run metadata records device IDs and effective calibration. Update the Python client together with the application. One measurement uses one device of each required kind; simultaneous multi-lock-in acquisition is not included.

既存JSONは各機器を1台目として読み込みます。新しい機器一覧・パネル配置の保存は明示的に行い、旧ファイルを自動上書きしません。測定CSVは選択した信号と座標、raw CSVは装置読取値、run記録は機器IDと実効校正を保持します。Pythonクライアントはアプリと一緒に更新してください。1測定で使う機器は必要な種類ごとに1台で、複数ロックインの同時取得は含みません。

## Validation and downloads / 検証・配布

Software checks cover coordinate conversion, selection/configuration persistence, motion limits, backlash behavior, Hold cancellation, API ownership, duplicate requests and keepalive expiry. macOS GUI captures and a simulated Python-to-GUI run verify the display and configuration path. Release CI tests Windows/macOS/Linux and builds Windows x64 Setup/portable ZIP and macOS Apple silicon DMG/ZIP, with corresponding source, licenses, provenance and SHA-256 manifests. This is software validation, not physical qualification of every instrument combination.

座標換算・機器選択／設定保持・移動制限・補償動作・Hold停止・API権限・重複要求・継続通知失効をソフトウェアテストで検証します。macOSのGUI画像とPythonからGUIへの模擬測定で表示・設定経路を確認します。公開CIはWindows・macOS・Linuxを検証し、Windows x64 Setup／portable ZIP、Mac Apple silicon DMG／ZIP、対応ソース・権利表記・来歴・SHA-256一覧を作成します。すべての機器の組合せを実機認定するものではありません。

Windows packages are unsigned. macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名で公証はありません。
