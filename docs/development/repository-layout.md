# Repository layout

KohdaLab IV, TRKR, and Camera use the same organizing principles.

| Location | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | One authoritative Rust workspace and dependency lock at the root |
| `crates/<package>/` | Rust packages; implementation and package-owned resources stay together |
| `bindings/`, `notebook/` | Optional scripting/Notebook interfaces, when the product provides them |
| `native/` | Native helper sources and upstream licensing, when required |
| `packaging/` | Static installer/driver-setup inputs, when required |
| `scripts/` | Build, package, source-identity, and validation entry points |
| `tests/` | Cross-package packaging/compatibility checks, when applicable |
| `docs/` | User guides, development notes, references, validation evidence, and images |
| `target/`, `build/`, `dist/` | Ignored local build outputs; never source inputs |

Optional directories exist only when they have a real role. Native backends and
public package/bin names are product-specific; this layout does not introduce a
shared runtime dependency between repositories. Cargo commands run from the root.
The former `next/` preview workspace is no longer the maintained source location.
Existing installed application settings and distribution layouts are unchanged.

## Documentation rules

The root README is a short product page: overview, interface, downloads, supported
hardware, quick start, development, and documentation links, in that order.
Detailed procedures belong in `guides/`; contributor architecture and build
maintenance in `development/`; stable protocol/format/support descriptions in
`reference/`; dated/versioned measurements in `validation/`; figures in `images/`.
`docs/README.md` is the navigation index. Do not copy the full manual back into
the landing page. Preserve hardware qualification limits and historical evidence.

## Changes to paths

Move a file and update its consumers together: Cargo/build scripts, helper lookup,
CI path filters, packaging source allowlists, client discovery, examples, and
documentation links. Keep installed resource paths and public executable names
stable. Compare package metadata and dependency locks, run device-free checks,
and inspect source archives so ignored outputs or private data cannot leak.

## 日本語

KohdaLab IV・TRKR・Cameraは同じ整理方針を採用します。

| 配置 | 責務 |
| --- | --- |
| `Cargo.toml`・`Cargo.lock` | ルートに置く唯一のRustワークスペースと依存関係の固定情報 |
| `crates/<package>/` | Rustパッケージ。実装とパッケージ固有のリソースをまとめる |
| `bindings/`・`notebook/` | 製品が提供する場合のスクリプト・Notebookインターフェース |
| `native/` | 必要な場合のネイティブhelperソースと上流ライセンス |
| `packaging/` | 必要な場合の静的インストーラー・ドライバー設定入力 |
| `scripts/` | ビルド・配布・ソース識別・検証の入口 |
| `tests/` | 必要な場合のパッケージ横断の配布・互換性検証 |
| `docs/` | 操作ガイド・開発文書・仕様・検証記録・画像 |
| `target/`・`build/`・`dist/` | Git対象外のローカル出力。ソース入力には使わない |

任意のディレクトリは実際の役割がある場合だけ作成します。ネイティブ実装と公開パッケージ・実行ファイル名は製品固有です。この構成によりリポジトリ間の実行時依存を追加することはありません。Cargoはルートで実行します。旧`next/`は保守対象のソース配置ではありません。インストール済みアプリの設定と配布先構成は変えません。

### 文書のルール

ルートREADMEは、概要・画面・配布・対応機器・導入・開発・文書案内の順に並ぶ簡潔な製品ページにします。詳細な操作は`guides/`、設計とビルド保守は`development/`、安定した通信・形式・対応情報は`reference/`、日付・バージョン付きの測定記録は`validation/`、図は`images/`に置きます。`docs/README.md`が索引です。操作マニュアル全体をトップページへ複製しません。実機検証の範囲と履歴証跡を保持します。

### パスを変更する場合

ファイルを移動するときは、Cargo・ビルド、helper探索、CI対象パス、ソース収録リスト、クライアント探索、例、文書リンクをまとめて更新します。導入後のリソースパスと公開実行ファイル名は維持します。パッケージ情報と依存関係を比較し、実機を使わない検証を実行し、ソースアーカイブにローカル出力や私有データが混入していないことを確認します。

## Language policy / 言語方針

Maintain English and Japanese together. Use paired headings and prose on the
landing page and full counterpart sections for longer documents. Translate
conditions, tables, validation limits, and safety caveats as well as summaries.
Update both languages in the same change. Keep commands, API identifiers,
configuration keys, and data schemas unchanged. Immutable historical evidence
and exact upstream license texts remain original; provide bilingual navigation
and explanations around them.

英語と日本語を同時に保守します。トップページは見出し・段落を併記し、長い文書は
全文の対訳節に分けます。概要だけでなく、条件・表・検証範囲・安全上の注意も訳し、
両言語を同じ変更で更新します。コマンド、API識別子、設定キー、データ形式は
変更しません。不変の履歴証跡と上流ライセンス本文は原本を保持し、周囲の案内と
説明を英日併記にします。
