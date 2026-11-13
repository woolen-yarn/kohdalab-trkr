"""The package verifier must wait for actual GUI readiness."""
import importlib.util
import json
from pathlib import Path
from unittest.mock import Mock, patch

import pytest


def load_verifier(monkeypatch):
    scripts = Path(__file__).parents[1] / "scripts"
    monkeypatch.syspath_prepend(str(scripts))
    spec = importlib.util.spec_from_file_location("control_verifier", scripts / "verify_common_control.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_early_descriptor_is_not_ready_until_probe_responds(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    (tmp_path / "early.json").write_text("{}")
    endpoint = object()
    process = Mock()
    process.poll.return_value = None
    with patch.object(module, "discover", side_effect=[[], [endpoint]]) as discover, patch.object(module, "ControlClient") as client:
        assert module.wait_for_client(tmp_path, process) is client.return_value
        assert discover.call_count == 2
        client.assert_called_once_with("kohdalab-trkr", endpoint=endpoint)


def test_exited_gui_fails_without_retrying_discovery(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    process = Mock()
    process.poll.return_value = 1
    with patch.object(module, "discover") as discover, pytest.raises(RuntimeError, match="exited"):
        module.wait_for_client(tmp_path, process)
    discover.assert_not_called()


def test_packaged_profile_is_the_loadable_startup_profile(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    source = module.ROOT / "crates/trkr-desktop/assets/default.json"
    executable = tmp_path / "kohdalab-trkr"
    assert module.packaged_profile(executable) == source
    (tmp_path / "config").mkdir()
    bundled = tmp_path / "config" / "default.json"
    bundled.write_bytes(source.read_bytes())
    assert module.packaged_profile(executable) == bundled
    # `--config` rejects a profile without instruments.lockin.main.
    assert json.loads(bundled.read_text())["instruments"]["lockin"]["main"]
    bundled.write_text("{}")
    with pytest.raises(ValueError, match="differs"):
        module.packaged_profile(executable)


def test_packaged_profile_follows_macos_bundle_resources(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    source = module.ROOT / "crates/trkr-desktop/assets/default.json"
    (tmp_path / "Contents/MacOS").mkdir(parents=True)
    (tmp_path / "Contents/Resources/config").mkdir(parents=True)
    bundled = tmp_path / "Contents/Resources/config/default.json"
    bundled.write_bytes(source.read_bytes())
    found = module.packaged_profile(tmp_path / "Contents/MacOS/kohdalab-trkr")
    assert found.resolve() == bundled.resolve()


def test_startup_profile_accepts_canonicalized_spelling_of_the_same_file(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    real = tmp_path / "private" / "home"
    real.mkdir(parents=True)
    profile = real / "default.json"
    profile.write_text("{}")
    alias = tmp_path / "home"
    try:
        alias.symlink_to(real, target_is_directory=True)
    except OSError:
        pytest.skip("symlinks unavailable")
    # The GUI canonicalizes an explicit --config (macOS /var -> /private/var,
    # Windows verbatim prefix); the verifier holds the non-canonical spelling.
    module.require_startup_profile(str(profile.resolve()), alias / "default.json")


def test_startup_profile_rejects_another_file_with_useful_diagnostics(monkeypatch, tmp_path):
    module = load_verifier(monkeypatch)
    expected = tmp_path / "default.json"
    expected.write_text("{}")
    other = tmp_path / "other.json"
    other.write_text("{}")
    with pytest.raises(ValueError, match=r"config_path='.*other\.json', expected '.*default\.json'"):
        module.require_startup_profile(str(other), expected)
    with pytest.raises(ValueError, match=r"missing\.json"):
        module.require_startup_profile(str(tmp_path / "missing.json"), expected)
    with pytest.raises(ValueError, match="config_path=None"):
        module.require_startup_profile(None, expected)
