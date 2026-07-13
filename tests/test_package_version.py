from __future__ import annotations

import importlib.metadata
import runpy
from pathlib import Path


PACKAGE_INIT = Path("src/kohdalab/__init__.py")


def test_package_version_uses_installed_distribution(monkeypatch):
    monkeypatch.setattr(importlib.metadata, "version", lambda name: "9.8.7")

    namespace = runpy.run_path(str(PACKAGE_INIT))

    assert namespace["__version__"] == "9.8.7"


def test_package_version_falls_back_without_distribution(monkeypatch):
    def missing_distribution(name: str) -> str:
        raise importlib.metadata.PackageNotFoundError(name)

    monkeypatch.setattr(importlib.metadata, "version", missing_distribution)

    namespace = runpy.run_path(str(PACKAGE_INIT))

    assert namespace["__version__"] == "0.0.0"
