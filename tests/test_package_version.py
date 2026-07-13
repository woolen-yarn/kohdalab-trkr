from __future__ import annotations

import importlib
import importlib.metadata

import kohdalab


def test_package_version_uses_installed_distribution(monkeypatch):
    with monkeypatch.context() as context:
        context.setattr(importlib.metadata, "version", lambda name: "9.8.7")

        module = importlib.reload(kohdalab)

        assert module.__version__ == "9.8.7"

    importlib.reload(kohdalab)


def test_package_version_falls_back_without_distribution(monkeypatch):
    def missing_distribution(name: str) -> str:
        raise importlib.metadata.PackageNotFoundError(name)

    with monkeypatch.context() as context:
        context.setattr(importlib.metadata, "version", missing_distribution)

        module = importlib.reload(kohdalab)

        assert module.__version__ == "0.0.0"

    importlib.reload(kohdalab)
