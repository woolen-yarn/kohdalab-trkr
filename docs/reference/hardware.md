# Hardware and transport

The following models are implemented in the Rust drivers. Protocols are covered
by simulated-I/O tests; this is not a claim that every hardware/firmware
combination has been tested on a physical apparatus. See the
[compatibility audit](compatibility.md) for the verification scope and
legacy maintenance operations that are outside the measurement API.

| Category | Model / family | Connection | Implemented measurement operations |
| --- | --- | --- | --- |
| Lock-in | SR7265 | GPIB | XY/Rθ acquisition, sensitivity, time constant, coupling, slope, AC gain, auto actions |
| Lock-in | SR830 | GPIB | XY/Rθ acquisition, sensitivity, time constant, coupling, slope, auto actions |
| Lock-in | SR5210 | GPIB | XY/Rθ acquisition, sensitivity, time constant, slope, auto actions; AC coupling only |
| Lock-in | LI5640 | GPIB | XY/Rθ acquisition with display restoration, sensitivity, time constant, coupling, slope, auto actions |
| Delay controller | SHOT302GS | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Delay controller | GSC01 / GSC01A | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Scanner controller | CONEX-CC | Serial | Position readback, absolute moves, home/initialization, Stop, configurable software hysteresis compensation |
| Scanner controller | CONEX-AGAP | Serial | U/V axis control, position readback, absolute moves, closed-loop initialization and Stop |

Built-in motion presets supply defaults; experiment origins, sample conversion,
direction and limits remain profile settings.

| Stage / actuator preset | Controller | Native travel / unit |
| --- | --- | --- |
| SGSP46-500 | SHOT302GS | 500 mm travel |
| OSMS20-35 | SHOT302GS / GSC01 / GSC01A | 35 mm travel |
| OSMS26-300 | SHOT302GS / GSC01 / GSC01A | 300 mm travel |
| OSMS33-600 | SHOT302GS | 600 mm travel |
| TRA12CC | CONEX-CC | 0–12 mm |
| AG-M100D | CONEX-AGAP | −0.75–+0.75° |

GPIB supports primary addresses 1–30. On Windows, the application can use a
system **64-bit VISA runtime** for the selected `GPIB0::<address>::INSTR` resource.
The separate native USB-GPIB helper supports **NI USB-GPIB-HS+** and an
**already initialized Agilent 82357B**. Drivers and adapter initialization are
not bundled. VISA failures do not trigger a native-adapter fallback. On macOS,
serial devices use their `/dev/cu.*` ports. See
[transport setup and limitations](compatibility.md) and
[native helper details](../../native/README.md).

The demo bundles synthetic CSVs only. Experimental CSVs in a local `demo_csv/`
directory are ignored by Git and never copied into release artifacts.

## 日本語

以下はRustドライバーに実装した機種です。プロトコルは疑似通信でテストしていますが、すべての機器・ファームウェアの組合せを実機検証したことは意味しません。確認範囲と測定APIに含まれない旧保守操作は[互換性監査](compatibility.md)を参照してください。

| 分類 | 機種 | 接続 | 実装済みの測定操作 |
| --- | --- | --- | --- |
| ロックイン | SR7265 | GPIB | XY/Rθ取得、感度、時定数、結合、スロープ、ACゲイン、自動操作 |
| ロックイン | SR830 | GPIB | XY/Rθ取得、感度、時定数、結合、スロープ、自動操作 |
| ロックイン | SR5210 | GPIB | XY/Rθ取得、感度、時定数、スロープ、自動操作。AC結合のみ |
| ロックイン | LI5640 | GPIB | 表示復元付きXY/Rθ取得、感度、時定数、結合、スロープ、自動操作 |
| 遅延ステージ制御 | SHOT302GS | シリアル | 位置読取、絶対移動、microstep読取、初期化・原点復帰、通常Stop |
| 遅延ステージ制御 | GSC01 / GSC01A | シリアル | 位置読取、絶対移動、microstep読取、初期化・原点復帰、通常Stop |
| スキャナ制御 | CONEX-CC | シリアル | 位置読取、絶対移動、原点復帰・初期化、Stop、設定可能なソフトウェアヒステリシス補償 |
| スキャナ制御 | CONEX-AGAP | シリアル | U/V軸制御、位置読取、絶対移動、閉ループ初期化、Stop |

組込みプリセットは既定値を与えます。実験原点、試料上の換算、方向、制限は設定ごとに管理します。

| ステージ・アクチュエータ | コントローラー | 可動範囲・単位 |
| --- | --- | --- |
| SGSP46-500 | SHOT302GS | 500 mm |
| OSMS20-35 | SHOT302GS / GSC01 / GSC01A | 35 mm |
| OSMS26-300 | SHOT302GS / GSC01 / GSC01A | 300 mm |
| OSMS33-600 | SHOT302GS | 600 mm |
| TRA12CC | CONEX-CC | 0–12 mm |
| AG-M100D | CONEX-AGAP | −0.75–+0.75° |

GPIBは一次アドレス1–30に対応します。Windowsでは、選択した`GPIB0::<address>::INSTR`に対してシステムの**64-bit VISAランタイム**を使えます。独立したUSB-GPIB helperは**NI USB-GPIB-HS+**と**初期化済みAgilent 82357B**に対応します。ドライバーやアダプター初期化は同梱しません。VISAの失敗でネイティブ通信へ切り替えることはありません。macOSのシリアル機器には`/dev/cu.*`を使います。[通信設定と制約](compatibility.md)・[helper詳細](../../native/README.md)を参照してください。

Demo配布には合成CSVだけを含めます。ローカル`demo_csv/`の実験CSVはGit対象外で、リリース成果物にはコピーしません。
