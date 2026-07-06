from __future__ import annotations

import pytest

from kohdalab.api.status import (
    STATUS_MOVING_DELAY_STAGE,
    STATUS_READING_LOCKIN,
    moving_axis_from_status,
    moving_axis_status,
    moving_scanner_status,
)


def test_moving_axis_status_maps_scan_axes():
    assert moving_axis_status("t") == STATUS_MOVING_DELAY_STAGE
    assert moving_axis_status("x") == moving_scanner_status("x")
    assert moving_axis_status("Y") == moving_scanner_status("y")


def test_moving_axis_from_status_parses_measurement_motion():
    assert moving_axis_from_status(STATUS_MOVING_DELAY_STAGE) == "t"
    assert moving_axis_from_status(moving_scanner_status("x")) == "x"
    assert moving_axis_from_status("moving scanner x software hysteresis") == "x"
    assert moving_axis_from_status("moving scanner z") is None
    assert moving_axis_from_status(STATUS_READING_LOCKIN) is None


def test_moving_scanner_status_rejects_non_scanner_axes():
    with pytest.raises(ValueError, match="scanner axis"):
        moving_scanner_status("t")
