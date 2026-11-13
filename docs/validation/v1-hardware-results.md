# TRKR v1.0.0 candidate hardware verification / 実機検証

These observations are from Windows x64 candidates tested on 2026-11-13. Each row names the scope of the evidence. Rebuilt release packages have their own validation and hashes.
2026-11-13にWindows x64の候補で得た記録です。配布候補の再ビルド品は別途、動作確認とハッシュの照合が必要です。

| Instrument / 機器 | Connection / 接続 | Verified operations / 確認した操作 |
| --- | --- | --- |
| Signal Recovery SR7265 | Nautilus · GPIB 12 | Connected, read X/Y/R/θ during delay-stage motion, disconnected. The operator confirmed time-constant changes and Auto Phase separately. / 接続、ステージ動作中のX/Y/R/θ取得、切断。時定数変更とAuto Phaseは操作者が別途確認。 |
| Stanford Research Systems SR830 | Nautilus · GPIB via official VISA / メーカーVISA | Read voltage input and reference frequency, changed and restored TC, slope and sensitivity. The operator confirmed the front-panel Auto Phase response matched the GUI-triggered response. / 電圧入力と周波数を取得し、時定数・スロープ・感度の変更と復元を確認。Auto Phaseは操作者が本体操作とGUI操作の反応が同じことを確認。 |
| EG&G 5210 (SR5210) | Sardine · GPIB 13 via official VISA / メーカーVISA | TC, slope and sensitivity change/readback/restore, reference frequency, Auto Phase progress and live recovery, voltage units and resolution, disconnect. / 時定数・スロープ・感度の変更・読み戻し・復元、周波数、Auto Phase後のライブ復帰、電圧の単位と桁、切断。 |
| NF LI5640 | Abalone · GPIB 1 via official VISA / メーカーVISA | TC, slope and sensitivity, overload at 5 nV, X/Y ↔ R/θ, Auto Phase and live recovery, measured reference frequency and voltage/angle units. Signal Monitor saved both modes on the earlier candidate. / 時定数・スロープ・感度、5 nVでの過負荷表示、X/YとR/θの切り替え、Auto Phase後のライブ復帰、実測周波数、電圧・角度単位。先行候補では両モードのSignal Monitor保存も確認。 |
| Sigma Koki SHOT-302GS + SGSP46-500 | Nautilus · serial COM6 / シリアル | Delay pulse readback and relative movement 53657 → 53659 → 53657 during combined SR7265 acquisition. / SR7265との組み合わせで位置取得、相対移動、開始位置復帰。 |
| Newport CONEX-CC | Nautilus · serial COM5 (X), COM4 (Y) / シリアル | Both axes moved by approximately 10 µm and returned near their starting positions, controller error 0000 and closed-loop READY. / 両軸で約10 µm移動して開始位置付近に復帰、エラー0000、閉ループREADY。 |
| Newport CONEX-AGAP | Nautilus · serial / シリアル | Controller query and no-motion initialize, 0.094° before and after, then disconnect. / 本体照会、移動を伴わない初期化、切断。初期化前後は0.094°。 |
| Thorlabs PM100USB + S130C | Nautilus · USB with TLPMX / USB・TLPMX | Connected at 800 nm, read three distinct powers around 5.56 µW, disconnected, reopened independently to verify release. / 800 nmで接続、約5.56 µWの異なる3点を取得、切断後に別セッションで再接続して解放確認。 |

## Scope limits / 検証範囲

- The 5210 and LI5640 current-input display, Linux/macOS instruments, other GPIB adapters/backends, and full physical scan workflows are outside the recorded hardware verification. / 5210・LI5640の電流入力、Linux/macOS実機、別のGPIBアダプター・経路、全測定モードでの実走査は未検証です。
- CONEX-AGAP had no commanded displacement. The GUI live update rate on LI5640 was reported as slow by the operator and has not been benchmarked. / CONEX-AGAPの指令移動は未検証です。LI5640のライブ更新は操作者から遅く感じるとの指摘があり、更新周期の定量評価は未実施です。
- Candidate evidence: Nautilus motion/SR7265/PM100USB portable ZIP SHA-256 `8b5e8bf2e129b8e5ab49f818114dba9845b7c9b10f24ec4bc902f1aacb2fe2e4`; Sardine SR5210 display candidate executable `4095bdc56388f30b2216aa354bd1790306c08730a9a7383c79105dd0b906c22b`; Abalone LI5640 display candidate executable `3bd33c419bc2715356de960a789c98b6afce8dbb5a960df37c6252abfa7935e2`.
