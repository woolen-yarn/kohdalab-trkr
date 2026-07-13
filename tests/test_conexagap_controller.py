from __future__ import annotations

import math
from types import SimpleNamespace

import pytest
import serial

import kohdalab.instruments.scanner.conexagap as conexagap_module
from kohdalab.instruments.scanner.conexagap import ConexAgap


class ScriptedSerial:
    def __init__(self, responses: dict[str, list[str]]):
        self.responses = {key: list(value) for key, value in responses.items()}
        self.commands: list[str] = []
        self.is_open = True
        self.reset_count = 0
        self.flush_count = 0

    def reset_input_buffer(self):
        self.reset_count += 1
        return None

    def write(self, data: bytes):
        self.commands.append(data.decode("ascii").strip())

    def flush(self):
        self.flush_count += 1
        return None

    def readline(self) -> bytes:
        command = self.commands[-1]
        body = command[1:]
        responses = self.responses.get(body)
        if responses:
            return f"{responses.pop(0)}\r\n".encode("ascii")
        return f"{command}\r\n".encode("ascii")

    def close(self):
        self.is_open = False


def test_conexagap_move_from_ready_waits_for_motion_to_finish():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000032", "1TS000028", "1TS000033"],
            "TE": ["1TE@"],
            "TPU": ["1TPU0.2500"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    assert controller.move_abs_raw(0.25) == 0.25
    assert ser.commands == ["1TS", "1PAU0.2500", "1TE", "1TS", "1TS", "1TPU"]


def test_conexagap_move_enables_controller_from_disable():
    ser = ScriptedSerial(
        {
            "TS": ["1TS00003C", "1TS000034", "1TS000028", "1TS000033"],
            "TE": ["1TE@", "1TE@"],
            "TPV": ["1TPV-0.2500"],
        }
    )
    controller = ConexAgap(port="COM11", axis="V", ser=ser)

    assert controller.move_abs_raw(-0.25) == -0.25
    assert ser.commands == [
        "1TS",
        "1MM1",
        "1TE",
        "1TS",
        "1PAV-0.2500",
        "1TE",
        "1TS",
        "1TS",
        "1TPV",
    ]


@pytest.mark.parametrize(
    "response",
    [
        "",
        "1TS",
        "2TS000032",
        "1TS00003Z",
        "1TS000032EXTRA",
    ],
)
def test_conexagap_rejects_malformed_status_before_motion(response):
    ser = ScriptedSerial({"TS": [response]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    error = TimeoutError if response == "" else RuntimeError
    match = (
        "timed out waiting for TS response"
        if response == ""
        else "Unexpected CONEX-AGAP TS response"
    )
    with pytest.raises(error, match=match):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS"]


def test_conexagap_rejects_positioner_error_before_motion():
    ser = ScriptedSerial({"TS": ["1TS002032"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="positioner error 0020"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS"]


def test_conexagap_rejects_unknown_state_before_motion():
    ser = ScriptedSerial({"TS": ["1TS00000A"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="unknown state 0A"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS"]


def test_conexagap_does_not_treat_disable_during_motion_as_success():
    ser = ScriptedSerial(
        {"TS": ["1TS000032", "1TS000028", "1TS00003D"], "TE": ["1TE@"]}
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="unsafe state 3D"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS", "1PAU0.2500", "1TE", "1TS", "1TS"]


def test_conexagap_checks_positioner_error_while_waiting_for_motion():
    ser = ScriptedSerial(
        {"TS": ["1TS000032", "1TS000028", "1TS002033"], "TE": ["1TE@"]}
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="positioner error 0020"):
        controller.move_abs_raw(0.25)

    assert "1TPU" not in ser.commands


def test_conexagap_rejects_command_error_before_waiting_for_motion():
    ser = ScriptedSerial({"TS": ["1TS000032"], "TE": ["1TEC"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="command error C"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS", "1PAU0.2500", "1TE"]


@pytest.mark.parametrize(
    "response",
    [
        "1TE",
        "2TE@",
        "1TE@@",
        "1TEX",
    ],
)
def test_conexagap_rejects_malformed_command_error_response(response):
    ser = ScriptedSerial({"TS": ["1TS000032"], "TE": [response]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match="Unexpected CONEX-AGAP TE response"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS", "1PAU0.2500", "1TE"]


def test_conexagap_rejects_non_disable_state_while_enabling_controller():
    ser = ScriptedSerial({"TS": ["1TS00003C", "1TS000028"], "TE": ["1TE@"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(RuntimeError, match=r"failed to enter READY state \(state 28\)"):
        controller.move_abs_raw(0.25)

    assert ser.commands == ["1TS", "1MM1", "1TE", "1TS"]


def test_conexagap_times_out_if_controller_remains_disabled(monkeypatch):
    ser = ScriptedSerial({"TS": ["1TS00003C", "1TS00003D"], "TE": ["1TE@"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)
    times = iter([10.0, 16.0])
    monkeypatch.setattr(
        conexagap_module,
        "time",
        SimpleNamespace(monotonic=lambda: next(times), sleep=lambda _seconds: None),
    )

    with pytest.raises(TimeoutError, match="failed to leave DISABLE state"):
        controller.move_abs_raw(0.25, timeout=0.5)

    assert ser.commands == ["1TS", "1MM1", "1TE", "1TS"]


def test_conexagap_motion_timeout_uses_monotonic_clock(monkeypatch):
    ser = ScriptedSerial({"TS": ["1TS000028"]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)
    times = iter([10.0, 11.0])
    monkeypatch.setattr(conexagap_module.time, "monotonic", lambda: next(times))

    with pytest.raises(TimeoutError, match="motion timeout"):
        controller.wait_until_stopped(timeout=0.5, poll_interval=0.0)


@pytest.mark.parametrize("target", [math.nan, math.inf, -math.inf, True])
def test_conexagap_rejects_non_finite_target_before_io(target):
    ser = ScriptedSerial({})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(ValueError, match="target must be finite"):
        controller.move_abs_raw(target)

    assert ser.commands == []


def test_conexagap_rejects_query_timeout_non_finite_position_and_closed_connection():
    timeout_ser = ScriptedSerial({"TS": [""]})
    controller = ConexAgap(port="COM11", axis="U", ser=timeout_ser)
    with pytest.raises(TimeoutError, match="timed out waiting for TS response"):
        controller.get_state()

    position_ser = ScriptedSerial({"TPU": ["1TPUinf"]})
    controller = ConexAgap(port="COM11", axis="U", ser=position_ser)
    with pytest.raises(RuntimeError, match="non-finite position"):
        controller.get_pos_raw()

    position_ser.is_open = False
    with pytest.raises(ConnectionError, match="serial connection is closed"):
        controller.get_state()


@pytest.mark.parametrize(
    ("response", "error", "match"),
    [
        ("2TPU0.2500", RuntimeError, "Unexpected TP response"),
        ("1TPV0.2500", RuntimeError, "Unexpected TP response"),
        ("1TPUnot-a-number", ValueError, "could not convert string to float"),
    ],
)
def test_conexagap_rejects_malformed_position_response(response, error, match):
    ser = ScriptedSerial({"TPU": [response]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(error, match=match):
        controller.get_pos_raw()

    assert ser.commands == ["1TPU"]


def test_conexagap_reports_disconnect_during_motion_with_command_context():
    class DroppingSerial(ScriptedSerial):
        def readline(self) -> bytes:
            if len(self.commands) == 1:
                return b"1TS000028\r\n"
            self.is_open = False
            raise serial.SerialException("device disconnected")

    controller = ConexAgap(port="COM11", axis="U", ser=DroppingSerial({}))

    with pytest.raises(
        ConnectionError,
        match=r"COM11.*I/O failed for TS.*disconnected",
    ):
        controller.wait_until_stopped(poll_interval=0.0)


def test_conexagap_rejects_non_ascii_response():
    class NonAsciiSerial(ScriptedSerial):
        def readline(self) -> bytes:
            return b"\xff\r\n"

    controller = ConexAgap(port="COM11", axis="U", ser=NonAsciiSerial({}))

    with pytest.raises(RuntimeError, match="non-ASCII data"):
        controller.get_state()


@pytest.mark.parametrize(
    ("axis", "expected"), [(" u ", "U"), ("2", "V"), (1, "U"), (2, "V")]
)
def test_conexagap_normalizes_axis_and_configures_runtime_settings(axis, expected):
    ser = ScriptedSerial({})
    controller = ConexAgap(port="COM11", axis=axis, ser=ser)

    assert controller.axis == expected

    controller.configure(axis="V", controller_address=3, pos_unit="mm")

    assert controller.axis == "V"
    assert controller.controller_address == 3
    assert controller.get_pos_unit() == "mm"


def test_conexagap_configure_without_options_preserves_runtime_settings():
    controller = ConexAgap(port="COM11", axis="V", ser=ScriptedSerial({}))

    controller.configure()

    assert controller.axis == "V"
    assert controller.controller_address == 1
    assert controller.get_pos_unit() == "deg"


@pytest.mark.parametrize("axis", [0, 3, "x", ""])
def test_conexagap_rejects_unsupported_axis(axis):
    with pytest.raises(ValueError, match="Unsupported CONEX-AGAP axis"):
        ConexAgap(port="COM11", axis=axis, ser=ScriptedSerial({}))


def test_conexagap_owned_and_borrowed_serial_close_lifecycle(monkeypatch):
    owned = ScriptedSerial({})
    monkeypatch.setattr(conexagap_module.serial, "Serial", lambda **_kwargs: owned)
    owned_controller = ConexAgap(port="COM11")
    borrowed = ScriptedSerial({})
    borrowed_controller = ConexAgap(port="COM12", ser=borrowed)

    assert owned_controller.is_connected()
    assert borrowed_controller.is_connected()

    owned_controller.close()
    borrowed_controller.close()

    assert not owned_controller.is_connected()
    assert borrowed_controller.is_connected()


def test_conexagap_query_clears_input_and_flushes_each_command(capsys):
    ser = ScriptedSerial({"TS": ["1TS000032"]})
    controller = ConexAgap(port="COM11", ser=ser)

    assert controller.debug_query("TS") == "1TS000032"
    assert capsys.readouterr().out == "'TS' -> '1TS000032'\n"
    assert ser.reset_count == 1
    assert ser.flush_count == 1


def test_conexagap_state_accessors_and_initialize_snapshot():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000032", "1TS000032", "1TS000028"],
            "TPU": ["1TPU0.1250"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    assert controller.get_error_code() == "0000"
    assert controller.get_state_code() == "32"
    assert controller.is_moving()

    ser.responses["TS"] = ["1TS000032", "1TS000032", "1TS000032"]
    assert controller.initialize() == {
        "axis": "U",
        "state": "1TS000032",
        "moving": False,
        "pos_raw": 0.125,
        "pos_unit": "deg",
    }


def test_conexagap_initialize_with_home_waits_using_requested_timeout(monkeypatch):
    controller = ConexAgap(port="COM11", axis="V", ser=ScriptedSerial({}))
    calls = []
    monkeypatch.setattr(controller, "_ensure_ready", lambda: calls.append("ready"))
    monkeypatch.setattr(controller, "home", lambda: calls.append("home"))
    monkeypatch.setattr(
        controller,
        "wait_until_stopped",
        lambda *, timeout: calls.append(("wait", timeout)),
    )
    monkeypatch.setattr(controller, "get_state", lambda: "1TS000032")
    monkeypatch.setattr(controller, "is_moving", lambda: False)
    monkeypatch.setattr(controller, "get_pos_raw", lambda: 0.5)

    snapshot = controller.initialize(home=True, timeout=7.5)

    assert calls == ["ready", "home", ("wait", 7.5)]
    assert snapshot["axis"] == "V"
    assert snapshot["pos_raw"] == 0.5


def test_conexagap_relative_move_reports_intermediate_and_final_positions():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000032", "1TS000028", "1TS000033"],
            "TE": ["1TE@"],
            "TPU": ["1TPU0.1000", "1TPU0.2500"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)
    positions = []

    assert controller.move_rel_raw(0.15, on_position=positions.append) == 0.25
    assert positions == [0.1, 0.25]
    assert "1PRU0.1500" in ser.commands


@pytest.mark.parametrize("target", [math.nan, math.inf, -math.inf, True])
def test_conexagap_rejects_non_finite_relative_target_before_io(target):
    ser = ScriptedSerial({})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(ValueError, match="relative target must be finite"):
        controller.move_rel_raw(target)

    assert ser.commands == []


def test_conexagap_stop_checks_command_result_and_configuration_is_not_ready():
    stop_ser = ScriptedSerial({"TE": ["1TE@"]})
    controller = ConexAgap(port="COM11", axis="U", ser=stop_ser)

    controller.stop()

    assert stop_ser.commands == ["1STU", "1TE"]

    blocked = ConexAgap(
        port="COM11", axis="U", ser=ScriptedSerial({"TS": ["1TS000014"]})
    )
    with pytest.raises(RuntimeError, match="not ready for motion.*state 14"):
        blocked.home()


def test_conexagap_disable_transition_retries_until_ready(monkeypatch):
    ser = ScriptedSerial(
        {
            "TS": ["1TS00003C", "1TS00003D", "1TS000034"],
            "TE": ["1TE@"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)
    sleeps: list[float] = []
    monkeypatch.setattr(conexagap_module.time, "sleep", sleeps.append)

    controller._ensure_ready(poll_interval=0.02)

    assert ser.commands == ["1TS", "1MM1", "1TE", "1TS", "1TS"]
    assert sleeps == [0.02]


def test_conexagap_absolute_move_reports_intermediate_and_final_callback_positions():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000032", "1TS000028", "1TS000033"],
            "TE": ["1TE@"],
            "TPU": ["1TPU0.1000", "1TPU0.2500"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)
    positions: list[float] = []

    assert controller.move_abs_raw(0.25, on_position=positions.append) == 0.25
    assert positions == [0.1, 0.25]


def test_conexagap_relative_move_without_callback_skips_intermediate_position_read():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000033", "1TS000033"],
            "TE": ["1TE@"],
            "TPU": ["1TPU0.2500"],
        }
    )
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    assert controller.move_rel_raw(0.25) == 0.25
    assert ser.commands == ["1TS", "1PRU0.2500", "1TE", "1TS", "1TPU"]


def test_conexagap_empty_command_error_response_is_timeout():
    ser = ScriptedSerial({"TE": [""]})
    controller = ConexAgap(port="COM11", axis="U", ser=ser)

    with pytest.raises(TimeoutError, match="timed out waiting for TE response"):
        controller.stop()

    assert ser.commands == ["1STU", "1TE"]
