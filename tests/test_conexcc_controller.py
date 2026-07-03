from __future__ import annotations

import pytest

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

    assert ser.commands == ["1TS", "1SC?", "1SC1", "1MM1", "1TS", "1PA2.5000", "1TS", "1TS", "1TS", "1TP"]


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
