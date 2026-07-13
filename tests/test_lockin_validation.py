from __future__ import annotations

from collections import deque
from concurrent.futures import ThreadPoolExecutor
from threading import Event
from typing import Any

import pytest

from kohdalab.instruments.lockin import LI5640, SR5210, SR7265, SR830
from kohdalab.instruments.lockin._validation import (
    ensure_connected,
    integer_response,
    parse_float_response,
    resolve_index_from_table,
    wait_time,
)
from kohdalab.interfaces.lockin import Lockin


DRIVERS = (SR830, LI5640, SR5210, SR7265)


class QueueVisa:
    def __init__(self, responses: list[object] | None = None):
        self.responses = deque(responses or [])
        self.writes: list[str] = []
        self.session: object | None = object()
        self.write_termination = ""
        self.read_termination = ""
        self.write_error: Exception | None = None

    def clear(self) -> None:
        return None

    def write(self, command: str) -> None:
        if self.write_error is not None:
            raise self.write_error
        self.writes.append(command)

    def read(self) -> object:
        response = self.responses.popleft()
        if isinstance(response, Exception):
            raise response
        return response

    def close(self) -> None:
        self.session = None


@pytest.mark.parametrize("driver_class", DRIVERS)
def test_lockin_queries_reject_closed_transport_before_io(driver_class):
    transport = QueueVisa(["1"])
    driver = driver_class(transport)
    transport.session = None

    with pytest.raises(ConnectionError, match="connection is closed"):
        driver.ask_float("TEST", delay=0.0)

    assert transport.writes == []


@pytest.mark.parametrize("driver_class", DRIVERS)
def test_lockin_read_failures_include_model_and_command(driver_class):
    transport = QueueVisa([OSError("link lost")])
    driver = driver_class(transport)

    with pytest.raises(
        ConnectionError, match=rf"{driver_class.__name__} VISA read failed for TEST"
    ):
        driver.ask_float("TEST", delay=0.0)


@pytest.mark.parametrize("driver_class", DRIVERS)
def test_lockin_write_failures_are_classified_as_connection_errors(driver_class):
    transport = QueueVisa()
    transport.write_error = OSError("link lost")
    driver = driver_class(transport)

    with pytest.raises(
        ConnectionError, match=rf"{driver_class.__name__} VISA write failed"
    ):
        driver.auto_phase()


@pytest.mark.parametrize("driver_class", DRIVERS)
@pytest.mark.parametrize("response", ["value=1.25", "NaN", "Inf", "１.０"])
def test_lockin_scalar_queries_reject_malformed_or_nonfinite_values(
    driver_class, response
):
    transport = QueueVisa([response])
    driver = driver_class(transport)

    with pytest.raises(RuntimeError):
        driver.ask_float("TEST", delay=0.0)


@pytest.mark.parametrize("driver_class", DRIVERS)
@pytest.mark.parametrize("value", [float("nan"), float("inf"), True])
def test_lockin_setters_reject_nonfinite_and_boolean_values_without_io(
    driver_class, value
):
    transport = QueueVisa()
    driver = driver_class(transport)

    with pytest.raises(ValueError):
        driver.set_time_constant(value)

    assert transport.writes == []


@pytest.mark.parametrize("driver_class", DRIVERS)
@pytest.mark.parametrize("multiplier", [-1.0, float("nan"), float("inf"), True])
def test_lockin_wait_time_rejects_invalid_multiplier(driver_class, multiplier):
    driver = driver_class(QueueVisa())
    driver.get_time_constant = lambda: 1.0

    with pytest.raises(ValueError):
        driver.get_wait_time(multiplier)


@pytest.mark.parametrize(
    ("driver_class", "getter", "command"),
    [
        (SR830, "get_time_constant", "OFLT?"),
        (LI5640, "get_time_constant", "TCON?"),
        (SR5210, "get_time_constant", "TC"),
        (SR7265, "get_imode", "IMODE"),
    ],
)
def test_lockin_index_queries_reject_fractional_responses(
    driver_class, getter, command
):
    transport = QueueVisa(["1.5"])
    driver = driver_class(transport)

    with pytest.raises(RuntimeError, match="non-integer"):
        getattr(driver, getter)()

    assert transport.writes == [command]


def test_sr7265_bulk_read_does_not_hide_mid_command_disconnect():
    transport = QueueVisa(["1,2", OSError("disconnected")])
    driver = SR7265(transport)

    with pytest.raises(ConnectionError, match="XY.;MP."):
        driver.get_live_data_raw()

    assert transport.writes == ["XY.;MP."]


def test_li5640_restores_temporary_output_mode_after_invalid_numeric_response():
    transport = QueueVisa(["1,2", "bad"])
    driver = LI5640(transport)

    with pytest.raises(RuntimeError, match="Unexpected response for DOUT"):
        driver._read_output_values([1, 2], expected_count=2)

    assert transport.writes == ["OTYP?", "OTYP 1,2", "DOUT?", "OTYP 1,2"]


class ConcurrentController:
    def __init__(self) -> None:
        self.active = 0
        self.max_active = 0
        self.first_call_entered = Event()
        self.release_first_call = Event()

    def get_time_constant(self) -> float:
        self.active += 1
        self.max_active = max(self.max_active, self.active)
        if not self.first_call_entered.is_set():
            self.first_call_entered.set()
            if not self.release_first_call.wait(timeout=1.0):
                raise TimeoutError("test did not release the first controller call")
        self.active -= 1
        return 1.0


def test_lockin_wrapper_serializes_direct_controller_calls():
    controller = ConcurrentController()
    lockin = Lockin(controller=controller, config={})  # type: ignore[arg-type]

    with ThreadPoolExecutor(max_workers=4) as executor:
        first = executor.submit(lockin.get_time_constant)
        assert controller.first_call_entered.wait(timeout=1.0)
        remaining = [executor.submit(lockin.get_time_constant) for _index in range(7)]
        controller.release_first_call.set()
        results = [first.result(), *(future.result() for future in remaining)]

    assert results == [1.0] * 8
    assert controller.max_active == 1


@pytest.mark.parametrize("response", [b"\xff", object()])
def test_lockin_rejects_non_ascii_or_invalid_response_types(response: Any):
    driver = SR830(QueueVisa([response]))

    with pytest.raises(RuntimeError):
        driver.ask("TEST", delay=0.0)


class UnreadableSession:
    @property
    def session(self) -> object:
        raise OSError("transport state unavailable")


def test_connection_check_fails_closed_when_session_state_is_unreadable():
    with pytest.raises(ConnectionError, match="MODEL VISA connection is closed"):
        ensure_connected(UnreadableSession(), "MODEL")


@pytest.mark.parametrize("expected_count", [True, 1.0, "1"])
def test_float_response_parser_rejects_non_integer_expected_count(expected_count):
    with pytest.raises(ValueError, match="positive integer"):
        parse_float_response("1", expected_count=expected_count, cmd="TEST")  # type: ignore[arg-type]


def test_float_response_parser_accepts_complete_finite_numeric_sequence():
    assert parse_float_response(" -1.25, +2E-3, .5 ", expected_count=3, cmd="TEST") == [
        -1.25,
        0.002,
        0.5,
    ]


def test_integer_response_accepts_integral_float_representation():
    assert integer_response("2.0", context="index") == 2


def test_table_index_resolution_accepts_roundoff_but_rejects_unknown_value():
    table = {0: 1e-6, 1: 2e-6}

    assert resolve_index_from_table(2e-6 * (1 + 5e-10), table, "time constant") == 1

    with pytest.raises(ValueError, match=r"Available values: 1e-06, 2e-06"):
        resolve_index_from_table(3e-6, table, "time constant")


def test_wait_time_multiplies_finite_values_and_rejects_negative_constant():
    assert wait_time(2.5, 0.4) == pytest.approx(1.0)

    with pytest.raises(RuntimeError, match="time constant must be non-negative"):
        wait_time(2.5, -0.4)
