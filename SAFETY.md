# Safety Notes

KohdaLab TRKR controls motion stages, scanners, and measurement instruments.
Software checks are useful, but they are not a substitute for mechanical
limits, interlocks, correct wiring, and operator supervision.

## Before Measurement

- Confirm the sample, optics, stages, and scanners are clear to move.
- Confirm coordinate system and origin.
- Confirm travel limits and scan ranges.
- Start with a small range and low point count.
- Verify the output file path before long measurements.
- Keep a manual way to stop motion or disable hardware.

## During Measurement

- Do not disconnect instruments during an active scan.
- Use the GUI Stop action or interrupt the CLI so cleanup can run.
- Watch for unexpected motion, overloads, or unstable readings.
- Stop immediately if the sample, optics, or instruments behave unexpectedly.

## After Measurement

- Confirm stages and scanners returned to the intended position.
- Save the config used for the run.
- Record any manual hardware changes in lab notes.

The software is provided without warranty. Operators are responsible for safe
laboratory practice and for following instrument manuals.

## 日本語

KohdaLab TRKRはステージ、スキャナ、測定機器を制御します。ソフトウェアの確認機能は、機械的制限、インターロック、正しい配線、操作者の監視の代わりにはなりません。

### 測定前

- 試料、光学系、ステージ、スキャナが移動しても問題ないことを確認します。
- 座標系と原点を確認します。
- 可動範囲とスキャン範囲を確認します。
- 最初は狭い範囲と少ない点数を使います。
- 長時間測定の前に保存先を確認します。
- 手動で移動を停止する、または機器を無効にする手段を用意します。

### 測定中

- スキャン中に機器を切断しないでください。
- 後処理を実行できるよう、GUIのStopまたはCLIの割込みを使います。
- 意図しない移動、過負荷、不安定な読取値を監視します。
- 試料、光学系、機器に異常があれば直ちに停止します。

### 測定後

- ステージとスキャナが意図した位置にあることを確認します。
- 使用した設定を保存します。
- 手動で変更した機器設定を実験記録に残します。

本ソフトウェアは無保証です。操作者は安全な実験手順と機器マニュアルに従う責任を負います。
