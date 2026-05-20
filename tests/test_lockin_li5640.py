from __future__ import annotations

from collections import deque

import pytest

from kohdalab.api.devices.lockin import get_lockin_wait_time, read_lockin_overload, read_lockin_settings, read_lockin_signal
from kohdalab.instruments.lockin import LI5640, LOCKIN_CONTROLLERS
from kohdalab.interfaces.lockin import Lockin


class FakeVisaInstrument:
    def __init__(self, responses: dict[str, str | list[str]]):
        self.responses = {
            command: deque(value if isinstance(value, list) else [value])
            for command, value in responses.items()
        }
        self.commands: list[str] = []
        self.write_termination = ""
        self.read_termination = ""
        self.session = object()
        self._last_command = ""
        self.closed = False
        self.ren_modes = []

    def clear(self):
        return None

    def write(self, command: str):
        self.commands.append(command)
        self._last_command = command

    def read(self) -> str:
        values = self.responses[self._last_command]
        if len(values) == 1:
            return values[0]
        return values.popleft()

    def close(self):
        self.closed = True
        self.session = None

    def control_ren(self, mode):
        self.ren_modes.append(mode)


def make_li5640(responses: dict[str, str | list[str]]) -> Lockin:
    controller = LI5640(FakeVisaInstrument(responses))
    return Lockin(controller=controller, config={"model": "LI5640", "resource": "GPIB0::3::INSTR"})


def test_li5640_is_registered_as_lockin_controller():
    assert LOCKIN_CONTROLLERS["LI5640"] is LI5640


def test_li5640_read_helpers_match_common_lockin_shape():
    lockin = make_li5640(
        {
            "OTYP?": ["1,2", "1,2", "1,2"],
            "DDEF? 1": "0",
            "DDEF? 2": "0",
            "DOUT?": ["1.0E-6,-2.0E-6", "2.236E-6,-63.4", "1.0000E+03"],
            "VSEN?": "14",
            "TCON?": "10",
            "FREQ?": "1000.0",
            "OVCR?": "9",
        }
    )

    signal = read_lockin_signal(lockin=lockin)
    assert list(signal) == ["X", "Y", "R", "Theta"]
    assert signal == {
        "X": 1.0e-6,
        "Y": -2.0e-6,
        "R": 2.236e-6,
        "Theta": -63.4,
    }
    assert lockin.controller.inst.commands[-2:] == ["DDEF 1,0", "DDEF 2,0"]
    assert read_lockin_settings(lockin=lockin) == {
        "Sensitivity": 100e-6,
        "Time Constant": 1.0,
        "Ref. Freq": 1000.0,
    }
    assert read_lockin_overload(lockin=lockin)["overload_byte"] == 9
    assert get_lockin_wait_time(lockin=lockin, multiplier=4.0) == 4.0


def test_li5640_read_restores_r_theta_display_when_that_was_active():
    lockin = make_li5640(
        {
            "OTYP?": ["1,2", "1,2"],
            "DDEF? 1": "1",
            "DDEF? 2": "1",
            "DOUT?": ["1.0E-6,-2.0E-6", "2.236E-6,-63.4"],
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": 1.0e-6,
        "Y": -2.0e-6,
        "R": 2.236e-6,
        "Theta": -63.4,
    }
    assert lockin.controller.inst.commands[-2:] == ["DDEF 1,1", "DDEF 2,1"]


def test_li5640_common_overload_uses_any_ovcr_overlevel_for_gui_display():
    lockin = make_li5640({"OVCR?": "8"})

    status = read_lockin_overload(lockin=lockin)

    assert status["raw_input_overload"] is False
    assert status["input_overload"] is True
    assert status["overload"] is True
    assert status["data2_display_overload"] is True
    assert status["overload_byte"] == 8


def test_li5640_writes_model_specific_setting_commands():
    lockin = make_li5640({"VSEN?": "14", "TCON?": "10", "SLOP?": "3", "ICPL?": "1"})
    inst = lockin.controller.inst

    lockin.set_sensitivity(100e-6)
    lockin.set_time_constant(1.0)
    lockin.set_coupling("AC")
    lockin.set_slope(24)
    lockin.auto_phase()
    lockin.auto_sensitivity()

    assert inst.commands == ["VSEN 14", "TCON 10", "ICPL 0", "SLOP 3", "APHS", "ASEN"]


def test_li5640_rejects_sr7265_only_ac_gain():
    lockin = make_li5640({})

    with pytest.raises(NotImplementedError, match="AC gain"):
        lockin.set_ac_gain(10.0)


def test_li5640_close_sends_go_to_local_before_closing():
    lockin = make_li5640({})
    inst = lockin.controller.inst

    lockin.close()

    assert inst.ren_modes
    assert inst.ren_modes[-1].name == "address_gtl"
    assert inst.closed is True
