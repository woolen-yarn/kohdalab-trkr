# Multiple devices and Alignment validation / 複数機器・Alignment検証

Development branch based on `aff3794` (published v0.4.10), checked on 2026-11-05.
This is feature-development evidence, not a hardware qualification or release record.

公開済みv0.4.10の`aff3794`を基点に、2026-11-05に開発ブランチを確認しました。
本記録は機能開発の検証であり、実機の動作保証・公開済みリリースの記録ではありません。

| Check / 検証 | Result / 結果 |
| --- | --- |
| Rust workspace tests | 355 passed, 0 failed / 成功355、失敗0 |
| Python API, notebook, packaging and layout-validator tests | 67 passed, 0 failed / 成功67、失敗0 |
| Rust formatting and Clippy (`--workspace --all-targets -- -D warnings`) | Passed / 成功 |
| Desktop debug build, macOS | Passed / 成功 |
| Actual GUI, five measurement tabs × expanded/scrolled device panels | 10 geometry captures passed; fixed outer widths, Config rows, position fields and Output/Run margins retained / 10画面成功。外側の固定幅、Config行、位置欄、Output/Run余白を維持 |
| Alignment synthetic GUI | 2 delay panels at 1566×700 and 4 scanner panels at 1566×820 logical pixels, Retina 2×; visually inspected / 合成データで表示・目視確認 |
| Measurement Settings and inactive devices | Settings at 1566×700, inactive cards at 1566×900, Retina 2×; current captures visually inspected / Settingsと未選択機器の表示を目視確認 |
| Compiled GUI + Python client | Demo TRKR and SRKR, 3 rows each: workflow-specific selection, zeros, saved/reloaded profile, `.run.json` device IDs, and GUI survival after Python close verified / 実GUIのDemoで測定別選択・ゼロ・保存と再読込・機器ID・Python終了後のGUI存続を確認 |
| Physical instruments / 実機 | Not operated or qualified / 未操作・未検証 |

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
