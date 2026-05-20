from __future__ import annotations

import pytest

from kohdalab.api.session import DeviceSession


def config_with_devices() -> dict:
    return {
        "instruments": {
            "lockin": {"main": {}},
            "delay_stage": {"t": {}},
            "scanner": {"x": {}, "y": {}},
        }
    }


@pytest.mark.parametrize(
    ("ref", "expected"),
    [
        ("signal", ("lockin", "main")),
        ("lockin", ("lockin", "main")),
        ("delay", ("delay_stage", "t")),
        ("delay_stage", ("delay_stage", "t")),
        ("stage", ("delay_stage", "t")),
        ("t", ("delay_stage", "t")),
        ("scanner_x", ("scanner", "x")),
        ("scanner_y", ("scanner", "y")),
        ("x", ("scanner", "x")),
        ("y", ("scanner", "y")),
        ("lockins.main", ("lockin", "main")),
        ("delay_stages.t", ("delay_stage", "t")),
        ("scanners.x", ("scanner", "x")),
    ],
)
def test_resolve_ref_aliases(ref, expected):
    assert DeviceSession(config_with_devices()).resolve_ref(ref) == expected


def test_resolve_ref_with_default_kind_treats_plain_ref_as_key():
    session = DeviceSession(config_with_devices())

    assert session.resolve_ref("main", default_kind="lockin") == ("lockin", "main")
    assert session.resolve_ref("t", default_kind="delay_stage") == ("delay_stage", "t")


def test_resolve_ref_rejects_plain_ref_without_default_kind():
    with pytest.raises(ValueError, match="Device reference"):
        DeviceSession(config_with_devices()).resolve_ref("main")


def test_resolve_ref_requires_explicit_key_when_alias_kind_has_multiple_devices():
    config = config_with_devices()
    config["instruments"]["lockin"]["aux"] = {}

    with pytest.raises(ValueError, match="Multiple instruments.lockin"):
        DeviceSession(config).resolve_ref("lockin")
