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
