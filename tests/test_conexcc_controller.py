from __future__ import annotations

import math

import pytest
import serial

import kohdalab.instruments.scanner.conexcc as conexcc_module
from kohdalab.instruments.scanner.conexcc import ConexCC


class ScriptedSerial:
    def __init__(self, responses: dict[str, list[str]]):
        self.responses = {key: list(value) for key, value in responses.items()}
        self.commands: list[str] = []
        self.is_open = True

    def reset_input_buffer(self):
        return None

    def write(self, data: bytes):
        self.commands.append(data.decode("ascii").strip())

    def flush(self):
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


def test_conexcc_move_enables_closed_loop_from_disable():
    ser = ScriptedSerial(
        {
            "TS": ["1TS00003C", "1TS000034", "1TS000028", "1TS000033", "1TS000033"],
            "SC?": ["1SC0"],
            "TP": ["1TP2.5000"],
            "TB": ["1TB"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    assert controller.move_abs_raw(2.5) == 2.5

    assert ser.commands == [
        "1TS",
        "1SC?",
        "1SC1",
        "1MM1",
        "1TS",
        "1PA2.5000",
        "1TS",
        "1TS",
        "1TS",
        "1TP",
    ]


def test_conexcc_first_ready_move_cycles_through_closed_loop_enable():
    ser = ScriptedSerial(
        {
            "TS": [
                "1TS000033",
                "1TS00003C",
                "1TS000034",
                "1TS000028",
                "1TS000033",
                "1TS000033",
            ],
            "SC?": ["1SC0"],
            "TP": ["1TP2.5000"],
            "TB": ["1TB"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    assert controller.move_abs_raw(2.5) == 2.5

    assert ser.commands == [
        "1TS",
        "1MM0",
        "1TS",
        "1SC?",
        "1SC1",
        "1MM1",
        "1TS",
        "1PA2.5000",
        "1TS",
        "1TS",
        "1TS",
        "1TP",
    ]


def test_conexcc_move_rejects_not_referenced_state():
    ser = ScriptedSerial({"TS": ["1TS00000A"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="not referenced"):
        controller.move_abs_raw(2.5)

    assert ser.commands == ["1TS"]


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
def test_conexcc_rejects_malformed_status_before_motion(response):
    ser = ScriptedSerial({"TS": [response]})
    controller = ConexCC(
        port="COM5",
        ser=ser,
        ensure_closed_loop_on_move=False,
    )

    error = TimeoutError if response == "" else RuntimeError
    with pytest.raises(error):
        controller.move_abs_raw(2.5)

    assert ser.commands == ["1TS"]


def test_conexcc_rejects_positioner_error_and_unknown_state_before_motion():
    error_ser = ScriptedSerial({"TS": ["1TS002032"], "TB": ["1TBfollowing error"]})
    controller = ConexCC(port="COM5", ser=error_ser)

    with pytest.raises(RuntimeError, match="CONEX-CC error 0020"):
        controller.move_abs_raw(2.5)
    assert error_ser.commands == ["1TS", "1TB"]

    unknown_ser = ScriptedSerial({"TS": ["1TS000099"]})
    controller = ConexCC(port="COM5", ser=unknown_ser)
    with pytest.raises(RuntimeError, match="unknown state 99"):
        controller.move_abs_raw(2.5)
    assert unknown_ser.commands == ["1TS"]


def test_conexcc_does_not_treat_disable_during_motion_as_success():
    ser = ScriptedSerial({"TS": ["1TS000028", "1TS00003C"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="unsafe state 3C"):
        controller.wait_until_stopped()


def test_conexcc_motion_timeout_uses_monotonic_clock(monkeypatch):
    ser = ScriptedSerial({"TS": ["1TS000028"]})
    controller = ConexCC(port="COM5", ser=ser)
    times = iter([10.0, 11.0])
    monkeypatch.setattr(conexcc_module.time, "monotonic", lambda: next(times))

    with pytest.raises(TimeoutError, match="motion timeout"):
        controller.wait_until_stopped(timeout=0.5, poll_interval=0.0)


@pytest.mark.parametrize("target", [math.nan, math.inf, -math.inf, True])
def test_conexcc_rejects_non_finite_target_before_io(target):
    ser = ScriptedSerial({})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(ValueError, match="target must be finite"):
        controller.move_abs_raw(target)

    assert ser.commands == []


def test_conexcc_rejects_non_finite_position_and_closed_connection():
    ser = ScriptedSerial({"TP": ["1TPnan"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="non-finite position"):
        controller.get_pos_raw()

    ser.is_open = False
    with pytest.raises(ConnectionError, match="serial connection is closed"):
        controller.get_state()


def test_conexcc_reports_disconnect_during_motion_with_command_context():
    class DroppingSerial(ScriptedSerial):
        def readline(self) -> bytes:
            if len(self.commands) == 1:
                return b"1TS000028\r\n"
            self.is_open = False
            raise serial.SerialException("device disconnected")

    controller = ConexCC(port="COM5", ser=DroppingSerial({}))

    with pytest.raises(ConnectionError, match=r"COM5.*I/O failed for TS.*disconnected"):
        controller.wait_until_stopped(poll_interval=0.0)


def test_conexcc_rejects_non_ascii_response():
    class NonAsciiSerial(ScriptedSerial):
        def readline(self) -> bytes:
            return b"\xff\r\n"

    controller = ConexCC(port="COM5", ser=NonAsciiSerial({}))

    with pytest.raises(RuntimeError, match="non-ASCII data"):
        controller.get_state()


@pytest.mark.parametrize("response", ["2TP1.25", "1TPnot-a-number"])
def test_conexcc_rejects_malformed_position_response(response):
    ser = ScriptedSerial({"TP": [response]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises((RuntimeError, ValueError)):
        controller.get_pos_raw()

    assert ser.commands == ["1TP"]


def test_conexcc_rejects_malformed_closed_loop_response_before_enable():
    ser = ScriptedSerial({"SC?": ["2SC0"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="Unexpected SC response"):
        controller._set_closed_loop()

    assert ser.commands == ["1SC?"]


def test_conexcc_does_not_reenable_closed_loop_when_already_enabled():
    ser = ScriptedSerial({"SC?": ["1SC1"]})
    controller = ConexCC(port="COM5", ser=ser)

    controller._set_closed_loop()

    assert ser.commands == ["1SC?"]


def test_conexcc_error_without_diagnostic_response_preserves_error_code():
    ser = ScriptedSerial({"TS": ["1TS002032"], "TB": [""]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match=r"CONEX-CC error 0020$"):
        controller.get_state_code()

    assert ser.commands == ["1TS", "1TB"]


def test_conexcc_wait_for_ready_rejects_not_referenced_transition():
    ser = ScriptedSerial({"TS": ["1TS00000B"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="not referenced"):
        controller._wait_for_ready(poll_interval=0.0)

    assert ser.commands == ["1TS"]


def test_conexcc_wait_for_disable_times_out(monkeypatch):
    ser = ScriptedSerial({"TS": ["1TS000033"]})
    controller = ConexCC(port="COM5", ser=ser)
    times = iter([10.0, 11.0])
    monkeypatch.setattr(conexcc_module.time, "monotonic", lambda: next(times))

    with pytest.raises(TimeoutError, match="failed to reach DISABLE state"):
        controller._wait_for_disable(timeout=0.5, poll_interval=0.0)


def test_conexcc_rejects_active_motion_as_not_ready_for_new_move():
    ser = ScriptedSerial({"TS": ["1TS000028"]})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match=r"not ready for motion \(state 28\)"):
        controller.move_abs_raw(1.0)

    assert ser.commands == ["1TS"]


def test_conexcc_homing_state_must_finish_ready_before_motion():
    ser = ScriptedSerial(
        {
            "TS": [
                "1TS00001E",
                "1TS00001E",
                "1TS000033",
                "1TS000033",
                "1TS00003C",
            ],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(RuntimeError, match="unsafe state 3C"):
        controller.move_abs_raw(1.0)

    assert ser.commands == ["1TS", "1TS", "1TS", "1TS", "1PA1.0000", "1TS"]


@pytest.mark.parametrize("target", [math.nan, math.inf, -math.inf, True])
def test_conexcc_rejects_non_finite_relative_target_before_io(target):
    ser = ScriptedSerial({})
    controller = ConexCC(port="COM5", ser=ser)

    with pytest.raises(ValueError, match="relative target must be finite"):
        controller.move_rel_raw(target)

    assert ser.commands == []


def test_conexcc_configure_updates_metadata_and_invalidates_address_preparation():
    ser = ScriptedSerial({})
    controller = ConexCC(port="COM5", ser=ser)
    controller._closed_loop_prepared = True

    controller.configure(controller_address=2, pos_unit="deg", axis="ignored")

    assert controller.controller_address == 2
    assert controller.get_pos_unit() == "deg"
    assert controller._closed_loop_prepared is False


def test_conexcc_configure_same_address_preserves_prepared_state():
    controller = ConexCC(port="COM5", ser=ScriptedSerial({}))
    controller._closed_loop_prepared = True

    controller.configure(controller_address=1)

    assert controller._closed_loop_prepared is True


def test_conexcc_close_only_closes_owned_serial_transport():
    borrowed = ScriptedSerial({})
    borrowed_controller = ConexCC(port="COM5", ser=borrowed)

    borrowed_controller.close()
    assert borrowed.is_open

    owned = ScriptedSerial({})
    owned_controller = ConexCC(port="COM5", ser=owned)
    owned_controller._owns_serial = True
    owned_controller.close()
    assert not owned.is_open
    assert not owned_controller.is_connected()


def test_conexcc_write_allows_empty_response_and_control_commands():
    ser = ScriptedSerial({"ST": [""], "OR": [""]})
    controller = ConexCC(port="COM5", ser=ser)

    controller.stop()
    controller.home()

    assert ser.commands == ["1ST", "1OR"]


def test_conexcc_status_accessors_decode_error_and_motion_state():
    ser = ScriptedSerial({"TS": ["1TS000028", "1TS000033"]})
    controller = ConexCC(port="COM5", ser=ser)

    assert controller.is_moving()
    assert controller.get_error_code() == "0000"


@pytest.mark.parametrize("failure_point", ["reset_input_buffer", "write", "flush"])
def test_conexcc_query_wraps_pre_read_transport_failures(failure_point):
    ser = ScriptedSerial({})
    controller = ConexCC(port="COM5", ser=ser)

    def fail(*_args):
        raise serial.SerialException("transport lost")

    setattr(ser, failure_point, fail)

    with pytest.raises(ConnectionError, match=r"I/O failed for TS.*transport lost"):
        controller.query("TS")


def test_conexcc_initialize_enables_closed_loop_from_disable():
    ser = ScriptedSerial(
        {
            "TS": [
                "1TS00003C",
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
            ],
            "SC?": ["1SC0"],
            "TP": ["1TP1.2500"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    info = controller.initialize(home=False)

    assert info == {
        "axis": 1,
        "state": "1TS000033",
        "moving": False,
        "error_code": "0000",
        "state_code": "33",
        "pos_raw": 1.25,
        "pos_unit": "mm",
    }
    assert ser.commands == [
        "1TS",
        "1SC?",
        "1SC1",
        "1MM1",
        "1TS",
        "1TS",
        "1TS",
        "1TS",
        "1TS",
        "1TP",
    ]


def test_conexcc_initialize_home_waits_for_homing_to_finish():
    ser = ScriptedSerial(
        {
            "TS": [
                "1TS00001E",
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
            ],
            "TP": ["1TP0.0000"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    info = controller.initialize(home=True)

    assert info["state_code"] == "33"
    assert info["moving"] is False
    assert info["pos_raw"] == 0.0
    assert ser.commands[:3] == ["1OR", "1TS", "1TS"]


@pytest.mark.parametrize(
    ("relative", "motion_command"),
    [(False, "1PA2.0000"), (True, "1PR2.0000")],
)
def test_conexcc_move_reports_intermediate_and_final_positions(
    relative: bool, motion_command: str
):
    ser = ScriptedSerial(
        {
            "TS": ["1TS000033", "1TS000028", "1TS000033", "1TS000033"],
            "TP": ["1TP1.0000", "1TP2.0000"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser, ensure_closed_loop_on_move=False)
    positions: list[float] = []
    move = controller.move_rel_raw if relative else controller.move_abs_raw

    assert move(2.0, on_position=positions.append) == 2.0

    assert positions == [1.0, 2.0]
    assert motion_command in ser.commands


def test_conexcc_wait_for_ready_retries_motion_state_and_sleeps(monkeypatch):
    ser = ScriptedSerial({"TS": ["1TS000028", "1TS000033"]})
    controller = ConexCC(port="COM5", ser=ser)
    sleeps: list[float] = []
    monkeypatch.setattr(conexcc_module.time, "sleep", sleeps.append)

    controller._wait_for_ready(timeout=1.0, poll_interval=0.125)

    assert ser.commands == ["1TS", "1TS"]
    assert sleeps == [0.125]


def test_conexcc_initialize_from_ready_state_does_not_toggle_closed_loop():
    ser = ScriptedSerial(
        {
            "TS": [
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
                "1TS000033",
            ],
            "TP": ["1TP3.5000"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser)

    info = controller.initialize(home=False)

    assert info["state_code"] == "33"
    assert info["pos_raw"] == 3.5
    assert all("SC" not in command and "MM" not in command for command in ser.commands)


def test_conexcc_relative_move_without_callback_skips_position_notifications():
    ser = ScriptedSerial(
        {
            "TS": ["1TS000033", "1TS000028", "1TS000033", "1TS000033"],
            "TP": ["1TP2.0000"],
        }
    )
    controller = ConexCC(port="COM5", ser=ser, ensure_closed_loop_on_move=False)

    assert controller.move_rel_raw(2.0) == 2.0
    assert ser.commands.count("1TP") == 1


def test_conexcc_configure_pos_unit_only_and_debug_query(capsys):
    ser = ScriptedSerial({"VE": ["1VE1.0"]})
    controller = ConexCC(port="COM5", ser=ser)
    controller._closed_loop_prepared = True

    controller.configure(pos_unit="deg")
    response = controller.debug_query("VE")

    assert controller.controller_address == 1
    assert controller._closed_loop_prepared is True
    assert controller.get_pos_unit() == "deg"
    assert response == "1VE1.0"
    assert "'VE' -> '1VE1.0'" in capsys.readouterr().out
