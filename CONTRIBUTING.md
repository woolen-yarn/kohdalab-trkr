# Contributing

Thank you for improving KohdaLab TRKR. This project controls real laboratory
instruments, so changes should favor safe motion, reproducible measurements,
and clear operator feedback.

## Development Setup

Use `uv` from the repository root:

```powershell
uv sync --all-extras --group dev
```

Run checks:

```powershell
uv run ruff check src tests
uv run pytest
```

Start the GUI:

```powershell
uv run kohdalab-gui
```

## Change Guidelines

- Keep public workflow logic in `src/kohdalab/api`.
- Keep device-level command normalization in `src/kohdalab/interfaces`.
- Keep instrument command quirks in `src/kohdalab/instruments`.
- Keep GUI code thin; it should call the public API instead of duplicating
  measurement behavior.
- Add tests for config normalization, scan plans, coordinate conversion,
  row schemas, session ownership, and worker behavior.

## Hardware Changes

For new instruments or controllers, document:

- model and transport
- tested firmware or command mode if known
- coordinate conversion and units
- motion limits and homing behavior
- local/remote release behavior
- a small safe smoke-test procedure

## Pull Requests

Before opening a pull request:

```powershell
uv run ruff check src tests
uv run pytest
```

If the change affects real hardware behavior, include the hardware used and the
smallest range tested.
