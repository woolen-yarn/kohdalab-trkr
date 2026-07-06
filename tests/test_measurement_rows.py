from __future__ import annotations

from dataclasses import dataclass

from kohdalab.api import measurements
from kohdalab.api.models import Position
from kohdalab.api.scan_plan import signal_monitor_plan, srkr_2d_plan, srkr_plan, strkr_plan, trkr_plan
from kohdalab.api.status import STATUS_MOVING_DELAY_STAGE, moving_scanner_status


SIGNAL = {"X": 1.0, "Y": 2.0, "R": 3.0, "Theta": 4.0}


@dataclass
class FakeSession:
    config: dict

    def __post_init__(self):
        self.moves: list[tuple[str, float, str]] = []
        self.scanner_hysteresis_flags: list[bool] = []
        self.disconnected = False
        self.position = Position(
            t_ps=0.0,
            delay_stage_mm=1.0,
            delay_stage_pulse=100,
            x_um=0.0,
            y_um=0.0,
            scanner_x_value=6.0,
            scanner_x_unit="mm",
            scanner_y_value=7.0,
            scanner_y_unit="mm",
        )

    def read_lockin_signal(self):
        return SIGNAL

    def move_delay_stage(self, value: float, *, coordinate: str = "measurement", on_status=None):
        if on_status is not None:
            on_status(STATUS_MOVING_DELAY_STAGE)
        self.moves.append(("t", value, coordinate))
        self.position.t_ps = value
        self.position.delay_stage_mm = value / 10.0
        self.position.delay_stage_pulse = int(value * 100)
        return self.position

    def move_scanner(
        self,
        axis: str,
        value: float,
        *,
        coordinate: str = "measurement",
        apply_software_hysteresis: bool = True,
        on_status=None,
    ):
        if on_status is not None:
            on_status(moving_scanner_status(axis))
        self.moves.append((axis, value, coordinate))
        self.scanner_hysteresis_flags.append(bool(apply_software_hysteresis))
        if axis == "x":
            self.position.x_um = value
            self.position.scanner_x_value = value / 100.0
        else:
            self.position.y_um = value
            self.position.scanner_y_value = value / 100.0
        return self.position

    def read_position(self):
        return self.position

    def disconnect_all(self):
        self.disconnected = True


def base_config(tmp_path):
    return {
        "measurements": {
            "move_abs": {
                "zero": {
                    "t_ps": 10.0,
                    "x_um": 1.0,
                    "y_um": 2.0,
                }
            },
            "signal_monitor": {
                "output": {"dir": str(tmp_path), "filename": "signal", "auto_timestamp_suffix": False}
            },
            "trkr": {
                "coordinate": "measurement",
                "wait_s": 0.0,
                "return_to_zero": True,
                "output": {"dir": str(tmp_path), "filename": "trkr", "auto_timestamp_suffix": False},
            },
            "srkr": {
                "coordinate": "measurement",
                "wait_s": 0.0,
                "return_to_zero": True,
                "output": {"dir": str(tmp_path), "filename": "srkr", "auto_timestamp_suffix": False},
                "scan": {"axis": "x", "min": 0.0, "max": 1.0, "step": 1.0},
            },
            "strkr": {
                "wait_s": 0.0,
                "return_to_zero": {"fast_axis": True, "slow_axis": True},
                "output": {"dir": str(tmp_path), "filename": "strkr", "auto_timestamp_suffix": False},
            },
            "srkr_2d": {
                "wait_s": 0.0,
                "return_to_zero": {"fast_axis": True, "slow_axis": True},
                "output": {"dir": str(tmp_path), "filename": "srkr_2d", "auto_timestamp_suffix": False},
            },
        }
    }


def test_signal_monitor_row_shape(monkeypatch, tmp_path):
    monkeypatch.setattr(measurements, "DeviceSession", FakeSession)

    rows = measurements.run_signal_monitor(base_config(tmp_path), interval_s=0.0, n_points=1)

    assert len(rows) == 1
    assert {"timestamp", "measurement", "fast_axis", "target_elapsed_s", "elapsed_s", "X_V", "Y_V", "R_V", "Theta_deg"} <= rows[0].keys()
    assert "actual_elapsed_s" not in rows[0]
    assert rows[0]["measurement"] == "signal_monitor"
    assert rows[0]["fast_axis"] == "elapsed_s"
    assert rows[0]["target_elapsed_s"] == rows[0]["elapsed_s"]
    assert rows[0]["X_V"] == 1.0


def test_signal_monitor_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_signal_monitor(
        base_config(tmp_path),
        interval_s=0.0,
        n_points=1,
        session=session,
    )

    assert len(rows) == 1
    assert not session.disconnected


def test_signal_monitor_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = signal_monitor_plan(interval_s=0.0, n_points=2)

    rows = measurements.run_signal_monitor(base_config(tmp_path), plan=plan, session=session)

    assert len(rows) == 2
    assert [row["target_elapsed_s"] for row in rows] == [0.0, 0.0]
    assert not session.disconnected


def test_trkr_row_shape_and_return_to_zero(monkeypatch, tmp_path):
    sessions: list[FakeSession] = []

    def make_session(config):
        session = FakeSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    rows = measurements.run_trkr(base_config(tmp_path), scan_points=[10.0, 20.0], wait_s=0.0)

    assert [row["target_t_cor_ps"] for row in rows] == [10.0, 20.0]
    assert rows[1]["t_cor_ps"] == 10.0
    assert {"fast_axis", "delay_stage_mm", "delay_stage_pulse", "X_V", "Theta_deg"} <= rows[0].keys()
    assert rows[0]["measurement"] == "trkr"
    assert rows[0]["fast_axis"] == "t"
    assert rows[0]["elapsed_s"] is None
    assert sessions[0].moves == [
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
    ]
    assert sessions[0].disconnected


def test_trkr_target_points_are_corrected_display_values(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[-40.0, -30.0],
        target_points=[-50.0, -40.0],
        wait_s=0.0,
        session=session,
    )

    assert [row["target_t_cor_ps"] for row in rows] == [-50.0, -40.0]
    assert [move[1] for move in session.moves[:2]] == [-40.0, -30.0]


def test_trkr_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = trkr_plan(
        minimum_ps=-50.0,
        maximum_ps=-40.0,
        step_ps=10.0,
        t_zero_ps=10.0,
        coordinate="measurement",
    )

    rows = measurements.run_trkr(base_config(tmp_path), plan=plan, wait_s=0.0, session=session)

    assert [row["target_t_cor_ps"] for row in rows] == [-50.0, -40.0]
    assert [move[1] for move in session.moves[:2]] == [-40.0, -30.0]


def test_trkr_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(base_config(tmp_path), scan_points=[10.0], wait_s=0.0, session=session)

    assert rows[0]["target_t_cor_ps"] == 10.0
    assert session.moves[-1] == ("t", 10.0, "measurement")
    assert not session.disconnected


def test_trkr_normalizes_coordinate_aliases(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_trkr(
        base_config(tmp_path),
        scan_points=[1.0],
        coordinate="control",
        wait_s=0.0,
        return_to_zero=False,
        session=session,
    )

    assert rows[0]["coordinate"] == "interface"
    assert session.moves == [("t", 1.0, "interface")]


def test_srkr_row_shape_and_return_to_zero(monkeypatch, tmp_path):
    sessions: list[FakeSession] = []

    def make_session(config):
        session = FakeSession(config)
        sessions.append(session)
        return session

    monkeypatch.setattr(measurements, "DeviceSession", make_session)

    rows = measurements.run_srkr(base_config(tmp_path), axis="x", scan_points=[1.0, 2.0], wait_s=0.0)

    assert [row["target_x_cor_um"] for row in rows] == [1.0, 2.0]
    assert rows[0]["x_cor_um"] == 0.0
    assert {"fast_axis", "x_um", "x_cor_um", "x_scanner_mm", "X_V", "Theta_deg"} <= rows[0].keys()
    assert rows[0]["fast_axis"] == "x"
    assert rows[0]["y_um"] is None
    assert rows[0]["y_cor_um"] is None
    assert rows[0]["y_scanner_mm"] is None
    assert rows[0]["elapsed_s"] is None
    assert sessions[0].moves == [
        ("x", 1.0, "measurement"),
        ("x", 2.0, "measurement"),
        ("x", 1.0, "measurement"),
    ]
    assert sessions[0].scanner_hysteresis_flags == [True, False, True]
    assert sessions[0].disconnected


def test_srkr_target_points_are_corrected_display_values(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path),
        axis="x",
        scan_points=[51.0],
        target_points=[50.0],
        wait_s=0.0,
        session=session,
    )

    assert rows[0]["target_x_cor_um"] == 50.0
    assert session.moves[0] == ("x", 51.0, "measurement")


def test_srkr_accepts_scan_plan(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = srkr_plan(
        axis="x",
        minimum_um=50.0,
        maximum_um=60.0,
        step_um=10.0,
        zero_by_axis={"x": 1.0, "y": 2.0},
        coordinate="measurement",
    )

    rows = measurements.run_srkr(base_config(tmp_path), plan=plan, wait_s=0.0, session=session)

    assert [row["target_x_cor_um"] for row in rows] == [50.0, 60.0]
    assert [move[1] for move in session.moves[:2]] == [51.0, 61.0]
    assert session.scanner_hysteresis_flags[:2] == [True, False]


def test_srkr_uses_provided_session_without_disconnect(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(base_config(tmp_path), axis="x", scan_points=[1.0], wait_s=0.0, session=session)

    assert rows[0]["target_x_cor_um"] == 1.0
    assert session.moves[-1] == ("x", 1.0, "measurement")
    assert not session.disconnected


def test_srkr_normalizes_coordinate_aliases(tmp_path):
    session = FakeSession(base_config(tmp_path))

    rows = measurements.run_srkr(
        base_config(tmp_path),
        axis="x",
        scan_points=[1.0],
        coordinate="device",
        wait_s=0.0,
        return_to_zero=False,
        session=session,
    )

    assert rows[0]["coordinate"] == "interface"
    assert session.moves == [("x", 1.0, "interface")]


def test_strkr_runs_fast_slow_scan_and_returns_moved_axes(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = strkr_plan(
        fast_axis="t",
        slow_axis="x",
        ranges={
            "t": {"min": 0.0, "max": 10.0, "step": 10.0},
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 0.0, "max": 1.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": True, "slow_axis": True},
    )

    rows = measurements.run_strkr(base_config(tmp_path), plan=plan, wait_s=0.0, session=session)

    assert len(rows) == 4
    assert rows[0]["measurement"] == "strkr"
    assert rows[0]["fast_axis"] == "t"
    assert rows[0]["slow_axis"] == "x"
    assert [row["target_t_cor_ps"] for row in rows] == [0.0, 10.0, 0.0, 10.0]
    assert [row["target_x_cor_um"] for row in rows] == [0.0, 0.0, 1.0, 1.0]
    assert session.moves == [
        ("x", 1.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("x", 2.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("t", 20.0, "measurement"),
        ("t", 10.0, "measurement"),
        ("x", 1.0, "measurement"),
    ]
    assert session.scanner_hysteresis_flags == [True, False, True]


def test_srkr_2d_runs_xy_scan_without_touching_t(tmp_path):
    session = FakeSession(base_config(tmp_path))
    plan = srkr_2d_plan(
        fast_axis="x",
        slow_axis="y",
        ranges={
            "x": {"min": 0.0, "max": 1.0, "step": 1.0},
            "y": {"min": 0.0, "max": 1.0, "step": 1.0},
        },
        zero_by_axis={"t_ps": 10.0, "x_um": 1.0, "y_um": 2.0},
        return_to_zero={"fast_axis": False, "slow_axis": True},
    )

    rows = measurements.run_srkr_2d(base_config(tmp_path), plan=plan, wait_s=0.0, session=session)

    assert len(rows) == 4
    assert rows[0]["measurement"] == "srkr_2d"
    assert rows[0]["fast_axis"] == "x"
    assert rows[0]["slow_axis"] == "y"
    assert [move[0] for move in session.moves] == ["y", "x", "x", "x", "y", "x", "x", "y"]
    assert session.scanner_hysteresis_flags == [True, True, False, False, False, False, False, True]
    assert ("t", 10.0, "measurement") not in session.moves
