"""Notebook checks that require only Python's standard library and pytest."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest


ROOT = Path(__file__).parents[1]
NOTEBOOKS = {
    "move_abs_notebook.ipynb",
    "signal_monitor_notebook.ipynb",
    "trkr_notebook.ipynb",
    "srkr_notebook.ipynb",
    "strkr_notebook.ipynb",
    "srkr_2d_notebook.ipynb",
}


def notebook_paths() -> list[Path]:
    paths = sorted((ROOT / "notebook").glob("*.ipynb"))
    assert {path.name for path in paths} == NOTEBOOKS
    return paths


def test_notebook_cells_are_clean_and_syntactically_valid() -> None:
    for path in notebook_paths():
        notebook = json.loads(path.read_text(encoding="utf-8"))
        assert notebook["nbformat"] == 4
        assert notebook["metadata"]["kernelspec"]["name"] == "python3"
        for cell in notebook["cells"]:
            if cell["cell_type"] != "code":
                continue
            compile("".join(cell["source"]), f"{path}:{cell['id']}", "exec")
            assert cell["execution_count"] is None
            assert cell["outputs"] == []


def test_notebooks_run_in_simulation_against_the_canonical_binary(tmp_path: Path) -> None:
    binary = Path(os.environ.get("TRKR_NATIVE_BINARY", ROOT / "target" / "debug" / (
        "kohdalab-trkr.exe" if sys.platform == "win32" else "kohdalab-trkr"
    )))
    if not binary.is_file():
        pytest.skip("Build the canonical native TRKR binary first")
    code = r'''
import json
import os
import sys
from pathlib import Path

root = Path(sys.argv[1])
for path in sorted((root / "notebook").glob("*.ipynb")):
    notebook = json.loads(path.read_text(encoding="utf-8"))
    namespace = {"__name__": "__main__"}
    for cell in notebook["cells"]:
        if cell["cell_type"] == "code":
            exec(compile("".join(cell["source"]), f"{path}:{cell['id']}", "exec"), namespace)
    assert namespace["HARDWARE"] is False, path
    if path.name == "move_abs_notebook.ipynb":
        assert namespace["move_events"] == [], path
    else:
        assert namespace["samples"], path
        assert namespace["terminal"]["event"] == "finished", path
'''
    # Follow the dedicated-profile policy, without reading operator settings.
    home = tmp_path / "home"
    saved = home / ".kohdalab/trkr/config.json"
    saved.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "crates/trkr-desktop/assets/default.json", saved)
    env = os.environ | {"TRKR_BINARY": str(binary), "HOME": str(home), "USERPROFILE": str(home)}
    env.pop("TRKR_PROFILE", None)
    result = subprocess.run(
        [sys.executable, "-c", code, str(ROOT)],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


def test_notebook_setup_finds_portable_and_macos_app_layouts(tmp_path: Path) -> None:
    executable = "kohdalab-trkr.exe" if sys.platform == "win32" else "kohdalab-trkr"
    binary = Path(os.environ.get("TRKR_NATIVE_BINARY", ROOT / "target" / "debug" / executable))
    if not binary.is_file():
        pytest.skip("Build the canonical native TRKR binary first")
    setup_notebook = ROOT / "notebook" / "trkr_notebook.ipynb"
    source_profile = ROOT / "crates" / "trkr-desktop" / "assets" / "default.json"
    source_client = ROOT / "bindings" / "python" / "kohdalab_trkr.py"

    for kind in ("portable", "app"):
        root = tmp_path / ("portable" if kind == "portable" else "KohdaLab TRKR.app")
        notebook_dir = root / "notebook"
        client_dir = root / "notebook-client"
        config_dir = (
            root / "config"
            if kind == "portable"
            else root / "Contents" / "Resources" / "config"
        )
        binary_path = root / executable if kind == "portable" else root / "Contents" / "MacOS" / executable
        notebook_dir.mkdir(parents=True)
        client_dir.mkdir()
        config_dir.mkdir(parents=True)
        binary_path.parent.mkdir(parents=True, exist_ok=True)
        try:
            os.link(binary, binary_path)
        except OSError:
            shutil.copy2(binary, binary_path)
        shutil.copy2(source_client, client_dir / "kohdalab_trkr.py")
        # A packaged profile must not override the operator's reviewed profile.
        (config_dir / "default.json").write_text('{"application":"kohdalab-iv"}', encoding="utf-8")

        code = r'''
import json
import sys
from pathlib import Path

notebook = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
setup = next(cell for cell in notebook["cells"] if cell.get("id") == "setup")
namespace = {"__name__": "__main__"}
exec(compile("".join(setup["source"]), "setup", "exec"), namespace)
print(json.dumps({"binary": str(namespace["binary"]), "profile": str(namespace["profile_path"])}))
'''
        isolated_home = tmp_path / "empty-home"
        isolated_home.mkdir(exist_ok=True)
        saved = isolated_home / ".kohdalab/trkr/config.json"
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_profile, saved)
        env = os.environ | {"PATH": "", "HOME": str(isolated_home), "USERPROFILE": str(isolated_home)}
        env.pop("TRKR_BINARY", None)
        env.pop("TRKR_PROFILE", None)
        result = subprocess.run(
            [sys.executable, "-c", code, str(setup_notebook)],
            cwd=notebook_dir,
            env=env,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        assert result.returncode == 0, result.stdout + result.stderr
        resolved = json.loads(result.stdout.splitlines()[-1])
        assert Path(resolved["binary"]) == binary_path
        assert Path(resolved["profile"]) == saved
        # With no dedicated profile, fail with setup guidance rather than
        # silently using a bundled configuration (including another app's).
        saved.unlink()
        missing = subprocess.run([sys.executable, "-c", code, str(setup_notebook)],
                                 cwd=notebook_dir, env=env, capture_output=True,
                                 text=True, timeout=15, check=False)
        assert missing.returncode != 0
        assert "Save settings in the GUI" in missing.stderr
