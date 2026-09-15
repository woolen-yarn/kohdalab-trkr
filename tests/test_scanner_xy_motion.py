from __future__ import annotations

from dataclasses import dataclass
import math
from types import SimpleNamespace

import pytest

import kohdalab.api.session as session_module
from kohdalab.api.devices.scanner import move_scanners_xy_abs
from kohdalab.api.measurements import _move_rotated
from kohdalab.api.session import DeviceSession
from kohdalab.instruments.scanner.conexagap import ConexAgap
from kohdalab.instruments.scanner.conexcc import ConexCC
from kohdalab.interfaces.scanner import Scanner


class MotionSerial:
    def __init__(
        self,
        label: str,
        events: list[tuple[str, str]],
        responses: dict[str, list[str]],
    ) -> None:
        self.label = label
        self.events = events
        self.responses = {key: list(values) for key, values in responses.items()}
        self.is_open = True
        self.last_command = ""

    def reset_input_buffer(self) -> None:
        pass

    def write(self, data: bytes) -> None:
        self.last_command = data.decode("ascii").strip()
        self.events.append((self.label, self.last_command))

    def flush(self) -> None:
        pass

    def readline(self) -> bytes:
        responses = self.responses.get(self.last_command)
        response = responses.pop(0) if responses else self.last_command
        return f"{response}\r\n".encode("ascii")

    def close(self) -> None:
        self.is_open = False


def _hardware_scanner(controller, *, name: str) -> Scanner:
    return Scanner(
        controller=controller,
        config={
            "controller": name,
            "pos_unit": controller.pos_unit,
            "origin_pos": 0.0,
            "sample_um_per_unit": 100.0,
        },
    )


@dataclass
class PairScanner:
    axis: str
    events: list[tuple[str, str, float | None]]
    controller_name: str
    position: float = 0.0
    fail_start: bool = False
    fail_wait: bool = False
    fail_stop: bool = False

    def __post_init__(self) -> None:
        self.config = {
            "controller": self.controller_name,
            "origin_pos": 0.0,
            "sample_um_per_unit": 100.0,
        }

    def get_pos_unit(self) -> str:
        return "mm"

    def get_pos_mm(self) -> float:
        return self.position

    def validate_pos_raw(self, target: float) -> float:
        self.events.append((self.axis, "validate", target))
        return target

    def move_pos_mm(self, target: float) -> float:
        self.events.append((self.axis, "pre_move", target))
        self.position = target
        return target

    def prepare_move(self) -> None:
        self.events.append((self.axis, "prepare", None))

    def start_pos_raw(self, target: float) -> None:
        self.events.append((self.axis, "start", target))
        if self.fail_start:
            raise RuntimeError("start failed")
        self.position = target

    def wait_until_stopped(self) -> None:
        self.events.append((self.axis, "wait", None))
        if self.fail_wait:
            raise TimeoutError("motion did not stop")

    def stop(self) -> None:
        self.events.append((self.axis, "stop", None))
        if self.fail_stop:
            raise RuntimeError("stop failed")


@pytest.mark.parametrize("controller", ["CONEXAGAP", "CONEXCC"])
def test_xy_motion_launches_both_axes_before_waiting(controller: str) -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {axis: PairScanner(axis, events, controller) for axis in ("x", "y")}

    rows = move_scanners_xy_abs(
        scanners=scanners,  # type: ignore[arg-type]
        targets_um={"x": 100.0, "y": 200.0},
    )

    motion = [
        (axis, action)
        for axis, action, _ in events
        if action in {"prepare", "start", "wait"}
    ]
    assert motion == [
        ("x", "prepare"),
        ("y", "prepare"),
        ("x", "start"),
        ("y", "start"),
        ("x", "wait"),
        ("y", "wait"),
    ]
    assert rows["x"]["x_um"] == 100.0
    assert rows["y"]["y_um"] == 200.0


def test_cc_hysteresis_pre_moves_finish_before_xy_launch() -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {axis: PairScanner(axis, events, "CONEXCC") for axis in ("x", "y")}
    for scanner in scanners.values():
        scanner.config["software_hysteresis"] = {
            "enabled": True,
            "distance_um": 10.0,
            "direction": "negative",
        }

    statuses: list[str] = []
    move_scanners_xy_abs(
        scanners=scanners,  # type: ignore[arg-type]
        targets_um={"x": 100.0, "y": 200.0},
        on_status=statuses.append,
    )

    motion = [
        (axis, action, target)
        for axis, action, target in events
        if action in {"pre_move", "start"}
    ]
    assert motion == [
        ("x", "pre_move", 0.9),
        ("y", "pre_move", 1.9),
        ("x", "start", 1.0),
        ("y", "start", 2.0),
    ]
    assert statuses == [
        "moving scanner x software hysteresis",
        "moving scanner y software hysteresis",
        "moving scanner x",
        "moving scanner y",
    ]


def test_cc_hysteresis_pre_moves_without_status_callback() -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {axis: PairScanner(axis, events, "CONEXCC") for axis in ("x", "y")}
    for scanner in scanners.values():
        scanner.config["software_hysteresis"] = {
            "enabled": True,
            "distance_um": 10.0,
            "direction": "negative",
        }

    rows = move_scanners_xy_abs(
        scanners=scanners,  # type: ignore[arg-type]
        targets_um={"x": 100.0, "y": 200.0},
    )

    assert [(axis, action) for axis, action, _ in events if action == "pre_move"] == [
        ("x", "pre_move"),
        ("y", "pre_move"),
    ]
    assert rows["x"]["x_um"] == 100.0
    assert rows["y"]["y_um"] == 200.0


def test_xy_motion_stops_first_axis_if_second_start_fails() -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {
        "x": PairScanner("x", events, "CONEXCC"),
        "y": PairScanner("y", events, "CONEXCC", fail_start=True),
    }

    with pytest.raises(RuntimeError, match="start failed"):
        move_scanners_xy_abs(
            scanners=scanners,  # type: ignore[arg-type]
            targets_um={"x": 100.0, "y": 200.0},
        )

    assert ("x", "stop", None) in events
    assert ("y", "stop", None) in events
    assert not any(action == "wait" for _, action, _ in events)


def test_xy_motion_stops_both_axes_on_wait_timeout_and_notes_stop_failure() -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {
        "x": PairScanner("x", events, "CONEXCC", fail_stop=True),
        "y": PairScanner("y", events, "CONEXCC", fail_wait=True),
    }

    with pytest.raises(TimeoutError, match="motion did not stop") as raised:
        move_scanners_xy_abs(
            scanners=scanners,  # type: ignore[arg-type]
            targets_um={"x": 100.0, "y": 200.0},
        )

    assert ("x", "stop", None) in events
    assert ("y", "stop", None) in events
    assert any("Could not stop scanner x" in note for note in raised.value.__notes__)


def test_xy_motion_can_skip_cc_pre_approach() -> None:
    events: list[tuple[str, str, float | None]] = []
    scanners = {axis: PairScanner(axis, events, "CONEXCC") for axis in ("x", "y")}
    for scanner in scanners.values():
        scanner.config["software_hysteresis"] = {
            "enabled": True,
            "distance_um": 10.0,
        }

    move_scanners_xy_abs(
        scanners=scanners,  # type: ignore[arg-type]
        targets_um={"x": 100.0, "y": 200.0},
        apply_software_hysteresis=False,
    )

    assert not any(action == "pre_move" for _, action, _ in events)


@pytest.mark.parametrize("target", [True, math.nan, math.inf, -math.inf])
def test_split_motion_rejects_nonfinite_targets_before_serial_io(target) -> None:
    events: list[tuple[str, str]] = []
    ser = MotionSerial("controller", events, {})
    controllers = (
        ConexAgap(port="COM11", axis="U", ser=ser),
        ConexCC(port="COM5", ser=ser),
    )
    for controller in controllers:
        with pytest.raises(ValueError, match="must be finite"):
            controller.start_abs_raw(target)
        scanner = _hardware_scanner(controller, name=type(controller).__name__.upper())
        with pytest.raises(ValueError, match="must be finite"):
            scanner.validate_pos_raw(target)
    assert events == []


class CountingLock:
    def __init__(self) -> None:
        self.entered = 0
        self.exited = 0

    def __enter__(self):
        self.entered += 1
        return self

    def __exit__(self, *_args) -> None:
        self.exited += 1


def _session_config() -> dict:
    return {
        "instruments": {
            "scanner": {
                "x": {"controller": "CONEXCC", "port": "COM5", "axis": 1},
                "y": {"controller": "CONEXCC", "port": "COM6", "axis": 1},
            }
        }
    }


def _session_scanners(*, lock: CountingLock | None = None):
    events: list[tuple[str, str, float | None]] = []
    scanners = {axis: PairScanner(axis, events, "CONEXCC") for axis in ("x", "y")}
    for axis in ("x", "y"):
        scanners[axis].controller = SimpleNamespace(axis=axis)
        if lock is not None:
            scanners[axis]._io_lock = lock
    return scanners, events


def test_session_xy_motion_deduplicates_shared_serial_lock() -> None:
    shared_lock = CountingLock()
    scanners, events = _session_scanners(lock=shared_lock)
    session = DeviceSession(_session_config(), auto_connect=False)
    session.scanners.update(scanners)

    position = session.move_scanners_xy(100.0, 200.0)

    assert (position.x_um, position.y_um) == (100.0, 200.0)
    assert shared_lock.entered == shared_lock.exited == 1
    assert events.index(("y", "start", 2.0)) < events.index(("x", "wait", None))


def test_session_xy_motion_autoconnects_both_axes_and_requires_connections(
    monkeypatch,
) -> None:
    scanners, events = _session_scanners()
    monkeypatch.setattr(
        session_module,
        "connect_scanner",
        lambda config: scanners["x"] if config["port"] == "COM5" else scanners["y"],
    )
    monkeypatch.setattr(session_module, "disconnect_scanner", lambda _config: None)
    session = DeviceSession(_session_config(), auto_connect=True)

    position = session.move_scanners_xy(100.0, 200.0)

    assert (position.x_um, position.y_um) == (100.0, 200.0)
    assert set(session.scanners) == {"x", "y"}
    assert len([event for event in events if event[1] == "start"]) == 2
    session.close()

    disconnected = DeviceSession(_session_config(), auto_connect=False)
    disconnected.scanners["x"] = scanners["x"]
    with pytest.raises(RuntimeError, match="Device not connected: scanner.y"):
        disconnected.move_scanners_xy(100.0, 200.0)


def test_session_xy_motion_rejects_duplicate_controller_axis() -> None:
    scanners, _events = _session_scanners()
    scanners["y"].controller = scanners["x"].controller
    session = DeviceSession(_session_config(), auto_connect=False)
    session.scanners.update(scanners)

    with pytest.raises(ValueError, match="distinct controller axes"):
        session.move_scanners_xy(100.0, 200.0)


def test_session_xy_motion_rejects_cc_same_serial_address_but_allows_distinct() -> None:
    scanners, _events = _session_scanners()
    serial = MotionSerial("CC", [], {})
    scanners["x"].controller = ConexCC(port="COM5", ser=serial)
    scanners["y"].controller = ConexCC(port="COM5", ser=serial)
    session = DeviceSession(_session_config(), auto_connect=False)
    session.scanners.update(scanners)

    with pytest.raises(ValueError, match="distinct controller axes"):
        session.move_scanners_xy(100.0, 200.0)

    scanners["y"].controller.controller_address = 2
    position = session.move_scanners_xy(100.0, 200.0)
    assert (position.x_um, position.y_um) == (100.0, 200.0)


def test_rotated_scan_uses_xy_motion() -> None:
    calls: list[tuple[float, float, bool]] = []

    class Session:
        def move_scanners_xy(
            self,
            x_um: float,
            y_um: float,
            *,
            apply_software_hysteresis: bool,
            on_status=None,
        ) -> None:
            calls.append((x_um, y_um, apply_software_hysteresis))

        def move_scanner(self, *args, **kwargs) -> None:
            raise AssertionError("sequential axis move was used")

    _move_rotated(
        Session(),  # type: ignore[arg-type]
        targets={"x": 12.0, "y": -3.0},
        on_status=None,
        apply_hysteresis=False,
    )
    assert calls == [(12.0, -3.0, False)]


def test_agap_sends_both_pa_commands_before_motion_polling() -> None:
    events: list[tuple[str, str]] = []
    ser = MotionSerial(
        "AGAP",
        events,
        {
            "1TPU": ["1TPU0.0000", "1TPU1.0000"],
            "1TPV": ["1TPV0.0000", "1TPV2.0000"],
            "1TS": ["1TS000032", "1TS000032", "1TS000028", "1TS000032", "1TS000032"],
            "1TE": ["1TE@", "1TE@"],
        },
    )
    scanners = {
        "x": _hardware_scanner(
            ConexAgap(port="COM11", axis="U", pos_unit="deg", ser=ser),
            name="CONEXAGAP",
        ),
        "y": _hardware_scanner(
            ConexAgap(port="COM11", axis="V", pos_unit="deg", ser=ser),
            name="CONEXAGAP",
        ),
    }

    move_scanners_xy_abs(scanners=scanners, targets_um={"x": 100.0, "y": 200.0})

    commands = [command for _, command in events]
    assert commands.index("1PAU1.0000") < commands.index("1PAV2.0000")
    assert commands.index("1PAV2.0000") < commands.index(
        "1TS", commands.index("1PAV2.0000")
    )


def test_cc_sends_both_pa_commands_before_motion_polling() -> None:
    events: list[tuple[str, str]] = []
    scanners = {}
    for axis, port, target in (("x", "COM5", 1.0), ("y", "COM6", 2.0)):
        ser = MotionSerial(
            axis,
            events,
            {
                "1TP": ["1TP0.0000", f"1TP{target:.4f}"],
                "1TS": ["1TS000032", "1TS000028", "1TS000032"],
            },
        )
        scanners[axis] = _hardware_scanner(
            ConexCC(
                port=port,
                pos_unit="mm",
                ser=ser,
                ensure_closed_loop_on_move=False,
            ),
            name="CONEXCC",
        )

    move_scanners_xy_abs(scanners=scanners, targets_um={"x": 100.0, "y": 200.0})

    start_x = events.index(("x", "1PA1.0000"))
    start_y = events.index(("y", "1PA2.0000"))
    assert start_x < start_y
    assert ("x", "1TS") not in events[start_x + 1 : start_y]
    assert events[start_y + 1] == ("x", "1TS")
