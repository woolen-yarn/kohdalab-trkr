# Windows Instrument PC Setup

This checklist prepares a Windows PC for running KohdaLab TRKR with real
instruments.

## 1. Install System Tools

Install Git:

```powershell
winget install --id Git.Git -e --source winget
```

Install uv:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
```

Open a new PowerShell and check:

```powershell
git --version
uv --version
```

## 2. Install Instrument Drivers

Install the drivers needed by the connected instruments:

- NI-VISA or Keysight VISA runtime
- GPIB adapter driver
- USB serial drivers for delay stages and scanners

Confirm COM ports and VISA resources before starting Python.

## 3. Clone and Sync

```powershell
New-Item -ItemType Directory -Force -Path $HOME\pythonKernel
Set-Location $HOME\pythonKernel
git clone https://github.com/Kohdalab/kohdalab-trkr.git kohdalab-trkr
Set-Location kohdalab-trkr
uv sync --all-extras
```

## 4. Create a Local Config

```powershell
Copy-Item config\default.json config\instrument.local.json
```

Edit `config\instrument.local.json` for this PC's VISA and COM resource
strings. Local configs are ignored by Git.

## 5. Check Hardware Safely

1. Confirm coordinate systems and travel limits.
2. Connect devices in the GUI.
3. Use `Read Live`.
4. Move each axis over a tiny range.
5. Run a short signal monitor.
6. Run each scan mode with a small point count before real ranges.

## 6. Start GUI or Notebook

GUI:

```powershell
uv run kohdalab-gui
```

Notebook:

```powershell
uv run jupyter lab
```
