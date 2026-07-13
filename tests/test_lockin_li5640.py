from __future__ import annotations

from collections import deque

import pytest

from kohdalab.api.devices.lockin import (
    get_lockin_wait_time,
    read_lockin_overload,
    read_lockin_settings,
    read_lockin_signal,
)
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
    return Lockin(
        controller=controller, config={"model": "LI5640", "resource": "GPIB0::3::INSTR"}
    )


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


def test_li5640_empty_query_response_is_a_timeout():
    lockin = make_li5640({"TCON?": ""})

    with pytest.raises(TimeoutError, match=r"waiting for TCON\? response"):
        lockin.get_time_constant()


def test_li5640_clear_failure_does_not_hide_a_valid_response():
    lockin = make_li5640({"VSEN?": "26"})

    def fail_clear():
        raise RuntimeError("clear unavailable")

    lockin.controller.inst.clear = fail_clear

    assert lockin.get_sensitivity() == 1.0


@pytest.mark.parametrize(
    ("method_name", "response", "message"),
    [
        ("get_time_constant", "20", "Unexpected time constant index: 20"),
        ("get_sensitivity", "27", "Unexpected sensitivity index: 27"),
        ("get_coupling", "2", "Unexpected coupling mode: 2"),
        ("get_slope", "4", "Unexpected slope mode: 4"),
    ],
)
def test_li5640_rejects_unknown_enumerated_responses(
    method_name: str, response: str, message: str
):
    command = {
        "get_time_constant": "TCON?",
        "get_sensitivity": "VSEN?",
        "get_coupling": "ICPL?",
        "get_slope": "SLOP?",
    }[method_name]
    lockin = make_li5640({command: response})

    with pytest.raises(RuntimeError, match=message):
        getattr(lockin, method_name)()


@pytest.mark.parametrize("response", ["1.5", "nan", "inf"])
def test_li5640_rejects_non_integer_or_non_finite_setting_responses(response: str):
    lockin = make_li5640({"TCON?": response})

    with pytest.raises(RuntimeError, match="LI5640 TCON\\? response"):
        lockin.get_time_constant()


def test_li5640_restores_output_selection_after_malformed_data():
    lockin = make_li5640({"OTYP?": "3", "DOUT?": "not-a-number"})

    with pytest.raises(RuntimeError, match=r"DOUT\?"):
        lockin.get_ref_freq()

    assert lockin.controller.inst.commands[-1] == "OTYP 3"


@pytest.mark.parametrize("value", [True, 6.0, 0, 30])
def test_li5640_rejects_invalid_slope_contract_values(value):
    lockin = make_li5640({})

    with pytest.raises(ValueError, match="slope must|Unsupported slope"):
        lockin.set_slope(value)


@pytest.mark.parametrize("value", [None, 1, "", "resistive"])
def test_li5640_rejects_invalid_coupling_contract_values(value):
    lockin = make_li5640({})

    with pytest.raises(ValueError, match="coupling must|Unsupported coupling"):
        lockin.set_coupling(value)


def test_li5640_setting_endpoints_map_to_exact_table_indices():
    lockin = make_li5640({})

    lockin.set_sensitivity(2e-9)
    lockin.set_sensitivity(1.0)
    lockin.set_time_constant(10e-6)
    lockin.set_time_constant(30e3)

    assert lockin.controller.inst.commands == [
        "VSEN 0",
        "VSEN 26",
        "TCON 0",
        "TCON 19",
    ]


def test_li5640_capabilities_and_auto_measure_contract():
    lockin = make_li5640({})
    controller = lockin.controller

    assert controller.configure() is None
    assert controller.get_ac_gain() is None
    assert controller.get_available_couplings() == ["AC", "DC"]
    assert controller.get_available_slopes() == [6, 12, 18, 24]
    assert controller.get_available_time_constants() == sorted(
        controller.get_available_time_constants()
    )
    assert controller.get_available_sensitivities() == sorted(
        controller.get_available_sensitivities()
    )
    assert controller.get_available_ac_gains() == []

    controller.auto_measure()

    assert controller.inst.commands == ["ASEN"]


@pytest.mark.parametrize(
    ("command", "method", "response", "expected"),
    [
        ("ICPL?", "get_coupling", "0", "AC"),
        ("ICPL?", "get_coupling", "1", "DC"),
        ("SLOP?", "get_slope", "0", 6),
        ("SLOP?", "get_slope", "3", 24),
    ],
)
def test_li5640_getters_accept_enum_boundaries(command, method, response, expected):
    lockin = make_li5640({command: response})

    assert getattr(lockin, method)() == expected


def test_li5640_setters_normalize_valid_coupling_and_slope_boundaries():
    lockin = make_li5640({})

    lockin.set_coupling(" dc ")
    lockin.set_slope(6)
    lockin.set_slope(18)

    assert lockin.controller.inst.commands == ["ICPL 1", "SLOP 0", "SLOP 2"]


def test_li5640_reads_output_when_saved_output_selection_is_unavailable():
    lockin = make_li5640({"OTYP?": "", "DOUT?": "123.5"})

    assert lockin.get_ref_freq() == 123.5
    assert lockin.controller.inst.commands == ["OTYP?", "OTYP 3", "DOUT?"]


def test_li5640_live_read_continues_when_display_snapshot_is_malformed():
    lockin = make_li5640(
        {
            "DDEF? 1": "not-an-index",
            "OTYP?": ["1,2", "1,2"],
            "DOUT?": ["1.0,-2.0", "2.5,-30.0"],
        }
    )

    assert lockin.get_live_data_raw() == {
        "X": 1.0,
        "Y": -2.0,
        "R": 2.5,
        "Theta": -30.0,
    }
    assert not any(
        command.startswith("DDEF 1,not") for command in lockin.controller.inst.commands
    )


def test_li5640_close_without_remote_control_support_still_closes(monkeypatch):
    monkeypatch.delattr(FakeVisaInstrument, "control_ren")
    lockin = make_li5640({})
    inst = lockin.controller.inst

    assert lockin.controller.is_connected()
    lockin.close()

    assert inst.closed
    assert not lockin.controller.is_connected()


def test_li5640_close_failure_releasing_remote_still_closes_instrument():
    lockin = make_li5640({})
    inst = lockin.controller.inst

    def fail_release(_mode):
        raise RuntimeError("remote release failed")

    inst.control_ren = fail_release

    with pytest.raises(RuntimeError, match="remote release failed"):
        lockin.close()

    assert inst.closed
    assert not lockin.controller.is_connected()
