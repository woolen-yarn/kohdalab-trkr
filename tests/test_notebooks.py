from __future__ import annotations

import json
from pathlib import Path


EXPECTED_NOTEBOOKS = {
    "move_abs_notebook.ipynb",
    "signal_monitor_notebook.ipynb",
    "srkr_2d_notebook.ipynb",
    "srkr_notebook.ipynb",
    "strkr_notebook.ipynb",
    "trkr_notebook.ipynb",
}


def _notebooks() -> list[Path]:
    return sorted(Path("notebook").glob("*.ipynb"))


def test_maintained_notebook_set_is_explicit():
    assert {path.name for path in _notebooks()} == EXPECTED_NOTEBOOKS


def test_notebooks_are_clean_compilable_and_do_not_patch_import_paths():
    for path in _notebooks():
        notebook = json.loads(path.read_text(encoding="utf-8"))
        assert notebook["nbformat"] == 4
        assert notebook["metadata"]["kernelspec"]["name"] == "python3"
        for cell in notebook["cells"]:
            if cell["cell_type"] != "code":
                continue
            source = "".join(cell["source"])
            compile(source, f"{path}:{cell['id']}", "exec")
            assert cell["execution_count"] is None
            assert cell["outputs"] == []
            assert "sys.path" not in source
            assert "trkr_config_kikuchi.json" not in source


def test_notebook_setup_cells_load_packaged_config_without_connecting_hardware():
    for path in _notebooks():
        notebook = json.loads(path.read_text(encoding="utf-8"))
        setup = next(cell for cell in notebook["cells"] if cell.get("id") == "setup")
        namespace: dict[str, object] = {}

        exec(compile("".join(setup["source"]), f"{path}:setup", "exec"), namespace)

        experiment = namespace["experiment"]
        assert experiment.auto_connect is False
        assert namespace["config_path"] is None
        assert namespace["config"]["profile"]["name"] == "default"
        assert experiment.connected_devices() and not any(
            experiment.connected_devices().values()
        )
