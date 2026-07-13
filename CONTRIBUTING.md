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
uv run ruff check .
uv run ruff format --check src tests scripts
uv run mypy
uv run pytest --cov --cov-report=term-missing
uv run pip-audit
uv lock --check
```

The test command enforces branch coverage and a project-wide minimum of 100%.
New or changed failure paths must be tested explicitly so that the invariant is
preserved.

Core data-boundary modules listed in the Mypy override must remain fully typed:
do not introduce untyped functions, incomplete signatures, or generic containers
without explicit type arguments. Extend this strict module list as legacy device
drivers are fully annotated.

Before a release, reproduce the package CI check in PowerShell:

```powershell
$version = "0.2.0"
uv run --only-group package python scripts/verify_release.py
uv run --only-group package python scripts/verify_release.py --tag "v$version"
$env:SOURCE_DATE_EPOCH = "1735689600"
uv build --no-sources --out-dir dist/first
uv build --no-sources --out-dir dist/second
uv run --only-group package python scripts/verify_distributions.py dist/first dist/second
uv run --only-group package twine check --strict dist/first/*
uv run --only-group package check-wheel-contents dist/first/*.whl
```

The verifier requires byte-identical wheels, content-identical source
distributions, complete runtime resources and source-release files, valid wheel
`RECORD` hashes, and metadata/entry points matching `pyproject.toml`.

Prepare release metadata before creating a `vMAJOR.MINOR.PATCH` tag:

1. Set the same stable `MAJOR.MINOR.PATCH` version in `pyproject.toml`,
   `CITATION.cff`, README, and the matching ROADMAP heading.
2. Move all change entries from `Unreleased` into a non-empty
   `## [MAJOR.MINOR.PATCH] - YYYY-MM-DD` section, leaving `Unreleased` present
   without change entries.
3. Set `date-released: "YYYY-MM-DD"` in `CITATION.cff` to the same date.
4. Run both release-verifier commands above, then create the exact
   `vMAJOR.MINOR.PATCH` tag.

Tag pushes run the full test, dependency-audit, package, reproducibility, and
strict release-metadata checks. The workflow validates release readiness; it
does not publish artifacts automatically.

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
uv run ruff check .
uv run ruff format --check src tests scripts
uv run pytest
```

If the change affects real hardware behavior, include the hardware used and the
smallest range tested.
