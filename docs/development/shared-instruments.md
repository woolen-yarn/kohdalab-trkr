# Shared GPIB development mode

The updated TRKR transport can connect through the shared instrument service
developed in the KohdaLab IV source tree. This is explicit opt-in development
support, not a bundled service or hardware-qualified backend.

The service alone opens the native GPIB adapter. Different addresses can be
owned by IV and TRKR simultaneously; the same address has only one owner.
TRKR retains its existing measurement, cancellation and Stop behavior.
A client disconnect never sends global REN-off/reset/EXIT to a peer's bus.

Build the updated TRKR and the IV workspace's `kohdalab-instrument-hub` binary.
Start the service with a dedicated private state directory and an operator
device map that registers the TRKR lock-in address as `instrument` and every
output source with its exact `source` model. Unregistered addresses are refused.
See the IV repository's `docs/guides/shared-instruments-development.md` for the
service command, device-map format, safety policies and recovery procedure.

Launch this rebuilt TRKR from a shell with these variables:

```sh
export KOHDALAB_INSTRUMENT_HUB_CONFIG=/private/path/to/endpoint.json
export KOHDALAB_INSTRUMENT_HUB_CLIENT=/absolute/path/to/kohdalab-instrument-hub
```

On Windows, set the same variables in PowerShell (`$env:NAME = 'path'`) before
launching the rebuilt executable. An unavailable/incompatible service or bad
configuration is an error; TRKR never opens a local GPIB helper as fallback.
The default backend is unchanged when the configuration variable is absent.

The shared service currently supports one native NI/82357B controller and
primary GPIB addresses 1-30. Official VISA support, automatic startup/packaging,
same-instrument observation, and physical macOS/Windows validation are pending.
Without the opt-in service configuration, TRKR/IV keep whole-adapter exclusion.
Do not remove their lock files to try to share a controller.

## 日本語

TRKRの通信層は、IVソースツリーで開発中の共有機器サービスへ任意で接続できます。サービスは別途ビルド・導入が必要で、配布物への同梱・実機適合確認は行っていません。

ネイティブGPIBアダプターを開くのはサービスだけです。異なるアドレスはIVとTRKRで同時利用でき、同じアドレスの所有者は1つに制限します。TRKRの測定・キャンセル・Stop処理は維持し、クライアントの切断で他アプリのバスに対するREN解除・リセット・全体終了は行いません。

IVの`kohdalab-instrument-hub`をビルドし、専用の非公開状態フォルダーと機器マップで起動します。TRKRのロックインは`instrument`、出力Sourceは正しい`source`機種として登録し、未登録アドレスは拒否します。起動コマンド・認証・復旧手順はIVの`docs/guides/shared-instruments-development.md`を参照してください。

上記の`KOHDALAB_INSTRUMENT_HUB_CONFIG`と`KOHDALAB_INSTRUMENT_HUB_CLIENT`を設定してTRKRを起動します。WindowsではPowerShellの`$env:NAME = 'path'`で指定します。サービスや設定に問題があればエラーにし、通常のnative/VISA接続へ勝手に切り替えません。環境変数を指定しなければ従来の接続方式です。

現状は1台のnative NI/82357Bコントローラー、一次GPIBアドレス1–30が対象です。共有サービスの公式VISA対応、自動起動・同梱、同一機器の監視共有、Mac/Windowsでの実機確認は未対応です。共有モードを指定しない場合はアダプター全体を排他にします。共有のために既存のロックファイルを削除しないでください。
