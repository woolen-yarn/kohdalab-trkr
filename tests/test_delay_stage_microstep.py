from __future__ import annotations

import pytest

from kohdalab.instruments.delay_stage.shot302 import Shot302GS


@pytest.mark.parametrize(
    ("response", "expected"),
    [
        ("S1=4", 4),
        ("S2 20", 20),
        ("4", 4),
    ],
)
def test_shot302_microstep_parser_uses_returned_division(response: str, expected: int):
    controller = Shot302GS.__new__(Shot302GS)
    controller.default_axis = 1
    controller.axis_count = 1
    controller.query_internal = lambda code: response

    assert controller.get_microstep_division(axis=1) == expected


def test_shot302_microstep_parser_retries_empty_response():
    controller = Shot302GS.__new__(Shot302GS)
    controller.default_axis = 1
    controller.axis_count = 1
    responses = iter(["", "S1=20"])
    controller.query_internal = lambda code: next(responses)

    assert controller.get_microstep_division(axis=1) == 20
