"""CI renderer files must never modify a shipping archive or binary."""
import importlib.util
from pathlib import Path

import pytest


def load_builder(monkeypatch):
    scripts = Path(__file__).parents[1] / "scripts"
    monkeypatch.syspath_prepend(str(scripts))
    spec = importlib.util.spec_from_file_location("release_builder", scripts / "build_release.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_renderer_only_changes_extracted_copy(monkeypatch, tmp_path):
    module = load_builder(monkeypatch)
    renderer = tmp_path / "renderer"
    renderer.mkdir()
    (renderer / "opengl32.dll").write_bytes(b"CI renderer")
    extracted = tmp_path / "extracted"
    extracted.mkdir()
    binary = extracted / "kohdalab-trkr.exe"
    binary.write_bytes(b"reviewed binary")
    archive = tmp_path / "portable.zip"
    module.make_zip(extracted, archive)
    archive_before = archive.read_bytes()
    monkeypatch.setenv("KOHDALAB_CI_OPENGL_DIR", str(renderer))
    monkeypatch.setenv("GITHUB_ACTIONS", "true")
    monkeypatch.setattr(module.sys, "platform", "win32")
    module.prepare_ci_gui_renderer(binary)
    assert binary.read_bytes() == b"reviewed binary"
    assert archive.read_bytes() == archive_before
    assert (extracted / "opengl32.dll").read_bytes() == b"CI renderer"
    with pytest.raises(ValueError, match="overwrite"):
        module.prepare_ci_gui_renderer(binary)


def test_override_rejected_outside_windows_ci(monkeypatch, tmp_path):
    module = load_builder(monkeypatch)
    monkeypatch.setenv("KOHDALAB_CI_OPENGL_DIR", str(tmp_path))
    monkeypatch.delenv("GITHUB_ACTIONS", raising=False)
    with pytest.raises(RuntimeError, match="restricted"):
        module.prepare_ci_gui_renderer(tmp_path / "app.exe")
