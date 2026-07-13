from __future__ import annotations

import tomllib
import importlib
from pathlib import Path


def test_gui_entrypoints_are_explicit():
    pyproject = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))

    scripts = pyproject["project"]["scripts"]

    assert scripts["kohdalab-gui"] == "kohdalab.apps.trkr_gui:main"
    assert "kohdalab-gui-advanced" not in scripts


def test_gui_workers_remain_available_from_entrypoint_module():
    gui = importlib.import_module("kohdalab.apps.trkr_gui")
    workers = importlib.import_module("kohdalab.apps.trkr_gui_workers")

    assert gui.MeasurementWorker is workers.MeasurementWorker
    assert gui.DeviceCommandWorker is workers.DeviceCommandWorker
