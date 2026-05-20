from __future__ import annotations

import tomllib
from pathlib import Path


def test_gui_entrypoints_are_explicit():
    pyproject = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))

    scripts = pyproject["project"]["scripts"]

    assert scripts["kohdalab-gui"] == "kohdalab.apps.trkr_gui:main"
    assert scripts["kohdalab-web"] == "kohdalab.apps.web_ui:main"
    assert "kohdalab-gui-advanced" not in scripts
