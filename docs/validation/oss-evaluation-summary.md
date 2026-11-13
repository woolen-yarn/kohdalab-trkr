# OSS evaluation summary / OSS評価の要約

## English

Evaluated on **2026-11-09**, against TRKR **v0.5.4**, commit
`e7302faabc23525625a517c0df2880b75abb390e`, on **macOS arm64**, using
**Rust 1.99.1 and Python 3**. The isolated-component evaluation was followed by
an opt-in comparison using the actual application with isolated settings.
Native macOS captures used a 2× pixel scale.

### Results and decisions

| Candidate | Observed result | Decision |
| --- | --- | --- |
| egui_kittest 0.33.3 | The real `precision::Form` handled Reverse → Reverse → Normal, disabled Start and re-enabled Start clicks; both miswiring and missing-disable injections were detected. The follow-up passed four actual-App tests for measurement tabs, panel collapse, Settings and Alignment with icons both enabled and disabled; 121 tests passed including existing GUI tests, with one explicit benchmark excluded. | Recommend limited adoption for interaction regressions, retaining existing geometry and state tests. |
| Insta 1.49.0 | Detected real RawWriter precision and column-name changes, as did plain `assert_eq!`; snapshot diffs made changed lines easier to identify. Ordinary text snapshots missed CRLF → LF. | Conditional adoption for structured settings/API contracts or escaped raw representations; retain semantic and raw-byte checks. Screenshots also vary with popup fades and time displays, so simple image equality must not be the sole correctness test. |
| Puffin 0.20.0 | Raw-copy outputs matched across all 135 runs: 4,000 rows, 398,001 bytes, 45 runs per mode. Release-build median raw-copy times were 1.267 ms without instrumentation, 1.269 ms OFF and 1.282 ms ON. Synthetic waiting and saving were recorded separately. The follow-up added optional actual-App update scopes but kept profiling disabled during screen comparisons. | Recommend development/diagnostics use, OFF during ordinary measurement. The roughly 1% difference has overlapping distributions and is not a general overhead estimate. Hardware timing effects and GUI overhead with profiling enabled remain unverified. |
| Lucide, pinned upstream commit | Nine SVGs totalled 3,066 bytes. Settings/panel icons were clearer at 20 px; arrows remained distinguishable at 16 px. The follow-up used 16 logical px panel chevrons while preserving button dimensions and accessible labels. Same-binary CPU timing median changes ranged from −0.36% to +0.64%, with overlapping measurements. | Limited adoption is appropriate as a supplement to text. No material speed improvement was demonstrated and this does not justify replacing all buttons. Start/Stop and settings were not reduced to icons alone. |

The isolated normal cases passed 18 tests including existing component tests;
one synthetic-baseline generator was excluded from ordinary execution.

### Fault-injection detection

| Injected change | kittest | Insta text | Plain `assert_eq!` | Insta JSON-escaped text |
| --- | --- | --- | --- | --- |
| Reverse miswired to Normal | Detected | — | — | — |
| Disabled Start treated as enabled | Detected | — | — | — |
| Shortened raw-value significant digits | — | Detected | Detected | Not tested |
| Changed CSV column name | — | Detected | Detected | Not tested |
| Changed raw response CRLF to LF | — | **Missed** | Detected | Detected |

Faults were injected only into experimental fixtures, without rewriting production
code. Snapshot baselines were not updated during injection (`INSTA_UPDATE=no`).

### Scope and original data

**No physical instrument I/O and no distribution release.** This was
not whole-GUI E2E validation. Actual-App headless CPU timings exclude
tessellation, GPU rendering, native-window events and instrument I/O; they are
neither FPS nor acquisition-rate measurements. Geometry comparisons do not prove
semantic correctness and there is no pixel-perfect screenshot claim. Windows,
other DPI settings, hardware communication/Stop protection, long recordings,
API-owned interactions, busy/error states, memory/power consumption and file
dialog interaction were not newly qualified. Existing API, layout and hardware
validation remains necessary.

Original data / 元データの場所: commit `b652b710bfc80d2aaa06c5802bea3229ee1bd034`,
`experiments/oss-evaluation/` (the parent commit immediately before removal;
削除直前の親commit). The original workspace and detailed results remain in Git
history.

## 日本語

評価日は **2026-11-09**。対象はTRKR **v0.5.4**、commit
`e7302faabc23525625a517c0df2880b75abb390e`。環境は **macOS arm64**、
**Rust 1.99.1・Python 3**。独立部品の評価に続き、隔離した設定と任意機能で
実アプリを比較しました。macOSのネイティブ画面取得は2×ピクセルスケールです。

### 結果と判断

| 候補 | 実測結果 | 判断 |
| --- | --- | --- |
| egui_kittest 0.33.3 | 本体の`precision::Form`でReverse→Reverse→Normal、無効Start、再有効化後のクリックを確認。誤配線・無効化漏れの注入を両方検出。後続比較では、アイコン有効・無効の両方で、実Appの測定タブ・パネル収納・Settings・Alignmentの4テストが成功。既存GUIテストを含め121件成功、明示実行ベンチマーク1件は除外。 | 操作イベントの回帰確認に限定導入を推奨。既存の幾何・状態テストを残す。 |
| Insta 1.49.0 | 実RawWriterの桁数・列名の変化を検出。通常の`assert_eq!`も検出するが、snapshotの差分は変更行を特定しやすい。通常の文字列snapshotはCRLF→LFを見逃した。 | 構造化した設定/API契約やエスケープしたraw表現に条件付き導入。意味検証・rawバイト比較を維持する。画面もポップアップのフェードや時間表示が変動するため、単純な画像一致だけを合否にしない。 |
| Puffin 0.20.0 | 4000行・398001 bytes、各モード45回、全135回のrawコピー出力が一致。releaseビルドの中央値は計測なし1.267 ms、OFF 1.269 ms、ON 1.282 ms。合成待ち処理と保存時間を別々に記録。後続比較では実Appの更新処理に任意の計測区間を追加したが、画面比較中の計測は無効。 | 開発・診断用に推奨。通常測定ではOFF。約1%の差は測定分布が重なり、一般的な増加率とは扱わない。実機でのタイミング影響とGUI計測有効時の負荷は未検証。 |
| Lucide、上流commit固定 | 9 SVG合計3066 bytes。設定・パネルは20 px、矢印は16 pxでも判別可能。後続比較では左右収納を16 logical pxの線画にし、ボタン寸法とアクセス可能なラベルを維持。同一バイナリのCPU時間の中央値差は−0.36〜＋0.64%で測定範囲が重なる。 | 文字を補助する限定導入が妥当。大きな速度改善は確認できず、全ボタン置換の根拠にはしない。Start/Stopや設定項目をアイコンだけにはしていない。 |

独立評価の正常系は既存部品テストを含む18件成功。合成基準生成用1件は通常実行から除外しました。

### 故障注入の検出

| 注入した変化 | kittest | Insta text | 通常の`assert_eq!` | Insta JSONエスケープ文字列 |
| --- | --- | --- | --- | --- |
| ReverseをNormal値へ誤配線 | 検出 | — | — | — |
| disabled Startを有効扱い | 検出 | — | — | — |
| raw値の有効桁を短縮 | — | 検出 | 検出 | 未試験 |
| CSV列名を変更 | — | 検出 | 検出 | 未試験 |
| 生応答のCRLFをLFへ変更 | — | **見逃し** | 検出 | 検出 |

故障注入は実験fixtureだけに行い、本体コードは書き換えませんでした。
注入中は`INSTA_UPDATE=no`を強制し、snapshot基準を更新していません。

### 範囲と元データ

**実機I/Oなし・配布物なし。GUI全体のE2E検証ではありません。** 実Appの
headless CPU時間にはtessellation・GPU描画・ネイティブウィンドウイベント・
機器I/Oを含まず、FPSや取得速度の測定ではありません。幾何比較だけでは意味的な
正しさを証明せず、ピクセル完全一致も主張しません。Windows・他のDPI・実機通信と
Stop保護・長時間記録・API所有時の操作・busy/error状態・メモリ/消費電力・
ファイルダイアログ操作は新たに検証していません。既存API・レイアウト・実機の検証は
引き続き必要です。

元データの場所は上の英日併記行を参照してください。元workspaceと詳細結果はGit履歴に残ります。
