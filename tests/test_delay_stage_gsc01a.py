from __future__ import annotations

import pytest

from kohdalab.instruments.delay_stage import DELAY_STAGE_CONTROLLERS, GSC01A


def test_gsc01a_is_registered():
    assert DELAY_STAGE_CONTROLLERS["GSC01A"] is GSC01A


def test_gsc01a_uses_axis_for_logical_zero_and_deceleration_stop():
    controller = GSC01A.__new__(GSC01A)
    controller.default_axis = 1
    controller.axis_count = 1
    commands = []
    controller.ask = lambda cmd: commands.append(cmd) or "OK"

    controller.set_logical_zero(axis=1)
    controller.stop(axis=1)

    assert commands == ["R:1", "L:1"]


def test_gsc01a_immediate_stop_command():
    controller = GSC01A.__new__(GSC01A)
    controller.axis_count = 1
    commands = []
    controller.ask = lambda cmd: commands.append(cmd) or "OK"

    controller.immediate_stop()

    assert commands == ["L:E"]


def test_gsc01a_microstep_parser_uses_axis_query():
    controller = GSC01A.__new__(GSC01A)
    controller.default_axis = 1
    controller.axis_count = 1
    commands = []
    controller.query_internal = lambda code: commands.append(code) or "2"

    assert controller.get_microstep_division(axis=1) == 2
    assert commands == ["S1"]


def test_gsc01a_microstep_parser_retries_empty_response():
    controller = GSC01A.__new__(GSC01A)
    controller.default_axis = 1
    controller.axis_count = 1
    responses = iter(["", "1"])
    controller.query_internal = lambda code: next(responses)

    assert controller.get_microstep_division(axis=1) == 1


def test_gsc01a_microstep_parser_rejects_three_malformed_responses(monkeypatch):
    controller = GSC01A.__new__(GSC01A)
    controller.default_axis = 1
    controller.axis_count = 1
    controller.query_internal = lambda code: "invalid"
    monkeypatch.setattr(
        "kohdalab.instruments.delay_stage.gsc01a.time.sleep", lambda _: None
    )

    with pytest.raises(RuntimeError, match=r"Unexpected GSC-01A \?S response: invalid"):
        controller.get_microstep_division(axis=1)


def test_gsc01a_sensor_status_uses_sensor_selector():
    controller = GSC01A.__new__(GSC01A)
    commands = []
    controller.ask = lambda cmd: commands.append(cmd) or "0"

    assert controller.query_sensor_status(sensor=5) == "0"
    assert commands == ["?:L5"]
