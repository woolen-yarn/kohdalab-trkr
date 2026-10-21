# Native Rust workspace

The maintained TRKR application lives in this workspace. Run from the repository root:

```sh
cargo run --manifest-path Cargo.toml --locked --bin kohdalab-trkr
cargo test --manifest-path Cargo.toml --locked
```

Crates separate the experiment plan, device transport, motion safety and desktop GUI.
See the root README for source-built releases and Notebook access.

## Compatibility contracts

Profiles, `--stdio` JSON-lines commands/events, measurement CSV, and adjacent
`.run.json` metadata are the established unversioned **revision 1** contracts.
The application does not add a schema field to existing user JSON: arbitrary
metadata remains user data. Recognised Python-era profile aliases are
normalised in memory while unknown keys are preserved. A future revision must
be opt-in at its input/output boundary and keep revision-1 input readable.


## Extending devices and measurements

- Add scanner capability flags to `ScannerModel::capabilities` and actuator
  calibration defaults to the actuator descriptor in `trkr-motion`. Keep
  model-specific command syntax in `protocol`; configuration, planning and
  initialization consume capabilities rather than duplicating model checks.
- Add lock-in options, terminators and auto commands to `ModelDescriptor`.
  Keep read conversions and model-specific setting commands in the driver.
  SR7265 sensitivity still depends on the queried input mode; it must not be
  replaced with one static table.
- Add workflow names, ordering, default output names, plot counts and axis
  constraints to `WorkflowDefinition` in `trkr-core`. Frontends consume the
  same definition as plan validation. Distinct acquisition, coordinate/profile
  mappings and plot algorithms still need explicit implementations; the table
  is not a runtime plugin interface.
- Keep scan sequencing in `engine/execution.rs` and compensation planning in
  `MotionRig::plan_move`. Preflight and execution must use the same plan. The
  single instrument-owning worker executes it, preserving cancellation, STOP,
  polling deadlines and serial bus access.
- Add states/errors at their typed source, retaining the legacy string boundary
  until an explicit protocol revision is introduced. Profile migrations belong
  in `config/compatibility.rs`; do not reinterpret unknown user metadata.

Before adding a device, run the fake-bus command-order, cancellation, limits and
readback tests. Before adding a workflow, exercise every axis pair, line entry,
compensation, completion/Stop return and retained plot. Preserve the frozen
Python references and compare all existing CSV/profile outputs, excluding only
generation timestamps. Rust workspace tests, strict Clippy, Notebook client
checks and device-free GUI smoke tests cover these existing contracts.

## 日本語

保守対象のTRKRアプリはこのワークスペースにあります。リポジトリのルートで冒頭の`cargo run`・`cargo test`を実行します。crateは実験計画、機器通信、移動の安全性、GUIに分離しています。ソースからのビルドとNotebook利用はルートREADMEを参照してください。

### 互換性の契約

設定、`--stdio`のJSON-lines命令・イベント、測定CSV、隣接する`.run.json`は既存の、バージョンフィールドを持たない**revision 1**の契約です。既存ユーザーJSONにschemaフィールドを追加しません。任意メタデータはユーザーデータとして扱います。既知のPython時代の別名はメモリー内で正規化し、未知項目は保持します。将来のrevisionは入出力境界で明示選択式にし、revision 1の入力を読み続けられる必要があります。

### 機器と測定の拡張

- スキャナの機能フラグは`ScannerModel::capabilities`、アクチュエータ校正の既定値は`trkr-motion`の記述子へ追加します。機種固有の構文は`protocol`に置き、設定・計画・初期化は機種判定を重複させず機能フラグを使います。
- ロックインの選択肢、終端、auto命令は`ModelDescriptor`へ追加します。読取変換と機種固有の設定命令はdriverに保持します。SR7265の感度は取得した入力モードに依存するため、単一固定表に置換しません。
- 測定名、順序、既定保存名、プロット数、軸制約は`trkr-core`の`WorkflowDefinition`へ追加します。フロントエンドと計画検証は同じ定義を使います。個別の取得処理、座標・設定対応、描画アルゴリズムは明示実装が必要で、この表は実行時プラグインの仕組みではありません。
- スキャン順序は`engine/execution.rs`、補償計画は`MotionRig::plan_move`へ保持します。事前検証と実行は同じ計画を使い、単一の機器所有workerがキャンセル、STOP、読取期限、シリアル通信を保って実行します。
- 状態とエラーは型を定義する箇所に追加し、明示的なプロトコル改訂までは既存文字列の境界を保ちます。設定移行は`config/compatibility.rs`に置き、未知メタデータを再解釈しません。

機器追加前に疑似通信の命令順序、キャンセル、制限、読取位置をテストします。測定追加前に全軸ペア、行開始、補償、完了・Stop時の復帰、プロット保持を確認します。固定Python参照を維持し、生成時刻だけを除外して既存CSVと設定出力を比較します。Rustテスト、厳格なClippy、Notebook連携検証、実機を使わないGUIスモークテストが既存の契約を確認します。
