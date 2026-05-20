# KohdaLab Web UI

Japanese version: [`web_ui_ja.md`](web_ui_ja.md).

`kohdalab-web` is the browser UI for occasional remote operation. The intended
split is to keep the existing desktop GUI (`kohdalab-gui`) for local operation
and start the Web UI only when remote access is needed.

```text
local:  uv run kohdalab-gui
remote: uv run kohdalab-web
```

The Web UI owns one `Experiment(..., auto_connect=False)` on the measurement PC
and calls the same `kohdalab.api` layer as the desktop GUI. The browser never
talks to instruments directly.

## Start

For same-PC access:

```powershell
uv run kohdalab-web --config config\kikuchi.json
```

After a config has been loaded once, later launches can omit `--config` and
reuse the last config automatically.

```powershell
uv run kohdalab-web
```

Startup config resolution order:

```text
--config
KOHDALAB_CONFIG
last loaded config
lab default
no config selected
```

If the lab default does not exist, the Web UI still starts with no config
selected. Enter a profile in Session `Config path` and click `Load`.

Open:

```text
http://127.0.0.1:8765
```

For remote access, prefer an SSH tunnel.

Measurement PC:

```powershell
uv run kohdalab-web --host 127.0.0.1 --port 8765
```

Client PC:

```bash
ssh -L 8765:127.0.0.1:8765 user@measurement-pc
```

Client browser:

```text
http://127.0.0.1:8765
```

Use `--host 0.0.0.0` only when you intentionally need LAN access. Do not expose
the instrument-control UI to the public internet.

## Current Scope

The Web UI follows the same operator surface as the desktop GUI:

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

## Operation Rules

Do not run `kohdalab-gui` and `kohdalab-web` at the same time. Separate
processes can compete for Serial/GPIB/VISA handles.

Normal use:

```text
local day:  run only kohdalab-gui
remote day: run only kohdalab-web
```

Like the GUI, the Web UI uses `auto_connect=False`. If required devices are
missing before Start, it shows an error instead of connecting implicitly.

For multiple users, keep real operating profiles in a measurement-PC folder
rather than treating the repo sample config as the only editable profile.

```text
C:\jupKernel\kohdalab\configs\
  default.json
  kikuchi.json
  sato.json
```
