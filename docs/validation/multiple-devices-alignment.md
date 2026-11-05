# Multiple devices and Alignment validation / 複数機器・Alignment検証

Development branch based on `aff3794` (published v0.4.10), checked on 2026-11-05.
This is feature-development evidence, not a hardware qualification or release record.

公開済みv0.4.10の`aff3794`を基点に、2026-11-05に開発ブランチを確認しました。
本記録は機能開発の検証であり、実機の動作保証・公開済みリリースの記録ではありません。

| Check / 検証 | Result / 結果 |
| --- | --- |
| Rust workspace tests | 384 passed, 0 failed / 成功384、失敗0 |
| Python API, notebook, packaging and layout-validator tests | 67 passed, 0 failed / 成功67、失敗0 |
| Rust formatting and Clippy (`--workspace --all-targets -- -D warnings`) | Passed / 成功 |
| Desktop debug build, macOS | Passed / 成功 |
| Actual GUI, five measurement tabs × Scanner Device settings | 5 current geometry captures passed; fixed outer widths, Config rows, position fields and Output/Run margins retained / 5画面成功。外側の固定幅、Config行、位置欄、Output/Run余白を維持 |
| Alignment synthetic GUI | 2 delay panels and 4 scanner panels and their Options popups at 1566×700 logical pixels, Retina 2×; visually inspected / 合成データで表示・目視確認 |
| Measurement Settings and inactive devices | Settings at 1566×700, inactive cards at 1566×900, Retina 2×; earlier captures visually inspected / Settingsと未選択機器の表示を目視確認 |
| Compiled GUI + Python client | Demo TRKR and SRKR, 3 rows each: workflow-specific selection, zeros, saved/reloaded profile, `.run.json` device IDs, and GUI survival after Python close verified / 実GUIのDemoで測定別選択・ゼロ・保存と再読込・機器ID・Python終了後のGUI存続を確認 |
| Physical instruments / 実機 | Not operated or qualified / 未操作・未検証 |

## Selective Alignment panels / Alignmentパネルの選択

Alignment Step editing now stops at zero; negative keyboard input is clamped,
positive input is preserved, and zero cannot start Step/Hold. The selectors allow
None and exclude devices used by another panel of the same kind. Defaults are
assigned only once; cleared, deleted or duplicate bindings are not silently
retargeted after an inventory or view change. Four added regression tests cover
these input and selection behaviors. The live macOS preview confirms None
remains selected after visiting SRKR and returning to Alignment.

Stepは0で止まり、負数入力は0に制限、正の入力値は維持し、0ではStep／Holdを開始しません。
機器選択はNoneに対応し、同種の別パネルで使用中の機器を除外します。初期割当は初回のみで、
空欄・削除・重複した機器を画面や一覧の変更で勝手に再割当しません。4件の追加試験で確認し、
Mac実画面でもNoneにしてSRKRへ移動した後、Alignmentへ戻って空欄の保持を確認しました。

CONEX-CC currently offers only TRA12CC. Its Actuator dropdown follows the
controller-filtered TOML catalog; future catalog additions require a rebuild,
with no selector code changes. **384 Rust tests (99 UI tests)**, Clippy and the
macOS build pass. The motion worker and instrument I/O are unchanged.

CONEX-CCの候補はTRA12CCのみです。ActuatorのプルダウンはTOMLの制御器別候補に連動し、
将来の機種追加はTOMLと再ビルドで反映します。**Rust 384件（UI 99件）**、静的検査、
Macビルドが成功しました。移動ワーカーと装置通信は変更していません。

## Compact-control foundation / コンパクト操作の基礎検証

Measurement-tab navigation preserves the inspected device group, disclosure
state and card geometry. Workflow assignment, readiness and scan hints use the
measurement's bound devices; Start projects that assignment before acquisition.
The new tests cover all five tabs, a different bound scanner, delay calibration
and unavailable time-constant data for a different lock-in.

測定タブの移動では、表示中の機器・Deviceの開閉・カード配置を維持します。
開始可否と走査の換算値は測定に割り当てた機器で判定し、Startでその割当を使用します。
全5タブ、表示と異なるScanner、Delayの校正、別ロックインのTCが取得できない場合を
回帰テストで確認しました。

Delay and Scanner use one arrow for click-to-step and hold-to-move. Input tests
cover the 0.35 s threshold, one Hold request, pointer drift outside the button,
release/focus loss, invalid steps, busy blocking and no extra step after Hold.
Selected-unit coordinates and limits follow current offsets. Delay end arrows
are checked across units, offsets and both directions. Options is a popup;
Initialize opens a separate confirmation and rechecks connection and ownership.
Independent review findings for pointer capture and confirmation readiness were
corrected and re-reviewed.

DelayとScannerは同じ矢印でクリック移動と長押し移動を使い分けます。0.35秒の判定、
Holdの重複防止、ボタン外へのポインター移動、解放・フォーカス喪失、不正Step、処理中の制限、
解放後に余分なStepが出ないことを検証しました。単位別の座標・範囲は最新offsetを反映し、
Delayの端移動は両方向・全単位で確認しています。Optionsはポップアップ化し、Initializeは
別の確認時に接続・操作権を再確認します。独立レビューの指摘を修正し、再確認済みです。

Both Alignment views and popups fit 1566×700. Five current measurement geometry
captures pass, alongside **380 Rust tests (including 95 UI tests)**, **67 Python
tests**, formatting, Clippy and the macOS build. The compiled GUI/Python Demo
check verifies three TRKR and three SRKR rows, IDs, zeros, labels, order,
configuration reload, run metadata and GUI survival after Python closes.
No physical instruments were operated.

Alignmentの両画面・ポップアップは1566×700に収まり、測定5画面の配置検査も成功しました。
**Rust 380件（UI 95件を含む）・Python 67件**、書式・静的検査・Macビルドが成功しています。
実GUIとPythonのDemoでTRKR/SRKR各3行、機器ID・ゼロ・名前・順序・設定再読込・メタデータ・
Python終了後のGUI存続を確認しています。実機は操作していません。

## Earlier development checks / これまでの開発検証

Focused regressions cover shared serial-port ownership, per-axis SHOT302GS Stop,
native and VISA GPIB resource pooling, cancellation response retirement, optical
round-trip conversion, direction reversal including mechanical compensation,
partial-axis configuration retention, exact Step input, held-pointer release,
and correlated device selection without opening hardware.

重点試験では、シリアルポート共有、SHOT302GSの軸別Stop、native/VISAの複数GPIB接続、
取消応答の処理、往復数による換算、補償方向を含むDirection反転、未接続軸の設定保持、
Step入力値、長押し解放、実機を開かない機器選択APIの完了通知を確認しています。

Independent reviews found and corrected serial reopening during lock-in-only
disconnect, premature replacement of connection state, and partial-axis settings
being overwritten by a direction-update response. The connection worker retains
ownership of all hardware I/O; the GUI never opens an extra movement connection.

独立レビューで見つかった、ロックイン単独切断時のシリアル再接続、接続状態の早すぎる置換、
Direction応答による未接続軸の設定上書きは修正済みです。
装置通信は既存の制御ワーカーが所有し、GUIから別の移動接続は作りません。

See [the bilingual guide and screenshots](../guides/multiple-devices-alignment.md).
使い方と画面は[英日ガイド](../guides/multiple-devices-alignment.md)を参照してください。

The measurement Settings follow-up also covers independent workflow bindings,
device-owned coordinate zeros, staged OK/Cancel edits, correlated selection
rollback, and history-tab changes during acquisition. Independent review
confirmed the idle-restore retry guard and pending-selection UI lock. The actual
GUI Demo check found and corrected stale device IDs in run metadata after a
cross-workflow switch; Demo now records the requested run configuration.

測定Settingsの追加では、測定別の機器割当、装置ごとのゼロ位置、OK/Cancel、切替失敗時の
復帰、測定中の履歴タブ切替を検証しました。独立レビューでは、自動復帰失敗時の再試行防止と
切替確認中の操作制限を確認しています。実GUIのDemo試験で、測定切替後のメタデータに
前の機器IDが残る問題を修正し、その測定で指定した設定を記録することを確認しました。


The UI refinement places Settings below Wait, removes duplicate timing controls,
and shows only workflow-required device roles. Inventory regressions cover
confirmed deletion, reassignment of all affected workflows, independent zeros,
label save/reload, and busy/connected deletion rejection. Profiles can retain new
motion devices without assigned ports; acquisition and connection still require
strict validation, including controller/actuator compatibility and calibration.
Independent review corrected the canonical selection after deletion outside
Alignment, with both ordinary measurement and Alignment cases tested.

今回のUI調整では、SettingsをWaitの下へ移動し、Timingの重複をなくして測定別の機器欄に
整理しました。削除後の全測定の割当、機器別ゼロ、名前の保存・再読込み、実行中・接続中の
削除拒否を回帰テストで確認しています。追加直後のポート未設定機器は保存できますが、
実際の接続・測定では、ポート・機種の組合せ・校正の検証を省略しません。
独立レビューで、削除後の表示と実際の選択を一致させる修正を行い、通常測定とAlignmentの
両経路を確認しました。

Actual macOS captures cover Add/Delete dialogs, all workflow Settings, and the
Alignment direction pad. The final GUI + Python simulation also verified labels
survive save/reload alongside per-workflow device IDs and coordinate zeros.
Physical motion remains untested; all displayed Alignment positions are synthetic.

Mac実画面で追加・削除確認、測定別Settings、Alignmentの十字操作を確認しました。
最終GUIとPythonの模擬測定でも、機器ID・座標ゼロに加えて名前の保存・再読込みを確認しました。
実機移動は未検証で、Alignmentの確認画面に表示した位置は合成データです。


The compact-card follow-up adds inline `Scanner: Pump` names, a fixed bottom Add
button, and cross-kind drag ordering. Regression tests simulate header typing, press/drag/drop
and Escape cancellation, then verify stable device IDs, workflow assignments,
calibration and save/reload order. Invalid/stale order keys are canonicalized.
The actual compiled GUI + Python Demo check also retains the requested panel
order across workflow changes and configuration save/reload.

カードの改良では見出し内の名前、下端固定Add、種類をまたぐドラッグ並べ替えを追加しました。
名前入力・ドラッグ・ドロップ・Escape取消を入力試験で再現し、機器ID・測定割当・校正と並び順の
保存／再読込を確認しています。削除済み・重複した順序情報も整理されます。
実GUIとPythonのDemo測定でも、測定切替と設定保存／再読込後の順序を確認しました。

Six measurement-layout states (30 workflow captures) pass geometry checks,
including fixed-footer clearance and long-name/reordered cards. Alignment was
inspected at 1566×820, expanded Options at 1566×900/980, and minimum-height
scanner scrolling at 1566×700. The scanner uses font-independent arrows; delay
rail scales and current readbacks have separate rows. Independent code reviews
checked the inventory and Alignment changes. Motion commands and physical
qualification scope are unchanged.

測定画面6状態・合計30画面で配置検査に成功し、下端Addの余白と長い名前・並べ替え後の
配置を確認しました。Alignmentは1566×820、Options展開は1566×900／980、Scannerの
最小高さとスクロールは1566×700で確認しています。矢印はフォントに依存せず描画し、
Delayの目盛りと現在値は別行です。機器一覧とAlignmentは独立レビューでも確認しました。
移動命令と実機検証の範囲は変更していません。


The unified-controls follow-up adds Ctrl+Enter with the same readiness rule as
Start. Input tests verify modifier matching, autorepeat suppression, dialog and
pending-operation blocking, and a replay start using the numeric edit from the
same frame. Device fields now share an order and show transient local-edit
feedback; live direction updates use device/axis-correlated success or failure.
An unrelated background read error cannot fail a pending direction update.

操作統一では、Startと同じ条件のCtrl+Enterを追加しました。修飾キー・キーリピート・
ダイアログ・処理待ちの制限、同じ描画周期で入力した値を使う再生開始を入力試験で確認しました。
Deviceの項目順を統一し、ローカル変更は一時的な強調表示、接続中Directionの結果は
機器・軸に対応した確認結果で表示します。無関係な読取エラーで変更失敗にはなりません。

Current captures cover four expanded/scrolled/edited/reordered device states
(20 workflow geometry checks) and both Alignment views, including expanded
Options. Visual inspection corrected delay readback/input spacing and scanner
hint/arrow overlap. The final UI suite passes 86 tests; formatting, Clippy and
the rebuilt macOS GUI pass. The actual GUI/Python Demo check again produces
three TRKR and three SRKR rows and preserves selections, labels, zeros, order,
metadata and configuration reload. Independent review confirmed the targeted
Direction failure handling. All checks use synthetic data; no physical motion.

今回のDevice表示4状態・合計20画面で配置検査に成功し、両Alignmentの通常・Options展開も
目視確認しました。Delayの現在値と入力欄、Scannerの説明と矢印の重なりを修正しました。
最終UIテスト86件、書式・静的検査・Mac再ビルドが成功しています。実GUIとPythonのDemoで
TRKR・SRKR各3行を再確認し、機器選択・名前・ゼロ・順序・メタデータ・設定再読込を維持しました。
独立レビューでDirection失敗の対象識別も再確認しています。実機移動は行っていません。
