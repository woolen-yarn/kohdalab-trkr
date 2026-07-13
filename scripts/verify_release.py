from __future__ import annotations

import argparse
import datetime as dt
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import yaml


STABLE_VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")
RELEASE_HEADING = re.compile(
    r"^## \[(?P<version>[0-9]+\.[0-9]+\.[0-9]+)\] - "
    r"(?P<date>[0-9]{4}-[0-9]{2}-[0-9]{2})$",
    re.MULTILINE,
)


@dataclass(frozen=True)
class ReleaseMetadata:
    version: str
    release_date: dt.date | None


def _section(markdown: str, heading: str) -> str:
    match = re.search(
        rf"^## {re.escape(heading)}\s*$\n(?P<body>.*?)(?=^## |\Z)",
        markdown,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise RuntimeError(f"CHANGELOG.md is missing the {heading!r} section.")
    return match.group("body").strip()


def _citation_date(value: Any) -> dt.date | None:
    if value is None:
        return None
    if isinstance(value, dt.datetime):
        return value.date()
    if isinstance(value, dt.date):
        return value
    if not isinstance(value, str):
        raise RuntimeError("CITATION.cff date-released must be an ISO date.")
    try:
        return dt.date.fromisoformat(value)
    except ValueError as error:
        raise RuntimeError("CITATION.cff date-released must be YYYY-MM-DD.") from error


def verify_release(root: Path, *, tag: str | None = None) -> ReleaseMetadata:
    project = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]
    version = project.get("version")
    if not isinstance(version, str) or STABLE_VERSION.fullmatch(version) is None:
        raise RuntimeError(
            "Project version must use the stable MAJOR.MINOR.PATCH form."
        )

    citation = yaml.safe_load((root / "CITATION.cff").read_text(encoding="utf-8"))
    if not isinstance(citation, dict):
        raise RuntimeError("CITATION.cff must contain a YAML mapping.")
    if str(citation.get("version")) != version:
        raise RuntimeError("CITATION.cff version does not match pyproject.toml.")

    readme = (root / "README.md").read_text(encoding="utf-8")
    documented_versions = re.findall(
        r"current development version is `([^`]+)`", readme, re.IGNORECASE
    )
    if documented_versions != [version]:
        raise RuntimeError("README.md must document the current project version once.")

    roadmap = (root / "ROADMAP.md").read_text(encoding="utf-8")
    if (
        len(re.findall(rf"^## v{re.escape(version)}(?:\s|$)", roadmap, re.MULTILINE))
        != 1
    ):
        raise RuntimeError(
            "ROADMAP.md must contain one heading for the current version."
        )

    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if len(re.findall(r"^## Unreleased\s*$", changelog, re.MULTILINE)) != 1:
        raise RuntimeError("CHANGELOG.md must contain exactly one Unreleased section.")
    unreleased = _section(changelog, "Unreleased")

    citation_date = _citation_date(citation.get("date-released"))
    if tag is None:
        return ReleaseMetadata(version=version, release_date=citation_date)

    if tag != f"v{version}":
        raise RuntimeError(f"Git tag must be exactly v{version}, not {tag!r}.")
    if re.search(r"^\s*[-*+]\s+", unreleased, re.MULTILINE):
        raise RuntimeError(
            "Unreleased must contain no change entries for a tagged release."
        )

    releases = [
        match
        for match in RELEASE_HEADING.finditer(changelog)
        if match["version"] == version
    ]
    if len(releases) != 1:
        raise RuntimeError(
            f"CHANGELOG.md must contain one release heading for version {version}."
        )
    release_date = dt.date.fromisoformat(releases[0]["date"])
    if release_date > dt.datetime.now(dt.UTC).date():
        raise RuntimeError("The CHANGELOG.md release date cannot be in the future.")
    if not _section(changelog, f"[{version}] - {release_date.isoformat()}"):
        raise RuntimeError("The tagged CHANGELOG.md release section must not be empty.")
    if citation_date != release_date:
        raise RuntimeError(
            "CITATION.cff date-released must match the CHANGELOG.md release date."
        )
    return ReleaseMetadata(version=version, release_date=release_date)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify source metadata before building or tagging a release."
    )
    parser.add_argument("--project-root", type=Path, default=Path.cwd())
    parser.add_argument(
        "--tag",
        help="Enable strict tagged-release checks (for example, v0.2.0).",
    )
    args = parser.parse_args()
    metadata = verify_release(args.project_root.resolve(), tag=args.tag)
    suffix = (
        f" released {metadata.release_date.isoformat()}"
        if metadata.release_date is not None and args.tag is not None
        else " development metadata"
    )
    print(f"Verified kohdalab {metadata.version}{suffix}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
