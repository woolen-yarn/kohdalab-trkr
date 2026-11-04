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


## Pending LI5640 mode selection / 未公開：LI5640取得モード

`GuiClient.set_lockin("display_mode", "xy")` or `"r_theta"` selects the pair through the GUI worker; it may be set before connecting. The status snapshot includes `signal_mode`. LI5640 signal objects use null for the unacquired pair, and main CSV leaves those columns blank. No components are derived. Changes sent during a running operation are serialized after it; wait for the returned receipt before starting a measurement. Other models still acquire all four components.

GUIワーカー経由でXY／Rθを選択し、接続前にも指定できます。statusの`signal_mode`で確認できます。LI5640の未取得側はAPIでnull、CSVで空欄とし、計算で補いません。実行中の処理への変更要求は後続処理となるため、返された処理結果の完了を待ってから測定を開始してください。他機種は引き続き全4成分を取得します。


Saved sample JSON includes top-level `signal_unit` and `quality_flags`. Legacy signal field names ending in `_v` are retained for compatibility; always use the explicit unit rather than interpreting the name as proof of volts. Live events also expose `signal_unit` and `unit_qualification`: hardware live values use `last_observed_status` (cached status, up to 500 ms cadence), not an atomic ADC-unit observation; Demo uses `synthetic`. Missing unit metadata in old live messages defaults to `unknown`.

保存点のJSONにはトップレベルの`signal_unit`・`quality_flags`を含めます。互換性のため信号の`_v`付きフィールド名は維持しますが、単位は名前から推測せず明示された値を使ってください。ライブイベントにも単位と`unit_qualification`を含め、実機は`last_observed_status`（最大500 ms周期の直近状態で、ADCと同時の確認ではない）、Demoは`synthetic`とします。旧ライブメッセージに単位情報がない場合は`unknown`です。
