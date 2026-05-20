KohdaLab-TRKR
============

KohdaLab-TRKR is organized around a small public API for laboratory control.

For practical examples, see `docs/api_usage.md`.

Quick Start (譌･譛ｬ隱・
-------------------

譁ｰ縺励＞ PC 縺ｧ GUI 貂ｬ螳壹ｒ蟋九ａ繧区怙遏ｭ謇矩・〒縺吶８indows PowerShell 繧呈Φ螳壹＠縺ｦ縺・∪縺吶・讓呎ｺ悶〒縺ｯ `C:\pythonKernel\kohdalab-trkr` 縺ｫ鄂ｮ縺阪∪縺吶・
### 1. uv 繧偵う繝ｳ繧ｹ繝医・繝ｫ

蜈ｬ蠑・installer:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
```

繧､繝ｳ繧ｹ繝医・繝ｫ蠕後∵眠縺励＞ PowerShell 繧帝幕縺・※遒ｺ隱阪＠縺ｾ縺吶・
```powershell
uv --version
```

uv 縺ｮ蜈ｬ蠑・install docs:

```text
https://docs.astral.sh/uv/getting-started/installation/
```

### 2. GitHub repo 繧貞叙蠕・
```powershell
New-Item -ItemType Directory -Force -Path C:\pythonKernel
Set-Location C:\pythonKernel
git clone https://github.com/Kohdalab/kohdalab-trkr.git kohdalab-trkr
Set-Location kohdalab-trkr
```

`C:\pythonKernel` 縺ｮ菴懈・縺ｧ讓ｩ髯舌お繝ｩ繝ｼ縺悟・繧・PC 縺ｧ縺ｯ縲∵怙蛻昴・ 2 陦後□縺台ｻ･荳九↓鄂ｮ縺肴鋤縺医∪縺吶・
```powershell
New-Item -ItemType Directory -Force -Path $HOME\pythonKernel
Set-Location $HOME\pythonKernel
```

縺吶〒縺ｫ zip 繧・USB 縺ｧ repo folder 繧呈戟縺｣縺ｦ縺阪◆蝣ｴ蜷医・縲√◎縺ｮ folder 縺ｫ遘ｻ蜍輔☆繧後・ OK 縺ｧ縺吶・
### 3. 萓晏ｭ倡腸蠅・ｒ菴懈・

GUI 縺ｨ notebook 繧ゆｽｿ縺・壼ｸｸ繧ｻ繝・ヨ繧｢繝・・:

```powershell
uv sync --all-extras
```

髢狗匱繧・test 繧り｡後≧ PC 縺ｧ縺ｯ:

```powershell
uv sync --all-extras --group dev
```

### 4. 螳滓ｩ・PC 縺ｫ蠢・ｦ√↑螟夜Κ driver 繧堤｢ｺ隱・
Python package 縺ｨ縺ｯ蛻･縺ｫ縲∝ｮ滓ｩ・PC 蛛ｴ縺ｫ莉･荳九′蠢・ｦ√〒縺吶・
- NI-VISA 縺ｾ縺溘・ Keysight VISA 縺ｪ縺ｩ縺ｮ VISA runtime
- delay stage / scanner 逕ｨ縺ｮ USB serial driver
- Windows 縺ｮ Device Manager 縺ｧ COM port 縺瑚ｦ九∴繧九％縺ｨ
- lock-in 縺ｮ GPIB/VISA resource 縺瑚ｦ九∴繧九％縺ｨ

### 5. GUI 繧定ｵｷ蜍・
```powershell
uv run kohdalab-gui
```

GUI 縺瑚ｵｷ蜍輔＠縺溘ｉ:

1. `Load` 縺ｧ config 繧堤｢ｺ隱阪＠縺ｾ縺吶・2. Lock-in resource 縺ｨ蜷・COM port 繧・`Refresh` 縺励※驕ｸ縺ｳ逶ｴ縺励∪縺吶・3. `Save` 縺ｧ縺昴・ PC 逕ｨ縺ｮ config 縺ｫ菫晏ｭ倥＠縺ｾ縺吶・4. `Connect All` 縺ｾ縺溘・蛟句挨 `Connect` 縺ｧ謗･邯壹＠縺ｾ縺吶・5. `Read Live` 縺ｧ live status 縺梧峩譁ｰ縺輔ｌ繧九％縺ｨ繧堤｢ｺ隱阪＠縺ｾ縺吶・6. 蟆上＆縺・ｯ・峇縺ｧ `Signal Monitor`縲～TRKR`縲～SRKR`縲～STRKR`縲～SRKR 2D` 繧定ｩｦ縺励∪縺吶・
螳滓ｩ溽｢ｺ隱・checklist 縺ｯ `docs/hardware_smoke_test_ja.md` 繧剃ｽｿ縺｣縺ｦ縺上□縺輔＞縲・
### 6. CLI / notebook 繧ゆｽｿ縺医∪縺・
CLI:

```powershell
uv run kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json trkr
```

Notebook:

```powershell
uv run jupyter lab
```

maintained notebooks:

- `notebook/move_abs_notebook.ipynb`
- `notebook/signal_monitor_notebook.ipynb`
- `notebook/trkr_notebook.ipynb`
- `notebook/srkr_notebook.ipynb`
- `notebook/strkr_notebook.ipynb`
- `notebook/srkr_2d_notebook.ipynb`

GUI 縺ｯ螳牙・縺ｮ縺溘ａ `auto_connect=False` 縺ｧ縲∝・縺ｫ譏守､ｺ謗･邯壹＠縺ｦ縺九ｉ貂ｬ螳壹＠縺ｾ縺吶・LI 縺ｨ notebook 縺ｯ譌｢螳壹〒 `auto_connect=True` 縺ｪ縺ｮ縺ｧ縲∝ｿ・ｦ・device 繧定・蜍墓磁邯壹＠縺ｫ陦後″縺ｾ縺吶・
Quick Start (English)
---------------------

This is the shortest path for starting GUI measurements on a new PC. The
commands assume Windows PowerShell. The standard location is
`C:\pythonKernel\kohdalab-trkr`.

### 1. Install uv

Official installer:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
```

Open a new PowerShell and verify the install:

```powershell
uv --version
```

Official uv installation docs:

```text
https://docs.astral.sh/uv/getting-started/installation/
```

### 2. Clone the GitHub repository

```powershell
New-Item -ItemType Directory -Force -Path C:\pythonKernel
Set-Location C:\pythonKernel
git clone https://github.com/Kohdalab/kohdalab-trkr.git kohdalab-trkr
Set-Location kohdalab-trkr
```

If creating `C:\pythonKernel` fails because of permissions, replace only the
first two lines with:

```powershell
New-Item -ItemType Directory -Force -Path $HOME\pythonKernel
Set-Location $HOME\pythonKernel
```

If the repository was copied by zip or USB storage, just open PowerShell in
that folder instead.

### 3. Create the Python environment

Standard setup for GUI and notebooks:

```powershell
uv sync --all-extras
```

For development and tests:

```powershell
uv sync --all-extras --group dev
```

### 4. Install hardware drivers on the instrument PC

Python dependencies are not enough for real hardware. The instrument PC also
needs:

- NI-VISA, Keysight VISA, or another working VISA runtime
- USB serial drivers for the delay stage and scanners
- visible COM ports in Windows Device Manager
- visible GPIB/VISA resources for the lock-in

### 5. Start the GUI

```powershell
uv run kohdalab-gui
```

After the GUI opens:

1. Check the config with `Load`.
2. Use `Refresh` to select the lock-in resource and COM ports for this PC.
3. Save the PC-specific config with `Save`.
4. Connect devices with `Connect All` or individual `Connect` buttons.
5. Use `Read Live` to confirm live status updates.
6. Start with small ranges for `Signal Monitor`, `TRKR`, `SRKR`, `STRKR`, and
   `SRKR 2D`.

Use `docs/hardware_smoke_test.md` for the hardware verification checklist.

### 6. CLI and notebooks are also available

CLI:

```powershell
uv run kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json trkr
```

Notebook:

```powershell
uv run jupyter lab
```

Maintained notebooks:

- `notebook/move_abs_notebook.ipynb`
- `notebook/signal_monitor_notebook.ipynb`
- `notebook/trkr_notebook.ipynb`
- `notebook/srkr_notebook.ipynb`
- `notebook/strkr_notebook.ipynb`
- `notebook/srkr_2d_notebook.ipynb`

The GUI uses `auto_connect=False` for safety, so devices must be explicitly
connected before a measurement starts. CLI and notebooks use
`auto_connect=True` by default and may connect required devices automatically.

Layering
--------

The intended dependency direction is:

1. `kohdalab.instruments`
   Low-level device drivers. These modules talk directly to VISA, serial,
   sockets, or vendor-specific command sets.

2. `kohdalab.interfaces`
   Device-level control APIs. These modules normalize controller differences,
   units, limits, connection reuse, and convenience operations.

3. `kohdalab.api`
   Public workflow API. Notebook, GUI, CLI, and future Web UI code should call
   this layer. `Experiment` owns config, device sessions, live status, moves,
   and measurement runs.

4. `kohdalab.apps`
   User-facing applications. Apps should stay thin and delegate device and
   measurement behavior to `kohdalab.api`.

Design Notes
------------

- Keep dependencies flowing downward only: apps/notebooks -> api ->
  interfaces -> instruments.
- Scanner interfaces use each actuator's native `pos_unit` such as mm or deg.
  SRKR APIs expose sample positions in um, while TRKR APIs expose delay
  positions in ps.
- Measurement scans choose the input layer with `coordinate`: `measurement` or
  `interface` for new scanner/SRKR code, with `instrument` still accepted as a
  compatibility alias. For TRKR these correspond to ps, stage mm, and pulse.
  For SRKR, `measurement` is sample um and `interface` is scanner actuator
  mm/deg.
- Scanner sample conversion is configured with `sample_um_per_unit`.
- Hardware home/origin belongs to the control layer. Measurement coordinates
  use the middle of each device's min/max travel as zero. For example,
  TRA12CC `0-12 mm` maps `x_um = 0` to `scanner_mm = 6.0`, and TRKR uses the
  middle of the delay-stage travel as `t_ps = 0`.
- Keep instrument-specific command quirks in `instruments`.
- Put reusable UI-independent orchestration in `api` so notebooks, GUI, CLI,
  and Web UI can share the same behavior.

Python API
----------

```python
from kohdalab.api import Experiment, load_config, trkr_plan_from_config

config = load_config("src/kohdalab/config/trkr_config_kikuchi.json")
experiment = Experiment(config)
experiment.connect_all()
status = experiment.read_live_status()
experiment.move_delay_stage(0.0, coordinate="measurement")
plan = trkr_plan_from_config(config)
rows = experiment.run_trkr(plan=plan)
experiment.disconnect_all()
```

CLI fallback
------------

The everyday GUI entry point is `kohdalab-gui`. The same measurement runners
can also be started from a terminal:

For post-change hardware verification, use `docs/hardware_smoke_test.md`.

```powershell
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json signal-monitor
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json trkr
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json srkr --axis x
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json strkr --fast-axis t --slow-axis x
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json srkr-2d --fast-axis x --slow-axis y
kohdalab-cli --config src\kohdalab\config\trkr_config_kikuchi.json move-abs --axis x --coordinate measurement --value 10
```

If the package script is not installed, run the module directly:

```powershell
$env:PYTHONPATH='src'
uv run python -m kohdalab.api.cli --config src\kohdalab\config\trkr_config_kikuchi.json trkr
```

The CLI prints start/status/point progress, writes measurement rows to the
output path configured for each measurement, prints the final saved path, and
prints errors to stderr. It keeps the notebook/CLI-friendly behavior of
`Experiment`, including automatic connection of devices used by the command.

Notebook entry points
---------------------

The maintained notebooks are:

- `notebook/move_abs_notebook.ipynb`
- `notebook/signal_monitor_notebook.ipynb`
- `notebook/trkr_notebook.ipynb`
- `notebook/srkr_notebook.ipynb`
- `notebook/strkr_notebook.ipynb`
- `notebook/srkr_2d_notebook.ipynb`

These notebooks, `kohdalab-cli`, and `kohdalab-gui` all call the same
`kohdalab.api.Experiment` facade and measurement plan builders.

