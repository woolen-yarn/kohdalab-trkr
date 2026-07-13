from __future__ import annotations

import pytest
import serial

import kohdalab.instruments.delay_stage.gsc01 as gsc01_module
from kohdalab.instruments.delay_stage.gsc01 import GSC01
import kohdalab.instruments.delay_stage.shot302 as shot302_module
from kohdalab.instruments.delay_stage.shot302 import Shot302GS


class ScriptedSerial:
    def __init__(self, responses: dict[str, list[str]]):
        self.responses = {
            command: list(values) for command, values in responses.items()
        }
        self.commands: list[str] = []
        self.is_open = True

    def reset_input_buffer(self):
        return None

    def write(self, data: bytes):
        self.commands.append(data.decode("ascii").strip())

    def read_until(self, _terminator: bytes) -> bytes:
        command = self.commands[-1]
        responses = self.responses.get(command, [])
        response = responses.pop(0) if responses else ""
        return f"{response}\r\n".encode("ascii")

    def close(self):
        self.is_open = False


@pytest.fixture(params=[GSC01, Shot302GS], ids=["gsc01", "shot302"])
def controller_factory(request):
    def make(responses: dict[str, list[str]], *, axis_count: int = 1):
        transport = ScriptedSerial(responses)
        controller = request.param(
            port="fake",
            axis_count=axis_count,
            default_axis=1,
            inst=transport,
        )
        return controller, transport

    return make


def test_absolute_move_uses_transport_and_reports_progress(controller_factory):
    controller, transport = controller_factory(
        {
            "A:1+P100": ["OK"],
            "G:": ["OK"],
            "Q:": ["+100", "+100"],
            "!:": ["R"],
        }
    )
    positions: list[int] = []

    assert controller.move_abs_raw(100, on_position=positions.append) == 100
    assert positions == [100]
    assert transport.commands == ["A:1+P100", "G:", "Q:", "!:", "Q:"]


def test_relative_move_uses_current_position_to_compute_target(controller_factory):
    controller, transport = controller_factory(
        {
            "Q:": ["+10", "+15", "+15"],
            "M:1+P5": ["OK"],
            "G:": ["OK"],
            "!:": ["R"],
        }
    )

    assert controller.move_rel_raw(5) == 15
    assert transport.commands == ["Q:", "M:1+P5", "G:", "Q:", "!:", "Q:"]


def test_rejected_move_does_not_send_execute_command(controller_factory):
    controller, transport = controller_factory({"A:1-P10": ["NG: LIMIT"]})

    with pytest.raises(RuntimeError, match="A command failed"):
        controller.move_abs_raw(-10)

    assert transport.commands == ["A:1-P10"]


def test_malformed_position_response_is_rejected(controller_factory):
    controller, transport = controller_factory({"Q:": ["INVALID", "INVALID"]})

    with pytest.raises(RuntimeError, match="Unexpected .* Q response"):
        controller.get_pos_raw()

    assert transport.commands == ["Q:"]


def test_wait_position_timeout_reports_last_position_without_sleep(controller_factory):
    controller, transport = controller_factory({"Q:": ["+9"]})

    with pytest.raises(TimeoutError, match="target=10, current=9"):
        controller.wait_position(10, timeout=0.0, poll_interval=0.0)

    assert transport.commands == ["Q:"]


def test_transport_lifecycle_is_observable(controller_factory):
    controller, transport = controller_factory({})

    assert controller.is_connected()
    controller.close()
    assert not controller.is_connected()


def test_multi_axis_position_response_and_axis_bounds(controller_factory):
    controller, transport = controller_factory(
        {"Q:": ["+10, -20, K, K", "+10, -20, K, K"]},
        axis_count=2,
    )

    assert controller.get_positions() == [10, -20]
    with pytest.raises(ValueError, match="Axis 3"):
        controller.get_pos_raw(axis=3)
    assert transport.commands == ["Q:"]


def test_wait_ready_polls_busy_state(controller_factory):
    controller, transport = controller_factory({"!:": ["B", "R"]})

    controller.wait_ready(poll_interval=0.0, timeout=1.0)
    assert transport.commands == ["!:", "!:"]


def test_wait_ready_zero_timeout_does_not_issue_transport_command(controller_factory):
    controller, transport = controller_factory({})

    with pytest.raises(TimeoutError, match="stayed busy"):
        controller.wait_ready(poll_interval=0.0, timeout=0.0)

    assert transport.commands == []


def test_control_commands_use_expected_protocol_messages(controller_factory):
    controller, transport = controller_factory(
        {
            "C:11": ["OK"],
            "J:1-": ["OK"],
            "G:": ["OK"],
            "R:": ["OK"],
            "H:1": ["OK"],
            "!:": ["R"],
            "L:E": ["OK"],
            "D:1S100F1000R200": ["OK"],
        }
    )

    controller.set_excitation(True)
    controller.jog(positive=False)
    controller.set_logical_zero()
    controller.home()
    controller.stop()
    controller.set_speed(1, 100, 1000, 200)

    assert transport.commands == [
        "C:11",
        "J:1-",
        "G:",
        "R:",
        "H:1",
        "!:",
        "L:E",
        "D:1S100F1000R200",
    ]


def test_empty_and_partial_responses_are_timeouts(controller_factory):
    controller, transport = controller_factory({"Q:": [""]})

    with pytest.raises(TimeoutError, match="timed out waiting for Q: response"):
        controller.get_status()

    transport.read_until = lambda _terminator: b"+10"
    with pytest.raises(TimeoutError, match="timed out waiting for Q: response"):
        controller.get_status()


def test_non_ascii_response_is_rejected(controller_factory):
    controller, transport = controller_factory({})
    transport.read_until = lambda _terminator: b"\xff\r\n"

    with pytest.raises(RuntimeError, match="non-ASCII data"):
        controller.get_status()


def test_disconnect_during_query_has_command_context(controller_factory):
    controller, transport = controller_factory({})

    def disconnect(_terminator):
        transport.is_open = False
        raise serial.SerialException("device disconnected")

    transport.read_until = disconnect

    with pytest.raises(
        ConnectionError, match=r"serial I/O failed for Q:.*disconnected"
    ):
        controller.get_status()


def test_closed_connection_is_rejected_before_io(controller_factory):
    controller, transport = controller_factory({})
    transport.is_open = False

    with pytest.raises(ConnectionError, match="serial connection is closed"):
        controller.get_status()

    assert transport.commands == []


@pytest.mark.parametrize("response", ["ERR 123", "+10 garbage", "R,+20"])
def test_position_parser_rejects_digits_embedded_in_invalid_response(
    controller_factory,
    response: str,
):
    controller, transport = controller_factory({"Q:": [response]})

    with pytest.raises(RuntimeError, match="Unexpected .* Q response"):
        controller.get_pos_raw()

    assert transport.commands == ["Q:"]


def test_ready_parser_rejects_unknown_state(controller_factory):
    controller, transport = controller_factory({"!:": ["READY"]})

    with pytest.raises(RuntimeError, match="Unexpected .* ! response"):
        controller.is_ready()

    assert transport.commands == ["!:"]


@pytest.mark.parametrize("target", [1.5, True, "10"])
@pytest.mark.parametrize("relative", [False, True])
def test_move_rejects_non_integer_pulse_before_io(
    controller_factory,
    target,
    relative: bool,
):
    controller, transport = controller_factory({})

    move = controller.move_rel_raw if relative else controller.move_abs_raw
    with pytest.raises(ValueError, match="integer pulse value"):
        move(target)

    assert transport.commands == []


@pytest.mark.parametrize("axis", [0, 2, 1.5, True])
def test_move_rejects_invalid_axis_before_io(controller_factory, axis):
    controller, transport = controller_factory({})

    with pytest.raises(ValueError, match="Axis"):
        controller.move_abs_raw(10, axis=axis)

    assert transport.commands == []


def test_configure_rejects_invalid_axes_atomically(controller_factory):
    controller, transport = controller_factory({}, axis_count=2)

    with pytest.raises(ValueError, match="default_axis 2 is outside 1..1"):
        controller.configure(axis_count=1, default_axis=2)

    assert controller.axis_count == 2
    assert controller.default_axis == 1
    assert transport.commands == []


@pytest.mark.parametrize(
    ("operation", "message"),
    [
        (
            lambda controller: controller.set_excitation("yes"),
            "enabled must be boolean",
        ),
        (lambda controller: controller.jog(positive=1), "positive must be boolean"),
        (
            lambda controller: controller.set_speed(1, 100.5, 1000, 200),
            "integer pulse value",
        ),
        (
            lambda controller: controller.set_speed(1, 1000, 100, 200),
            "min_pps must not exceed max_pps",
        ),
    ],
)
def test_control_parameters_are_validated_before_io(
    controller_factory,
    operation,
    message: str,
):
    controller, transport = controller_factory({})

    with pytest.raises(ValueError, match=message):
        operation(controller)

    assert transport.commands == []


@pytest.mark.parametrize("failure_point", ["reset_input_buffer", "write"])
def test_gsc01_wraps_serial_failures_before_read_with_command_context(failure_point):
    transport = ScriptedSerial({})
    controller = GSC01(port="fake", inst=transport)

    def fail(*_args):
        raise serial.SerialException("link lost")

    setattr(transport, failure_point, fail)

    with pytest.raises(ConnectionError, match=r"serial I/O failed for Q:.*link lost"):
        controller.get_status()


def test_gsc01_send_writes_without_attempting_to_read():
    transport = ScriptedSerial({})
    controller = GSC01(port="fake", inst=transport)

    def unexpected_read(_terminator):
        raise AssertionError("send must not read a response")

    transport.read_until = unexpected_read

    controller.send("L:E")

    assert transport.commands == ["L:E"]


@pytest.mark.parametrize(
    ("min_pps", "max_pps", "accel_ms"),
    [(-1, 100, 10), (0, -1, 10), (0, 100, -1)],
)
def test_gsc01_speed_rejects_each_negative_field_before_io(
    min_pps: int, max_pps: int, accel_ms: int
):
    transport = ScriptedSerial({})
    controller = GSC01(port="fake", inst=transport)

    with pytest.raises(ValueError, match="must be non-negative"):
        controller.set_speed(1, min_pps, max_pps, accel_ms)

    assert transport.commands == []


def test_gsc01_configure_failure_preserves_axes_and_position_unit():
    transport = ScriptedSerial({})
    controller = GSC01(
        port="fake", axis_count=2, default_axis=2, pos_unit="pulse", inst=transport
    )

    with pytest.raises(ValueError, match="default_axis 2 is outside 1..1"):
        controller.configure(axis_count=1, pos_unit="step")

    assert (controller.axis_count, controller.default_axis, controller.pos_unit) == (
        2,
        2,
        "pulse",
    )


def test_gsc01_configure_applies_valid_axes_and_position_unit():
    transport = ScriptedSerial({})
    controller = GSC01(
        port="fake", axis_count=2, default_axis=1, pos_unit="pulse", inst=transport
    )

    controller.configure(default_axis=2, pos_unit="microstep")

    assert (controller.axis_count, controller.default_axis, controller.pos_unit) == (
        2,
        2,
        "microstep",
    )


def test_gsc01_initialize_reports_state_after_home_transition():
    transport = ScriptedSerial(
        {
            "H:1": ["OK"],
            "!:": ["R", "R"],
            "Q:": ["+25", "+25"],
        }
    )
    controller = GSC01(port="fake", inst=transport)

    info = controller.initialize(home=True)

    assert info == {
        "ready": True,
        "status": "+25",
        "axis": 1,
        "axis_count": 1,
        "pos_raw": 25,
        "pos_unit": "pulse",
    }
    assert transport.commands == ["H:1", "!:", "!:", "Q:", "Q:"]


def test_gsc01_microstep_query_recovers_and_normalizes_supported_modes(monkeypatch):
    transport = ScriptedSerial({"?:MS": ["invalid", "MS=1", "MS=0"]})
    controller = GSC01(port="fake", inst=transport)
    monkeypatch.setattr(gsc01_module.time, "sleep", lambda _seconds: None)

    assert controller.get_microstep_division() == 2
    assert controller.get_microstep_division() == 1
    assert transport.commands == ["?:MS", "?:MS", "?:MS"]


def test_gsc01_microstep_query_rejects_unsupported_mode():
    transport = ScriptedSerial({"?:MS": ["MS=2"]})
    controller = GSC01(port="fake", inst=transport)

    with pytest.raises(RuntimeError, match="Unsupported GSC01 microstep mode"):
        controller.get_microstep_division()

    assert transport.commands == ["?:MS"]


def test_gsc01_microstep_query_rejects_three_malformed_responses(monkeypatch):
    transport = ScriptedSerial({"?:MS": ["invalid", "invalid", "invalid"]})
    controller = GSC01(port="fake", inst=transport)
    monkeypatch.setattr(gsc01_module.time, "sleep", lambda _seconds: None)

    with pytest.raises(RuntimeError, match=r"Unexpected GSC01 \?MS response"):
        controller.get_microstep_division()

    assert transport.commands == ["?:MS", "?:MS", "?:MS"]


def test_gsc01_internal_sensor_query_and_write_use_exact_protocol():
    transport = ScriptedSerial({"?:L": ["L=3"], "Z:MS1": ["OK"]})
    controller = GSC01(port="fake", inst=transport)

    assert controller.query_sensor_status() == "L=3"
    assert controller.write_internal("MS", 1) == "OK"
    assert transport.commands == ["?:L", "Z:MS1"]


def test_gsc01_rejects_status_with_too_few_axes():
    transport = ScriptedSerial({"Q:": ["+10"]})
    controller = GSC01(port="fake", axis_count=2, inst=transport)

    with pytest.raises(RuntimeError, match="Unexpected GSC01 Q response"):
        controller.get_positions()


def test_gsc01_close_and_connection_state_tolerate_detached_transport():
    controller = GSC01(port="fake", inst=ScriptedSerial({}))
    controller.inst = None

    controller.close()

    assert not controller.is_connected()


def test_gsc01_opens_serial_with_requested_transport_settings(monkeypatch):
    transport = ScriptedSerial({})
    captured = {}

    def open_serial(**settings):
        captured.update(settings)
        return transport

    monkeypatch.setattr(gsc01_module.serial, "Serial", open_serial)

    controller = GSC01(port="COM9", baudrate=19200, timeout=2.5)

    assert captured == {
        "port": "COM9",
        "baudrate": 19200,
        "bytesize": serial.EIGHTBITS,
        "parity": serial.PARITY_NONE,
        "stopbits": serial.STOPBITS_ONE,
        "timeout": 2.5,
    }
    assert controller.is_connected()
    controller.close()
    assert not controller.is_connected()


@pytest.mark.parametrize(
    ("axis_count", "default_axis", "message"),
    [
        (True, 1, "axis_count must be a positive integer"),
        (0, 1, "axis_count must be a positive integer"),
        (2, False, "default_axis must be an integer"),
        (2, 3, "outside 1..2"),
    ],
)
def test_gsc01_constructor_rejects_axis_configuration_boundaries(
    axis_count, default_axis, message
):
    with pytest.raises(ValueError, match=message):
        GSC01(
            port="fake",
            axis_count=axis_count,
            default_axis=default_axis,
            inst=ScriptedSerial({}),
        )


def test_gsc01_configure_updates_axis_count_default_axis_and_unit_together():
    controller = GSC01(port="fake", inst=ScriptedSerial({}))

    controller.configure(axis_count=2, default_axis=2, pos_unit="microstep")

    assert controller.axis_count == 2
    assert controller.default_axis == 2
    assert controller.get_pos_unit() == "microstep"


def test_gsc01_debug_query_and_raw_capture_protocol(capsys, monkeypatch):
    class DebugSerial(ScriptedSerial):
        def __init__(self):
            super().__init__({"Q:": ["+10"]})
            self.raw = bytearray(b"RAW")

        @property
        def in_waiting(self):
            return len(self.raw)

        def read(self, count):
            data = bytes(self.raw[:count])
            del self.raw[:count]
            return data

    transport = DebugSerial()
    controller = GSC01(port="fake", inst=transport)
    monkeypatch.setattr(gsc01_module.time, "sleep", lambda _seconds: None)

    assert controller.debug_query("Q:") == "+10"
    assert controller.debug_query_raw("?:L", write_termination="\r", wait=0.0) == b"RAW"

    output = capsys.readouterr().out
    assert "'Q:' -> '+10'" in output
    assert "'?:L' + '\\r' -> b'RAW'" in output
    assert transport.commands == ["Q:", "?:L"]


def test_gsc01_empty_command_response_contract_is_rejected():
    with pytest.raises(RuntimeError, match="G command returned an empty response"):
        GSC01._check_response("", "G")


def test_gsc01_wait_position_reports_busy_progress_before_ready(monkeypatch):
    transport = ScriptedSerial({"Q:": ["+9", "+10"], "!:": ["R"]})
    controller = GSC01(port="fake", inst=transport)
    positions = []
    monkeypatch.setattr(gsc01_module.time, "sleep", lambda _seconds: None)

    controller.wait_position(
        10, poll_interval=0.0, timeout=1.0, on_position=positions.append
    )

    assert positions == [9, 10]
    assert transport.commands == ["Q:", "Q:", "!:"]


def test_gsc01_initialize_without_home_and_noop_configure_preserve_settings():
    transport = ScriptedSerial({"!:": ["R"], "Q:": ["+5", "+5"]})
    controller = GSC01(port="fake", pos_unit="pulse", inst=transport)

    controller.configure()
    info = controller.initialize(home=False)

    assert info["pos_raw"] == 5
    assert info["pos_unit"] == "pulse"
    assert "H:1" not in transport.commands


@pytest.mark.parametrize("failure_point", ["reset_input_buffer", "write"])
def test_shot302_wraps_pre_read_serial_failures_with_command_context(failure_point):
    transport = ScriptedSerial({})
    controller = Shot302GS(port="fake", inst=transport)

    def fail(*_args):
        raise serial.SerialException("SHOT link lost")

    setattr(transport, failure_point, fail)

    with pytest.raises(ConnectionError, match=r"serial I/O failed for Q:.*link lost"):
        controller.get_status()


def test_shot302_send_writes_without_reading():
    transport = ScriptedSerial({})
    controller = Shot302GS(port="fake", inst=transport)

    def unexpected_read(_terminator):
        raise AssertionError("send must not read a response")

    transport.read_until = unexpected_read

    controller.send("L:E")

    assert transport.commands == ["L:E"]


@pytest.mark.parametrize(
    ("min_pps", "max_pps", "accel_ms"),
    [(-1, 100, 10), (0, -1, 10), (0, 100, -1)],
)
def test_shot302_speed_rejects_each_negative_field_before_io(
    min_pps: int, max_pps: int, accel_ms: int
):
    transport = ScriptedSerial({})
    controller = Shot302GS(port="fake", inst=transport)

    with pytest.raises(ValueError, match="must be non-negative"):
        controller.set_speed(1, min_pps, max_pps, accel_ms)

    assert transport.commands == []


@pytest.mark.parametrize(
    ("axis_count", "default_axis", "message"),
    [
        (True, 1, "axis_count must be a positive integer"),
        (0, 1, "axis_count must be a positive integer"),
        (2, False, "default_axis must be an integer"),
        (2, 3, "outside 1..2"),
    ],
)
def test_shot302_configure_rejects_axis_boundaries_atomically(
    axis_count, default_axis, message: str
):
    transport = ScriptedSerial({})
    controller = Shot302GS(
        port="fake", axis_count=2, default_axis=2, pos_unit="pulse", inst=transport
    )

    with pytest.raises(ValueError, match=message):
        controller.configure(
            axis_count=axis_count,
            default_axis=default_axis,
            pos_unit="step",
        )

    assert (controller.axis_count, controller.default_axis, controller.pos_unit) == (
        2,
        2,
        "pulse",
    )
    assert transport.commands == []


def test_shot302_initialize_reports_state_after_home_transition():
    transport = ScriptedSerial(
        {
            "H:1": ["OK"],
            "!:": ["B", "R", "R"],
            "Q:": ["+25", "+25"],
        }
    )
    controller = Shot302GS(port="fake", inst=transport)

    info = controller.initialize(home=True)

    assert info == {
        "ready": True,
        "status": "+25",
        "axis": 1,
        "axis_count": 1,
        "pos_raw": 25,
        "pos_unit": "pulse",
    }
    assert transport.commands == ["H:1", "!:", "!:", "!:", "Q:", "Q:"]


def test_shot302_microstep_query_recovers_from_transient_malformed_responses(
    monkeypatch,
):
    transport = ScriptedSerial({"?:S1": ["invalid", "still invalid", "S1=8"]})
    controller = Shot302GS(port="fake", inst=transport)
    monkeypatch.setattr(shot302_module.time, "sleep", lambda _seconds: None)

    assert controller.get_microstep_division() == 8
    assert transport.commands == ["?:S1", "?:S1", "?:S1"]


def test_shot302_microstep_query_rejects_three_malformed_responses(monkeypatch):
    transport = ScriptedSerial({"?:S1": ["invalid", "invalid", "invalid"]})
    controller = Shot302GS(port="fake", inst=transport)
    monkeypatch.setattr(shot302_module.time, "sleep", lambda _seconds: None)

    with pytest.raises(RuntimeError, match=r"Unexpected SHOT-302GS \?S response"):
        controller.get_microstep_division()

    assert transport.commands == ["?:S1", "?:S1", "?:S1"]


def test_shot302_debug_query_and_raw_capture_protocol(capsys, monkeypatch):
    class DebugSerial(ScriptedSerial):
        def __init__(self):
            super().__init__({"Q:": ["+20"]})
            self.raw = bytearray(b"SHOT-RAW")

        @property
        def in_waiting(self):
            return len(self.raw)

        def read(self, count):
            data = bytes(self.raw[:count])
            del self.raw[:count]
            return data

    transport = DebugSerial()
    controller = Shot302GS(port="fake", inst=transport)
    monkeypatch.setattr(shot302_module.time, "sleep", lambda _seconds: None)

    assert controller.debug_query("Q:") == "+20"
    assert controller.debug_query_raw("?:L", write_termination="\r", wait=0.0) == (
        b"SHOT-RAW"
    )

    output = capsys.readouterr().out
    assert "'Q:' -> '+20'" in output
    assert "'?:L' + '\\r' -> b'SHOT-RAW'" in output
    assert transport.commands == ["Q:", "?:L"]


def test_shot302_wait_position_polls_busy_position_before_ready(monkeypatch):
    transport = ScriptedSerial({"Q:": ["+19", "+20"], "!:": ["R"]})
    controller = Shot302GS(port="fake", inst=transport)
    positions = []
    monkeypatch.setattr(shot302_module.time, "sleep", lambda _seconds: None)

    controller.wait_position(
        20, poll_interval=0.0, timeout=1.0, on_position=positions.append
    )

    assert positions == [19, 20]
    assert transport.commands == ["Q:", "Q:", "!:"]


def test_shot302_initialize_without_home_and_noop_configure_preserve_settings():
    transport = ScriptedSerial({"!:": ["R"], "Q:": ["+5", "+5"]})
    controller = Shot302GS(port="fake", pos_unit="pulse", inst=transport)

    controller.configure()
    info = controller.initialize(home=False)

    assert info["pos_raw"] == 5
    assert info["pos_unit"] == "pulse"
    assert "H:1" not in transport.commands


def test_shot302_empty_command_response_contract_is_rejected():
    with pytest.raises(RuntimeError, match="G command returned an empty response"):
        Shot302GS._check_response("", "G")


def test_shot302_close_and_connection_state_tolerate_detached_transport():
    controller = Shot302GS(port="fake", inst=ScriptedSerial({}))
    controller.inst = None

    controller.close()

    assert not controller.is_connected()


def test_shot302_internal_sensor_and_microstep_queries_use_exact_protocol():
    transport = ScriptedSerial({"?:L": ["L=5"], "?:MT": ["MT=2"]})
    controller = Shot302GS(port="fake", inst=transport)

    assert controller.query_sensor_status() == "L=5"
    assert controller.query_microstep() == "MT=2"
    assert transport.commands == ["?:L", "?:MT"]


def test_shot302_rejected_home_does_not_poll_ready_state():
    transport = ScriptedSerial({"H:1": ["NG: SENSOR"]})
    controller = Shot302GS(port="fake", inst=transport)

    with pytest.raises(RuntimeError, match="H command failed"):
        controller.home()

    assert transport.commands == ["H:1"]


def test_shot302_execute_failure_stops_move_before_position_polling():
    transport = ScriptedSerial({"A:1+P10": ["OK"], "G:": ["ERR: BUSY"]})
    controller = Shot302GS(port="fake", inst=transport)

    with pytest.raises(RuntimeError, match="G command failed"):
        controller.move_abs_raw(10)

    assert transport.commands == ["A:1+P10", "G:"]


def test_shot302_status_with_too_few_axes_is_rejected():
    transport = ScriptedSerial({"Q:": ["+10"]})
    controller = Shot302GS(port="fake", axis_count=2, inst=transport)

    with pytest.raises(RuntimeError, match="Unexpected SHOT-302GS Q response"):
        controller.get_positions()


def test_shot302_opens_serial_with_explicit_transport_configuration(monkeypatch):
    transport = ScriptedSerial({})
    observed: list[dict] = []

    def open_serial(**kwargs):
        observed.append(kwargs)
        return transport

    monkeypatch.setattr(shot302_module.serial, "Serial", open_serial)

    controller = Shot302GS(port="COM7", baudrate=19200, timeout=2.5)

    assert controller.inst is transport
    assert observed == [
        {
            "port": "COM7",
            "baudrate": 19200,
            "bytesize": serial.EIGHTBITS,
            "parity": serial.PARITY_NONE,
            "stopbits": serial.STOPBITS_ONE,
            "timeout": 2.5,
        }
    ]


def test_shot302_configure_applies_valid_axes_and_position_unit():
    controller = Shot302GS(
        port="fake",
        axis_count=2,
        default_axis=1,
        pos_unit="pulse",
        inst=ScriptedSerial({}),
    )

    controller.configure(default_axis=2, pos_unit="microstep")

    assert (controller.axis_count, controller.default_axis, controller.pos_unit) == (
        2,
        2,
        "microstep",
    )


def test_shot302_internal_write_uses_exact_protocol():
    transport = ScriptedSerial({"Z:MS4": ["OK"]})
    controller = Shot302GS(port="fake", inst=transport)

    assert controller.write_internal("MS", 4) == "OK"
    assert transport.commands == ["Z:MS4"]


def test_shot302_wait_position_requires_ready_after_reaching_target(monkeypatch):
    transport = ScriptedSerial(
        {
            "Q:": ["+10", "+10"],
            "!:": ["B", "R"],
        }
    )
    controller = Shot302GS(port="fake", inst=transport)
    monkeypatch.setattr(shot302_module.time, "sleep", lambda _seconds: None)

    controller.wait_position(10, poll_interval=0.0, timeout=1.0)

    assert transport.commands == ["Q:", "!:", "Q:", "!:"]
