from __future__ import annotations

import runpy
from pathlib import Path
from typing import Any

import pytest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "verify_release.py"
VERIFY_RELEASE = runpy.run_path(str(SCRIPT))["verify_release"]


def _write_project(
    root: Path,
    *,
    version: str = "1.2.3",
    citation_version: str | None = None,
    citation_date: str | None = None,
    unreleased: str = "- Work in progress",
    release_date: str | None = None,
    release_body: str = "- Stable change",
) -> None:
    (root / "pyproject.toml").write_text(
        f'[project]\nname = "example"\nversion = "{version}"\n', encoding="utf-8"
    )
    citation = f'version: "{citation_version or version}"\n'
    if citation_date is not None:
        citation += f'date-released: "{citation_date}"\n'
    (root / "CITATION.cff").write_text(citation, encoding="utf-8")
    (root / "README.md").write_text(
        f"The current development version is `{version}`.\n", encoding="utf-8"
    )
    (root / "ROADMAP.md").write_text(
        f"# Roadmap\n\n## v{version} - Current\n", encoding="utf-8"
    )
    changelog = f"# Changelog\n\n## Unreleased\n\n{unreleased}\n"
    if release_date is not None:
        changelog += f"\n## [{version}] - {release_date}\n\n{release_body}\n"
    (root / "CHANGELOG.md").write_text(changelog, encoding="utf-8")


def _verify(root: Path, *, tag: str | None = None) -> Any:
    return VERIFY_RELEASE(root, tag=tag)


def test_repository_development_metadata_is_consistent() -> None:
    metadata = _verify(ROOT)

    assert metadata.version == "0.2.2"


def test_development_check_rejects_citation_version_mismatch(tmp_path: Path) -> None:
    _write_project(tmp_path, citation_version="1.2.4")

    with pytest.raises(RuntimeError, match="CITATION.cff version"):
        _verify(tmp_path)


def test_tagged_release_requires_exact_tag_and_empty_unreleased(
    tmp_path: Path,
) -> None:
    _write_project(tmp_path, citation_date="2025-01-01", release_date="2025-01-01")

    with pytest.raises(RuntimeError, match="Git tag must be exactly"):
        _verify(tmp_path, tag="1.2.3")
    with pytest.raises(RuntimeError, match="Unreleased must contain no change entries"):
        _verify(tmp_path, tag="v1.2.3")


def test_tagged_release_accepts_matching_dates(tmp_path: Path) -> None:
    _write_project(
        tmp_path,
        citation_date="2025-01-01",
        unreleased="",
        release_date="2025-01-01",
    )

    metadata = _verify(tmp_path, tag="v1.2.3")

    assert metadata.release_date is not None
    assert metadata.release_date.isoformat() == "2025-01-01"


def test_tagged_release_rejects_citation_date_mismatch(tmp_path: Path) -> None:
    _write_project(
        tmp_path,
        citation_date="2025-01-02",
        unreleased="",
        release_date="2025-01-01",
    )

    with pytest.raises(RuntimeError, match="date-released must match"):
        _verify(tmp_path, tag="v1.2.3")


def test_tagged_release_requires_nonempty_release_notes(tmp_path: Path) -> None:
    _write_project(
        tmp_path,
        citation_date="2025-01-01",
        unreleased="",
        release_date="2025-01-01",
        release_body="",
    )

    with pytest.raises(RuntimeError, match="release section must not be empty"):
        _verify(tmp_path, tag="v1.2.3")
