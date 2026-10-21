"""Executable/profile discovery must never connect devices or launch candidates."""
from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]


@pytest.fixture
def client(tmp_path, monkeypatch):
    spec = importlib.util.spec_from_file_location("trkr_discovery", ROOT / "bindings/python/kohdalab_trkr.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    monkeypatch.setattr(module, "_PLATFORM", "linux")
    monkeypatch.setenv("PATH", "")
    monkeypatch.delenv("TRKR_BINARY", raising=False)
    monkeypatch.delenv("TRKR_PROFILE", raising=False)
    home = tmp_path / "home"
    home.mkdir()
    monkeypatch.setattr(Path, "home", classmethod(lambda cls: home))
    # Searching must inspect paths only, never execute a candidate to identify it.
    monkeypatch.setattr(module.subprocess, "Popen", lambda *a, **k: pytest.fail("discovery launched a process"))
    return module


def touch(path: Path) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()
    return path.resolve()


def test_explicit_paths_take_precedence_and_invalid_overrides_do_not_fall_back(client, tmp_path, monkeypatch):
    portable = touch(tmp_path / "kohdalab-trkr")
    explicit = touch(tmp_path / "chosen controller")
    monkeypatch.setenv("TRKR_BINARY", str(explicit))
    assert client.find_binary(search_from=tmp_path) == explicit
    assert client.find_binary(portable, search_from=tmp_path) == portable
    monkeypatch.setenv("TRKR_BINARY", str(tmp_path / "missing"))
    with pytest.raises(FileNotFoundError, match="TRKR_BINARY"):
        client.find_binary(search_from=tmp_path)
    with pytest.raises(FileNotFoundError):
        client.find_binary("", search_from=tmp_path)
    local_profile = touch(tmp_path / "config/default.json")
    monkeypatch.setenv("TRKR_PROFILE", str(tmp_path / "missing.json"))
    with pytest.raises(FileNotFoundError, match="TRKR_PROFILE"):
        client.find_profile(search_from=tmp_path)
    assert client.find_profile(local_profile, search_from=tmp_path) == local_profile


def test_windows_per_user_install_and_saved_gui_profile(client, tmp_path, monkeypatch):
    monkeypatch.setattr(client, "_PLATFORM", "win32")
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path / "Local AppData"))
    monkeypatch.setattr(client, "_windows_install_directories", lambda: [tmp_path / "custom install", tmp_path / "Local AppData/KohdaLab/TRKR"])
    binary = touch(tmp_path / "Local AppData/KohdaLab/TRKR/kohdalab-trkr.exe")
    profile = touch(Path.home() / ".kohdalab/config/trkr-next.json")
    assert client.find_binary(search_from=tmp_path / "unrelated") == binary
    assert client.find_profile(binary=binary, search_from=tmp_path / "unrelated") == profile
    custom = touch(tmp_path / "custom install/kohdalab-trkr.exe")
    assert client.find_binary(search_from=tmp_path / "unrelated") == custom


def test_windows_default_directory_without_registry(client, tmp_path, monkeypatch):
    monkeypatch.setenv("LOCALAPPDATA", str(tmp_path / "local"))
    assert tmp_path / "local/KohdaLab/TRKR" in client._windows_install_directories()


def test_macos_user_install_and_embedded_notebook(client, tmp_path, monkeypatch):
    monkeypatch.setattr(client, "_PLATFORM", "darwin")
    app = Path.home() / "Applications/KohdaLab TRKR.app"
    binary = touch(app / "Contents/MacOS/kohdalab-trkr")
    profile = touch(app / "Contents/Resources/config/default.json")
    unrelated = tmp_path / "unrelated notebook"
    assert client.find_binary(search_from=unrelated) == binary
    assert client.find_profile(binary=binary, search_from=unrelated) == profile
    embedded = app / "Contents/Resources/notebook"
    embedded.mkdir()
    assert client.find_binary(search_from=embedded) == binary
    assert client.find_profile(binary=binary, search_from=embedded) == profile


def test_portable_package_wins_over_path_and_uses_its_own_profile(client, tmp_path, monkeypatch):
    portable = tmp_path / "portable"
    binary = touch(portable / "kohdalab-trkr")
    profile = touch(portable / "config/default.json")
    monkeypatch.setattr(client.shutil, "which", lambda _: str(tmp_path / "old on PATH"))
    assert client.find_binary(search_from=portable / "notebook") == binary
    assert client.find_profile(binary=binary, search_from=portable / "notebook") == profile


def test_prepare_profile_uses_rust_validation_in_utf8(client, tmp_path, monkeypatch):
    binary = touch(tmp_path / "controller")
    profile = touch(tmp_path / "設定.json")
    calls = []
    def check_output(args, **kwargs):
        calls.append((args, kwargs))
        return '{"description":"測定設定", "hardware":{}, "plans":{}}'
    monkeypatch.setattr(client.subprocess, "check_output", check_output)
    prepared = client.prepare_profile(profile, binary=binary)
    assert prepared["description"] == "測定設定"
    assert calls[0][0] == [str(binary), "--profile-json", str(profile)]
    assert calls[0][1]["encoding"] == "utf-8"


def test_saved_gui_profile_precedes_source_example_defaults(client, tmp_path):
    source = touch(tmp_path / "crates/trkr-desktop/assets/default.json")
    saved = touch(Path.home() / ".kohdalab/config/trkr-next.json")
    assert client.find_profile(search_from=tmp_path) == saved
    saved.unlink()
    assert client.find_profile(search_from=tmp_path) == source


def test_saved_gui_profile_precedes_nearby_app_bundled_defaults(client, tmp_path, monkeypatch):
    monkeypatch.setattr(client, "_PLATFORM", "darwin")
    binary = touch(tmp_path / "KohdaLab TRKR.app/Contents/MacOS/kohdalab-trkr")
    bundled = touch(tmp_path / "KohdaLab TRKR.app/Contents/Resources/config/default.json")
    saved = touch(Path.home() / ".kohdalab/config/trkr-next.json")
    assert client.find_profile(binary=binary, search_from=tmp_path / "notebook") == saved
    saved.unlink()
    assert client.find_profile(binary=binary, search_from=tmp_path / "notebook") == bundled


def test_root_workspace_precedes_legacy_source_layout_and_keeps_fallback(client, tmp_path):
    legacy = touch(tmp_path / "next/target/debug/kohdalab-trkr")
    current = touch(tmp_path / "target/debug/kohdalab-trkr")
    assert client.find_binary(search_from=tmp_path) == current
    current.unlink()
    assert client.find_binary(search_from=tmp_path) == legacy
    legacy_profile = touch(tmp_path / "next/crates/trkr-desktop/assets/default.json")
    current_profile = touch(tmp_path / "crates/trkr-desktop/assets/default.json")
    assert client.find_profile(search_from=tmp_path) == current_profile
    current_profile.unlink()
    assert client.find_profile(search_from=tmp_path) == legacy_profile
