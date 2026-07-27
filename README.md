# KohdaLab TRKR

[![Test](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml/badge.svg)](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Python](https://img.shields.io/badge/python-3.13%2B-blue)

KohdaLab TRKR is a Python toolkit for time-resolved Kerr rotation experiments, laboratory instrument control, and reproducible measurement data acquisition.

## What It Does

- Provides a typed Python API for experiment control.
- Includes GUI and CLI entry points for lab workflows.
- Supports Signal Monitor, TRKR, SRKR, STRKR, and SRKR 2D workflows.
- Validates scan targets before opening hardware sessions.
- Stores UTC timestamps and provenance metadata with measurement CSV files.
- Tests drivers with simulated transports without requiring hardware for every check.

## Quick Start

For a new PC, start with the setup guide:

- [Initial setup](docs/initial_setup.md)

For repository development:

```powershell
uv sync --all-extras --group dev --frozen
uv run kohdalab-cli --help
uv run kohdalab-gui
```

Replay real measurement CSV files in a hardware-free, non-recording demo:

```powershell
uv run kohdalab-replay-gui
```

Loading the config automatically selects the TRKR, SRKR, STRKR, and SRKR 2D
files found in `demo_csv`. Use `--data-dir` to select a different directory.
Packaged config templates live in `src/kohdalab/config`. At startup, the GUI
refreshes only `default.json` and `default_demo.json` under
`~/.kohdalab/config`; all user-created JSON profiles in that directory are
preserved across Git and package upgrades. The normal and replay GUIs keep
separate default and last-used config state.
Experimental CSV files are not included in the public woolen-yarn repository;
see [`demo_csv/README.md`](demo_csv/README.md) for placement and data-handling
notes.

For a local installation without development tools:

```powershell
python -m pip install .
python -m pip install ".[gui]"
python -m pip install ".[notebook]"
```

The base package installs the API and CLI. GUI and Notebook dependencies are
explicit extras so headless instrument or automation environments do not need
the full Qt/Jupyter stack.

Run checks:

```powershell
uv run ruff check .
uv run ruff format --check src tests scripts
uv run mypy
uv run pytest --cov --cov-branch -q
uv lock --check
uv build --no-sources
```

## Documentation

- [Initial setup](docs/initial_setup.md): install Git, GitHub CLI, uv, clone the repository, and verify the environment.
- [Usage guide](docs/usage.md): detailed setup, GUI, API, and measurement-sequence notes.
- [API usage examples](docs/api_usage.md): practical public API examples.
- [Measurement sequences](docs/measurement_sequences.md): sequence diagrams and experiment flow.
- [Measurement CSV replay GUI](docs/replay_gui_ja.md): hardware-free repeating demo playback.
- [Demo publishing guide](docs/demo_publishing_ja.md): keep experimental CSV files out of the public repository.
- [Windows setup](docs/windows_setup.md): Windows instrument-PC preparation notes.
- [Roadmap](ROADMAP.md): planned milestones.
- [Safety notes](SAFETY.md): safety assumptions and operator responsibilities.
- [Contributing](CONTRIBUTING.md): development workflow and pull request expectations.

## Project Status

The current development version is `0.2.4`. It adds the maintained hardware-free
CSV replay GUI, separates normal and Demo defaults, and preserves user-created
config profiles across Git and package upgrades.
Hardware operation still requires the checks described in
[SAFETY.md](SAFETY.md) and the
[hardware smoke-test guide](docs/hardware_smoke_test.md).

## License

MIT. See [LICENSE](LICENSE).
