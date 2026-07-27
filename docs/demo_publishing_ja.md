# Demo版のGitHub公開手順

Demo版はコードと実験CSVで公開範囲が異なります。

## woolen-yarnへ公開する内容

`woolen-yarn/kohdalab-trkr` にはコード、テスト、ドキュメント、Demo起動用VBSを
公開します。`demo_csv/*.csv` は実験データなので公開しません。

公開前に次を実行し、何も表示されないことを確認します。

```powershell
git ls-files "demo_csv/*.csv"
```

CSVはルートの `.gitignore` で除外されています。`demo_csv/README.md` は
データ配置方法を示すため公開します。

## Kohdalab内部へデータ込みで公開する内容

Kohdalab用はwoolen-yarn用の作業ディレクトリと分離します。別フォルダへ新しく
cloneし、Kohdalab用GitHubリポジトリだけを `origin` に設定してください。

```powershell
git clone https://github.com/woolen-yarn/kohdalab-trkr.git kohdalab-trkr-demo-internal
Set-Location kohdalab-trkr-demo-internal
git remote set-url origin <Kohdalab内部リポジトリURL>
```

その別作業コピーへ承認済みCSVをコピーし、明示的に追加します。

```powershell
Copy-Item <実測CSV保存場所>\*.csv .\demo_csv\
git add -f .\demo_csv\*.csv
git status --short
```

`git add -f` はKohdalab内部用の別作業コピーだけで実行します。現在の
woolen-yarn作業コピーでは実行しません。

## 最終確認

公開先ごとに以下を確認します。

- `git remote -v` が意図したリポジトリだけを示している
- woolen-yarn側では `git ls-files "demo_csv/*.csv"` が空
- Kohdalab側では追加する4ファイルだけが表示される
- `.venv`、ログ、キャッシュ、PC固有configが含まれていない
- `uv run pytest --cov --cov-branch -q` が成功する
