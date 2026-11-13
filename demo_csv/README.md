# Demo CSV directory

このフォルダには、リプレイGUIで使用する実測CSVを配置します。ファイル名は
自由です。CSVの `measurement` 列からTRKR、SRKR、STRKR、SRKR 2Dを判別します。
ConfigをLoadすると、各測定につき1ファイルを自動選択します。

必要な構成:

- TRKR用CSV: 1ファイル
- SRKR用CSV: 1ファイル
- STRKR用CSV: 1ファイル
- SRKR 2D用CSV: 1ファイル

## データの取り扱い

このフォルダの `*.csv` は実験データのため、公開
`woolen-yarn/kohdalab-trkr` リポジトリではGit管理しません。`.gitignore` により
除外されています。

Kohdalab内部のDemoリポジトリへ配布するときだけ、公開リポジトリとは別の
作業コピーでCSVを追加してください。同じGit履歴にCSVをコミットしてリモートだけ
切り替える運用は、誤pushによる公開を防げないため使用しません。

## English

Place experimental CSV files used by the replay GUI in this directory. Filenames are unrestricted. The `measurement` column identifies TRKR, SRKR, STRKR, or SRKR 2D. Loading Config automatically selects one file per measurement.

Required set: one CSV each for TRKR, SRKR, STRKR, and SRKR 2D.

### Data handling

The `*.csv` files here are experimental data and are excluded from Git in the public `woolen-yarn/kohdalab-trkr` repository by `.gitignore`.

Only add CSVs in a separate working copy when distributing to the internal Kohdalab Demo repository. Do not commit them into the same Git history and merely switch remotes: that does not prevent accidental public pushes.
