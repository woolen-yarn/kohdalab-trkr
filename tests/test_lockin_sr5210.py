from __future__ import annotations

from collections import deque

import pytest

from kohdalab.api.devices.lockin import (
    get_lockin_wait_time,
    read_lockin_overload,
    read_lockin_settings,
    read_lockin_signal,
)
from kohdalab.instruments.lockin import LOCKIN_CONTROLLERS, SR5210
from kohdalab.instruments.lockin import sr5210 as sr5210_module
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


def make_sr5210(responses: dict[str, str | list[str]]) -> Lockin:
    controller = SR5210(FakeVisaInstrument(responses))
    return Lockin(
        controller=controller,
        config={"model": "SR5210", "resource": "GPIB0::10::INSTR"},
    )


def test_sr5210_is_registered_as_lockin_controller():
    assert LOCKIN_CONTROLLERS["SR5210"] is SR5210


def test_sr5210_uses_sr7265_style_terminations():
    lockin = make_sr5210({})

    assert lockin.controller.inst.write_termination == "\r"
    assert lockin.controller.inst.read_termination == "\n"


def test_sr5210_read_helpers_match_common_lockin_shape_with_unit_conversion():
    lockin = make_sr5210(
        {
            "XY": "5000,-2500",
            "MP": "5590,-90000",
            "XY;MP": ["5000,-2500", "5590,-90000"],
            "SEN": "8",
            "TC": "6",
            "FRQ": "1234567",
            "N": "80",
        }
    )

    signal = read_lockin_signal(lockin=lockin)
    assert list(signal) == ["X", "Y", "R", "Theta"]
    assert signal == {
        "X": pytest.approx(0.5e-3),
        "Y": pytest.approx(-0.25e-3),
        "R": pytest.approx(0.559e-3),
        "Theta": -90.0,
    }
    assert read_lockin_settings(lockin=lockin) == {
        "Sensitivity": 1e-3,
        "Time Constant": 1.0,
        "Ref. Freq": 1234.567,
    }
    overload = read_lockin_overload(lockin=lockin)
    assert overload["input_overload"] is True
    assert overload["raw_input_overload"] is True
    assert overload["reference_unlock"] is False
    assert overload["overload_byte"] == 80
    assert get_lockin_wait_time(lockin=lockin, multiplier=4.0) == 4.0


def test_sr5210_output_overload_sets_common_gui_input_overload():
    lockin = make_sr5210({"N": "8"})

    overload = read_lockin_overload(lockin=lockin)

    assert overload["y_output_overload"] is True
    assert overload["raw_input_overload"] is False
    assert overload["input_overload"] is True
    assert overload["overload"] is True


def test_sr5210_can_fall_back_to_single_value_signal_queries():
    lockin = make_sr5210(
        {
            "SEN": "10",
            "XY;MP": ["bad", "bad"],
            "X;Y;MAG;PHA": ["10000", "-10000", "5000", "180000"],
            "XY": "bad",
            "MP": "bad",
            "X": "10000",
            "Y": "-10000",
            "MAG": "5000",
            "PHA": "180000",
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": 10e-3,
        "Y": -10e-3,
        "R": 5e-3,
        "Theta": 180.0,
    }


def test_sr5210_computes_r_theta_from_xy_when_mp_mag_pha_are_empty():
    lockin = make_sr5210(
        {
            "SEN": "8",
            "XY;MP": ["bad", "bad"],
            "X;Y;MAG;PHA": ["bad", "bad", "bad", "bad"],
            "XY": "3000,4000",
            "MP": "",
            "MAG": "",
            "PHA": "",
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": pytest.approx(0.3e-3),
        "Y": pytest.approx(0.4e-3),
        "R": pytest.approx(0.5e-3),
        "Theta": pytest.approx(53.13010235415598),
    }


def test_sr5210_retries_empty_response_before_failing():
    lockin = make_sr5210(
        {
            "SEN": "8",
            "XY;MP": ["", "3000,4000", "", "5000,53130"],
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": pytest.approx(0.3e-3),
        "Y": pytest.approx(0.4e-3),
        "R": pytest.approx(0.5e-3),
        "Theta": pytest.approx(53.13),
    }


def test_sr5210_writes_model_specific_setting_commands():
    lockin = make_sr5210({"SEN": "8", "TC": "6", "XDB": "1"})
    inst = lockin.controller.inst

    lockin.set_sensitivity(1e-3)
    lockin.set_time_constant(1.0)
    lockin.set_slope(12)
    lockin.set_coupling("AC")
    lockin.auto_phase()
    lockin.auto_sensitivity()

    assert inst.commands == ["SEN 8", "TC 6", "XDB 1", "AQN", "AS"]


def test_sr5210_rejects_unsupported_settings():
    lockin = make_sr5210({})

    with pytest.raises(NotImplementedError, match="AC gain"):
        lockin.set_ac_gain(10.0)
    with pytest.raises(ValueError, match="AC coupling"):
        lockin.set_coupling("DC")
    with pytest.raises(ValueError, match="Unsupported slope"):
        lockin.set_slope(24)


@pytest.mark.parametrize("response_count", [True, 0, 1.5])
def test_sr5210_rejects_invalid_response_count_before_io(response_count):
    lockin = make_sr5210({})
    inst = lockin.controller.inst

    with pytest.raises(ValueError, match="positive integer"):
        lockin.controller.ask_responses("XY", response_count=response_count)

    assert inst.commands == []


def test_sr5210_times_out_on_empty_single_and_partial_multi_response(monkeypatch):
    monkeypatch.setattr(sr5210_module.time, "sleep", lambda _delay: None)
    single = make_sr5210({"SEN": ""})
    partial = make_sr5210({"XY": ["1", ""]})

    with pytest.raises(TimeoutError, match="SEN response"):
        single.controller.ask("SEN")
    with pytest.raises(TimeoutError, match=r"response 2/2 to XY"):
        partial.controller.ask_responses("XY", response_count=2)


@pytest.mark.parametrize(
    ("command", "method", "response", "message"),
    [
        ("SEN", "get_sensitivity", "16", "Unexpected sensitivity index"),
        ("TC", "get_time_constant", "14", "Unexpected time constant index"),
        ("XDB", "get_slope", "2", "Unexpected slope mode"),
        ("SEN", "get_sensitivity", "1.5", "Unexpected non-integer"),
    ],
)
def test_sr5210_rejects_unknown_or_fractional_enum_responses(
    command, method, response, message
):
    lockin = make_sr5210({command: response})

    with pytest.raises((RuntimeError, ValueError), match=message):
        getattr(lockin, method)()


def test_sr5210_setters_accept_table_boundaries_and_write_exact_indices():
    lockin = make_sr5210({})
    inst = lockin.controller.inst

    lockin.set_sensitivity(100e-9)
    lockin.set_sensitivity(3.0)
    lockin.set_time_constant(1e-3)
    lockin.set_time_constant(3e3)
    lockin.set_slope(6)
    lockin.set_coupling(" ac ")

    assert inst.commands == ["SEN 0", "SEN 15", "TC 0", "TC 13", "XDB 0"]


@pytest.mark.parametrize("slope", [True, 6.0, "6"])
def test_sr5210_rejects_ambiguous_slope_types_before_io(slope):
    lockin = make_sr5210({})
    inst = lockin.controller.inst

    with pytest.raises(ValueError, match="slope must be one of"):
        lockin.set_slope(slope)

    assert inst.commands == []


def test_sr5210_capabilities_auto_measure_and_connection_lifecycle():
    lockin = make_sr5210({})
    controller = lockin.controller
    inst = controller.inst

    assert controller.configure() is None
    assert controller.get_ac_gain() is None
    assert controller.get_coupling() == "AC"
    assert controller.get_available_couplings() == ["AC"]
    assert controller.get_available_slopes() == [6, 12]
    assert controller.get_available_sensitivities() == sorted(
        controller.get_available_sensitivities()
    )
    assert controller.get_available_time_constants() == sorted(
        controller.get_available_time_constants()
    )
    assert controller.get_available_ac_gains() == []
    assert controller.is_connected()

    controller.auto_measure()
    controller.close()

    assert inst.commands == ["ASM"]
    assert inst.closed
    assert not controller.is_connected()


def test_sr5210_four_response_fallback_preserves_valid_xy_pair():
    lockin = make_sr5210(
        {
            "SEN": "8",
            "XY;MP": ["bad", "bad"],
            "XY": "3000,4000",
            "MP": "bad",
            "X;Y;MAG;PHA": ["ignored-x", "ignored-y", "5000", "53130"],
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": pytest.approx(0.3e-3),
        "Y": pytest.approx(0.4e-3),
        "R": pytest.approx(0.5e-3),
        "Theta": pytest.approx(53.13),
    }


def test_sr5210_four_response_fallback_preserves_valid_magnitude_phase_pair():
    lockin = make_sr5210(
        {
            "SEN": "8",
            "XY;MP": ["bad", "bad"],
            "XY": "bad",
            "MP": "5000,53130",
            "X;Y;MAG;PHA": ["3000", "4000", "ignored-r", "ignored-theta"],
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": pytest.approx(0.3e-3),
        "Y": pytest.approx(0.4e-3),
        "R": pytest.approx(0.5e-3),
        "Theta": pytest.approx(53.13),
    }


def test_sr5210_final_individual_queries_can_supply_all_signal_components():
    lockin = make_sr5210(
        {
            "SEN": "8",
            "XY;MP": ["bad", "bad"],
            "XY": "bad",
            "MP": "bad",
            "X;Y;MAG;PHA": ["bad", "bad", "bad", "bad"],
            "X": "3000",
            "Y": "4000",
            "MAG": "5000",
            "PHA": "53130",
        }
    )

    assert read_lockin_signal(lockin=lockin) == {
        "X": pytest.approx(0.3e-3),
        "Y": pytest.approx(0.4e-3),
        "R": pytest.approx(0.5e-3),
        "Theta": pytest.approx(53.13),
    }
