# Promote release artifacts / 配布物の昇格

## Select the build

1. Open a release PR that updates the workspace and Python versions, `CITATION.cff`,
   `CHANGELOG.md` and [release notes](../release-notes.md). Confirm that **Test**,
   **Documentation** and **Build release artifacts** pass on the release PR, then merge it.
2. Record the full SHA of the release commit on `main`, the matching `v<version>`
   tag and the successful build run ID. Find the run ID in its Actions URL or use
   `gh run list --workflow "Build release artifacts"`. For a PR run, select the
   run for the merged PR's final head. The verifier compares the tree built by
   Actions with the release commit and compares both platform source archives.
3. Start a fresh **Build release artifacts** run on `main` while the intended
   commit is its tip if the PR artifacts have expired (retention is 14 days),
   the build tree differs or is unavailable, or the platform artifacts come
   from different attempts. Rerunning only one platform can leave mixed-attempt
   artifacts, which cannot be promoted. Use the fresh run ID and its exact
   commit SHA. The verifier requires the latest matching **Test** and
   **Documentation** runs to succeed, using the PR head for PR builds and
   the push to the release commit on `main` for manual builds.

## Verify and create a draft

In Actions, run **Promote release artifacts** on `main`. Enter the build's
`run_id`, the full release `commit` and `tag`. A blank `commit` uses the tip
of `main` at dispatch. Supply `hardware_evidence` only if that commit contains
the repository-relative record for this version. Its Result table must have
at least one `pass` and zero `fail` rows for `hardware_tested` to be true.

Keep **dry_run** checked to verify the inputs without writing a Release. This
mode allows an existing tag or Release for historical reproduction. Inspect
`verified-release`, including `RELEASE-VERIFICATION.json` and `SHA256SUMS.txt`.
For a new version, repeat with the same explicit inputs and **dry_run** unchecked
to check tag and Release availability and create a draft. If a verification step fails, read **Verify the build and stage its assets**
for the first rejected input or artifact. A draft creation or upload failure can
leave an incomplete draft. Inspect it in Releases before retrying.

The same verification can be run from a Git checkout with Python 3.11+, an
authenticated GitHub CLI (`gh auth login`) and a current `origin/main`.
Set `RUN_ID`, `COMMIT` and `TAG` to the values above, then run from the repo root:

```bash
git fetch origin
stage=$(mktemp -d)
python3 scripts/promote_release.py verify --run "$RUN_ID" --commit "$COMMIT" \
  --tag "$TAG" --output "$stage/assets" &&
git show "$COMMIT:docs/release-notes.md" > "$stage/release-notes.md"
```

Add `--hardware-evidence PATH` to `verify` if applicable. After verification,
check in GitHub that there is no existing draft, published Release or tag for
`$TAG`. Then run:

```bash
gh release create "$TAG" "$stage/assets/"* --draft --target "$COMMIT" \
  --notes-file "$stage/release-notes.md"
```

For a read-only reproduction against an already published release, run only
`verify` with `--allow-existing-release` and use a new output directory. The
original artifacts must still be available.

Before publishing, confirm the draft target commit, both OS checksum manifests,
packaged versions and corresponding source. Check the Windows installer lifecycle
results and the extracted Mac app's signature and launch. Publish the verified
draft in GitHub, which creates the tag at the specified commit. Tag pushes do
not build packages. Leave already published release files intact.

## ビルドを選ぶ

1. ワークスペースとPythonの版番号、`CITATION.cff`、`CHANGELOG.md`、
   [リリースノート](../release-notes.md)を更新するリリースPRを作ります。
   **Test**、**Documentation**、**Build release artifacts** がすべて成功したらマージします。
2. `main` にあるリリース対象コミットの完全なSHA、対応する `v<version>` タグ、
   成功した配布ビルドのrun IDを控えます。run IDはActionsのURLか
   `gh run list --workflow "Build release artifacts"` で確認できます。
   PRの場合はマージ済みPRの最終headのrunを選びます。検証では、Actionsがビルドした
   treeとリリース対象のコミットを照合し、両OSのソースアーカイブも確認します。
3. PRの配布物が失効した場合（保存期間は14日間）、ビルド時のtreeが一致しないか
   取得できない場合、または両OSの配布物が異なる試行で作られた場合は、対象コミットが
   `main` の先頭にある間に **Build release artifacts** を新規に手動実行します。
   片方のOSだけを再実行すると、異なる試行の配布物が混在し、昇格できません。
   新規runのIDとビルドしたコミットの完全なSHAを使います。検証では、PRビルドなら
   PRのhead、手動ビルドならリリース対象コミットの `main` へのpushについて、
   対応する最新の **Test** と **Documentation** の成功を確認します。

## 検証して下書きを作る

Actionsで **Promote release artifacts** を `main` から手動実行します。
`run_id`、リリース対象の完全な `commit`、`tag` を入力します。
`commit` が空欄なら実行を指示した時点の `main` の先頭を使用します。
`hardware_evidence` は、対象版の実機記録が対象コミットに含まれている場合に、
リポジトリ内の相対パスで指定します。Result表に `pass` が1行以上あり、
`fail` が0行のときに `hardware_tested` がtrueになります。

**dry_run** を有効のまま実行すると、Releaseを作成せずに検証できます。
過去の版も再現できるよう、既存のタグやReleaseがあっても検証できます。
`verified-release` の `RELEASE-VERIFICATION.json` と `SHA256SUMS.txt` を
確認してください。新しい版では同じ値を明示して **dry_run** を無効にし、
タグとReleaseの未作成を確認してから下書きを作ります。
検証失敗時は **Verify the build and stage its assets** のログで最初に拒否された入力や
ファイルを確認します。下書き作成やアップロードが失敗した場合は、ファイルが揃って
いない下書きが残ることがあります。再実行前にReleasesで確認してください。

手元でも、Gitのチェックアウト、Python 3.11以降、認証済みのGitHub CLI
（`gh auth login`）、最新の `origin/main` があれば同じ検証ができます。
上のコマンドの `RUN_ID`、`COMMIT`、`TAG` に控えた値を設定し、
リポジトリ直下で実行してください。実機記録を使う場合は `verify` に
`--hardware-evidence PATH` を追加します。検証後、同じタグの下書き、公開済みRelease、
タグが存在しないことをGitHubで確認してから、上の `gh release create` を実行します。

公開済みの版で検証を再現する場合は、配布物が有効なうちに、新規出力先を指定して
`--allow-existing-release` 付きで `verify` だけを実行してください。

公開前に下書きの対象コミット、両OSのチェックサム、同梱版番号、対応ソースを照合します。
Windowsインストーラーのライフサイクル検証結果と、展開したMacアプリの署名・起動も
確認します。GitHubで検証済みの下書きを公開すると、指定したコミットにタグが作成されます。
タグをpushしても配布物はビルドされません。公開済みリリースのファイルは差し替えないでください。
