from __future__ import annotations

from pathlib import Path

from kohdalab.interfaces.common import load_toml, merge_config, midpoint


def test_load_toml_reads_nested_values(tmp_path: Path):
    path = tmp_path / "settings.toml"
    path.write_text('[device]\nname = "stage"\naxis = 2\n', encoding="utf-8")

    assert load_toml(path) == {"device": {"name": "stage", "axis": 2}}


def test_merge_config_ignores_none_without_mutating_inputs():
    defaults = {"port": "COM1", "timeout": 1.0}
    overrides = {"port": None, "timeout": 2.0, "axis": 1}

    assert merge_config(defaults, overrides) == {
        "port": "COM1",
        "timeout": 2.0,
        "axis": 1,
    }
    assert defaults == {"port": "COM1", "timeout": 1.0}
    assert overrides["port"] is None


def test_midpoint_uses_bounds_or_default():
    assert midpoint(-2, 4) == 1.0
    assert midpoint(None, 4, default=3) == 3.0
    assert midpoint(-2, None, default=-1.5) == -1.5
