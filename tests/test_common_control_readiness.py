"""The package verifier must wait for actual GUI readiness."""
import importlib.util
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
