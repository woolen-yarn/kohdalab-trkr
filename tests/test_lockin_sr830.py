from __future__ import annotations

import pytest

from kohdalab.api.devices.lockin import get_lockin_wait_time, read_lockin_overload, read_lockin_settings, read_lockin_signal
from kohdalab.instruments.lockin import LOCKIN_CONTROLLERS, SR830
from kohdalab.interfaces.lockin import Lockin


class FakeVisaInstrument:
    def __init__(self, responses: dict[str, str]):
        self.responses = responses
        self.commands: list[str] = []
        self.write_termination = ""
        self.read_termination = ""
        self.session = object()
        self._last_command = ""
        self.closed = False

    def clear(self):
        return None

    def write(self, command: str):
        self.commands.append(command)
        self._last_command = command

    def read(self) -> str:
        return self.responses[self._last_command]

    def close(self):
        self.closed = True
        self.session = None


def make_sr830(responses: dict[str, str]) -> Lockin:
    controller = SR830(FakeVisaInstrument(responses))
    return Lockin(controller=controller, config={"model": "SR830", "resource": "GPIB0::8::INSTR"})


def test_sr830_is_registered_as_lockin_controller():
    assert LOCKIN_CONTROLLERS["SR830"] is SR830


def test_sr830_read_helpers_match_common_lockin_shape():
    lockin = make_sr830(
        {
            "SNAP?1,2,3,4": "1.0e-6,-2.0e-6,2.236e-6,-63.4",
            "SENS?": "14",
            "OFLT?": "10",
            "OFSL?": "3",
            "FREQ?": "1000.0",
            "LIAS?": "13",
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": 1.0e-6,
        "Y": -2.0e-6,
        "R": 2.236e-6,
        "Theta": -63.4,
    }
    assert read_lockin_settings(lockin=lockin) == {
        "Sensitivity": 100e-6,
        "Time Constant": 1.0,
        "Ref. Freq": 1000.0,
    }
    assert read_lockin_overload(lockin=lockin)["overload_byte"] == 13
    assert get_lockin_wait_time(lockin=lockin, multiplier=4.0) == 4.0


def test_lockin_signal_keeps_device_values_without_sensitivity_based_conversion():
    lockin = make_sr830(
        {
            "SNAP?1,2,3,4": "0.012,-0.006,0.013416,45.0",
            "SENS?": "21",
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": 0.012,
        "Y": -0.006,
        "R": 0.013416,
        "Theta": 45.0,
    }


def test_sr830_status_bits_are_mapped_to_common_overload_keys():
    lockin = make_sr830({"LIAS?": "13"})

    status = read_lockin_overload(lockin=lockin)

    assert status["input_overload"] is True
    assert status["overload"] is True
    assert status["reference_unlock"] is True
    assert status["filter_overload"] is False
    assert status["overload_byte"] == 13


def test_sr830_common_overload_uses_input_overload_only():
    lockin = make_sr830({"LIAS?": "4"})

    status = read_lockin_overload(lockin=lockin)

    assert status["input_overload"] is False
    assert status["overload"] is False
    assert status["overload_byte"] == 4


def test_sr830_writes_model_specific_setting_commands():
    lockin = make_sr830({"SENS?": "14", "OFLT?": "10", "OFSL?": "3", "ICPL?": "1"})
    inst = lockin.controller.inst

    lockin.set_sensitivity(100e-6)
    lockin.set_time_constant(1.0)
    lockin.set_coupling("AC")
    lockin.set_slope(24)
    lockin.auto_phase()
    lockin.auto_sensitivity()

    assert inst.commands == ["SENS 14", "OFLT 10", "ICPL 0", "OFSL 3", "APHS", "AGAN"]


def test_sr830_rejects_sr7265_only_ac_gain():
    lockin = make_sr830({})

    with pytest.raises(NotImplementedError, match="AC gain"):
        lockin.set_ac_gain(10.0)
