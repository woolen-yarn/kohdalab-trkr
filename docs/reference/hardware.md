# Hardware and transport

The following models are implemented in the Rust drivers. Protocols are covered
by simulated-I/O tests; this is not a claim that every hardware/firmware
combination has been tested on a physical apparatus. See the
[compatibility audit](compatibility.md) for the verification scope and
legacy maintenance operations that are outside the measurement API.

| Category | Model / family | Connection | Implemented measurement operations |
| --- | --- | --- | --- |
| Lock-in | SR7265 | GPIB | XY/Rθ acquisition, sensitivity, time constant, coupling, filter slope, AC gain, auto actions ([settings](#lock-in-settings)) |
| Lock-in | SR830 | GPIB | XY/Rθ acquisition, sensitivity (voltage input), time constant, coupling, filter slope, auto actions |
| Lock-in | SR5210 | GPIB | XY/Rθ acquisition, sensitivity (voltage mode), time constant, filter slope, auto actions; AC coupling only |
| Lock-in | LI5640 | GPIB | Selected XY or Rθ acquisition (0.4.8; previously both with display restoration), sensitivity, time constant, coupling, filter slope, auto actions |
| Delay controller | SHOT302GS | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Delay controller | GSC01 / GSC01A | Serial | Position readback, absolute moves, microstep readout, initialization/home, normal Stop |
| Scanner controller | CONEX-CC | Serial | Position readback, absolute moves, home/initialization, Stop, configurable software hysteresis compensation |
| Scanner controller | CONEX-AGAP | Serial | U/V axis control, position readback, absolute moves, closed-loop initialization and Stop |
| Optical power meter (from 0.5.3) | Thorlabs PM100USB / PM100D | Windows x64: VISA USBTMC, plus Thorlabs TLPMX/TLPM from 0.5.7 | Optional sequential power readback and recording; not physically qualified |

In 0.4.8, CONEX-AGAP arrival tolerance defaults to 0.01° independently of its 0.001° minimum motion; set per-axis `position_tolerance` in native units to override it. CONEX-CC keeps its previous tolerance.

## Lock-in settings

The lock-in card shows **Device → live display → Settings**. Settings lists
only values from the instrument's own readback (`available` tables for the
observed input mode). It is disabled while disconnected, in Demo, while an
operation runs or while a command is pending. A selection is applied by the
worker as *read → validate → write → read back → verify* (see
[control API](control-api.md#lock-in-settings--ロックイン設定)).

**Filter slope** and **AC gain** are different controls. Filter slope is the
roll-off of the output (post-detector) low-pass filter in dB/octave and exists
on every supported model. AC gain is the SR7265 signal-channel gain before the
detector, 0–90 dB in 10 dB steps (`ACGAIN n` = n × 10 dB). It is shown only for
the SR7265 and is rejected without I/O for other models.

| Model | Sensitivity | Time constant | Filter slope (dB/oct) | AC gain (dB) | Auto Phase | Input mode read first |
| --- | --- | --- | --- | --- | --- | --- |
| SR830 | `SENS i` / `SENS?` | `OFLT i` / `OFLT?` | `OFSL i`: 6/12/18/24 | — | `APHS` | `ISRC?` |
| SR7265 | `SEN n` / `SEN.` | `TC n` / `TC.` | `SLOPE n`: 6/12/18/24 | `ACGAIN n` / `ACGAIN` | `AQN` | `IMODE` |
| SR5210 | `SEN n` | `TC n` | `XDB n`: 6/12 | — | `AQN` | `N` status bits 1/2 |
| LI5640 | `VSEN` / `ISEN` | `TCON` | `SLOP`: 6/12/18/24 | — | `APHS` | `ISRC?` |

Input-mode-dependent sensitivity: SR7265 uses the `IMODE` 0/1/2 table
(voltage, 10^6 and 10^8 V/A current); LI5640 uses `VSEN` or `ISEN`
selected from `ISRC?` (existing driver note: LI5640 manual §4.6). SR830
current input (`ISRC` 2/3) and SR5210 current mode report sensitivity as
unqualified (null) and refuse sensitivity changes. No current-input
table is assumed for them. A changed input mode between the pre-read and
readback fails the change.

With the bundled Windows VISA helper, SR5210 setting writes wait for the
GPIB command-complete bit before verification. Auto Phase first checks
the reference frequency and then waits for the auto-function bit to clear
before refreshing settings and resuming live sampling. The GUI shows that
Auto Phase is in progress during this interval. Serial polling is verified
for the official VISA path. SR5210 automatic adjustments require that path.
SR830 display changes require the affected channel outputs to be set to X/Y
(`FPOP? = 1`) so that the display change leaves the output sources unchanged.

SR7265 automatic AC gain (`AUTOMATIC`) is not controlled by this application.
If the instrument does not keep a requested AC gain, readback verification
reports the mismatch instead of displaying the request.

**LI5610 is not supported.** It is not treated as compatible with the LI5640;
no LI5610 command set has been verified.

Manufacturer references:
[SRS SR830 manual](https://www.thinksrs.com/downloads/PDFs/Manuals/SR830m.pdf),
[AMETEK 7265 instruction manual 190163-A-MNL-D](https://www.ameteksi.com/-/media/ameteksi/download_links/documentations/supportcenter/signalrecovery/instruction_manuals/190163-a-mnl-d.pdf),
[NF LI5640 manual](https://www.nfcorp.co.jp/english/sup/download/manual/mi/pdf/e_li5640.pdf),
[EG&G 5210 manual §6.4.03](https://www.artisantg.com/info/Princeton_Applied_Research_EG_G_5210_Manual_201972510325.pdf).
For SR830 see remote programming pp. 5-6 and 5-11. For SR7265 see
chapter 6.4 and appendix E, including AC gain on p. 6-11, Auto Phase on
p. 6-13, and the output filters in §6.4.03. For LI5640 see §5.6,
the reference, sensitivity, and time-constant messages. See the [hardware test matrix](../validation/v1-hardware-results.md) for the measured scope.

### ロックイン設定（日本語）

ロックインカードは**Device → ライブ表示 → Settings**の順です。Settingsには装置の読戻し（観測した入力モードの`available`表）にある値だけを表示します。未接続・Demo・処理実行中・コマンド待ちの間は無効です。選択した値はワーカーが「読取 → 検証 → 書込み → 読戻し → 照合」で適用します。

**フィルタスロープ**と**ACゲイン**は別の設定です。フィルタスロープは出力（検波後）ローパスフィルタの減衰傾度（dB/oct）で全機種にあります。ACゲインはSR7265の検波前の信号系ゲインで、0–90 dB・10 dB刻み（`ACGAIN n` = n × 10 dB）です。SR7265のみに表示し、他機種では通信せずに拒否します。

感度は入力モードに依存します。SR7265は`IMODE`、LI5640は`ISRC?`で表を選びます。SR830の電流入力（`ISRC` 2/3）とSR5210の電流モードでは、感度を未確定（null）として変更を拒否し、電流入力の表を推定しません。事前読取と読戻しの間に入力モードが変わった場合は失敗とします。SR7265の自動ACゲイン（`AUTOMATIC`）は制御しません。要求したACゲインが保持されなければ、読戻しの不一致として報告します。

**LI5610は非対応です。** LI5640互換とはみなさず、コマンド体系も未検証です。

SR5210の自動調整にはシリアルポーリング対応の公式VISA接続が必要です。SR830では、表示が変わるチャンネルの出力がX/Y固定の場合に本体表示を切り替えます。

SR830は取扱説明書の5-6・5-11ページ、SR7265は6.4節・付録E（ACゲイン6-11、自動位相6-13、出力フィルタ6.4.03）、LI5640は5.6節を参照してください。実機で確認した範囲は[実機検証表](../validation/v1-hardware-results.md)を参照してください。

## Motion presets and transport

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

Optional PM100USB/PM100D support is available from 0.5.3 via Windows x64 VISA
USBTMC. From 0.5.7, Windows x64 also supports installed Thorlabs TLPMX/TLPM drivers.
When using VISA, PM100D must be in its driver/VISA mode, and Thorlabs TLPM/TLPMX
installations can require the manufacturer's driver switcher. No switch is needed
if the installed Thorlabs driver can discover the meter. The native Rust transport
requires no Python. macOS supports the GUI and demo only; physical USB
power-meter access is unsupported. Consult the [power-meter guide](../guides/power-meter.md)
and the [PM100D operation manual](https://plexon.com/wp-content/uploads/2017/06/ThORLABS-PM100D-Power-Meter-Operation-Manual.pdf).

On 2026-11-10, a test build using the same connection code as 0.5.7 connected to
a physical meter via TLPMX in one configuration. Live display, recording, and
other transport paths were not verified on hardware.

## 日本語

以下はRustドライバーに実装した機種です。プロトコルは疑似通信でテストしていますが、すべての機器・ファームウェアの組合せを実機検証したことは意味しません。確認範囲と測定APIに含まれない旧保守操作は[互換性監査](compatibility.md)を参照してください。

| 分類 | 機種 | 接続 | 実装済みの測定操作 |
| --- | --- | --- | --- |
| ロックイン | SR7265 | GPIB | XY/Rθ取得、感度、時定数、結合、スロープ、ACゲイン、自動操作 |
| ロックイン | SR830 | GPIB | XY/Rθ取得、感度、時定数、結合、スロープ、自動操作 |
| ロックイン | SR5210 | GPIB | XY/Rθ取得、感度、時定数、スロープ、自動操作。AC結合のみ |
| ロックイン | LI5640 | GPIB | 選択したXYまたはRθのみ取得（0.4.8。従来は表示復元付き両側取得）、感度、時定数、結合、スロープ、自動操作 |
| 遅延ステージ制御 | SHOT302GS | シリアル | 位置読取、絶対移動、microstep読取、初期化・原点復帰、通常Stop |
| 遅延ステージ制御 | GSC01 / GSC01A | シリアル | 位置読取、絶対移動、microstep読取、初期化・原点復帰、通常Stop |
| スキャナ制御 | CONEX-CC | シリアル | 位置読取、絶対移動、原点復帰・初期化、Stop、設定可能なソフトウェアヒステリシス補償 |
| スキャナ制御 | CONEX-AGAP | シリアル | U/V軸制御、位置読取、絶対移動、閉ループ初期化、Stop |
| 光パワーメータ（0.5.3から） | Thorlabs PM100USB / PM100D | Windows x64：VISA USBTMC、0.5.7からThorlabs TLPMX/TLPM | 任意の逐次パワー読取・記録。実機検証済みではありません |

0.4.8ではCONEX-AGAPの到達許容差を最小移動0.001°と分離して既定0.01°とします。軸ごとの`position_tolerance`（装置単位）で変更できます。CONEX-CCの従来の許容差は維持します。

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

任意のPM100USB/PM100Dは0.5.3からWindows x64のVISA USBTMC経由で利用できます。0.5.7からは、インストール済みのThorlabs TLPMX/TLPMドライバーにも対応します。VISAで使う場合はPM100Dをdriver/VISAモードにし、Thorlabs TLPM/TLPMXを導入している環境ではメーカー提供のdriver switcherが必要なことがあります。公式ドライバーで検出できる場合は切替不要です。通信はnative RustでPythonは不要です。macOSはGUI・Demoのみで、実機USBパワーメータは未対応です。[パワーメータガイド](../guides/power-meter.md)と[PM100D操作マニュアル](https://plexon.com/wp-content/uploads/2017/06/ThORLABS-PM100D-Power-Meter-Operation-Manual.pdf)を確認してください。

2026-11-10に、0.5.7と同じ接続コードの検証版で、TLPMX経由の実機接続を1構成で確認しました。ライブ表示・記録・他の通信経路は実機では未確認です。
