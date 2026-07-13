from __future__ import annotations

import tomllib
from pathlib import Path

import yaml

from kohdalab import __version__


REPOSITORY_URL = "https://github.com/woolen-yarn/kohdalab-trkr"


def test_project_metadata_has_no_placeholder_and_matches_runtime_version():
    project = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]

    assert project["version"] == __version__
    assert "Add your description" not in project["description"]
    assert project["license"] == "MIT"
    assert project["license-files"] == ["LICENSE"]
    assert project["urls"]["Repository"] == REPOSITORY_URL


def test_citation_metadata_matches_project_identity():
    project = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]
    citation = yaml.safe_load(Path("CITATION.cff").read_text(encoding="utf-8"))

    assert citation["version"] == project["version"]
    assert citation["license"] == "MIT"
    assert citation["repository-code"] == REPOSITORY_URL


def test_ci_covers_supported_python_and_desktop_platforms():
    workflow = yaml.safe_load(
        Path(".github/workflows/test.yml").read_text(encoding="utf-8")
    )
    matrix = workflow["jobs"]["test"]["strategy"]["matrix"]

    assert matrix["os"] == ["ubuntu-24.04", "macos-15", "windows-2025"]
    assert matrix["python-version"] == ["3.13", "3.14"]
    assert workflow["jobs"]["package"]["runs-on"] == "ubuntu-24.04"
    assert workflow["jobs"]["audit"]["runs-on"] == "ubuntu-24.04"


def test_pytest_configuration_fails_closed():
    metadata = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))
    tools = metadata["tool"]
    config = tools["pytest"]["ini_options"]
    dev_dependencies = metadata["dependency-groups"]["dev"]

    assert config["testpaths"] == ["tests"]
    assert config["xfail_strict"] is True
    assert config["filterwarnings"] == ["error"]
    assert config["required_plugins"] == ["pytest-cov>=7.0"]
    assert {"--strict-config", "--strict-markers"} <= set(config["addopts"])
    assert tools["coverage"]["run"]["branch"] is True
    assert tools["coverage"]["run"]["relative_files"] is True
    assert tools["coverage"]["report"]["fail_under"] == 100
    assert tools["mypy"]["files"] == ["src/kohdalab"]
    assert {"E4", "E7", "E9", "F", "W", "B", "PIE", "S"} <= set(
        tools["ruff"]["lint"]["select"]
    )
    security_exceptions = tools["ruff"]["lint"]["per-file-ignores"]
    assert security_exceptions == {
        "tests/**": ["S101"],
        "tests/test_notebooks.py": ["S102"],
        "tests/test_run_metadata.py": ["S105"],
    }
    assert tools["mypy"]["extra_checks"] is True
    assert tools["mypy"]["strict"] is True
    assert tools["mypy"]["disallow_untyped_calls"] is True
    assert tools["mypy"]["disallow_untyped_decorators"] is True
    assert tools["mypy"]["strict_equality"] is True
    assert tools["mypy"]["warn_redundant_casts"] is True
    assert tools["mypy"]["warn_return_any"] is True
    assert "overrides" not in tools["mypy"]
    assert "ruff" in dev_dependencies
    assert "black" not in dev_dependencies
