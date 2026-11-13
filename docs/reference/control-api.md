# KohdaLab TRKR control API

TRKR exposes an opt-in, loopback-only JSON-lines control endpoint when launched
with its GUI API option. The descriptor is published atomically in
`~/.kohdalab/trkr/run`; it contains a per-instance token and must never be
shared outside the local user account.

The additive `control.*` namespace follows **KohdaLab Control Protocol 1**.
Use `control.describe` to discover the operations and safety policy, then
`control.acquire` with a fresh UUID `client_id`. Mutating calls require both
the returned opaque `lease` and `client_id`; renew it with `control.heartbeat`.
Existing native `action` requests remain supported for compatibility.

All long-running operations return a receipt. A timeout means the outcome is
unknown; inspect `control.result` before considering a retry. `control.stop`
requests the existing TRKR stop path and does not claim cleanup until the
native status confirms it.


## LI5640 mode selection (0.4.8) / LI5640取得モード（0.4.8）

`GuiClient.set_lockin("display_mode", "xy")` or `"r_theta"` selects the pair through the GUI worker; it may be set before connecting. The status snapshot includes `signal_mode`. LI5640 signal objects use null for the unacquired pair, and main CSV leaves those columns blank. No components are derived. Changes sent during a running operation are serialized after it; wait for the returned receipt before starting a measurement. Other models still acquire all four components.

GUIワーカー経由でXY／Rθを選択し、接続前にも指定できます。statusの`signal_mode`で確認できます。LI5640の未取得側はAPIでnull、CSVで空欄とし、計算で補いません。実行中の処理への変更要求は後続処理となるため、返された処理結果の完了を待ってから測定を開始してください。他機種は引き続き全4成分を取得します。

## Lock-in settings / ロックイン設定

`set_lockin` with `sensitivity`, `time_constant`, `slope` (output filter roll-off, dB/oct), `coupling` or `ac_gain` (SR7265 signal-channel AC gain, dB) and `auto_lockin` (`phase`, `sensitivity`, `measure`) are **rejected** with `Lock-in settings cannot be changed while a measurement or motion operation is running` while any acquisition, alignment, move, initialization or replay is running; they are no longer queued behind it. `display_mode` keeps the serialized behavior above. When idle, the worker reads the current settings, validates the value against the table for the observed input mode, writes, reads back and acknowledges only when the readback matches and the input mode is unchanged. The receipt's settings are that readback. See [lock-in settings](hardware.md#lock-in-settings) for the per-model commands.

`sensitivity`・`time_constant`・`slope`（出力フィルタの減衰傾度 dB/oct）・`coupling`・`ac_gain`（SR7265の信号系ACゲイン dB）と`auto_lockin`は、測定・アライメント・移動・初期化・リプレイの実行中は後続処理にせず**拒否**します。`display_mode`は上記の直列処理のままです。待機中は現在値を読み、観測した入力モードの表で値を検証し、書込み後に読み戻して一致し入力モードが変わっていない場合のみ完了とします。


Saved sample JSON includes top-level `signal_unit` and `quality_flags`. Legacy signal field names ending in `_v` are retained for compatibility; always use the explicit unit rather than interpreting the name as proof of volts. Live events also expose `signal_unit` and `unit_qualification`: hardware live values use `last_observed_status` (cached status, up to 500 ms cadence), not an atomic ADC-unit observation; Demo uses `synthetic`. Missing unit metadata in old live messages defaults to `unknown`.

保存点のJSONにはトップレベルの`signal_unit`・`quality_flags`を含めます。互換性のため信号の`_v`付きフィールド名は維持しますが、単位は名前から推測せず明示された値を使ってください。ライブイベントにも単位と`unit_qualification`を含め、実機は`last_observed_status`（最大500 ms周期の直近状態で、ADCと同時の確認ではない）、Demoは`synthetic`とします。旧ライブメッセージに単位情報がない場合は`unknown`です。

## Alignment (0.5.0) / Alignment（0.5.0）

Native actions below also have `control.` aliases. Mutations use the existing
owner lease and GUI worker; they never create a second instrument connection.
Use `control.describe` to check support. Motion calls return ordinary receipts;
wait for completion, or use `stop_and_wait()` to request and confirm a stop.

以下の操作には`control.`付きの共通APIもあります。変更は既存の操作権限とGUIの
制御処理を使用し、装置へ別途接続しません。対応状況は`control.describe`で確認できます。
移動操作は実行レシートを返します。完了を待ち、停止には`stop_and_wait()`を使用できます。

| Action / 操作 | Parameters and result / 引数と結果 |
| --- | --- |
| `alignment_layout` | Read `{delays: [...], scanners: [...]}`. Each panel contains `id`, nullable `device_id`, `unit`, and `step`. / パネル一覧を取得します。 |
| `set_alignment_layout` | `layout`: replace the entire layout while idle. Up to 2 delays and 4 scanners; unique panel IDs and device bindings. Invalid input leaves the previous layout intact. / 待機中に一括変更。不正な入力では旧設定を維持します。 |
| `alignment` | `kind`, `device_id`, `motion`: run Step, Jog, End or Initialize on a connected axis. / 接続済み軸で移動・初期化します。 |
| `alignment_reverse` | `kind`, `device_id`, `axis`: reverse Direction while preserving displayed coordinates. / 表示座標を維持してDirectionを反転します。 |
| `alignment_keepalive` | `run_id`: renew only that owner's active Jog receipt. / 自分が開始した実行中Jogのみ継続します。 |

`kind` is `delay` or `scanner`; `axis` is 0 for delay, 1 for scanner X and 2 for
scanner Y. `unit` is `physical` (ps or µm), `corrected` (offset coordinates), or
`native` (controller position units). Panel `step` must be finite and nonnegative;
zero disables motion. Step/Jog API `amount` is finite, nonzero and **signed**.

`kind`は`delay`／`scanner`、`axis`はDelayが0、Scanner Xが1、Yが2です。
`unit`は`physical`（ps／µm）・`corrected`（補正座標）・`native`（制御器の位置単位）。
パネルの`step`は有限の非負数で、0では移動を無効にします。APIのStep／Jogの`amount`は
有限・非ゼロの**符号付き移動量**です。

`motion` uses one of these shapes:

```json
{"action":"step","axis":1,"amount":2.0,"unit":"physical"}
{"action":"jog","axis":0,"amount":-1.0,"unit":"physical"}
{"action":"end","axis":0,"positive":true}
{"action":"initialize","axis":1,"home":false,"move_to_origin":true}
```

Jog requires an explicit keepalive about every 100 ms; a 300 ms lapse requests
Stop and prevents further segments. Owner release also stops Jog, even when
ordinary measurements use the default continue-on-disconnect policy. Ordinary
heartbeat cannot renew Jog. Stop, expiry and completed receipts cannot be
revived by later keepalives or duplicate request IDs. Physical stopping latency
still depends on the controller and transport. Demo mode and unconnected axes
reject motion. Panel changes do not connect devices or change measurement bindings.

Jogは約100 msごとの専用継続通知を必要とし、300 ms途切れるとStopを要求して次の
移動区間を開始しません。操作権限の解放でもJogを停止します。これは通常測定の
「通信断時も継続」の既定動作とは独立です。通常のheartbeatではJogを延長できず、
停止・期限切れ・完了した処理を後続の通知や重複要求で再開できません。実際の停止遅延は
制御器と通信に依存します。Demo・未接続軸の移動は拒否します。パネル変更で装置接続や
測定機器の割当は変わりません。

Layout changes enter the current profile; **Config → Save** persists them.
See the [Python examples](../../bindings/python/README.md).

パネル変更は現在の設定に反映し、**Config → Save**でファイルに保存します。
[Pythonの使用例](../../bindings/python/README.md)も参照してください。
