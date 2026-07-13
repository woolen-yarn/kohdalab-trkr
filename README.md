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
- [Windows setup](docs/windows_setup.md): Windows instrument-PC preparation notes.
- [Roadmap](ROADMAP.md): planned milestones.
- [Safety notes](SAFETY.md): safety assumptions and operator responsibilities.
- [Contributing](CONTRIBUTING.md): development workflow and pull request expectations.

## Project Status

The current development version is `0.2.2`. It adds responsive lock-in live
status during manual motion while retaining per-device I/O serialization,
strict config and scan preflight validation, fail-closed controller handling,
package smoke tests, and measurement provenance sidecars. Hardware operation
still requires the checks described in [SAFETY.md](SAFETY.md) and the
[hardware smoke-test guide](docs/hardware_smoke_test.md).

## License

MIT. See [LICENSE](LICENSE).
