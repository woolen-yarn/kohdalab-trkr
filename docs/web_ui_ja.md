# KohdaLab Web UI

English version: [`web_ui.md`](web_ui.md).

`kohdalab-web` は remote 操作用の browser UI です。通常の local 実験では
既存の desktop GUI (`kohdalab-gui`) を使い、remote のときだけ Web UI を
起動する想定です。

```text
local:  uv run kohdalab-gui
remote: uv run kohdalab-web
```

Web UI は測定 PC 上で 1 つの `Experiment(..., auto_connect=False)` を持ち、
desktop GUI と同じ `kohdalab.api` を呼びます。ブラウザは装置を直接触らず、
測定 PC 上の Python server に命令を送ります。

## 起動

同じ測定 PC から使う場合:

```powershell
uv run kohdalab-web --config config\kikuchi.json
```

一度 config を読み込んだ後は、次回から `--config` なしでも前回の config を
自動で開きます。

```powershell
uv run kohdalab-web
```

起動時の config 解決順:

```text
--config
KOHDALAB_CONFIG
前回読み込んだ config
lab default
未選択
```

`lab default` が存在しない場合でも Web UI は未選択状態で起動します。Session
の `Config path` に使いたい profile を入れて `Load` してください。

ブラウザで開く URL:

```text
http://127.0.0.1:8765
```

remote では SSH tunnel 推奨です。

測定 PC 側:

```powershell
uv run kohdalab-web --host 127.0.0.1 --port 8765
```

手元 PC 側:

```bash
ssh -L 8765:127.0.0.1:8765 user@measurement-pc
```

手元ブラウザ:

```text
http://127.0.0.1:8765
```

LAN に直接出す必要がある場合だけ `--host 0.0.0.0` を使います。装置制御 UI
なので、public internet には公開しないでください。

## 現在の範囲

Web UI は desktop GUI と同じ運用単位に合わせています。

- config load/save
- Connect All / Disconnect All
- individual device connect/disconnect
- delay stage / scanner initialize
- Read Live
- t/x/y manual move
- Signal Monitor / TRKR / SRKR / STRKR / SRKR 2D start/stop
- measurement progress, table, simple plot
- Save Now
- log and snapshot

## 運用ルール

`kohdalab-gui` と `kohdalab-web` は同時に起動しないでください。Serial/GPIB/VISA
handle を別プロセスで同時に掴む可能性があります。

通常運用:

```text
local で操作する日:  kohdalab-gui だけ起動
remote で操作する日: kohdalab-web だけ起動
```

Web UI も GUI と同じく `auto_connect=False` です。Start 前に必要 device が
missing の場合、暗黙接続せずエラーにします。

複数ユーザーで使う場合、実運用 profile は repo 内の sample config ではなく
測定 PC 側の共有フォルダに置く運用が便利です。

```text
C:\jupKernel\kohdalab\configs\
  default.json
  kikuchi.json
  sato.json
```
