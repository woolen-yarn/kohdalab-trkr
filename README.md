# KohdaLab TRKR

[![Test](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml/badge.svg)](https://github.com/woolen-yarn/kohdalab-trkr/actions/workflows/test.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
![Python](https://img.shields.io/badge/python-3.13%2B-blue)

KohdaLab TRKR is a Python toolkit for time-resolved Kerr rotation experiments and laboratory instrument control.

## What It Does

- Provides a small Python API for experiment control.
- Includes GUI and CLI entry points for lab workflows.
- Supports TRKR-related measurement sequences and analysis-oriented examples.
- Keeps measurement logic testable without requiring hardware for every check.

## Quick Start

For a new PC, start with the setup guide:

- [Initial setup](docs/initial_setup.md)

After setup:

```powershell
uv sync --all-extras --group dev --frozen
uv run kohdalab-cli --help
uv run kohdalab-gui
```

Run checks:

```powershell
uv run ruff check src tests
uv run pytest -q
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

`v0.1.0` is the repository baseline: licensing, CI, branch protection, dependency maintenance, and documentation structure are in place. Measurement reliability and simulated hardware sessions are planned next.

## License

MIT. See [LICENSE](LICENSE).
