from __future__ import annotations

import pytest

from kohdalab.api.devices.lockin import (
    get_lockin_wait_time,
    read_lockin_overload,
    read_lockin_settings,
    read_lockin_signal,
)
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
    return Lockin(
        controller=controller, config={"model": "SR830", "resource": "GPIB0::8::INSTR"}
    )


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


def test_sr830_exposes_complete_model_capabilities_and_boundaries():
    controller = SR830(FakeVisaInstrument({}))

    assert controller.configure() is None
    assert controller.get_ac_gain() is None
    assert controller.get_available_ac_gains() == []
    assert controller.get_available_couplings() == ["AC", "DC"]
    assert controller.get_available_slopes() == [6, 12, 18, 24]
    time_constants = controller.get_available_time_constants()
    sensitivities = controller.get_available_sensitivities()
    assert [time_constants[0], time_constants[-1]] == [10e-6, 30e3]
    assert [sensitivities[0], sensitivities[-1]] == [2e-9, 1.0]


@pytest.mark.parametrize(
    ("responses", "getter", "expected"),
    [
        ({"OFLT?": "0"}, "get_time_constant", 10e-6),
        ({"OFLT?": "19"}, "get_time_constant", 30e3),
        ({"SENS?": "0"}, "get_sensitivity", 2e-9),
        ({"SENS?": "26"}, "get_sensitivity", 1.0),
        ({"ICPL?": "0"}, "get_coupling", "AC"),
        ({"ICPL?": "1"}, "get_coupling", "DC"),
        ({"OFSL?": "0"}, "get_slope", 6),
        ({"OFSL?": "3"}, "get_slope", 24),
    ],
)
def test_sr830_getters_accept_documented_boundary_indices(responses, getter, expected):
    controller = SR830(FakeVisaInstrument(responses))

    assert getattr(controller, getter)() == expected


@pytest.mark.parametrize(
    ("response", "getter", "message"),
    [
        ({"OFLT?": "20"}, "get_time_constant", "Unexpected time constant index: 20"),
        ({"SENS?": "27"}, "get_sensitivity", "Unexpected sensitivity index: 27"),
        ({"ICPL?": "2"}, "get_coupling", "Unexpected coupling mode: 2"),
        ({"OFSL?": "4"}, "get_slope", "Unexpected slope mode: 4"),
    ],
)
def test_sr830_getters_fail_closed_for_unknown_instrument_indices(
    response, getter, message
):
    controller = SR830(FakeVisaInstrument(response))

    with pytest.raises(RuntimeError, match=message):
        getattr(controller, getter)()


def test_sr830_accepts_normalized_coupling_and_all_documented_slopes():
    controller = SR830(FakeVisaInstrument({}))

    controller.set_coupling(" dc ")
    for slope in (6, 12, 18, 24):
        controller.set_slope(slope)

    assert controller.inst.commands == [
        "ICPL 1",
        "OFSL 0",
        "OFSL 1",
        "OFSL 2",
        "OFSL 3",
    ]


@pytest.mark.parametrize("value", [None, 1, True])
def test_sr830_rejects_non_string_coupling_without_io(value):
    controller = SR830(FakeVisaInstrument({}))

    with pytest.raises(ValueError, match="coupling must be AC or DC"):
        controller.set_coupling(value)  # type: ignore[arg-type]

    assert controller.inst.commands == []


def test_sr830_rejects_unknown_coupling_without_io():
    controller = SR830(FakeVisaInstrument({}))

    with pytest.raises(ValueError, match="Unsupported coupling: GND"):
        controller.set_coupling("GND")

    assert controller.inst.commands == []


@pytest.mark.parametrize("value", [True, 12.0, "12", 0, 30])
def test_sr830_rejects_invalid_slope_without_io(value):
    controller = SR830(FakeVisaInstrument({}))

    with pytest.raises(ValueError, match="slope must|Unsupported slope"):
        controller.set_slope(value)  # type: ignore[arg-type]

    assert controller.inst.commands == []


def test_sr830_status_maps_every_documented_status_bit():
    controller = SR830(FakeVisaInstrument({"LIAS?": "123"}))

    assert controller.get_overload_status() == {
        "overload": True,
        "input_overload": True,
        "filter_overload": True,
        "reference_unlock": True,
        "range_changed": True,
        "time_constant_changed": True,
        "data_storage_triggered": True,
        "overload_byte": 123,
    }


def test_sr830_query_continues_when_optional_visa_clear_fails(monkeypatch):
    controller = SR830(FakeVisaInstrument({"FREQ?": "123.5"}))

    def fail_clear():
        raise OSError("clear unsupported")

    monkeypatch.setattr(controller.inst, "clear", fail_clear)

    assert controller.get_ref_freq() == 123.5
    assert controller.inst.commands == ["FREQ?"]


def test_sr830_empty_query_response_is_a_timeout():
    controller = SR830(FakeVisaInstrument({"FREQ?": ""}))

    with pytest.raises(TimeoutError, match=r"waiting for FREQ\? response"):
        controller.get_ref_freq()


def test_sr830_auto_measure_and_close_follow_transport_contract():
    controller = SR830(FakeVisaInstrument({}))

    assert controller.is_connected() is True
    controller.auto_measure()
    controller.close()

    assert controller.inst.commands == ["AGAN"]
    assert controller.inst.closed is True
    assert controller.inst.session is None
    assert controller.is_connected() is False
