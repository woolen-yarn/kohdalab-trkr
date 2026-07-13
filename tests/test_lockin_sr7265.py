from __future__ import annotations

import pytest

from kohdalab.instruments.lockin.sr7265 import SR7265


class FakeVisaInstrument:
    def __init__(self, responses: list[str]):
        self.responses = list(responses)
        self.writes: list[str] = []
        self.write_termination = None
        self.read_termination = None
        self.session = 1

    def clear(self):
        return None

    def write(self, command: str):
        self.writes.append(command)

    def read(self) -> str:
        return self.responses.pop(0) if self.responses else ""

    def close(self):
        self.session = None


def controller(responses: list[str]) -> tuple[SR7265, FakeVisaInstrument]:
    transport = FakeVisaInstrument(responses)
    return SR7265(transport), transport


def test_sr7265_bulk_signal_read_uses_two_response_command():
    lockin, transport = controller(["1.25, -2.5", "3.0, 45.0"])

    assert lockin.get_live_data_raw() == {"X": 1.25, "Y": -2.5, "R": 3.0, "Theta": 45.0}
    assert transport.writes == ["XY.;MP."]


def test_sr7265_falls_back_to_separate_vector_commands():
    lockin, transport = controller(["bad", "bad", "1.0,2.0", "3.0,4.0"])

    assert lockin.get_live_data_raw() == {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}
    assert transport.writes == ["XY.;MP.", "XY.", "MP."]


def test_sr7265_falls_back_to_scalar_commands():
    lockin, transport = controller(["bad", "bad", "bad", "1.0", "2.0", "3.0", "4.0"])

    assert lockin.get_live_data_raw() == {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}
    assert transport.writes == ["XY.;MP.", "XY.", "X.", "Y.", "MAG.", "PHA."]


def test_sr7265_empty_scalar_response_is_rejected():
    lockin, _transport = controller([""])

    with pytest.raises(TimeoutError, match="timed out waiting for TC response"):
        lockin.ask_float("TC", delay=0.0)


def test_sr7265_overload_bits_are_decoded():
    lockin, transport = controller(["210"])

    status = lockin.get_overload_status()

    assert status["ch1_output_overload"]
    assert status["x_output_overload"]
    assert status["input_overload"]
    assert status["reference_unlock"]
    assert status["overload"]
    assert status["overload_byte"] == 210
    assert transport.writes == ["N"]


def test_sr7265_setting_commands_use_validated_table_indexes():
    lockin, transport = controller(["0"])

    lockin.set_sensitivity(10e-9)
    lockin.set_time_constant(1.0)
    lockin.set_ac_gain(20.0)
    lockin.set_coupling("dc")
    lockin.set_slope(24)

    assert transport.writes == [
        "IMODE",
        "SEN 3",
        "TC 14",
        "ACGAIN 2",
        "CP 1",
        "SLOPE 3",
    ]


@pytest.mark.parametrize(
    ("method", "value", "message"),
    [
        ("set_time_constant", 0.03, "Unsupported time constant"),
        ("set_ac_gain", 15.0, "Unsupported AC gain"),
        ("set_coupling", "ground", "Unsupported coupling"),
        ("set_slope", 30, "Unsupported slope"),
    ],
)
def test_sr7265_invalid_setting_is_rejected_without_transport_write(
    method, value, message
):
    lockin, transport = controller([])

    with pytest.raises(ValueError, match=message):
        getattr(lockin, method)(value)

    assert transport.writes == []


def test_sr7265_transport_lifecycle():
    lockin, transport = controller([])

    assert lockin.is_connected()
    lockin.close()
    assert not lockin.is_connected()
    assert transport.write_termination == "\r"
    assert transport.read_termination == "\n"


def test_sr7265_configure_is_an_explicit_noop():
    lockin, transport = controller([])

    assert lockin.configure() is None
    assert transport.writes == []


def test_sr7265_capability_lists_are_copies_and_match_active_input_mode():
    lockin, transport = controller(["0"])

    couplings = lockin.get_available_couplings()
    slopes = lockin.get_available_slopes()
    time_constants = lockin.get_available_time_constants()
    sensitivities = lockin.get_available_sensitivities()
    ac_gains = lockin.get_available_ac_gains()

    assert couplings == ["AC", "DC"]
    assert slopes == [6, 12, 18, 24]
    assert time_constants[0] < time_constants[-1]
    assert sensitivities[0] < sensitivities[-1]
    assert ac_gains == [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0]
    assert transport.writes == ["IMODE"]

    couplings.clear()
    assert lockin.get_available_couplings() == ["AC", "DC"]


def test_sr7265_setting_queries_decode_transport_values():
    lockin, transport = controller(["0.5", "2", "1e-6", "80", "1", "2", "0.5"])

    assert lockin.get_time_constant() == 0.5
    assert lockin.get_ac_gain() == 20.0
    assert lockin.get_sensitivity() == 1e-6
    assert lockin.get_ref_freq() == 80.0
    assert lockin.get_coupling() == "DC"
    assert lockin.get_slope() == 18
    assert lockin.get_wait_time(multiplier=4.0) == 2.0
    assert transport.writes == ["TC.", "ACGAIN", "SEN.", "FRQ.", "CP", "SLOPE", "TC."]


def test_sr7265_automatic_commands_write_directly_to_transport():
    lockin, transport = controller([])

    lockin.auto_phase()
    lockin.auto_sensitivity()
    lockin.auto_measure()

    assert transport.writes == ["AQN", "AS", "ASM"]


def test_sr7265_rejects_unsupported_input_mode():
    lockin, transport = controller(["9"])

    with pytest.raises(ValueError, match="Unsupported IMODE"):
        lockin.get_available_sensitivities()

    assert transport.writes == ["IMODE"]


@pytest.mark.parametrize("response_count", [0, -1, 1.5, True])
def test_sr7265_rejects_invalid_bulk_response_count_before_write(response_count):
    lockin, transport = controller([])

    with pytest.raises(ValueError, match="positive integer"):
        lockin.ask_responses("XY.", response_count)  # type: ignore[arg-type]

    assert transport.writes == []


def test_sr7265_bulk_read_reports_which_response_timed_out():
    lockin, transport = controller(["1,2", ""])

    with pytest.raises(TimeoutError, match=r"response 2/2 to XY\.;MP\."):
        lockin.ask_responses("XY.;MP.", 2, delay=0.0)

    assert transport.writes == ["XY.;MP."]


def test_sr7265_clear_failure_does_not_hide_valid_response():
    lockin, transport = controller(["1.0"])

    def fail_clear():
        raise RuntimeError("clear unsupported")

    transport.clear = fail_clear

    assert lockin.get_time_constant() == 1.0


def test_sr7265_clear_failure_does_not_hide_valid_bulk_responses():
    lockin, transport = controller(["1,2", "3,4"])

    def fail_clear():
        raise RuntimeError("clear unsupported")

    transport.clear = fail_clear

    assert lockin.ask_responses("XY.;MP.", 2, delay=0.0) == ["1,2", "3,4"]


@pytest.mark.parametrize(
    ("method", "response", "message"),
    [
        ("get_coupling", "2", "Unexpected coupling mode: 2"),
        ("get_slope", "4", "Unexpected slope mode: 4"),
    ],
)
def test_sr7265_rejects_unknown_enum_responses(method, response, message):
    lockin, _transport = controller([response])

    with pytest.raises(RuntimeError, match=message):
        getattr(lockin, method)()


def test_sr7265_set_sensitivity_rejects_unsupported_input_mode_without_setting():
    lockin, transport = controller(["9"])

    with pytest.raises(ValueError, match="Unsupported IMODE"):
        lockin.set_sensitivity(1e-6)

    assert transport.writes == ["IMODE"]


@pytest.mark.parametrize(
    ("method", "value", "message"),
    [
        ("set_coupling", 1, "coupling must"),
        ("set_slope", True, "slope must"),
        ("set_slope", 6.0, "slope must"),
    ],
)
def test_sr7265_setters_reject_wrong_types_before_transport(method, value, message):
    lockin, transport = controller([])

    with pytest.raises(ValueError, match=message):
        getattr(lockin, method)(value)

    assert transport.writes == []
