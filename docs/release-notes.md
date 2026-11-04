# KohdaLab TRKR 0.4.9

## Changes / 変更点

- Windows Output now uses IV's native Explorer-style folder picker, titled **Output directory**, instead of the legacy folder tree dialog. / WindowsのOutputを従来のツリー型からIVと同じエクスプローラー形式・**Output directory**タイトルへ変更します。
- Config, Output, snapshot-save and Demo CSV dialogs have consistent task-specific titles across platforms. Snapshot saving remains **Save measurement snapshot**, reflecting TRKR's existing Save Now behavior. / Config・Output・スナップショット保存・Demo CSV読込の各ダイアログを用途別のタイトルへ統一します。TRKR固有のSave Nowは**Save measurement snapshot**とし、保存動作は維持します。
- The log identifies the dialog being opened and records cancellation, rather than leaving an opening notice behind. / ログには開く画面の用途とキャンセル完了を表示し、開いているという表示が残らないようにします。
- macOS now falls back to the nearest existing folder for a not-yet-created nested path, matching Windows. Literal bracket/apostrophe paths remain supported. / Macでも未作成の入れ子パスは最寄りの既存フォルダーから開き、角括弧・引用符を含むパスも維持します。

## Compatibility / 互換性

Existing JSON profiles and measurement files remain compatible; no migration is required. Measurement, motion, lock-in acquisition, CSV/API, ownership and Stop behavior are unchanged. Windows adds only the same `rfd` version used by IV, with existing locked dependency versions retained. File selection remains asynchronous so the GUI keeps updating; cancelling does not change the path. The current release's configuration and result directories are retained.

既存JSON設定・測定ファイルはそのまま利用でき、移行は不要です。測定・移動・ロックイン取得・CSV/API・排他制御・停止動作は変更しません。WindowsはIVと同じ`rfd`バージョンを使い、既存の依存バージョンは維持します。選択画面は非同期で開き、キャンセルしてもパスを変更しません。設定・結果の保存場所も維持します。

## Validation and downloads / 検証・配布

Targeted tests cover initial-folder fallback and literal paths; release CI runs Windows, macOS and Linux tests, strict lints and packaging checks. It builds Windows x64 Setup/portable ZIP and macOS Apple silicon DMG/ZIP and checks the isolated Windows installer lifecycle. Corresponding source/build kits, provenance and SHA-256 manifests accompany the packages. Actual GUI synthetic smoke covers all five workflows; this does not claim interactive Windows chooser or physical-instrument qualification.

初期フォルダーのフォールバック・特殊文字パスをテストし、CIで3OSのテスト・Clippy・配布チェックを行います。Windows x64インストーラー／portable ZIP、Mac Apple silicon DMG／ZIPを作成し、隔離したWindowsでインストール・削除を確認します。対応ソース・来歴・SHA-256一覧を同梱します。実GUIの合成データで全5測定を確認しますが、Windows選択画面の実画面・実機測定の検証とは区別します。

Windows artifacts remain unsigned; macOS bundles use ad hoc signing and are not notarized. / Windows版は未署名、macOS版はアドホック署名で公証はありません。
