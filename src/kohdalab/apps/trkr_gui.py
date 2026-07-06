from __future__ import annotations

import csv
import sys
import time
from copy import deepcopy
from pathlib import Path
from typing import Any

import numpy as np
from PySide6 import QtCore, QtWidgets
import pyqtgraph as pg
from serial.tools import list_ports

from kohdalab import __version__
from kohdalab.api import Experiment, MeasurementPoint, load_config
from kohdalab.api.config import (
    DEFAULT_CONFIG_PATH,
    normalize_config,
    normalize_delay_stage_name,
    resolve_config_path,
    save_config,
    write_last_config_path,
)
from kohdalab.api.devices.delay_stage import list_stages
from kohdalab.api.measurement_rows import axis_target_key, fields_for_row, fields_for_rows, output_rows
from kohdalab.api.scan_limits import delay_stage_scan_limits, scanner_scan_limits
from kohdalab.api.scan_plan import (
    Scan2DPlan,
    SrkrPlan,
    TrkrPlan,
    signal_monitor_plan,
    srkr_2d_plan,
    srkr_plan,
    strkr_plan,
    trkr_plan,
)
from kohdalab.api.status import (
    STATUS_READING_LOCKIN,
    STATUS_RUNNING,
    STATUS_SLOW_AXIS_READY,
    STATUS_STOPPED,
    STATUS_WAITING,
    moving_axis_from_status,
)
from kohdalab.apps.trkr_gui_output import build_output_path, output_settings_from_fields
from kohdalab.apps.trkr_gui_plot import scan2d_uses_equal_spatial_units
from kohdalab.apps.trkr_gui_coordinates import scanner_axis_spin_value, scanner_scale_label_for_actuator
from kohdalab.apps.trkr_gui_signal import (
    lockin_display_from_settings,
    overload_display_from_status,
    signal_view_config,
    time_constant_display,
)
from kohdalab.apps.trkr_gui_snapshot import format_snapshot_value
from kohdalab.interfaces.lockin import list_visa_resources
from kohdalab.interfaces.scanner import ACTUATORS, ACTUATOR_NAMES
from kohdalab.instruments.delay_stage import DELAY_STAGE_CONTROLLERS
from kohdalab.instruments.scanner import SCANNER_CONTROLLERS


LOCKIN_MODELS = ["SR7265", "SR830", "LI5640", "SR5210"]
MEASUREMENT_ROW_TRAILING_WIDTH = 116
MOVE_COMMAND_COOLDOWN_S = 0.35
RANGE_KEYS = ("min", "max", "step")
SCAN2D_ROLES = ("fast_axis", "slow_axis")
OUTPUT_TRAILING_WIDTH = 104
RDBU_R_LUT = pg.ColorMap(
    np.array([0.0, 0.25, 0.5, 0.75, 1.0]),
    np.array(
        [
            [33, 102, 172, 255],
            [103, 169, 207, 255],
            [247, 247, 247, 255],
            [239, 138, 98, 255],
            [178, 24, 43, 255],
        ],
        dtype=np.ubyte,
    ),
).getLookupTable(0.0, 1.0, 256)


def _format_value(value: float | None, decimals: int = 3) -> str:
    return "-" if value is None else f"{float(value):.{decimals}f}"


def _motion_axis_display_text(status: str) -> str:
    return "BH..." if "software hysteresis" in status.strip().lower() else "Moving..."


def _fmt_bound(value: float | None, unit: str) -> str:
    return "-" if value is None else f"{float(value):.6g} {unit}"


def _axis_cor_key(axis: str) -> str:
    axis = axis.strip().lower()
    return "t_cor_ps" if axis == "t" else f"{axis}_cor_um"


def _axis_raw_key(axis: str) -> str:
    axis = axis.strip().lower()
    return "t_ps" if axis == "t" else f"{axis}_um"


def _axis_unit(axis: str) -> str:
    return "ps" if axis.strip().lower() == "t" else "um"


def _default_axis_range(axis: str) -> tuple[float, float, float]:
    return (-50.0, 300.0, 5.0) if axis.strip().lower() == "t" else (-30.0, 30.0, 1.0)


def _unique_values(values) -> list[float]:
    unique: list[float] = []
    for value in values:
        if value is None:
            continue
        number = float(value)
        if number not in unique:
            unique.append(number)
    return unique


def _normalized_by_abs_max(image: np.ndarray) -> np.ndarray:
    finite = image[np.isfinite(image)]
    if finite.size == 0:
        return image
    max_abs = float(np.max(np.abs(finite)))
    if max_abs <= 0.0:
        return image
    return image / max_abs


def _format_duration(seconds: float | None) -> str:
    if seconds is None:
        return "-"
    total = max(0, int(round(float(seconds))))
    hours, remainder = divmod(total, 3600)
    minutes, seconds = divmod(remainder, 60)
    if hours:
        return f"{hours:d}:{minutes:02d}:{seconds:02d}"
    if minutes:
        return f"{minutes:d}:{seconds:02d}"
    return f"{seconds:d}s"


def _set_combo_text(combo: QtWidgets.QComboBox, text: str) -> None:
    index = combo.findText(text)
    if index < 0 and text:
        combo.insertItem(0, text)
        index = 0
    if index >= 0:
        combo.setCurrentIndex(index)


def _replace_combo_items(combo: QtWidgets.QComboBox, items: list[str], current: str | None = None, *, allow_custom: bool = True) -> None:
    current_text = current if current is not None else combo.currentText()
    combo.blockSignals(True)
    combo.clear()
    combo.addItems(items)
    if allow_custom or current_text in items:
        _set_combo_text(combo, current_text)
    elif items:
        combo.setCurrentIndex(0)
    combo.blockSignals(False)


def _output_settings(output_dir: QtWidgets.QLineEdit, filename: QtWidgets.QLineEdit, auto_suffix: QtWidgets.QCheckBox) -> dict[str, Any]:
    return output_settings_from_fields(
        output_dir=output_dir.text(),
        filename=filename.text(),
        auto_timestamp_suffix=auto_suffix.isChecked(),
        default_dir=Path.cwd(),
    )


def _axis_ticks(positions: list[float], labels: list[float], *, max_ticks: int = 8) -> list[tuple[float, str]]:
    if not positions:
        return []
    if len(positions) <= max_ticks:
        indexes = list(range(len(positions)))
    else:
        step = max(1, (len(positions) - 1) // (max_ticks - 1))
        indexes = list(range(0, len(positions), step))
        if indexes[-1] != len(positions) - 1:
            indexes.append(len(positions) - 1)
    return [(positions[index], f"{labels[index]:.3g}") for index in indexes]


def _valid_scan2d_axes(mode: str, fast_axis: str, slow_axis: str) -> tuple[str, str]:
    fast_axis = str(fast_axis or "").strip().lower()
    slow_axis = str(slow_axis or "").strip().lower()
    allowed = {("t", "x"), ("t", "y"), ("x", "t"), ("y", "t")} if mode == "strkr" else {("x", "y"), ("y", "x")}
    if (fast_axis, slow_axis) in allowed:
        return fast_axis, slow_axis
    if mode == "strkr":
        if fast_axis == "t":
            return "t", "x"
        if fast_axis in {"x", "y"}:
            return fast_axis, "t"
        if slow_axis == "t":
            return "x", "t"
        if slow_axis in {"x", "y"}:
            return "t", slow_axis
    else:
        if fast_axis == "x":
            return "x", "y"
        if fast_axis == "y":
            return "y", "x"
        if slow_axis == "x":
            return "y", "x"
        if slow_axis == "y":
            return "x", "y"
    return ("t", "x") if mode == "strkr" else ("x", "y")


class MeasurementWorker(QtCore.QObject):
    point_ready = QtCore.Signal(object)
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        experiment: Experiment,
        measurement: str,
        output_path: str,
        scan_plan: TrkrPlan | SrkrPlan | Scan2DPlan | None = None,
        axis: str | None = None,
        interval_s: float | None = None,
        n_points: int | None = None,
        wait_s: float | None = None,
        return_to_zero: bool | None = None,
    ):
        super().__init__()
        self.experiment = experiment
        self.measurement = measurement
        self.output_path = output_path
        self.scan_plan = scan_plan
        self.axis = axis
        self.interval_s = interval_s
        self.n_points = n_points
        self.wait_s = wait_s
        self.return_to_zero = return_to_zero
        self._running = True

    def stop(self):
        self._running = False

    def _should_continue(self) -> bool:
        return self._running

    def run(self):
        try:
            kwargs = {
                "output": self.output_path,
                "on_point": self.point_ready.emit,
                "on_status": self.status_changed.emit,
                "should_continue": self._should_continue,
            }
            if self.measurement == "signal_monitor":
                rows = self.experiment.run_signal_monitor(
                    interval_s=self.interval_s,
                    n_points=self.n_points,
                    **kwargs,
                )
            elif self.measurement == "trkr":
                rows = self.experiment.run_trkr(
                    plan=self.scan_plan,
                    wait_s=self.wait_s,
                    return_to_zero=self.return_to_zero,
                    **kwargs,
                )
            elif self.measurement == "srkr":
                rows = self.experiment.run_srkr(
                    axis=self.axis,
                    plan=self.scan_plan,
                    wait_s=self.wait_s,
                    return_to_zero=self.return_to_zero,
                    **kwargs,
                )
            elif self.measurement == "strkr":
                rows = self.experiment.run_strkr(
                    plan=self.scan_plan,
                    wait_s=self.wait_s,
                    **kwargs,
                )
            elif self.measurement == "srkr_2d":
                rows = self.experiment.run_srkr_2d(
                    plan=self.scan_plan,
                    wait_s=self.wait_s,
                    **kwargs,
                )
            else:
                raise ValueError(f"Unsupported measurement: {self.measurement}")
            self.finished.emit(rows)
        except Exception as e:
            self.error_occurred.emit(str(e))
            self.finished.emit([])


class DeviceCommandWorker(QtCore.QObject):
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        experiment: Experiment,
        command: str | None = None,
        kind: str | None = None,
        key: str | None = None,
        axis: str | None = None,
        ref: str | None = None,
        multiplier: float = 4.0,
        settings: dict[str, Any] | None = None,
    ):
        super().__init__()
        self.experiment = experiment
        self.command = command
        self.kind = kind
        self.key = key
        self.axis = axis
        self.ref = ref
        self.multiplier = multiplier
        self.settings = settings or {}

    def _request(self) -> dict[str, Any]:
        return {
            "command": self.command,
            "kind": self.kind,
            "key": self.key,
            "axis": self.axis,
            "ref": self.ref,
            "multiplier": self.multiplier,
            "settings": dict(self.settings),
        }

    def _device_ref(self, request: dict[str, Any]) -> str:
        if request.get("ref"):
            return str(request["ref"])
        if not request.get("kind") or not request.get("key"):
            raise ValueError("Device command requires kind/key or ref.")
        return f"{request['kind']}.{request['key']}"

    def _execute(self, request: dict[str, Any]) -> dict[str, Any]:
        command = str(request.get("command") or "")
        if command == "connect_all":
            self.status_changed.emit("connecting all")
            self.experiment.connect_all()
            return {"command": command}
        if command == "connect_device":
            ref = self._device_ref(request)
            self.status_changed.emit(f"connecting {ref}")
            self.experiment.connect_device(ref)
            return {"command": command, "ref": ref}
        if command in {"disconnect_all", "shutdown_disconnect_all"}:
            self.status_changed.emit("disconnecting all")
            self.experiment.disconnect_all()
            return {"command": command}
        if command == "disconnect_device":
            ref = self._device_ref(request)
            self.status_changed.emit(f"disconnecting {ref}")
            self.experiment.disconnect_device(ref)
            return {"command": command, "ref": ref}
        if command == "initialize_delay_stage":
            self.status_changed.emit("delay stage initializing")
            info = self.experiment.initialize_delay_stage("delay_stage.t", on_status=self.status_changed.emit)
            return {"command": command, "kind": "delay_stage", "axis": "t", "info": info}
        if command == "initialize_scanner" and request.get("axis") in {"x", "y"}:
            axis = str(request["axis"])
            self.status_changed.emit(f"scanner {axis} initializing")
            info = self.experiment.initialize_scanner(
                axis,
                f"scanner.{axis}",
                on_status=self.status_changed.emit,
            )
            return {"command": command, "kind": "scanner", "axis": axis, "info": info}
        if command == "lockin_wait_time":
            ref = str(request.get("ref") or "lockin.main")
            multiplier = float(request.get("multiplier", 4.0))
            self.status_changed.emit("reading lock-in wait time")
            try:
                wait_s = self.experiment.lockin_wait_time(ref, multiplier=multiplier)
            except Exception as first_error:
                if "Invalid session handle" not in str(first_error):
                    raise
                self.status_changed.emit("reconnecting lock-in")
                self.experiment.disconnect_device(ref)
                self.experiment.connect_device(ref)
                wait_s = self.experiment.lockin_wait_time(ref, multiplier=multiplier)
            return {"command": command, "ref": ref, "wait_s": float(wait_s)}
        if command == "set_lockin_settings":
            ref = str(request.get("ref") or "lockin.main")
            settings = dict(request.get("settings") or {})
            self.status_changed.emit("applying lock-in settings")
            applied = self.experiment.set_lockin_settings(ref, **settings)
            return {"command": command, "ref": ref, "settings": applied}
        raise ValueError(f"Unsupported device command: {command}")

    @QtCore.Slot(object)
    def run_command(self, request: object):
        try:
            data = request if isinstance(request, dict) else {}
            self.finished.emit(self._execute(data))
        except Exception as e:
            self.error_occurred.emit(str(e))

    def run(self):
        self.run_command(self._request())


class MoveWorker(QtCore.QObject):
    status_changed = QtCore.Signal(str)
    position_changed = QtCore.Signal(object)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        experiment: Experiment,
        axis: str,
        value: float,
        coordinate: str = "measurement",
    ):
        super().__init__()
        self.experiment = experiment
        self.axis = axis
        self.value = value
        self.coordinate = coordinate

    def run(self):
        try:
            if self.axis == "t":
                position = self.experiment.move_delay_stage(
                    self.value,
                    coordinate=self.coordinate,
                    on_status=self.status_changed.emit,
                    on_position=self.position_changed.emit,
                )
            elif self.axis in {"x", "y"}:
                position = self.experiment.move_scanner(
                    self.axis,
                    self.value,
                    coordinate=self.coordinate,
                    on_status=self.status_changed.emit,
                    on_position=self.position_changed.emit,
                )
            else:
                raise ValueError(f"Unsupported axis: {self.axis}")
            self.finished.emit(
                {
                    "axis": self.axis,
                    "value": self.value,
                    "coordinate": self.coordinate,
                    "position": position,
                }
            )
        except Exception as e:
            self.error_occurred.emit(str(e))


class LiveStatusWorker(QtCore.QObject):
    live_status_ready = QtCore.Signal(object, object)
    lockin_status_ready = QtCore.Signal(object, object, object)
    error_occurred = QtCore.Signal(str)

    def __init__(self, *, experiment: Experiment):
        super().__init__()
        self.experiment = experiment
        self._busy = False

    def _lockin_ref(self) -> str | None:
        for ref, connected in self.experiment.connected_devices().items():
            if connected and ref.startswith("lockin."):
                return ref
        return None

    def _read_lockin_settings(self, ref: str) -> dict[str, Any] | None:
        try:
            return self.experiment.read_lockin_settings(ref)
        except Exception:
            return None

    def _read_lockin_signal(self, ref: str) -> dict[str, Any] | None:
        try:
            return self.experiment.read_lockin_signal(ref)
        except Exception:
            return None

    def _read_lockin_overload(self, ref: str) -> dict[str, Any] | None:
        try:
            return self.experiment.read_lockin_overload(ref)
        except Exception:
            return {"_error": True}

    @QtCore.Slot()
    def read_full(self):
        if self._busy:
            return
        self._busy = True
        try:
            status = self.experiment.read_live_status()
            self.live_status_ready.emit(status, status.lockin_overload)
        except Exception as e:
            self.error_occurred.emit(str(e))
        finally:
            self._busy = False

    @QtCore.Slot()
    def read_lockin(self):
        if self._busy:
            return
        ref = self._lockin_ref()
        if ref is None:
            return
        self._busy = True
        try:
            settings = self._read_lockin_settings(ref)
            signal = self._read_lockin_signal(ref)
            overload = self._read_lockin_overload(ref)
            self.lockin_status_ready.emit(settings, signal, overload)
        except Exception as e:
            self.error_occurred.emit(str(e))
        finally:
            self._busy = False


class ResourceListWorker(QtCore.QObject):
    resources_ready = QtCore.Signal(object, object)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal()

    @QtCore.Slot()
    def run(self):
        errors: list[str] = []
        visa_resources: list[str] = []
        serial_ports: list[str] = []
        try:
            visa_resources = list(list_visa_resources())
        except Exception as e:
            errors.append(f"lock-in resources: {e}")
        try:
            serial_ports = sorted(port.device for port in list_ports.comports())
        except Exception as e:
            errors.append(f"serial ports: {e}")
        self.resources_ready.emit(visa_resources, serial_ports)
        if errors:
            self.error_occurred.emit("; ".join(errors))
        self.finished.emit()


class GuiLogStream(QtCore.QObject):
    text_ready = QtCore.Signal(str)

    def __init__(self, stream):
        super().__init__()
        self._stream = stream
        self._buffer = ""

    def write(self, text: str):
        self._stream.write(text)
        self._stream.flush()
        self._buffer += text
        while "\n" in self._buffer:
            line, self._buffer = self._buffer.split("\n", 1)
            if line:
                self.text_ready.emit(line)

    def flush(self):
        self._stream.flush()
        if self._buffer:
            self.text_ready.emit(self._buffer)
            self._buffer = ""


class TRKRGui(QtWidgets.QMainWindow):
    device_command_requested = QtCore.Signal(object)

    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"KohdaLab TRKR v{__version__}")
        self.resize(1440, 820)

        config_resolution = resolve_config_path()
        self.config_path = QtWidgets.QLineEdit(str(config_resolution.path or ""))
        if config_resolution.path is None:
            self.config = normalize_config({})
            self._startup_config_message = "No config loaded. Choose a config path and click Load."
        else:
            self.config = load_config(config_resolution.path)
            write_last_config_path(config_resolution.path)
            self._startup_config_message = f"Loaded config ({config_resolution.source}): {config_resolution.path}"
        self.experiment: Experiment | None = None
        self.thread: QtCore.QThread | None = None
        self.worker: MeasurementWorker | None = None
        self.device_thread: QtCore.QThread | None = None
        self.device_worker: DeviceCommandWorker | None = None
        self.device_command_active = False
        self.move_thread: QtCore.QThread | None = None
        self.move_worker: MoveWorker | None = None
        self.live_thread: QtCore.QThread | None = None
        self.live_worker: LiveStatusWorker | None = None
        self.resource_thread: QtCore.QThread | None = None
        self.resource_worker: ResourceListWorker | None = None
        self.running_move_axis: str | None = None
        self.pending_origin_axis: str | None = None
        self.pending_wait_spin: QtWidgets.QDoubleSpinBox | None = None
        self.running_measurement: str | None = None
        self.running_srkr_axis: str | None = None
        self.running_motion_axes: set[str] = set()
        self.rows_by_mode: dict[str, list[dict[str, Any]]] = {
            "signal_monitor": [],
            "trkr": [],
            "srkr": [],
            "strkr": [],
            "srkr_2d": [],
        }
        self.output_settings_by_mode: dict[str, dict[str, Any]] = {}
        self._last_measurement_for_output = "signal_monitor"
        self.point_text_by_mode: dict[str, str] = {
            "signal_monitor": "-",
            "trkr": "-",
            "srkr": "-",
            "strkr": "-",
            "srkr_2d": "-",
        }
        self.eta_text_by_mode: dict[str, str] = {
            "signal_monitor": "-",
            "trkr": "-",
            "srkr": "-",
            "strkr": "-",
            "srkr_2d": "-",
        }
        self._voltage_scale = 1.0
        self._voltage_unit = "V"
        self._last_live_refresh = 0.0
        self._move_block_until = 0.0
        self._shutdown_requested = False
        self._shutdown_complete = False
        self._scan2d_fast_point_count = 0
        self._scan2d_slow_point_count = 0
        self._scan2d_eta_anchor_at: float | None = None
        self._scan2d_eta_line_cycle_s: float | None = None
        self._current_position_values = {"t": None, "x": None, "y": None}

        self._build_widgets()
        self._build_layout()
        self._connect_signals()
        self._install_log_streams()
        self._load_config_into_fields(self.config)
        self.append_log(self._startup_config_message)
        self.refresh_all_ports()
        self._refresh_plot_labels()
        self._refresh_scan_limit_hints()

        self.live_timer = QtCore.QTimer(self)
        self.live_timer.setInterval(500)
        self.live_timer.timeout.connect(self.refresh_live_status)
        self.live_timer.start()

    def _build_widgets(self):
        self.load_button = QtWidgets.QPushButton("Load")
        self.save_button = QtWidgets.QPushButton("Save")
        self.browse_button = QtWidgets.QPushButton("Browse")

        self.connect_button = QtWidgets.QPushButton("Connect")
        self.connect_button.setText("Connect All")
        self.disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.disconnect_button.setText("Disconnect All")
        self.read_status_button = QtWidgets.QPushButton("Read Live")
        self.status_label = QtWidgets.QLabel("idle")

        self.device_toggles: list[QtWidgets.QToolButton] = []

        self.lockin_model_combo = self._combo(LOCKIN_MODELS)
        self.lockin_resource_combo = self._combo([])
        self.lockin_refresh_button = QtWidgets.QPushButton("Refresh")
        self.lockin_connect_button = QtWidgets.QPushButton("Connect")
        self.lockin_disconnect_button = QtWidgets.QPushButton("Disconnect")

        self.t_controller_combo = self._combo(sorted(DELAY_STAGE_CONTROLLERS))
        self.t_stage_combo = self._combo(list_stages(self.t_controller_combo.currentText()))
        self.t_port_combo = self._combo([])
        self.t_port_refresh_button = QtWidgets.QPushButton("Refresh")
        self.t_direction_spin = QtWidgets.QSpinBox()
        self.t_direction_spin.setButtonSymbols(QtWidgets.QAbstractSpinBox.NoButtons)
        self.t_direction_spin.setRange(-1, 1)
        self.t_direction_spin.setValue(1)
        self.t_connect_button = QtWidgets.QPushButton("Connect")
        self.t_disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.t_initialize_button = QtWidgets.QPushButton("Initialize")

        self.x_controller_combo = self._combo(sorted(SCANNER_CONTROLLERS))
        self.y_controller_combo = self._combo(sorted(SCANNER_CONTROLLERS))
        self.x_actuator_combo = self._combo(self._actuators_for_controller(self.x_controller_combo.currentText()))
        self.y_actuator_combo = self._combo(self._actuators_for_controller(self.y_controller_combo.currentText()))
        self.x_axis_spin = QtWidgets.QSpinBox()
        self.y_axis_spin = QtWidgets.QSpinBox()
        for spin in (self.x_axis_spin, self.y_axis_spin):
            spin.setButtonSymbols(QtWidgets.QAbstractSpinBox.NoButtons)
            spin.setRange(1, 8)
            spin.setValue(1)
        self.x_scale_spin = self._spin(-1_000_000, 1_000_000, 6, 582.0)
        self.y_scale_spin = self._spin(-1_000_000, 1_000_000, 6, 412.0)
        self.x_scale_label = QtWidgets.QLabel(scanner_scale_label_for_actuator(self.x_actuator_combo.currentText()))
        self.y_scale_label = QtWidgets.QLabel(scanner_scale_label_for_actuator(self.y_actuator_combo.currentText()))
        self.x_port_combo = self._combo([])
        self.y_port_combo = self._combo([])
        self.x_port_refresh_button = QtWidgets.QPushButton("Refresh")
        self.y_port_refresh_button = QtWidgets.QPushButton("Refresh")
        self.x_connect_button = QtWidgets.QPushButton("Connect")
        self.x_disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.x_initialize_button = QtWidgets.QPushButton("Initialize")
        self.y_connect_button = QtWidgets.QPushButton("Connect")
        self.y_disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.y_initialize_button = QtWidgets.QPushButton("Initialize")

        self.overload_label = QtWidgets.QLabel("-")
        self.sensitivity_label = QtWidgets.QLabel("-")
        self.tc_label = QtWidgets.QLabel("-")
        self.freq_label = QtWidgets.QLabel("-")
        self.signal_labels = {
            "X": QtWidgets.QLabel("-"),
            "Y": QtWidgets.QLabel("-"),
            "R": QtWidgets.QLabel("-"),
            "Theta": QtWidgets.QLabel("-"),
        }
        self.signal_title_labels = {
            "X": QtWidgets.QLabel("X (V)"),
            "Y": QtWidgets.QLabel("Y (V)"),
            "R": QtWidgets.QLabel("R (V)"),
            "Theta": QtWidgets.QLabel("Theta (deg)"),
        }
        self.position_labels = {
            "t": QtWidgets.QLabel("-"),
            "x": QtWidgets.QLabel("-"),
            "y": QtWidgets.QLabel("-"),
        }
        self.offset_labels = {
            "t": QtWidgets.QLabel("-"),
            "x": QtWidgets.QLabel("-"),
            "y": QtWidgets.QLabel("-"),
        }
        self.corrected_labels = {
            "t": QtWidgets.QLabel("-"),
            "x": QtWidgets.QLabel("-"),
            "y": QtWidgets.QLabel("-"),
        }
        self.move_t_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.move_x_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.move_y_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.move_t_button = QtWidgets.QPushButton("Move")
        self.move_x_button = QtWidgets.QPushButton("Move")
        self.move_y_button = QtWidgets.QPushButton("Move")

        self.t_zero_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.x_zero_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.y_zero_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.use_current_t_button = QtWidgets.QPushButton("Use Current")
        self.use_current_x_button = QtWidgets.QPushButton("Use Current")
        self.use_current_y_button = QtWidgets.QPushButton("Use Current")
        self.t_cor_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.x_cor_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.y_cor_spin = self._spin(-1_000_000, 1_000_000, 3, 0.0)
        self.t_cor_button = QtWidgets.QPushButton("Move")
        self.x_cor_button = QtWidgets.QPushButton("Move")
        self.y_cor_button = QtWidgets.QPushButton("Move")

        self.measurement_tabs = QtWidgets.QTabWidget()
        self.measurement_side_layouts: list[QtWidgets.QVBoxLayout] = []
        self.signal_interval_spin = self._spin(0.01, 3600, 2, 1.0)
        self.signal_points_spin = QtWidgets.QSpinBox()
        self.signal_points_spin.setRange(1, 1_000_000)
        self.signal_points_spin.setValue(10)

        self.trkr_min_spin = self._spin(-1_000_000, 1_000_000, 3, -50.0)
        self.trkr_max_spin = self._spin(-1_000_000, 1_000_000, 3, 300.0)
        self.trkr_step_spin = self._spin(-1_000_000, 1_000_000, 3, 50.0)
        self.trkr_min_hint = self._hint_label()
        self.trkr_max_hint = self._hint_label()
        self.trkr_step_hint = self._hint_label()
        self.trkr_wait_spin = self._spin(0.0, 3600, 2, 2.0)
        self.trkr_tc_button = QtWidgets.QPushButton("Use TC*4")
        self.trkr_axis_combo = QtWidgets.QComboBox()
        self.trkr_axis_combo.addItems(["t"])
        self.trkr_return_check = QtWidgets.QCheckBox("Return to t zero")
        self.trkr_return_check.setChecked(True)

        self.srkr_axis_combo = QtWidgets.QComboBox()
        self.srkr_axis_combo.addItems(["x", "y"])
        self.srkr_min_spin = self._spin(-1_000_000, 1_000_000, 3, -30.0)
        self.srkr_max_spin = self._spin(-1_000_000, 1_000_000, 3, 30.0)
        self.srkr_step_spin = self._spin(-1_000_000, 1_000_000, 3, 10.0)
        self.srkr_min_hint = self._hint_label()
        self.srkr_max_hint = self._hint_label()
        self.srkr_step_hint = self._hint_label()
        self.srkr_wait_spin = self._spin(0.0, 3600, 2, 2.0)
        self.srkr_tc_button = QtWidgets.QPushButton("Use TC*4")
        self.srkr_return_check = QtWidgets.QCheckBox("Return to origin")
        self.srkr_return_check.setChecked(True)

        self.strkr_fast_axis_combo = self._combo(["t", "x", "y"])
        self.strkr_slow_axis_combo = self._combo(["x", "t", "y"])
        self.strkr_fast_axis_combo.setEditable(False)
        self.strkr_slow_axis_combo.setEditable(False)
        self.strkr_range_spins = self._axis_range_spins(
            {
                "t": (-50.0, 300.0, 5.0),
                "x": (-30.0, 30.0, 1.0),
                "y": (-30.0, 30.0, 1.0),
            }
        )
        self.strkr_role_spins = {
            "fast_axis": self._range_spin_set(_default_axis_range("t")),
            "slow_axis": self._range_spin_set(_default_axis_range("x")),
        }
        self.strkr_role_labels = self._scan2d_role_labels()
        self.strkr_role_hints = self._scan2d_role_hints()
        self.strkr_wait_spin = self._spin(0.0, 3600, 2, 2.0)
        self.strkr_tc_button = QtWidgets.QPushButton("Use TC*4")

        self.srkr_2d_fast_axis_combo = self._combo(["x", "y"])
        self.srkr_2d_slow_axis_combo = self._combo(["y", "x"])
        self.srkr_2d_fast_axis_combo.setEditable(False)
        self.srkr_2d_slow_axis_combo.setEditable(False)
        self.srkr_2d_range_spins = self._axis_range_spins(
            {
                "x": (-30.0, 30.0, 1.0),
                "y": (-30.0, 30.0, 1.0),
            }
        )
        self.srkr_2d_role_spins = {
            "fast_axis": self._range_spin_set(_default_axis_range("x")),
            "slow_axis": self._range_spin_set(_default_axis_range("y")),
        }
        self.srkr_2d_role_labels = self._scan2d_role_labels()
        self.srkr_2d_role_hints = self._scan2d_role_hints()
        self.srkr_2d_wait_spin = self._spin(0.0, 3600, 2, 2.0)
        self.srkr_2d_tc_button = QtWidgets.QPushButton("Use TC*4")
        self.scan2d_role_axes = {
            "strkr": {"fast_axis": "t", "slow_axis": "x"},
            "srkr_2d": {"fast_axis": "x", "slow_axis": "y"},
        }

        self.output_dir_edit = QtWidgets.QLineEdit(str(Path.cwd()))
        self.output_name_edit = QtWidgets.QLineEdit("trkr_run")
        self.output_browse_button = QtWidgets.QPushButton("Browse")
        self.auto_suffix_check = QtWidgets.QCheckBox("Auto suffix")
        self.auto_suffix_check.setChecked(True)

        self.signal_mode_combo = QtWidgets.QComboBox()
        self.signal_mode_combo.addItems(["X / Y", "R / Theta"])
        self.start_button = QtWidgets.QPushButton("Start")
        self.stop_button = QtWidgets.QPushButton("Stop")
        self.save_rows_button = QtWidgets.QPushButton("Save Now")
        self.stop_button.setEnabled(False)
        self.right_panel_toggle = QtWidgets.QToolButton()
        self.right_panel_toggle.setText(">")
        self.right_panel_toggle.setCheckable(True)
        self.right_panel_toggle.setToolButtonStyle(QtCore.Qt.ToolButtonTextOnly)
        self.right_panel_toggle.setFixedWidth(24)
        self.right_panel_toggle.setSizePolicy(QtWidgets.QSizePolicy.Fixed, QtWidgets.QSizePolicy.Expanding)
        self.right_panel_toggle.setStyleSheet(
            "QToolButton { background: #3a3a3a; border: 1px solid #555; border-radius: 4px; color: #ddd; }"
            "QToolButton:hover { background: #484848; }"
        )

        self.point_label = QtWidgets.QLabel("-")
        self.eta_label = QtWidgets.QLabel("-")
        self.snapshot_table = QtWidgets.QTableWidget(0, 2)
        self.snapshot_table.setHorizontalHeaderLabels(["Field", "Value"])
        self.snapshot_table.verticalHeader().setVisible(False)
        self.snapshot_table.horizontalHeader().setStretchLastSection(True)
        self.snapshot_table.setEditTriggers(QtWidgets.QAbstractItemView.NoEditTriggers)
        self.snapshot_table.setSelectionMode(QtWidgets.QAbstractItemView.NoSelection)
        self.log = QtWidgets.QPlainTextEdit()
        self.log.setReadOnly(True)

        self.plot1 = pg.PlotWidget()
        self.plot2 = pg.PlotWidget()
        for plot in (self.plot1, self.plot2):
            plot.showGrid(x=True, y=True, alpha=0.25)
            plot.showAxis("top")
        self.curve1 = self.plot1.plot(pen=pg.mkPen("#1f77b4", width=2), name="signal 1")
        self.curve2 = self.plot2.plot(pen=pg.mkPen("#d62728", width=2), name="signal 2")
        self.standard_plot_widget = QtWidgets.QWidget()
        standard_layout = QtWidgets.QVBoxLayout(self.standard_plot_widget)
        standard_layout.setContentsMargins(0, 0, 0, 0)
        standard_layout.setSpacing(8)
        standard_layout.addWidget(self.plot1, 1)
        standard_layout.addWidget(self.plot2, 1)

        self.srkr_plot_widget = QtWidgets.QWidget()
        srkr_layout = QtWidgets.QGridLayout(self.srkr_plot_widget)
        srkr_layout.setContentsMargins(0, 0, 0, 0)
        srkr_layout.setSpacing(8)
        self.srkr_plots: dict[tuple[str, int], pg.PlotWidget] = {}
        self.srkr_curves = {}
        colors = {1: "#1f77b4", 2: "#d62728"}
        for col, axis in enumerate(("x", "y")):
            for row, signal_index in enumerate((1, 2)):
                plot = pg.PlotWidget()
                plot.showGrid(x=True, y=True, alpha=0.25)
                plot.showAxis("top")
                curve = plot.plot(pen=pg.mkPen(colors[signal_index], width=2), name=f"{axis}{signal_index}")
                self.srkr_plots[(axis, signal_index)] = plot
                self.srkr_curves[(axis, signal_index)] = curve
                srkr_layout.addWidget(plot, row, col)

        self.scan2d_plot_widget = QtWidgets.QWidget()
        scan2d_layout = QtWidgets.QGridLayout(self.scan2d_plot_widget)
        scan2d_layout.setContentsMargins(0, 0, 0, 0)
        scan2d_layout.setSpacing(8)
        self.scan2d_line_plots: dict[int, pg.PlotWidget] = {}
        self.scan2d_line_curves: dict[int, object] = {}
        self.scan2d_heatmap_plots: dict[int, pg.PlotWidget] = {}
        self.scan2d_heatmaps: dict[int, pg.ImageItem] = {}
        for row, signal_index in enumerate((1, 2)):
            line_plot = pg.PlotWidget()
            line_plot.showGrid(x=True, y=True, alpha=0.25)
            line_curve = line_plot.plot(pen=pg.mkPen(colors[signal_index], width=2), name=f"line{signal_index}")
            heatmap_plot = pg.PlotWidget()
            heatmap_plot.showGrid(x=True, y=True, alpha=0.15)
            heatmap = pg.ImageItem()
            heatmap.setLookupTable(RDBU_R_LUT)
            heatmap.setLevels((-1.0, 1.0))
            heatmap_plot.addItem(heatmap)
            self.scan2d_line_plots[signal_index] = line_plot
            self.scan2d_line_curves[signal_index] = line_curve
            self.scan2d_heatmap_plots[signal_index] = heatmap_plot
            self.scan2d_heatmaps[signal_index] = heatmap
            scan2d_layout.addWidget(line_plot, row, 0)
            scan2d_layout.addWidget(heatmap_plot, row, 1)
        scan2d_layout.setColumnStretch(0, 1)
        scan2d_layout.setColumnStretch(1, 1)

        self.plot_stack = QtWidgets.QStackedWidget()
        self.plot_stack.addWidget(self.standard_plot_widget)
        self.plot_stack.addWidget(self.srkr_plot_widget)
        self.plot_stack.addWidget(self.scan2d_plot_widget)

    def _combo(self, items: list[str]) -> QtWidgets.QComboBox:
        combo = QtWidgets.QComboBox()
        combo.setEditable(True)
        combo.addItems(items)
        return combo

    def _actuators_for_controller(self, controller: str) -> list[str]:
        controller_name = controller.strip().upper()
        names = []
        for name in ACTUATOR_NAMES:
            settings = ACTUATORS.get(name.upper().replace("-", ""), {})
            controllers = {str(item).upper() for item in settings.get("controllers", [])}
            if not controllers or controller_name in controllers:
                names.append(name)
        return sorted(names)

    def _spin(self, minimum: float, maximum: float, decimals: int, value: float) -> QtWidgets.QDoubleSpinBox:
        spin = QtWidgets.QDoubleSpinBox()
        spin.setButtonSymbols(QtWidgets.QAbstractSpinBox.NoButtons)
        spin.setRange(minimum, maximum)
        spin.setDecimals(decimals)
        spin.setValue(value)
        spin.setSingleStep(1.0)
        return spin

    def _axis_range_spins(self, defaults: dict[str, tuple[float, float, float]]) -> dict[str, dict[str, QtWidgets.QDoubleSpinBox]]:
        ranges: dict[str, dict[str, QtWidgets.QDoubleSpinBox]] = {}
        for axis, (minimum, maximum, step) in defaults.items():
            ranges[axis] = self._range_spin_set((minimum, maximum, step))
        return ranges

    def _range_spin_set(self, defaults: tuple[float, float, float]) -> dict[str, QtWidgets.QDoubleSpinBox]:
        minimum, maximum, step = defaults
        return {
            "min": self._spin(-1_000_000, 1_000_000, 3, minimum),
            "max": self._spin(-1_000_000, 1_000_000, 3, maximum),
            "step": self._spin(-1_000_000, 1_000_000, 3, step),
        }

    def _scan2d_role_labels(self) -> dict[str, dict[str, QtWidgets.QLabel]]:
        return {role: {key: QtWidgets.QLabel("") for key in RANGE_KEYS} for role in SCAN2D_ROLES}

    def _scan2d_role_hints(self) -> dict[str, dict[str, QtWidgets.QLabel]]:
        return {role: {key: self._hint_label() for key in RANGE_KEYS} for role in SCAN2D_ROLES}

    def _hint_label(self) -> QtWidgets.QLabel:
        label = QtWidgets.QLabel("-")
        label.setStyleSheet("color: #777;")
        label.setFixedWidth(MEASUREMENT_ROW_TRAILING_WIDTH)
        return label

    def _build_layout(self):
        central = QtWidgets.QWidget()
        self.setCentralWidget(central)
        root = QtWidgets.QHBoxLayout(central)
        root.setContentsMargins(8, 8, 8, 8)
        root.setSpacing(8)

        left_widget = QtWidgets.QWidget()
        left = QtWidgets.QVBoxLayout(left_widget)
        left.setSpacing(8)
        left.addWidget(self._session_group())
        left.addWidget(self._lockin_group())
        left.addWidget(self._motion_group("Delay Stage", "t"))
        left.addWidget(self._motion_group("Scanner X", "x"))
        left.addWidget(self._motion_group("Scanner Y", "y"))
        left.addStretch(1)
        left_scroll = QtWidgets.QScrollArea()
        left_scroll.setWidgetResizable(True)
        left_scroll.setHorizontalScrollBarPolicy(QtCore.Qt.ScrollBarAlwaysOff)
        left_scroll.setWidget(left_widget)
        left_scroll.setMinimumWidth(384)
        self.left_panel = left_scroll

        center_widget = QtWidgets.QWidget()
        center = QtWidgets.QVBoxLayout(center_widget)
        center.setSpacing(8)
        self.center_top_widget = self._measurement_group()
        self.center_top_widget.setMinimumHeight(216)
        center.addWidget(self.center_top_widget, 0)
        center.addWidget(self._plot_toolbar(), 0)
        center.addWidget(self.plot_stack, 1)

        self.right_panel = QtWidgets.QWidget()
        right_shell = QtWidgets.QHBoxLayout(self.right_panel)
        right_shell.setContentsMargins(0, 0, 0, 0)
        right_shell.setSpacing(4)
        right_shell.addWidget(self.right_panel_toggle)
        self.right_content = QtWidgets.QWidget()
        right = QtWidgets.QVBoxLayout(self.right_content)
        right.setSpacing(8)
        self.log.setMinimumHeight(216)
        right.addWidget(self.log, 0)
        right.addWidget(self.snapshot_table, 1)
        right_shell.addWidget(self.right_content, 1)

        root.addWidget(self.left_panel, 0)
        root.addWidget(center_widget, 1)
        root.addWidget(self.right_panel, 0)
        self._apply_panel_sizes()
        self._load_scan2d_role_ranges("strkr")
        self._load_scan2d_role_ranges("srkr_2d")

    def _session_group(self) -> QtWidgets.QGroupBox:
        group = QtWidgets.QGroupBox("Session")
        layout = QtWidgets.QVBoxLayout(group)
        top = QtWidgets.QHBoxLayout()
        top.setSpacing(8)
        for widget, stretch in (
            (self.config_path, 2),
            (self.browse_button, 1),
            (self.load_button, 1),
            (self.save_button, 1),
        ):
            widget.setSizePolicy(QtWidgets.QSizePolicy.Ignored, QtWidgets.QSizePolicy.Fixed)
            top.addWidget(widget, stretch)
        bottom = QtWidgets.QHBoxLayout()
        bottom.setSpacing(8)
        bottom.addWidget(self.connect_button, 1)
        bottom.addWidget(self.disconnect_button, 1)
        layout.addLayout(top)
        layout.addLayout(bottom)
        return group

    def _collapsible_device(self, content: QtWidgets.QWidget) -> QtWidgets.QWidget:
        wrapper = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(wrapper)
        layout.setContentsMargins(0, 0, 0, 0)
        toggle = QtWidgets.QToolButton()
        toggle.setText("Device")
        toggle.setCheckable(True)
        toggle.setToolButtonStyle(QtCore.Qt.ToolButtonTextBesideIcon)
        toggle.setArrowType(QtCore.Qt.RightArrow)
        content.setVisible(False)
        toggle.toggled.connect(lambda expanded: toggle.setArrowType(QtCore.Qt.DownArrow if expanded else QtCore.Qt.RightArrow))
        toggle.toggled.connect(content.setVisible)
        self.device_toggles.append(toggle)
        layout.addWidget(toggle, 0, QtCore.Qt.AlignLeft)
        layout.addWidget(content)
        return wrapper

    def _lockin_settings_tab(self) -> QtWidgets.QWidget:
        widget = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(widget)
        row = QtWidgets.QHBoxLayout()
        row.addWidget(self.lockin_resource_combo, 1)
        row.addWidget(self.lockin_refresh_button)
        layout.addRow("Model", self.lockin_model_combo)
        layout.addRow("Resource", row)
        buttons = QtWidgets.QHBoxLayout()
        buttons.addWidget(self.lockin_connect_button)
        buttons.addWidget(self.lockin_disconnect_button)
        layout.addRow("", buttons)
        return widget

    def _delay_stage_settings_tab(self) -> QtWidgets.QWidget:
        widget = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(widget)
        row = QtWidgets.QHBoxLayout()
        row.addWidget(self.t_port_combo, 1)
        row.addWidget(self.t_port_refresh_button)
        layout.addRow("Controller", self.t_controller_combo)
        layout.addRow("Stage", self.t_stage_combo)
        layout.addRow("Direction", self.t_direction_spin)
        layout.addRow("Port", row)
        buttons = QtWidgets.QHBoxLayout()
        buttons.addWidget(self.t_connect_button)
        buttons.addWidget(self.t_disconnect_button)
        buttons.addWidget(self.t_initialize_button)
        layout.addRow("", buttons)
        return widget

    def _scanner_settings_tab(self, axis: str) -> QtWidgets.QWidget:
        widget = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(widget)
        controller = self.x_controller_combo if axis == "x" else self.y_controller_combo
        actuator = self.x_actuator_combo if axis == "x" else self.y_actuator_combo
        axis_spin = self.x_axis_spin if axis == "x" else self.y_axis_spin
        scale_spin = self.x_scale_spin if axis == "x" else self.y_scale_spin
        scale_label = self.x_scale_label if axis == "x" else self.y_scale_label
        port_combo = self.x_port_combo if axis == "x" else self.y_port_combo
        refresh_button = self.x_port_refresh_button if axis == "x" else self.y_port_refresh_button
        row = QtWidgets.QHBoxLayout()
        row.addWidget(port_combo, 1)
        row.addWidget(refresh_button)
        layout.addRow("Controller", controller)
        layout.addRow("Actuator", actuator)
        layout.addRow("Axis", axis_spin)
        layout.addRow(scale_label, scale_spin)
        layout.addRow("Port", row)
        buttons = QtWidgets.QHBoxLayout()
        if axis == "x":
            buttons.addWidget(self.x_connect_button)
            buttons.addWidget(self.x_disconnect_button)
            buttons.addWidget(self.x_initialize_button)
        else:
            buttons.addWidget(self.y_connect_button)
            buttons.addWidget(self.y_disconnect_button)
            buttons.addWidget(self.y_initialize_button)
        layout.addRow("", buttons)
        return widget

    def _lockin_group(self) -> QtWidgets.QGroupBox:
        group = QtWidgets.QGroupBox("Lock-in")
        layout = QtWidgets.QVBoxLayout(group)
        layout.addWidget(self._collapsible_device(self._lockin_settings_tab()))
        grid = QtWidgets.QGridLayout()
        fields = [
            ("Overload", self.overload_label),
            ("Sensitivity", self.sensitivity_label),
            ("Time Constant", self.tc_label),
            ("Ref. Freq.", self.freq_label),
            (self.signal_title_labels["X"], self.signal_labels["X"]),
            (self.signal_title_labels["Y"], self.signal_labels["Y"]),
            (self.signal_title_labels["R"], self.signal_labels["R"]),
            (self.signal_title_labels["Theta"], self.signal_labels["Theta"]),
        ]
        for index, (label, value) in enumerate(fields):
            row = index % 4
            col = 0 if index < 4 else 2
            grid.addWidget(label if isinstance(label, QtWidgets.QLabel) else QtWidgets.QLabel(label), row, col)
            grid.addWidget(value, row, col + 1)
        layout.addLayout(grid)
        return group

    def _motion_group(self, title: str, axis: str) -> QtWidgets.QGroupBox:
        group = QtWidgets.QGroupBox(title)
        layout = QtWidgets.QVBoxLayout(group)
        device = self._delay_stage_settings_tab() if axis == "t" else self._scanner_settings_tab(axis)
        layout.addWidget(self._collapsible_device(device))
        grid = QtWidgets.QGridLayout()
        zero_spin = {"t": self.t_zero_spin, "x": self.x_zero_spin, "y": self.y_zero_spin}[axis]
        zero_button = {"t": self.use_current_t_button, "x": self.use_current_x_button, "y": self.use_current_y_button}[axis]
        live_value = self.position_labels[axis]
        move_spin = {"t": self.move_t_spin, "x": self.move_x_spin, "y": self.move_y_spin}[axis]
        move_button = {"t": self.move_t_button, "x": self.move_x_button, "y": self.move_y_button}[axis]
        cor_spin = {"t": self.t_cor_spin, "x": self.x_cor_spin, "y": self.y_cor_spin}[axis]
        cor_button = {"t": self.t_cor_button, "x": self.x_cor_button, "y": self.y_cor_button}[axis]
        unit = "ps" if axis == "t" else "um"
        label = {"t": "t", "x": "x", "y": "y"}[axis]
        rows = [
            (f"{label} ({unit})", live_value, move_spin, move_button),
            (f"offset ({unit})", self.offset_labels[axis], zero_spin, zero_button),
            (f"{label}_cor ({unit})", self.corrected_labels[axis], cor_spin, cor_button),
        ]
        for row, (row_label, value_label, spin, button) in enumerate(rows):
            grid.addWidget(QtWidgets.QLabel(row_label), row, 0)
            grid.addWidget(value_label, row, 1)
            grid.addWidget(spin, row, 2)
            grid.addWidget(button, row, 3)
        grid.setColumnStretch(0, 1)
        grid.setColumnStretch(1, 1)
        grid.setColumnStretch(2, 1)
        grid.setColumnStretch(3, 1)
        layout.addLayout(grid)
        return group

    def _measurement_group(self) -> QtWidgets.QGroupBox:
        group = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(group)
        layout.setContentsMargins(0, 0, 0, 0)
        self.measurement_side_layouts.clear()
        self.measurement_tabs.addTab(self._signal_tab(), "Signal Monitor")
        self.measurement_tabs.addTab(self._trkr_tab(), "TRKR")
        self.measurement_tabs.addTab(self._srkr_tab(), "SRKR")
        self.measurement_tabs.addTab(self._strkr_tab(), "STRKR")
        self.measurement_tabs.addTab(self._srkr_2d_tab(), "SRKR 2D")
        self.output_run_widget = self._output_run_widget()
        self._attach_output_run_to_tab(0)
        layout.addWidget(self.measurement_tabs)
        return group

    def _signal_tab(self) -> QtWidgets.QWidget:
        settings = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(settings)
        layout.addRow("Interval (s)", self.signal_interval_spin)
        layout.addRow("Points", self.signal_points_spin)
        return self._measurement_tab(settings)

    def _trkr_tab(self) -> QtWidgets.QWidget:
        settings = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(settings)
        layout.addRow("Fast Axis", self.trkr_axis_combo)
        layout.addRow("t min cor (ps)", self._with_hint(self.trkr_min_spin, self.trkr_min_hint))
        layout.addRow("t max cor (ps)", self._with_hint(self.trkr_max_spin, self.trkr_max_hint))
        layout.addRow("t step (ps)", self._with_hint(self.trkr_step_spin, self.trkr_step_hint))
        layout.addRow("Wait (s)", self._with_button(self.trkr_wait_spin, self.trkr_tc_button))
        return self._measurement_tab(settings)

    def _srkr_tab(self) -> QtWidgets.QWidget:
        settings = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(settings)
        layout.addRow("Fast Axis", self.srkr_axis_combo)
        layout.addRow("min cor (um)", self._with_hint(self.srkr_min_spin, self.srkr_min_hint))
        layout.addRow("max cor (um)", self._with_hint(self.srkr_max_spin, self.srkr_max_hint))
        layout.addRow("step (um)", self._with_hint(self.srkr_step_spin, self.srkr_step_hint))
        layout.addRow("Wait (s)", self._with_button(self.srkr_wait_spin, self.srkr_tc_button))
        return self._measurement_tab(settings)

    def _scan2d_axis_panel(
        self,
        title: str,
        axis_combo: QtWidgets.QComboBox,
        role_spins: dict[str, QtWidgets.QDoubleSpinBox],
        role_labels: dict[str, QtWidgets.QLabel],
        role_hints: dict[str, QtWidgets.QLabel],
        wait_spin: QtWidgets.QDoubleSpinBox | None = None,
        wait_button: QtWidgets.QPushButton | None = None,
    ) -> QtWidgets.QWidget:
        group = QtWidgets.QWidget()
        layout = QtWidgets.QFormLayout(group)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addRow("Fast Axis" if title == "Fast" else "Slow Axis", axis_combo)
        for key in RANGE_KEYS:
            layout.addRow(role_labels[key], self._with_hint(role_spins[key], role_hints[key]))
        if wait_spin is not None and wait_button is not None:
            layout.addRow("Wait (s)", self._with_button(wait_spin, wait_button))
        return group

    def _strkr_tab(self) -> QtWidgets.QWidget:
        settings = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(settings)
        layout.setContentsMargins(9, 9, 9, 9)
        axes = QtWidgets.QHBoxLayout()
        axes.setContentsMargins(0, 0, 0, 0)
        axes.addWidget(
            self._scan2d_axis_panel(
                "Fast",
                self.strkr_fast_axis_combo,
                self.strkr_role_spins["fast_axis"],
                self.strkr_role_labels["fast_axis"],
                self.strkr_role_hints["fast_axis"],
                self.strkr_wait_spin,
                self.strkr_tc_button,
            ),
            1,
        )
        axes.addWidget(
            self._scan2d_axis_panel(
                "Slow",
                self.strkr_slow_axis_combo,
                self.strkr_role_spins["slow_axis"],
                self.strkr_role_labels["slow_axis"],
                self.strkr_role_hints["slow_axis"],
            ),
            1,
        )
        layout.addLayout(axes)
        layout.addStretch(1)
        return self._measurement_tab(settings)

    def _srkr_2d_tab(self) -> QtWidgets.QWidget:
        settings = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(settings)
        layout.setContentsMargins(9, 9, 9, 9)
        axes = QtWidgets.QHBoxLayout()
        axes.setContentsMargins(0, 0, 0, 0)
        axes.addWidget(
            self._scan2d_axis_panel(
                "Fast",
                self.srkr_2d_fast_axis_combo,
                self.srkr_2d_role_spins["fast_axis"],
                self.srkr_2d_role_labels["fast_axis"],
                self.srkr_2d_role_hints["fast_axis"],
                self.srkr_2d_wait_spin,
                self.srkr_2d_tc_button,
            ),
            1,
        )
        axes.addWidget(
            self._scan2d_axis_panel(
                "Slow",
                self.srkr_2d_slow_axis_combo,
                self.srkr_2d_role_spins["slow_axis"],
                self.srkr_2d_role_labels["slow_axis"],
                self.srkr_2d_role_hints["slow_axis"],
            ),
            1,
        )
        layout.addLayout(axes)
        layout.addStretch(1)
        return self._measurement_tab(settings)

    def _measurement_tab(self, settings: QtWidgets.QWidget) -> QtWidgets.QWidget:
        widget = QtWidgets.QWidget()
        layout = QtWidgets.QHBoxLayout(widget)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)
        layout.addWidget(settings, 2)
        side = QtWidgets.QWidget()
        side_layout = QtWidgets.QVBoxLayout(side)
        side_layout.setContentsMargins(0, 0, 0, 0)
        side_layout.setSpacing(8)
        self.measurement_side_layouts.append(side_layout)
        layout.addWidget(side, 1)
        return widget

    def _with_button(self, widget: QtWidgets.QWidget, button: QtWidgets.QPushButton) -> QtWidgets.QWidget:
        row = QtWidgets.QWidget()
        layout = QtWidgets.QHBoxLayout(row)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        button.setFixedWidth(MEASUREMENT_ROW_TRAILING_WIDTH)
        layout.addWidget(widget, 1)
        layout.addWidget(button, 0)
        return row

    def _with_hint(self, widget: QtWidgets.QWidget, hint: QtWidgets.QLabel) -> QtWidgets.QWidget:
        row = QtWidgets.QWidget()
        layout = QtWidgets.QHBoxLayout(row)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        layout.addWidget(widget, 1)
        layout.addWidget(hint, 0)
        return row

    def _output_run_widget(self) -> QtWidgets.QWidget:
        widget = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(widget)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)
        layout.addWidget(self._output_group())
        layout.addWidget(self._run_group())
        layout.addStretch(1)
        return widget

    def _attach_output_run_to_tab(self, index: int):
        if not hasattr(self, "output_run_widget") or index < 0 or index >= len(self.measurement_side_layouts):
            return
        layout = self.measurement_side_layouts[index]
        layout.insertWidget(0, self.output_run_widget)

    def _output_group(self) -> QtWidgets.QGroupBox:
        group = QtWidgets.QGroupBox("Output")
        layout = QtWidgets.QFormLayout(group)
        self.output_browse_button.setFixedWidth(OUTPUT_TRAILING_WIDTH)
        self.auto_suffix_check.setFixedWidth(OUTPUT_TRAILING_WIDTH)
        directory_row = QtWidgets.QWidget()
        directory_layout = QtWidgets.QHBoxLayout(directory_row)
        directory_layout.setContentsMargins(0, 0, 0, 0)
        directory_layout.setSpacing(6)
        directory_layout.addWidget(self.output_dir_edit, 1)
        directory_layout.addWidget(self.output_browse_button, 0)
        file_row = QtWidgets.QWidget()
        file_layout = QtWidgets.QHBoxLayout(file_row)
        file_layout.setContentsMargins(0, 0, 0, 0)
        file_layout.setSpacing(6)
        file_layout.addWidget(self.output_name_edit, 1)
        file_layout.addWidget(self.auto_suffix_check, 0)
        layout.addRow("Directory", directory_row)
        layout.addRow("File", file_row)
        return group

    def _run_group(self) -> QtWidgets.QGroupBox:
        group = QtWidgets.QGroupBox("Run")
        layout = QtWidgets.QGridLayout(group)
        layout.addWidget(self.start_button, 0, 0)
        layout.addWidget(self.stop_button, 0, 1)
        layout.addWidget(self.save_rows_button, 0, 2)
        return group

    def _plot_toolbar(self) -> QtWidgets.QWidget:
        toolbar = QtWidgets.QWidget()
        layout = QtWidgets.QHBoxLayout(toolbar)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(8)
        layout.addWidget(QtWidgets.QLabel("Plot"))
        layout.addWidget(self.signal_mode_combo, 0)
        layout.addWidget(QtWidgets.QLabel("Step"))
        layout.addWidget(self.point_label, 0)
        layout.addWidget(QtWidgets.QLabel("ETA"))
        layout.addWidget(self.eta_label, 0)
        layout.addStretch(1)
        return toolbar

    def _connect_signals(self):
        self.browse_button.clicked.connect(self.browse_config)
        self.load_button.clicked.connect(self.load_config_file)
        self.save_button.clicked.connect(self.save_config_file)
        self.connect_button.clicked.connect(self.connect_all)
        self.disconnect_button.clicked.connect(self.disconnect_all)
        self.read_status_button.clicked.connect(self.read_live_status)
        self.lockin_refresh_button.clicked.connect(self.refresh_lockin_resources)
        self.t_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.x_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.y_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.t_controller_combo.currentTextChanged.connect(self.refresh_delay_stage_choices)
        self.t_stage_combo.currentTextChanged.connect(lambda _text: self._refresh_scan_limit_hints())
        self.t_direction_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.x_controller_combo.currentTextChanged.connect(lambda _text: self.refresh_scanner_choices("x"))
        self.y_controller_combo.currentTextChanged.connect(lambda _text: self.refresh_scanner_choices("y"))
        self.x_actuator_combo.currentTextChanged.connect(lambda _text: self._refresh_scanner_scale_label("x"))
        self.y_actuator_combo.currentTextChanged.connect(lambda _text: self._refresh_scanner_scale_label("y"))
        self.x_actuator_combo.currentTextChanged.connect(lambda _text: self._refresh_scan_limit_hints())
        self.y_actuator_combo.currentTextChanged.connect(lambda _text: self._refresh_scan_limit_hints())
        self.x_scale_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.y_scale_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.x_controller_combo.currentTextChanged.connect(lambda _text: self.sync_conexagap_ports("x"))
        self.y_controller_combo.currentTextChanged.connect(lambda _text: self.sync_conexagap_ports("y"))
        self.x_port_combo.currentTextChanged.connect(lambda _text: self.sync_conexagap_ports("x"))
        self.y_port_combo.currentTextChanged.connect(lambda _text: self.sync_conexagap_ports("y"))
        self.lockin_connect_button.clicked.connect(lambda: self.connect_device("lockin", "main"))
        self.lockin_disconnect_button.clicked.connect(lambda: self.disconnect_device("lockin", "main"))
        self.t_connect_button.clicked.connect(lambda: self.connect_device("delay_stage", "t"))
        self.t_disconnect_button.clicked.connect(lambda: self.disconnect_device("delay_stage", "t"))
        self.t_initialize_button.clicked.connect(lambda: self.initialize_device("delay_stage"))
        self.x_connect_button.clicked.connect(lambda: self.connect_device("scanner", "x"))
        self.x_disconnect_button.clicked.connect(lambda: self.disconnect_device("scanner", "x"))
        self.x_initialize_button.clicked.connect(lambda: self.initialize_device("scanner", "x"))
        self.y_connect_button.clicked.connect(lambda: self.connect_device("scanner", "y"))
        self.y_disconnect_button.clicked.connect(lambda: self.disconnect_device("scanner", "y"))
        self.y_initialize_button.clicked.connect(lambda: self.initialize_device("scanner", "y"))
        self.use_current_t_button.clicked.connect(lambda: self.set_origin_from_current("t"))
        self.use_current_x_button.clicked.connect(lambda: self.set_origin_from_current("x"))
        self.use_current_y_button.clicked.connect(lambda: self.set_origin_from_current("y"))
        self.t_zero_spin.valueChanged.connect(lambda _value: self._refresh_derived_position_labels())
        self.t_zero_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.x_zero_spin.valueChanged.connect(lambda _value: self._refresh_derived_position_labels())
        self.x_zero_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.y_zero_spin.valueChanged.connect(lambda _value: self._refresh_derived_position_labels())
        self.y_zero_spin.valueChanged.connect(lambda _value: self._refresh_scan_limit_hints())
        self.srkr_axis_combo.currentTextChanged.connect(lambda _text: self._refresh_scan_limit_hints())
        self.strkr_fast_axis_combo.currentTextChanged.connect(lambda _text: self._handle_2d_axis_changed("strkr"))
        self.strkr_slow_axis_combo.currentTextChanged.connect(lambda _text: self._handle_2d_axis_changed("strkr"))
        self.srkr_2d_fast_axis_combo.currentTextChanged.connect(lambda _text: self._handle_2d_axis_changed("srkr_2d"))
        self.srkr_2d_slow_axis_combo.currentTextChanged.connect(lambda _text: self._handle_2d_axis_changed("srkr_2d"))
        self.trkr_tc_button.clicked.connect(lambda: self.use_tc_wait_time(self.trkr_wait_spin))
        self.srkr_tc_button.clicked.connect(lambda: self.use_tc_wait_time(self.srkr_wait_spin))
        self.strkr_tc_button.clicked.connect(lambda: self.use_tc_wait_time(self.strkr_wait_spin))
        self.srkr_2d_tc_button.clicked.connect(lambda: self.use_tc_wait_time(self.srkr_2d_wait_spin))
        self.move_t_button.clicked.connect(lambda: self.move_absolute("t"))
        self.move_x_button.clicked.connect(lambda: self.move_absolute("x"))
        self.move_y_button.clicked.connect(lambda: self.move_absolute("y"))
        self.t_cor_button.clicked.connect(lambda: self.move_corrected("t"))
        self.x_cor_button.clicked.connect(lambda: self.move_corrected("x"))
        self.y_cor_button.clicked.connect(lambda: self.move_corrected("y"))
        self.output_browse_button.clicked.connect(self.browse_output_dir)
        self.output_dir_edit.editingFinished.connect(self._store_current_output_settings)
        self.output_name_edit.editingFinished.connect(self._store_current_output_settings)
        self.auto_suffix_check.stateChanged.connect(lambda _state: self._store_current_output_settings())
        self.signal_mode_combo.currentTextChanged.connect(self._refresh_plot_labels)
        self.measurement_tabs.currentChanged.connect(self._handle_measurement_tab_changed)
        self.start_button.clicked.connect(self.start_measurement)
        self.stop_button.clicked.connect(self.stop_measurement)
        self.save_rows_button.clicked.connect(self.save_rows)
        self.right_panel_toggle.toggled.connect(self.toggle_right_panel)

    def _handle_measurement_tab_changed(self, index: int):
        try:
            if self._last_measurement_for_output in {"strkr", "srkr_2d"}:
                self._sync_scan2d_role_values_to_axis_ranges(self._last_measurement_for_output)
            self._store_output_settings(self._last_measurement_for_output)
            self._attach_output_run_to_tab(index)
            measurement = self._measurement_name()
            self._normalize_2d_axis_controls(measurement)
            self._load_scan2d_role_ranges(measurement)
            self._apply_output_settings(measurement)
            self._last_measurement_for_output = measurement
            self._update_curves()
            self.point_label.setText(self.point_text_by_mode.get(measurement, "-"))
            self.eta_label.setText(self.eta_text_by_mode.get(measurement, "-"))
        except Exception as e:
            self.status_label.setText("tab error")
            self.append_log(f"Tab change error: {e}")

    def _handle_2d_axis_changed(self, mode: str) -> None:
        self._sync_scan2d_role_values_to_axis_ranges(mode)
        self._normalize_2d_axis_controls(mode)
        self._load_scan2d_role_ranges(mode)
        self._refresh_scan_limit_hints()
        if mode == self._measurement_name():
            self._update_curves()

    def _normalize_2d_axis_controls(self, mode: str) -> None:
        if mode == "strkr":
            fast_combo = self.strkr_fast_axis_combo
            slow_combo = self.strkr_slow_axis_combo
        elif mode == "srkr_2d":
            fast_combo = self.srkr_2d_fast_axis_combo
            slow_combo = self.srkr_2d_slow_axis_combo
        else:
            return
        fast_axis, slow_axis = _valid_scan2d_axes(mode, fast_combo.currentText(), slow_combo.currentText())
        for combo, value in ((fast_combo, fast_axis), (slow_combo, slow_axis)):
            if combo.currentText().lower() == value:
                continue
            combo.blockSignals(True)
            combo.setCurrentText(value)
            combo.blockSignals(False)

    def _scan2d_axis_range_widgets(self, mode: str) -> dict[str, dict[str, QtWidgets.QDoubleSpinBox]]:
        if mode == "strkr":
            return self.strkr_range_spins
        if mode == "srkr_2d":
            return self.srkr_2d_range_spins
        return {}

    def _scan2d_role_spin_widgets(self, mode: str) -> dict[str, dict[str, QtWidgets.QDoubleSpinBox]]:
        if mode == "strkr":
            return self.strkr_role_spins
        if mode == "srkr_2d":
            return self.srkr_2d_role_spins
        return {}

    def _scan2d_role_label_widgets(self, mode: str) -> dict[str, dict[str, QtWidgets.QLabel]]:
        if mode == "strkr":
            return self.strkr_role_labels
        if mode == "srkr_2d":
            return self.srkr_2d_role_labels
        return {}

    def _scan2d_role_hint_widgets(self, mode: str) -> dict[str, dict[str, QtWidgets.QLabel]]:
        if mode == "strkr":
            return self.strkr_role_hints
        if mode == "srkr_2d":
            return self.srkr_2d_role_hints
        return {}

    def _scan2d_control_axes(self, mode: str) -> tuple[str, str]:
        if mode == "strkr":
            return _valid_scan2d_axes(mode, self.strkr_fast_axis_combo.currentText(), self.strkr_slow_axis_combo.currentText())
        if mode == "srkr_2d":
            return _valid_scan2d_axes(
                mode,
                self.srkr_2d_fast_axis_combo.currentText(),
                self.srkr_2d_slow_axis_combo.currentText(),
            )
        return ("x", "y")

    def _sync_scan2d_role_values_to_axis_ranges(self, mode: str) -> None:
        axis_ranges = self._scan2d_axis_range_widgets(mode)
        role_spins = self._scan2d_role_spin_widgets(mode)
        role_axes = self.scan2d_role_axes.get(mode, {})
        for role in SCAN2D_ROLES:
            axis = role_axes.get(role)
            if axis not in axis_ranges or role not in role_spins:
                continue
            for key in RANGE_KEYS:
                axis_ranges[axis][key].setValue(role_spins[role][key].value())

    def _load_scan2d_role_ranges(self, mode: str) -> None:
        if mode not in {"strkr", "srkr_2d"}:
            return
        axis_ranges = self._scan2d_axis_range_widgets(mode)
        role_spins = self._scan2d_role_spin_widgets(mode)
        role_labels = self._scan2d_role_label_widgets(mode)
        fast_axis, slow_axis = self._scan2d_control_axes(mode)
        for role, axis in (("fast_axis", fast_axis), ("slow_axis", slow_axis)):
            self.scan2d_role_axes[mode][role] = axis
            unit = _axis_unit(axis)
            role_labels[role]["min"].setText(f"min cor ({unit})")
            role_labels[role]["max"].setText(f"max cor ({unit})")
            role_labels[role]["step"].setText(f"step ({unit})")
            for key in RANGE_KEYS:
                role_spins[role][key].blockSignals(True)
                role_spins[role][key].setValue(axis_ranges[axis][key].value())
                role_spins[role][key].blockSignals(False)

    def _install_log_streams(self):
        self._stdout_original = sys.stdout
        self._stderr_original = sys.stderr
        self._stdout_stream = GuiLogStream(sys.stdout)
        self._stderr_stream = GuiLogStream(sys.stderr)
        self._stdout_stream.text_ready.connect(self.append_log)
        self._stderr_stream.text_ready.connect(lambda text: self.append_log(f"stderr: {text}"))
        sys.stdout = self._stdout_stream
        sys.stderr = self._stderr_stream

    def _restore_log_streams(self):
        if getattr(self, "_stdout_original", None) is not None:
            sys.stdout = self._stdout_original
            self._stdout_original = None
        if getattr(self, "_stderr_original", None) is not None:
            sys.stderr = self._stderr_original
            self._stderr_original = None

    def toggle_right_panel(self, collapsed: bool):
        self.right_content.setVisible(not collapsed)
        self.right_panel_toggle.setText("<" if collapsed else ">")
        self._apply_panel_sizes()

    def _apply_panel_sizes(self):
        if not hasattr(self, "left_panel"):
            return
        available_width = max(self.centralWidget().width(), self.width())
        available_height = max(self.centralWidget().height(), self.height())
        left_width = max(384, int(available_width * 0.20))
        self.left_panel.setMinimumWidth(384)
        self.left_panel.setMaximumWidth(left_width)
        self.left_panel.setFixedWidth(left_width)

        top_height = max(216, int(available_height * 0.20))
        self.center_top_widget.setMinimumHeight(216)
        self.center_top_widget.setMaximumHeight(top_height)
        self.center_top_widget.setFixedHeight(top_height)
        self.log.setMinimumHeight(216)
        self.log.setMaximumHeight(top_height)
        self.log.setFixedHeight(top_height)

        if self.right_panel_toggle.isChecked():
            self.right_panel.setFixedWidth(self.right_panel_toggle.sizeHint().width() + 8)
        else:
            self.right_panel.setFixedWidth(max(1, int(available_width * 0.20)))

    def append_log(self, text: str):
        self.log.appendPlainText(text)

    def _selected_text(self, combo: QtWidgets.QComboBox) -> str:
        return combo.currentText().strip()

    def _replace_combo_preserving_current(self, combo: QtWidgets.QComboBox, items: list[str]):
        current = self._selected_text(combo)
        combo.blockSignals(True)
        combo.clear()
        combo.addItems(items)
        if current:
            _set_combo_text(combo, current)
        combo.blockSignals(False)

    def refresh_lockin_resources(self):
        self.refresh_all_ports()

    def refresh_all_ports(self):
        if self.resource_thread is not None:
            return
        self.resource_thread = QtCore.QThread(self)
        self.resource_worker = ResourceListWorker()
        self.resource_worker.moveToThread(self.resource_thread)
        self.resource_thread.started.connect(self.resource_worker.run)
        self.resource_worker.resources_ready.connect(self.handle_resource_list_ready)
        self.resource_worker.error_occurred.connect(self.handle_resource_list_error)
        self.resource_worker.finished.connect(self.resource_thread.quit)
        self.resource_worker.finished.connect(self.resource_worker.deleteLater)
        self.resource_thread.finished.connect(self.resource_thread.deleteLater)
        self.resource_thread.finished.connect(self.cleanup_resource_thread)
        self.resource_thread.start()

    def handle_resource_list_ready(self, visa_resources: object, serial_ports: object):
        visa_items = [str(item) for item in visa_resources] if isinstance(visa_resources, list) else []
        serial_items = [str(item) for item in serial_ports] if isinstance(serial_ports, list) else []
        self._replace_combo_preserving_current(self.lockin_resource_combo, visa_items)
        self._replace_combo_preserving_current(self.t_port_combo, serial_items)
        self._replace_combo_preserving_current(self.x_port_combo, serial_items)
        self._replace_combo_preserving_current(self.y_port_combo, serial_items)
        self.sync_conexagap_ports("x")

    def handle_resource_list_error(self, message: str):
        self.append_log(f"Could not refresh hardware resources: {message}")

    def cleanup_resource_thread(self):
        self.resource_thread = None
        self.resource_worker = None

    def refresh_delay_stage_choices(self):
        controller = self._selected_text(self.t_controller_combo)
        _replace_combo_items(self.t_stage_combo, list_stages(controller), allow_custom=False)

    def refresh_scanner_choices(self, axis: str):
        controller_combo = self.x_controller_combo if axis == "x" else self.y_controller_combo
        actuator_combo = self.x_actuator_combo if axis == "x" else self.y_actuator_combo
        _replace_combo_items(actuator_combo, self._actuators_for_controller(controller_combo.currentText()), allow_custom=False)
        if controller_combo.currentText().strip().upper() == "CONEXAGAP":
            (self.x_axis_spin if axis == "x" else self.y_axis_spin).setRange(1, 2)
            self.x_axis_spin.setValue(1)
            self.y_axis_spin.setValue(2)
        else:
            (self.x_axis_spin if axis == "x" else self.y_axis_spin).setRange(1, 8)
        self._refresh_scanner_scale_label(axis)
        self._refresh_scan_limit_hints()

    def _refresh_scanner_scale_label(self, axis: str):
        actuator_combo = self.x_actuator_combo if axis == "x" else self.y_actuator_combo
        label = self.x_scale_label if axis == "x" else self.y_scale_label
        label.setText(scanner_scale_label_for_actuator(self._selected_text(actuator_combo)))

    def _refresh_scanner_scale_labels(self):
        self._refresh_scanner_scale_label("x")
        self._refresh_scanner_scale_label("y")

    def sync_conexagap_ports(self, source_axis: str):
        x_is_agap = self.x_controller_combo.currentText().strip().upper() == "CONEXAGAP"
        y_is_agap = self.y_controller_combo.currentText().strip().upper() == "CONEXAGAP"
        if not (x_is_agap and y_is_agap):
            return
        source = self.x_port_combo if source_axis == "x" else self.y_port_combo
        target = self.y_port_combo if source_axis == "x" else self.x_port_combo
        text = source.currentText().strip()
        if text and target.currentText().strip() != text:
            target.blockSignals(True)
            _set_combo_text(target, text)
            target.blockSignals(False)

    def _lockin_config(self) -> dict[str, Any]:
        return {
            "model": self._selected_text(self.lockin_model_combo) or "SR7265",
            "resource": self._selected_text(self.lockin_resource_combo),
        }

    def _store_output_settings(self, measurement: str):
        self.output_settings_by_mode[measurement] = _output_settings(self.output_dir_edit, self.output_name_edit, self.auto_suffix_check)

    def _store_current_output_settings(self):
        if hasattr(self, "measurement_tabs"):
            self._store_output_settings(self._measurement_name())

    def _default_output_filename(self, measurement: str) -> str:
        return {
            "signal_monitor": "signal_monitor_run",
            "trkr": "trkr_run",
            "srkr": "srkr_run",
            "strkr": "strkr_run",
            "srkr_2d": "srkr_2d_run",
        }[measurement]

    def _measurement_output_from_config(self, measurement: str) -> dict[str, Any]:
        settings = self.config.get("measurements", {}).get(measurement, {})
        output = settings.get("output", {}) if isinstance(settings, dict) else {}
        return output if isinstance(output, dict) else {}

    def _apply_output_settings(self, measurement: str):
        settings = self.output_settings_by_mode.get(measurement)
        if settings is None:
            output = self._measurement_output_from_config(measurement)
            settings = output_settings_from_fields(
                output_dir=output.get("dir", output.get("output_dir", self.output_dir_edit.text())),
                filename=output.get("filename", self._default_output_filename(measurement)),
                auto_timestamp_suffix=bool(output.get("auto_timestamp_suffix", self.auto_suffix_check.isChecked())),
                default_dir=Path.cwd(),
                default_filename=self._default_output_filename(measurement),
            )
            self.output_settings_by_mode[measurement] = settings
        self.output_dir_edit.setText(str(settings["output_dir"]))
        self.output_name_edit.setText(str(settings["filename"]))
        self.auto_suffix_check.setChecked(bool(settings["auto_timestamp_suffix"]))

    def _delay_stage_config(self) -> dict[str, Any]:
        return {
            "controller": self._selected_text(self.t_controller_combo) or "SHOT302GS",
            "stage": normalize_delay_stage_name(self._selected_text(self.t_stage_combo)) or self._selected_text(self.t_stage_combo),
            "port": self._selected_text(self.t_port_combo),
            "direction": self.t_direction_spin.value(),
        }

    def _scanner_config(self, axis: str) -> dict[str, Any]:
        if axis == "x":
            controller = self.x_controller_combo
            actuator = self.x_actuator_combo
            axis_spin = self.x_axis_spin
            scale_spin = self.x_scale_spin
            port = self.x_port_combo
            source = self.config.get("instruments", {}).get("scanner", {}).get("x")
        else:
            controller = self.y_controller_combo
            actuator = self.y_actuator_combo
            axis_spin = self.y_axis_spin
            scale_spin = self.y_scale_spin
            port = self.y_port_combo
            source = self.config.get("instruments", {}).get("scanner", {}).get("y")
        config = {
            "controller": self._selected_text(controller) or "CONEXCC",
            "actuator": self._selected_text(actuator) or "TRA12CC",
            "port": self._selected_text(port),
            "axis": axis_spin.value(),
            "sample_um_per_unit": scale_spin.value(),
        }
        if isinstance(source, dict):
            merged = dict(source)
            merged.update(config)
            return merged
        return config

    def _delay_stage_hint_values(self) -> tuple[float | None, float | None, float | None]:
        microstep_division = None
        if self.experiment is not None and "t" in self.experiment.session.delay_stages:
            try:
                stage = self.experiment.session.delay_stages["t"]._stage
                microstep_division = stage.get_cached_microstep_division(axis=stage.axis)
            except Exception:
                microstep_division = None
        limits = delay_stage_scan_limits(
            stage=self._selected_text(self.t_stage_combo),
            direction=self.t_direction_spin.value(),
            t_zero_ps=self.t_zero_spin.value(),
            microstep_division=microstep_division,
        )
        return limits.minimum, limits.maximum, limits.minimum_step

    def _scanner_hint_values(self, axis: str) -> tuple[float | None, float | None, float | None]:
        config = self._scanner_config(axis)
        zero_um = self.x_zero_spin.value() if axis == "x" else self.y_zero_spin.value()
        limits = scanner_scan_limits(
            actuator=config.get("actuator"),
            sample_um_per_unit=float(config.get("sample_um_per_unit", 1.0)),
            zero_um=zero_um,
        )
        return limits.minimum, limits.maximum, limits.minimum_step

    def _axis_hint_values(self, axis: str) -> tuple[float | None, float | None, float | None]:
        return self._delay_stage_hint_values() if axis == "t" else self._scanner_hint_values(axis)

    def _refresh_scan2d_role_hints(self, mode: str) -> None:
        if mode not in {"strkr", "srkr_2d"}:
            return
        hints = self._scan2d_role_hint_widgets(mode)
        for role in SCAN2D_ROLES:
            axis = self.scan2d_role_axes.get(mode, {}).get(role, "x")
            if axis not in {"t", "x", "y"}:
                axis = "x"
            low, high, step = self._axis_hint_values(axis)
            unit = _axis_unit(axis)
            hints[role]["min"].setText(f"min > {_fmt_bound(low, unit)}")
            hints[role]["max"].setText(f"max < {_fmt_bound(high, unit)}")
            hints[role]["step"].setText(f"step > {_fmt_bound(step, unit)}")

    def _refresh_scan_limit_hints(self):
        trkr_low, trkr_high, trkr_step = self._delay_stage_hint_values()
        self.trkr_min_hint.setText(f"min > {_fmt_bound(trkr_low, 'ps')}")
        self.trkr_max_hint.setText(f"max < {_fmt_bound(trkr_high, 'ps')}")
        self.trkr_step_hint.setText(f"step > {_fmt_bound(trkr_step, 'ps')}")

        axis = self.srkr_axis_combo.currentText().strip().lower() or "x"
        srkr_low, srkr_high, srkr_step = self._scanner_hint_values(axis if axis in {"x", "y"} else "x")
        self.srkr_min_hint.setText(f"min > {_fmt_bound(srkr_low, 'um')}")
        self.srkr_max_hint.setText(f"max < {_fmt_bound(srkr_high, 'um')}")
        self.srkr_step_hint.setText(f"step > {_fmt_bound(srkr_step, 'um')}")
        self._refresh_scan2d_role_hints("strkr")
        self._refresh_scan2d_role_hints("srkr_2d")

    def browse_config(self):
        start_dir = self.config_path.text().strip() or str(DEFAULT_CONFIG_PATH)
        path, _ = QtWidgets.QFileDialog.getOpenFileName(self, "Load Config", start_dir, "JSON Files (*.json)")
        if path:
            self.config_path.setText(path)

    def load_config_file(self):
        try:
            resolution = resolve_config_path(self.config_path.text().strip() or None)
            if resolution.path is None:
                raise ValueError("Choose a config path before loading.")
            self.config = load_config(resolution.path)
            write_last_config_path(resolution.path)
            self.config_path.setText(str(resolution.path))
            self._load_config_into_fields(self.config)
            if self.experiment is not None:
                self.experiment.config = self._runtime_config()
            self.append_log(f"Loaded config ({resolution.source}): {resolution.path}")
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Config Error", str(e))

    def save_config_file(self):
        try:
            path_text = self.config_path.text().strip()
            if not path_text:
                raise ValueError("Choose a config path before saving.")
            path = Path(path_text)
            save_config(self._runtime_config(), path)
            write_last_config_path(path)
            self.append_log(f"Saved config: {path}")
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Config Error", str(e))

    def _load_config_into_fields(self, config: dict[str, Any]):
        instruments = config.get("instruments", {})
        lockin = instruments.get("lockin", {}).get("main", {})
        _set_combo_text(self.lockin_model_combo, str(lockin.get("model", self.lockin_model_combo.currentText())))
        _set_combo_text(self.lockin_resource_combo, str(lockin.get("resource", self.lockin_resource_combo.currentText())))

        delay_stage = instruments.get("delay_stage", {}).get("t", {})
        _set_combo_text(self.t_controller_combo, str(delay_stage.get("controller", self.t_controller_combo.currentText())))
        _set_combo_text(self.t_stage_combo, str(delay_stage.get("stage", self.t_stage_combo.currentText())))
        _set_combo_text(self.t_port_combo, str(delay_stage.get("port", self.t_port_combo.currentText())))
        self.t_direction_spin.setValue(int(delay_stage.get("direction", self.t_direction_spin.value())))

        x_scanner = instruments.get("scanner", {}).get("x", {})
        y_scanner = instruments.get("scanner", {}).get("y", {})
        _set_combo_text(self.x_controller_combo, str(x_scanner.get("controller", self.x_controller_combo.currentText())))
        _set_combo_text(self.x_actuator_combo, str(x_scanner.get("actuator", self.x_actuator_combo.currentText())))
        _set_combo_text(self.x_port_combo, str(x_scanner.get("port", self.x_port_combo.currentText())))
        self.x_axis_spin.setValue(scanner_axis_spin_value(x_scanner.get("axis", self.x_axis_spin.value())))
        self.x_scale_spin.setValue(float(x_scanner.get("sample_um_per_unit", self.x_scale_spin.value())))
        _set_combo_text(self.y_controller_combo, str(y_scanner.get("controller", self.y_controller_combo.currentText())))
        _set_combo_text(self.y_actuator_combo, str(y_scanner.get("actuator", self.y_actuator_combo.currentText())))
        _set_combo_text(self.y_port_combo, str(y_scanner.get("port", self.y_port_combo.currentText())))
        self.y_axis_spin.setValue(scanner_axis_spin_value(y_scanner.get("axis", self.y_axis_spin.value())))
        self.y_scale_spin.setValue(float(y_scanner.get("sample_um_per_unit", self.y_scale_spin.value())))
        self._refresh_scanner_scale_labels()

        measurements = config.get("measurements", {})
        move_abs = measurements.get("move_abs", {})
        zero = move_abs.get("zero", {}) if isinstance(move_abs, dict) else {}
        self.t_zero_spin.setValue(float(zero.get("t_ps", self.t_zero_spin.value())))
        self.x_zero_spin.setValue(float(zero.get("x_um", self.x_zero_spin.value())))
        self.y_zero_spin.setValue(float(zero.get("y_um", self.y_zero_spin.value())))
        targets = move_abs.get("targets", {}) if isinstance(move_abs, dict) else {}
        self.move_t_spin.setValue(float(targets.get("t", self.move_t_spin.value())))
        self.move_x_spin.setValue(float(targets.get("x", self.move_x_spin.value())))
        self.move_y_spin.setValue(float(targets.get("y", self.move_y_spin.value())))

        signal = measurements.get("signal_monitor", {})
        self.signal_interval_spin.setValue(float(signal.get("interval_s", self.signal_interval_spin.value())))
        self.signal_points_spin.setValue(int(signal.get("n_points", self.signal_points_spin.value())))
        self.output_settings_by_mode["signal_monitor"] = self._settings_from_measurement_output("signal_monitor", signal)

        trkr = measurements.get("trkr", {})
        trkr_scan = trkr.get("scan", {}) if isinstance(trkr.get("scan", {}), dict) else {}
        self.trkr_min_spin.setValue(float(trkr_scan.get("min", self.trkr_min_spin.value())))
        self.trkr_max_spin.setValue(float(trkr_scan.get("max", self.trkr_max_spin.value())))
        self.trkr_step_spin.setValue(float(trkr_scan.get("step", self.trkr_step_spin.value())))
        self.trkr_wait_spin.setValue(float(trkr.get("wait_s", self.trkr_wait_spin.value())))
        self.trkr_return_check.setChecked(bool(trkr.get("return_to_zero", True)))
        self.output_settings_by_mode["trkr"] = self._settings_from_measurement_output("trkr", trkr)

        srkr = measurements.get("srkr", {})
        srkr_scan = srkr.get("scan", {}) if isinstance(srkr.get("scan", {}), dict) else {}
        self.srkr_axis_combo.setCurrentText(str(srkr_scan.get("axis", self.srkr_axis_combo.currentText())).lower())
        self.srkr_min_spin.setValue(float(srkr_scan.get("min", self.srkr_min_spin.value())))
        self.srkr_max_spin.setValue(float(srkr_scan.get("max", self.srkr_max_spin.value())))
        self.srkr_step_spin.setValue(float(srkr_scan.get("step", self.srkr_step_spin.value())))
        self.srkr_wait_spin.setValue(float(srkr.get("wait_s", self.srkr_wait_spin.value())))
        self.srkr_return_check.setChecked(bool(srkr.get("return_to_zero", True)))
        self.output_settings_by_mode["srkr"] = self._settings_from_measurement_output("srkr", srkr)

        strkr = measurements.get("strkr", {})
        strkr_scan = strkr.get("scan", {}) if isinstance(strkr.get("scan", {}), dict) else {}
        self.strkr_fast_axis_combo.setCurrentText(str(strkr_scan.get("fast_axis", self.strkr_fast_axis_combo.currentText())).lower())
        self.strkr_slow_axis_combo.setCurrentText(str(strkr_scan.get("slow_axis", self.strkr_slow_axis_combo.currentText())).lower())
        self._load_axis_ranges(self.strkr_range_spins, strkr_scan.get("ranges", {}))
        self._normalize_2d_axis_controls("strkr")
        self._load_scan2d_role_ranges("strkr")
        self.strkr_wait_spin.setValue(float(strkr.get("wait_s", self.strkr_wait_spin.value())))
        self.output_settings_by_mode["strkr"] = self._settings_from_measurement_output("strkr", strkr)

        srkr_2d = measurements.get("srkr_2d", {})
        srkr_2d_scan = srkr_2d.get("scan", {}) if isinstance(srkr_2d.get("scan", {}), dict) else {}
        self.srkr_2d_fast_axis_combo.setCurrentText(
            str(srkr_2d_scan.get("fast_axis", self.srkr_2d_fast_axis_combo.currentText())).lower()
        )
        self.srkr_2d_slow_axis_combo.setCurrentText(
            str(srkr_2d_scan.get("slow_axis", self.srkr_2d_slow_axis_combo.currentText())).lower()
        )
        self._load_axis_ranges(self.srkr_2d_range_spins, srkr_2d_scan.get("ranges", {}))
        self._normalize_2d_axis_controls("srkr_2d")
        self._load_scan2d_role_ranges("srkr_2d")
        self.srkr_2d_wait_spin.setValue(float(srkr_2d.get("wait_s", self.srkr_2d_wait_spin.value())))
        self.output_settings_by_mode["srkr_2d"] = self._settings_from_measurement_output("srkr_2d", srkr_2d)
        self._last_measurement_for_output = self._measurement_name()
        self._apply_output_settings(self._last_measurement_for_output)

    def _settings_from_measurement_output(self, measurement: str, measurement_settings: dict[str, Any]) -> dict[str, Any]:
        output = measurement_settings.get("output", {}) if isinstance(measurement_settings.get("output", {}), dict) else {}
        return output_settings_from_fields(
            output_dir=output.get("dir", output.get("output_dir", Path.cwd())),
            filename=output.get("filename", self._default_output_filename(measurement)),
            auto_timestamp_suffix=bool(output.get("auto_timestamp_suffix", True)),
            default_dir=Path.cwd(),
            default_filename=self._default_output_filename(measurement),
        )

    def _load_axis_ranges(self, widgets: dict[str, dict[str, QtWidgets.QDoubleSpinBox]], ranges: dict[str, Any]):
        for axis, axis_widgets in widgets.items():
            axis_range = ranges.get(axis, {}) if isinstance(ranges, dict) else {}
            axis_range = axis_range if isinstance(axis_range, dict) else {}
            for key, widget in axis_widgets.items():
                widget.setValue(float(axis_range.get(key, widget.value())))

    def _axis_ranges_payload(self, widgets: dict[str, dict[str, QtWidgets.QDoubleSpinBox]]) -> dict[str, dict[str, float]]:
        return {
            axis: {
                "min": axis_widgets["min"].value(),
                "max": axis_widgets["max"].value(),
                "step": axis_widgets["step"].value(),
            }
            for axis, axis_widgets in widgets.items()
        }

    def _return_roles_payload(self) -> dict[str, bool]:
        return {"fast_axis": True, "slow_axis": True}

    def _runtime_config(self) -> dict[str, Any]:
        self._store_current_output_settings()
        self._sync_scan2d_role_values_to_axis_ranges("strkr")
        self._sync_scan2d_role_values_to_axis_ranges("srkr_2d")
        config = deepcopy(self.config)
        instruments = config.setdefault("instruments", {})
        instruments["lockin"] = {"main": self._lockin_config()}
        instruments["delay_stage"] = {"t": self._delay_stage_config()}
        instruments["scanner"] = {"x": self._scanner_config("x"), "y": self._scanner_config("y")}

        measurements = config.setdefault("measurements", {})
        move_abs = measurements.setdefault("move_abs", {})
        move_abs["coordinate"] = "measurement"
        move_abs["zero"] = {
            "t_ps": self.t_zero_spin.value(),
            "x_um": self.x_zero_spin.value(),
            "y_um": self.y_zero_spin.value(),
        }
        move_abs["targets"] = {
            "t": self.move_t_spin.value(),
            "x": self.move_x_spin.value(),
            "y": self.move_y_spin.value(),
            "t_cor": self.t_cor_spin.value(),
            "x_cor": self.x_cor_spin.value(),
            "y_cor": self.y_cor_spin.value(),
        }

        output_by_mode = {
            measurement: self.output_settings_by_mode.get(
                measurement,
                output_settings_from_fields(
                    output_dir=Path.cwd(),
                    filename=self._default_output_filename(measurement),
                    auto_timestamp_suffix=True,
                    default_dir=Path.cwd(),
                    default_filename=self._default_output_filename(measurement),
                ),
            )
            for measurement in ("signal_monitor", "trkr", "srkr", "strkr", "srkr_2d")
        }
        measurements["signal_monitor"] = {
            **measurements.get("signal_monitor", {}),
            "interval_s": self.signal_interval_spin.value(),
            "n_points": self.signal_points_spin.value(),
            "output": {
                "dir": output_by_mode["signal_monitor"]["output_dir"],
                "filename": output_by_mode["signal_monitor"]["filename"],
                "auto_timestamp_suffix": output_by_mode["signal_monitor"]["auto_timestamp_suffix"],
            },
        }
        measurements["trkr"] = {
            **measurements.get("trkr", {}),
            "coordinate": "measurement",
            "scan": {"min": self.trkr_min_spin.value(), "max": self.trkr_max_spin.value(), "step": self.trkr_step_spin.value()},
            "wait_s": self.trkr_wait_spin.value(),
            "return_to_zero": self.trkr_return_check.isChecked(),
            "output": {
                "dir": output_by_mode["trkr"]["output_dir"],
                "filename": output_by_mode["trkr"]["filename"],
                "auto_timestamp_suffix": output_by_mode["trkr"]["auto_timestamp_suffix"],
            },
        }
        measurements["srkr"] = {
            **measurements.get("srkr", {}),
            "coordinate": "measurement",
            "scan": {
                "axis": self.srkr_axis_combo.currentText().lower(),
                "min": self.srkr_min_spin.value(),
                "max": self.srkr_max_spin.value(),
                "step": self.srkr_step_spin.value(),
            },
            "wait_s": self.srkr_wait_spin.value(),
            "return_to_zero": self.srkr_return_check.isChecked(),
            "output": {
                "dir": output_by_mode["srkr"]["output_dir"],
                "filename": output_by_mode["srkr"]["filename"],
                "auto_timestamp_suffix": output_by_mode["srkr"]["auto_timestamp_suffix"],
            },
        }
        measurements["strkr"] = {
            **measurements.get("strkr", {}),
            "scan": {
                "fast_axis": self.strkr_fast_axis_combo.currentText().lower(),
                "slow_axis": self.strkr_slow_axis_combo.currentText().lower(),
                "ranges": self._axis_ranges_payload(self.strkr_range_spins),
            },
            "wait_s": self.strkr_wait_spin.value(),
            "return_to_zero": self._return_roles_payload(),
            "output": {
                "dir": output_by_mode["strkr"]["output_dir"],
                "filename": output_by_mode["strkr"]["filename"],
                "auto_timestamp_suffix": output_by_mode["strkr"]["auto_timestamp_suffix"],
            },
        }
        measurements["srkr_2d"] = {
            **measurements.get("srkr_2d", {}),
            "scan": {
                "fast_axis": self.srkr_2d_fast_axis_combo.currentText().lower(),
                "slow_axis": self.srkr_2d_slow_axis_combo.currentText().lower(),
                "ranges": self._axis_ranges_payload(self.srkr_2d_range_spins),
            },
            "wait_s": self.srkr_2d_wait_spin.value(),
            "return_to_zero": self._return_roles_payload(),
            "output": {
                "dir": output_by_mode["srkr_2d"]["output_dir"],
                "filename": output_by_mode["srkr_2d"]["filename"],
                "auto_timestamp_suffix": output_by_mode["srkr_2d"]["auto_timestamp_suffix"],
            },
        }
        return config

    def browse_output_dir(self):
        path = QtWidgets.QFileDialog.getExistingDirectory(self, "Output Directory", self.output_dir_edit.text() or str(Path.cwd()))
        if path:
            self.output_dir_edit.setText(path)

    def connect_all(self):
        self._start_device_command("connect_all", label="Connect all")

    def _start_device_command(
        self,
        command: str,
        *,
        label: str,
        kind: str | None = None,
        key: str | None = None,
        axis: str | None = None,
        ref: str | None = None,
        multiplier: float = 4.0,
        settings: dict[str, Any] | None = None,
        allow_during_shutdown: bool = False,
    ):
        if self.thread is not None and not allow_during_shutdown:
            QtWidgets.QMessageBox.warning(self, "Device Error", "Stop the running measurement first.")
            return
        if self.device_command_active or self.move_thread is not None:
            return
        try:
            self._ensure_experiment()
            self._ensure_device_worker()
            request = {
                "command": command,
                "kind": kind,
                "key": key,
                "axis": axis,
                "ref": ref,
                "multiplier": multiplier,
                "settings": settings or {},
            }
            self.device_command_active = True
            self._set_device_initializing(True)
            self.append_log(f"{label} started.")
            self.device_command_requested.emit(request)
        except Exception as e:
            self.cleanup_device_command()
            self.status_label.setText("error")
            QtWidgets.QMessageBox.critical(self, "Device Error", str(e))

    def _ensure_experiment(self) -> Experiment:
        if self.experiment is None:
            self.experiment = Experiment(self._runtime_config(), auto_connect=False)
        else:
            self.experiment.config = self._runtime_config()
        return self.experiment

    def _ensure_device_worker(self) -> DeviceCommandWorker:
        experiment = self._ensure_experiment()
        if self.device_worker is not None:
            self.device_worker.experiment = experiment
            return self.device_worker
        self.device_thread = QtCore.QThread(self)
        self.device_worker = DeviceCommandWorker(experiment=experiment)
        self.device_worker.moveToThread(self.device_thread)
        self.device_command_requested.connect(self.device_worker.run_command)
        self.device_worker.status_changed.connect(self.handle_device_status)
        self.device_worker.finished.connect(self.handle_device_command_finished)
        self.device_worker.error_occurred.connect(self.handle_device_error)
        self.device_thread.finished.connect(self.device_worker.deleteLater)
        self.device_thread.finished.connect(self.device_thread.deleteLater)
        self.device_thread.finished.connect(self.cleanup_device_thread)
        self.device_thread.start()
        return self.device_worker

    def _ensure_live_worker(self) -> LiveStatusWorker:
        experiment = self._ensure_experiment()
        if self.live_worker is not None:
            return self.live_worker
        self.live_thread = QtCore.QThread(self)
        self.live_worker = LiveStatusWorker(experiment=experiment)
        self.live_worker.moveToThread(self.live_thread)
        self.live_worker.live_status_ready.connect(self.handle_live_status_ready)
        self.live_worker.lockin_status_ready.connect(self.handle_lockin_status_ready)
        self.live_worker.error_occurred.connect(self.handle_live_status_error)
        self.live_thread.finished.connect(self.live_worker.deleteLater)
        self.live_thread.finished.connect(self.live_thread.deleteLater)
        self.live_thread.finished.connect(self.cleanup_live_thread)
        self.live_thread.start()
        return self.live_worker

    def _invoke_live_worker(self, slot_name: str):
        worker = self._ensure_live_worker()
        QtCore.QMetaObject.invokeMethod(worker, slot_name, QtCore.Qt.ConnectionType.QueuedConnection)

    def _missing_required_devices(self, measurement: str, axis: str | None = None) -> list[str]:
        if self.experiment is None:
            experiment = Experiment(self._runtime_config(), auto_connect=False)
        else:
            self.experiment.config = self._runtime_config()
            experiment = self.experiment
        fast_axis = None
        slow_axis = None
        if measurement == "strkr":
            fast_axis = self.strkr_fast_axis_combo.currentText().lower()
            slow_axis = self.strkr_slow_axis_combo.currentText().lower()
        elif measurement == "srkr_2d":
            fast_axis = self.srkr_2d_fast_axis_combo.currentText().lower()
            slow_axis = self.srkr_2d_slow_axis_combo.currentText().lower()
        return experiment.missing_devices(
            measurement,
            axis=axis or self.srkr_axis_combo.currentText().lower(),
            fast_axis=fast_axis,
            slow_axis=slow_axis,
        )

    def connect_device(self, kind: str, key: str):
        self._start_device_command(
            "connect_device",
            label=f"Connect {kind}.{key}",
            kind=kind,
            key=key,
        )

    def disconnect_device(self, kind: str, key: str):
        self._start_device_command(
            "disconnect_device",
            label=f"Disconnect {kind}.{key}",
            kind=kind,
            key=key,
        )

    def disconnect_all(self):
        self._start_device_command("disconnect_all", label="Disconnect all")

    def initialize_device(self, kind: str, axis: str | None = None):
        if kind == "delay_stage":
            self._start_device_command("initialize_delay_stage", label="Initialize delay stage", kind=kind, axis="t")
            return
        if kind == "scanner" and axis in {"x", "y"}:
            self._start_device_command("initialize_scanner", label=f"Initialize scanner {axis}", kind=kind, axis=axis)
            return
        QtWidgets.QMessageBox.warning(self, "Initialize Error", f"Unsupported initialize target: {kind} {axis or ''}".strip())

    def handle_device_status(self, status: str):
        self.status_label.setText(status)
        self.append_log(status)

    def handle_device_command_finished(self, result: object):
        try:
            info = result if isinstance(result, dict) else {}
            command = info.get("command")
            ref = info.get("ref")
            if command == "connect_all":
                self.status_label.setText("connected")
                self.append_log("Connected all configured devices.")
                self._request_full_live_status()
                return
            if command == "connect_device":
                self.status_label.setText(f"{ref} connected")
                self.append_log(f"Connected {ref}.")
                self._request_full_live_status()
                return
            if command == "disconnect_all":
                self.status_label.setText("disconnected")
                self.append_log("Disconnected.")
                self._request_full_live_status()
                return
            if command == "shutdown_disconnect_all":
                self.status_label.setText("disconnected")
                self.append_log("Disconnected for shutdown.")
                self._shutdown_complete = True
                QtCore.QTimer.singleShot(0, self.close)
                return
            if command == "disconnect_device":
                self.status_label.setText(f"{ref} disconnected")
                self.append_log(f"Disconnected {ref}.")
                self._request_full_live_status()
                return
            if command in {"initialize_delay_stage", "initialize_scanner"}:
                self.handle_device_initialized(info)
                return
            if command == "lockin_wait_time":
                wait_s = float(info.get("wait_s", 0.0))
                if self.pending_wait_spin is not None:
                    self.pending_wait_spin.setValue(wait_s)
                self.status_label.setText("lock-in wait read")
                self.append_log(f"Set wait time to TC*4 = {wait_s:.3g} s.")
                return
            if command == "set_lockin_settings":
                settings = info.get("settings")
                if isinstance(settings, dict):
                    self._apply_lockin_settings(settings)
                self.status_label.setText("lock-in settings applied")
                self.append_log("Applied lock-in settings.")
                self._request_lockin_live_status()
                return
            self.append_log(f"Device command finished: {command}")
        finally:
            self.cleanup_device_command()

    def handle_device_initialized(self, result: object):
        info = result if isinstance(result, dict) else {}
        kind = info.get("kind")
        axis = info.get("axis")
        label = "delay stage" if kind == "delay_stage" else f"scanner {axis}"
        self.status_label.setText(f"{label} initialized")
        self.append_log(f"Initialized {label}.")
        try:
            if self.experiment is not None:
                self._request_full_live_status()
        except Exception as e:
            self.append_log(f"Could not refresh initialized position: {e}")

    def handle_device_error(self, message: str):
        self.status_label.setText("device error")
        self.append_log(f"Device command error: {message}")
        self.cleanup_device_command()
        if self._shutdown_requested:
            self._shutdown_complete = True
            QtCore.QTimer.singleShot(0, self.close)
            return
        QtWidgets.QMessageBox.warning(self, "Device Error", message)

    def cleanup_device_thread(self):
        self.device_thread = None
        self.device_worker = None

    def cleanup_device_command(self):
        self.device_command_active = False
        self.pending_wait_spin = None
        self._set_device_initializing(False)

    def cleanup_live_thread(self):
        self.live_thread = None
        self.live_worker = None

    def read_live_status(self):
        self._ensure_experiment()
        if self.device_command_active:
            return
        if self.move_thread is not None:
            self._request_lockin_live_status()
            return
        self._request_full_live_status()

    def refresh_live_status(self):
        if self.thread is not None or self.device_command_active or self.experiment is None:
            return
        now = time.perf_counter()
        if now - self._last_live_refresh < 1.0:
            return
        self._last_live_refresh = now
        try:
            if self.move_thread is not None:
                self._request_lockin_live_status()
            else:
                self._request_full_live_status()
        except Exception:
            return

    def _request_full_live_status(self):
        self._invoke_live_worker("read_full")

    def _request_lockin_live_status(self):
        self._invoke_live_worker("read_lockin")

    def handle_live_status_ready(self, status: object, overload: object):
        self._apply_pending_origin(status.position)
        self._apply_live_status(status, overload=overload)
        self._refresh_scan_limit_hints()

    def handle_lockin_status_ready(self, settings: object, signal: object, overload: object):
        if isinstance(settings, dict):
            self._apply_lockin_settings(settings)
        if isinstance(signal, dict):
            self._apply_signal(signal)
        self._apply_overload_status(overload)

    def handle_live_status_error(self, message: str):
        self.append_log(f"Live status error: {message}")
        self.pending_origin_axis = None

    def _apply_pending_origin(self, position):
        axis = self.pending_origin_axis
        if axis is None:
            return
        self.pending_origin_axis = None
        values = {
            "t": position.t_ps,
            "x": position.x_um,
            "y": position.y_um,
        }
        value = values.get(axis)
        if value is None:
            QtWidgets.QMessageBox.warning(self, "Origin Error", f"{axis} position is unavailable.")
            return
        {"t": self.t_zero_spin, "x": self.x_zero_spin, "y": self.y_zero_spin}[axis].setValue(value)

    def _apply_live_status(self, status, *, overload: object = None):
        self._update_position_from_position(status.position)
        if status.lockin_settings:
            self._apply_lockin_settings(status.lockin_settings)
        if status.signal:
            self._apply_signal(status.signal)
        self._apply_overload_status(status.lockin_overload if overload is None else overload)

    def _apply_lockin_settings(self, settings: dict[str, Any]):
        if settings.get("Sensitivity") is not None:
            display = lockin_display_from_settings(settings)
            self._voltage_scale = display.voltage_scale
            self._voltage_unit = display.voltage_unit
            self.sensitivity_label.setText(display.sensitivity)
            self.tc_label.setText(display.time_constant)
            self.freq_label.setText(display.ref_freq)
        elif settings.get("Time Constant") is not None:
            self.tc_label.setText(time_constant_display(float(settings["Time Constant"])))
        self._refresh_plot_labels()

    def _apply_overload_status(self, overload: object):
        if overload is None:
            return
        if isinstance(overload, dict) and overload.get("_error"):
            self.overload_label.setText("?")
            return
        self.overload_label.setText(overload_display_from_status(overload))

    def _apply_signal(self, signal: dict[str, Any]):
        x = signal.get("X", signal.get("X_V"))
        y = signal.get("Y", signal.get("Y_V"))
        r = signal.get("R", signal.get("R_V"))
        theta = signal.get("Theta", signal.get("Theta_deg"))
        if x is not None:
            self.signal_labels["X"].setText(f"{float(x) * self._voltage_scale:.3f} {self._voltage_unit}")
        if y is not None:
            self.signal_labels["Y"].setText(f"{float(y) * self._voltage_scale:.3f} {self._voltage_unit}")
        if r is not None:
            self.signal_labels["R"].setText(f"{float(r) * self._voltage_scale:.3f} {self._voltage_unit}")
        if theta is not None:
            self.signal_labels["Theta"].setText(f"{float(theta):.3f} deg")

    def set_origin_from_current(self, axis: str):
        axis = axis.strip().lower()
        try:
            if axis not in {"t", "x", "y"}:
                raise ValueError(f"Unsupported axis: {axis}")
            self.pending_origin_axis = axis
            self._request_full_live_status()
        except Exception as e:
            self.pending_origin_axis = None
            QtWidgets.QMessageBox.warning(self, "Origin Error", str(e))

    def _move_axis_label(self, axis: str) -> str:
        return {"t": "delay line", "x": "scanner X", "y": "scanner Y"}[axis]

    def _move_axis_unit(self, axis: str) -> str:
        return {"t": "ps", "x": "um", "y": "um"}[axis]

    def _move_target_value(self, axis: str) -> float:
        return {"t": self.move_t_spin, "x": self.move_x_spin, "y": self.move_y_spin}[axis].value()

    def _motion_axis_is_blocked_by_measurement(self, axis: str) -> bool:
        if self.thread is None:
            return False
        return axis in self.running_motion_axes

    def move_absolute(self, axis: str):
        axis = axis.strip().lower()
        if self.move_thread is not None:
            return
        now = time.perf_counter()
        if now < self._move_block_until:
            return
        if self.device_command_active:
            QtWidgets.QMessageBox.warning(self, "Move Error", "Wait for device initialization to finish first.")
            return
        if self._motion_axis_is_blocked_by_measurement(axis):
            QtWidgets.QMessageBox.warning(self, "Move Error", "Stop the running measurement for this axis first.")
            return
        try:
            if axis not in {"t", "x", "y"}:
                raise ValueError(f"Unsupported axis: {axis}")
            experiment = self._ensure_experiment()
            value = self._move_target_value(axis)
            self.move_thread = QtCore.QThread(self)
            self.move_worker = MoveWorker(
                experiment=experiment,
                axis=axis,
                value=value,
                coordinate="measurement",
            )
            self.move_worker.moveToThread(self.move_thread)
            self.move_thread.started.connect(self.move_worker.run)
            self.move_worker.status_changed.connect(self.handle_move_status)
            self.move_worker.position_changed.connect(self.handle_move_position)
            self.move_worker.finished.connect(self.handle_move_finished)
            self.move_worker.error_occurred.connect(self.handle_move_error)
            self.move_worker.finished.connect(self.move_thread.quit)
            self.move_worker.finished.connect(self.move_worker.deleteLater)
            self.move_worker.error_occurred.connect(self.move_thread.quit)
            self.move_worker.error_occurred.connect(self.move_worker.deleteLater)
            self.move_thread.finished.connect(self.move_thread.deleteLater)
            self.move_thread.finished.connect(self.cleanup_move_thread)
            self.running_move_axis = axis
            self._move_block_until = time.perf_counter() + MOVE_COMMAND_COOLDOWN_S
            self._set_move_running(True)
            self.append_log(f"Move started: {self._move_axis_label(axis)} -> {value:.3f} {self._move_axis_unit(axis)}.")
            self.move_thread.start()
        except Exception as e:
            self.cleanup_move_thread()
            QtWidgets.QMessageBox.warning(self, "Move Error", str(e))

    def move_corrected(self, axis: str):
        zero = {"t": self.t_zero_spin, "x": self.x_zero_spin, "y": self.y_zero_spin}[axis].value()
        corrected = {"t": self.t_cor_spin, "x": self.x_cor_spin, "y": self.y_cor_spin}[axis].value()
        target = zero + corrected
        {"t": self.move_t_spin, "x": self.move_x_spin, "y": self.move_y_spin}[axis].setValue(target)
        self.move_absolute(axis)

    def handle_move_status(self, status: str):
        self.status_label.setText(status)
        self.append_log(status)
        axis = moving_axis_from_status(status)
        if axis in {"t", "x", "y"}:
            self.position_labels[axis].setText(_motion_axis_display_text(status))

    def handle_measurement_status(self, status: str):
        self.status_label.setText(status)
        axis = moving_axis_from_status(status)
        if axis is not None:
            self._set_measurement_axis_status(axis, _motion_axis_display_text(status))
            return
        if status == STATUS_WAITING:
            self.status_label.setText(STATUS_RUNNING)
            self._restore_running_motion_axis_values()
            return
        if status == STATUS_SLOW_AXIS_READY:
            self.status_label.setText(STATUS_RUNNING)
            self._restore_running_motion_axis_values()
            self._update_scan2d_eta_from_slow_ready()
            return
        if status in {STATUS_READING_LOCKIN, STATUS_STOPPED}:
            self._restore_running_motion_axis_values()

    def _set_measurement_axis_status(self, axis: str, text: str):
        self._restore_running_motion_axis_values()
        if axis in {"t", "x", "y"}:
            self.position_labels[axis].setText(text)

    def _restore_running_motion_axis_values(self):
        for axis in self.running_motion_axes:
            if axis in {"t", "x", "y"}:
                self._set_position_value(axis, self._current_position_values[axis])

    def _update_scan2d_eta_from_slow_ready(self):
        measurement = self.running_measurement or self._measurement_name()
        if measurement not in {"strkr", "srkr_2d"} or self._scan2d_fast_point_count <= 0:
            return
        now = time.perf_counter()
        completed = len(self.rows_by_mode.get(measurement, []))
        if completed <= 0:
            self._scan2d_eta_anchor_at = now
            return
        completed_lines = completed // self._scan2d_fast_point_count
        if completed_lines <= 0 or completed % self._scan2d_fast_point_count != 0 or self._scan2d_eta_anchor_at is None:
            return
        self._scan2d_eta_line_cycle_s = (now - self._scan2d_eta_anchor_at) / completed_lines
        self.eta_text_by_mode[measurement] = self._scan2d_eta_text(completed)
        if measurement == self._measurement_name():
            self.eta_label.setText(self.eta_text_by_mode[measurement])

    def handle_move_position(self, position: object):
        if isinstance(position, dict):
            self._update_position_from_row(position)
        elif position is not None:
            self._update_position_from_position(position, preserve_missing=True)

    def handle_move_finished(self, result: object):
        info = result if isinstance(result, dict) else {}
        axis = str(info.get("axis", self.running_move_axis or ""))
        value = float(info.get("value", self._move_target_value(axis))) if axis in {"t", "x", "y"} else 0.0
        position = info.get("position")
        if position is not None:
            self._update_position_from_position(position, preserve_missing=True)
        self.status_label.setText("move complete")
        if axis in {"t", "x", "y"}:
            self.append_log(f"Moved {self._move_axis_label(axis)} to {value:.3f} {self._move_axis_unit(axis)}.")
        try:
            self._refresh_scan_limit_hints()
        except Exception as e:
            self.append_log(f"Could not refresh move position hints: {e}")

    def handle_move_error(self, message: str):
        axis = self.running_move_axis
        if axis in {"t", "x", "y"}:
            self._set_position_value(axis, self._current_position_values[axis])
        self.status_label.setText("move error")
        self.append_log(f"Move error: {message}")
        QtWidgets.QMessageBox.warning(self, "Move Error", message)

    def cleanup_move_thread(self):
        axis = self.running_move_axis
        self.move_thread = None
        self.move_worker = None
        self.running_move_axis = None
        if axis in {"t", "x", "y"}:
            self._set_position_value(axis, self._current_position_values[axis])
        self._move_block_until = time.perf_counter() + MOVE_COMMAND_COOLDOWN_S
        self._set_move_running(False)

    def use_tc_wait_time(self, spin: QtWidgets.QDoubleSpinBox):
        if self.device_command_active or self.move_thread is not None:
            return
        self.pending_wait_spin = spin
        self._start_device_command(
            "lockin_wait_time",
            label="Read lock-in wait time",
            ref="lockin.main",
            multiplier=4.0,
        )

    def _measurement_name(self) -> str:
        label = self.measurement_tabs.tabText(self.measurement_tabs.currentIndex())
        return {
            "Signal Monitor": "signal_monitor",
            "TRKR": "trkr",
            "SRKR": "srkr",
            "STRKR": "strkr",
            "SRKR 2D": "srkr_2d",
        }[label]

    def _output_path(self) -> Path:
        self._store_current_output_settings()
        return build_output_path(self.output_settings_by_mode[self._measurement_name()])

    def start_measurement(self):
        if self.thread is not None:
            return
        if self.device_command_active or self.move_thread is not None:
            QtWidgets.QMessageBox.warning(self, "Run Error", "Wait for the active device operation to finish first.")
            return
        try:
            measurement = self._measurement_name()
            if measurement in {"strkr", "srkr_2d"}:
                self._sync_scan2d_role_values_to_axis_ranges(measurement)
            self._normalize_2d_axis_controls(measurement)
            self._load_scan2d_role_ranges(measurement)
            if measurement in {"strkr", "srkr_2d"}:
                self._sync_scan2d_role_values_to_axis_ranges(measurement)
            output_path = self._output_path()
            scan_plan = None
            axis = None
            interval_s = None
            n_points = None
            wait_s = None
            return_to_zero = None
            self.running_motion_axes = set()
            self._scan2d_fast_point_count = 0
            self._scan2d_slow_point_count = 0
            self._scan2d_eta_anchor_at = None
            self._scan2d_eta_line_cycle_s = None

            if measurement == "signal_monitor":
                plan = signal_monitor_plan(interval_s=self.signal_interval_spin.value(), n_points=self.signal_points_spin.value())
                interval_s = plan.interval_s
                n_points = plan.n_points
                summary = plan.summary
                self.running_srkr_axis = None
            elif measurement == "trkr":
                scan_plan = trkr_plan(
                    minimum_ps=self.trkr_min_spin.value(),
                    maximum_ps=self.trkr_max_spin.value(),
                    step_ps=self.trkr_step_spin.value(),
                    t_zero_ps=self.t_zero_spin.value(),
                    coordinate="measurement",
                )
                wait_s = self.trkr_wait_spin.value()
                return_to_zero = self.trkr_return_check.isChecked()
                summary = f"TRKR {len(scan_plan.scan_points)} points"
                self.running_srkr_axis = None
                self.running_motion_axes = {"t"}
            elif measurement == "srkr":
                axis = self.srkr_axis_combo.currentText().lower()
                scan_plan = srkr_plan(
                    axis=axis,
                    minimum_um=self.srkr_min_spin.value(),
                    maximum_um=self.srkr_max_spin.value(),
                    step_um=self.srkr_step_spin.value(),
                    zero_by_axis={"x": self.x_zero_spin.value(), "y": self.y_zero_spin.value()},
                    coordinate="measurement",
                )
                wait_s = self.srkr_wait_spin.value()
                return_to_zero = self.srkr_return_check.isChecked()
                summary = f"SRKR {axis.upper()} {len(scan_plan.scan_points)} points"
                self.running_srkr_axis = axis
                self.running_motion_axes = {axis}
            elif measurement == "strkr":
                scan_plan = strkr_plan(
                    fast_axis=self.strkr_fast_axis_combo.currentText(),
                    slow_axis=self.strkr_slow_axis_combo.currentText(),
                    ranges=self._axis_ranges_payload(self.strkr_range_spins),
                    zero_by_axis={
                        "t_ps": self.t_zero_spin.value(),
                        "x_um": self.x_zero_spin.value(),
                        "y_um": self.y_zero_spin.value(),
                    },
                    return_to_zero=self._return_roles_payload(),
                )
                wait_s = self.strkr_wait_spin.value()
                summary = scan_plan.summary
                self.running_srkr_axis = None
                self.running_motion_axes = {scan_plan.fast_axis, scan_plan.slow_axis}
                self._scan2d_fast_point_count = scan_plan.fast_point_count
                self._scan2d_slow_point_count = scan_plan.slow_point_count
            elif measurement == "srkr_2d":
                scan_plan = srkr_2d_plan(
                    fast_axis=self.srkr_2d_fast_axis_combo.currentText(),
                    slow_axis=self.srkr_2d_slow_axis_combo.currentText(),
                    ranges=self._axis_ranges_payload(self.srkr_2d_range_spins),
                    zero_by_axis={
                        "t_ps": self.t_zero_spin.value(),
                        "x_um": self.x_zero_spin.value(),
                        "y_um": self.y_zero_spin.value(),
                    },
                    return_to_zero=self._return_roles_payload(),
                )
                wait_s = self.srkr_2d_wait_spin.value()
                summary = scan_plan.summary
                self.running_srkr_axis = None
                self.running_motion_axes = {scan_plan.fast_axis, scan_plan.slow_axis}
                self._scan2d_fast_point_count = scan_plan.fast_point_count
                self._scan2d_slow_point_count = scan_plan.slow_point_count
            else:
                raise ValueError(f"Unsupported measurement: {measurement}")

            missing = self._missing_required_devices(measurement, axis)
            if missing:
                raise RuntimeError("Connect required devices before starting: " + ", ".join(missing))
            experiment = self._ensure_experiment()

            if measurement == "srkr":
                active_axis = self.srkr_axis_combo.currentText().lower()
                self.rows_by_mode["srkr"] = [
                    row for row in self.rows_by_mode["srkr"] if row.get("fast_axis") != active_axis
                ]
            else:
                self.rows_by_mode[measurement].clear()
            self.point_text_by_mode[measurement] = "-"
            self.eta_text_by_mode[measurement] = "-"
            self._update_curves()
            self.point_label.setText("-")
            self.eta_label.setText("-")

            self.thread = QtCore.QThread(self)
            self.worker = MeasurementWorker(
                experiment=experiment,
                measurement=measurement,
                output_path=str(output_path),
                scan_plan=scan_plan,
                axis=axis,
                interval_s=interval_s,
                n_points=n_points,
                wait_s=wait_s,
                return_to_zero=return_to_zero,
            )
            self.worker.moveToThread(self.thread)
            self.thread.started.connect(self.worker.run)
            self.worker.point_ready.connect(self.handle_point)
            self.worker.status_changed.connect(self.handle_measurement_status)
            self.worker.error_occurred.connect(self.handle_error)
            self.worker.finished.connect(self.handle_finished)
            self.worker.finished.connect(self.thread.quit)
            self.worker.finished.connect(self.worker.deleteLater)
            self.thread.finished.connect(self.thread.deleteLater)
            self.thread.finished.connect(self.cleanup_thread)
            self.running_measurement = measurement
            self._set_running(True)
            self.append_log(f"Started {summary} -> {output_path}")
            self.thread.start()
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Run Error", str(e))

    def stop_measurement(self):
        if self.worker is not None:
            self.worker.stop()
            self.append_log("Stop requested.")

    def handle_point(self, point: MeasurementPoint):
        measurement = self.running_measurement or self._measurement_name()
        self.rows_by_mode[measurement].append(point.row)
        self.point_text_by_mode[measurement] = f"{point.index}/{point.total_points}"
        self.eta_text_by_mode[measurement] = self._eta_text(measurement, point.index, point.total_points)
        if measurement == self._measurement_name():
            self.point_label.setText(self.point_text_by_mode[measurement])
            self.eta_label.setText(self.eta_text_by_mode[measurement])
        self._apply_signal(point.row)
        self._update_position_from_row(point.row)
        self._update_snapshot(point.row)
        if measurement == self._measurement_name():
            self._update_curves()

    def _eta_text(self, measurement: str, completed: int, total: int) -> str:
        if measurement not in {"strkr", "srkr_2d"}:
            return "-"
        return self._scan2d_eta_text(completed)

    def _scan2d_eta_text(self, completed: int) -> str:
        if (
            self._scan2d_eta_line_cycle_s is None
            or self._scan2d_fast_point_count <= 0
            or self._scan2d_slow_point_count <= 0
        ):
            return "-"
        completed_lines = completed / self._scan2d_fast_point_count
        remaining_cycles = max(0.0, self._scan2d_slow_point_count - completed_lines)
        return _format_duration(self._scan2d_eta_line_cycle_s * remaining_cycles)

    def _update_position_from_position(self, position, *, preserve_missing: bool = False):
        if not preserve_missing or position.t_ps is not None:
            self._set_position_value("t", position.t_ps)
        if not preserve_missing or position.x_um is not None:
            self._set_position_value("x", position.x_um)
        if not preserve_missing or position.y_um is not None:
            self._set_position_value("y", position.y_um)

    def _update_position_from_row(self, row: dict[str, Any]):
        if row.get("t_ps") is not None:
            self._set_position_value("t", row.get("t_ps"))
        if row.get("x_um") is not None:
            self._set_position_value("x", row.get("x_um"))
        if row.get("y_um") is not None:
            self._set_position_value("y", row.get("y_um"))

    def _set_position_value(self, axis: str, value: float | None):
        self._current_position_values[axis] = value
        self.position_labels[axis].setText(_format_value(value))
        self._refresh_derived_position_labels()

    def _refresh_derived_position_labels(self):
        zero_by_axis = {
            "t": self.t_zero_spin.value(),
            "x": self.x_zero_spin.value(),
            "y": self.y_zero_spin.value(),
        }
        for axis, current in self._current_position_values.items():
            zero = zero_by_axis[axis]
            self.offset_labels[axis].setText(_format_value(zero))
            self.corrected_labels[axis].setText("-" if current is None else _format_value(float(current) - zero))

    def _update_snapshot(self, row: dict[str, Any]):
        keys = fields_for_row(row)
        keys.extend(key for key in row if key not in keys)
        self.snapshot_table.setRowCount(len(keys))
        for index, key in enumerate(keys):
            field = QtWidgets.QTableWidgetItem(key)
            value = QtWidgets.QTableWidgetItem(format_snapshot_value(key, row.get(key), voltage_scale=self._voltage_scale))
            self.snapshot_table.setItem(index, 0, field)
            self.snapshot_table.setItem(index, 1, value)
        self.snapshot_table.resizeColumnsToContents()

    def handle_error(self, message: str):
        self.append_log(f"Error: {message}")
        QtWidgets.QMessageBox.critical(self, "Measurement Error", message)

    def handle_finished(self, rows: object):
        self.append_log(f"Finished. {len(rows)} points collected.")
        self._update_curves()

    def cleanup_thread(self):
        finished_measurement = self.running_measurement
        self.thread = None
        self.worker = None
        self.running_measurement = None
        self.running_srkr_axis = None
        self.running_motion_axes = set()
        self._scan2d_fast_point_count = 0
        self._scan2d_slow_point_count = 0
        self._scan2d_eta_anchor_at = None
        self._scan2d_eta_line_cycle_s = None
        self._set_running(False)
        if finished_measurement == self._measurement_name():
            self._update_curves()
        if self.experiment is not None and not self._shutdown_requested:
            QtCore.QTimer.singleShot(0, self._request_full_live_status)

    def _set_running(self, running: bool):
        if self.move_thread is not None:
            self.start_button.setEnabled(False)
            self.stop_button.setEnabled(running)
            self.load_button.setEnabled(False)
            self.save_button.setEnabled(False)
            self.connect_button.setEnabled(False)
            self.disconnect_button.setEnabled(False)
            self.read_status_button.setEnabled(True)
            for axis in ("t", "x", "y"):
                self._set_motion_axis_enabled(axis, False)
            return

        self.start_button.setEnabled(not running)
        self.stop_button.setEnabled(running)
        self.load_button.setEnabled(not running)
        self.save_button.setEnabled(not running)
        self.connect_button.setEnabled(not running)
        self.disconnect_button.setEnabled(not running)
        self._set_motion_axis_enabled("t", True)
        self._set_motion_axis_enabled("x", True)
        self._set_motion_axis_enabled("y", True)
        if not running:
            return
        for axis in self.running_motion_axes:
            self._set_motion_axis_enabled(axis, False)

    def _set_motion_axis_enabled(self, axis: str, enabled: bool):
        widgets_by_axis = {
            "t": (
                self.t_connect_button,
                self.t_disconnect_button,
                self.t_initialize_button,
                self.move_t_spin,
                self.move_t_button,
                self.t_zero_spin,
                self.use_current_t_button,
                self.t_cor_spin,
                self.t_cor_button,
                self.t_controller_combo,
                self.t_stage_combo,
                self.t_direction_spin,
                self.t_port_combo,
                self.t_port_refresh_button,
            ),
            "x": (
                self.x_connect_button,
                self.x_disconnect_button,
                self.x_initialize_button,
                self.move_x_spin,
                self.move_x_button,
                self.x_zero_spin,
                self.use_current_x_button,
                self.x_cor_spin,
                self.x_cor_button,
                self.x_controller_combo,
                self.x_actuator_combo,
                self.x_axis_spin,
                self.x_scale_spin,
                self.x_port_combo,
                self.x_port_refresh_button,
            ),
            "y": (
                self.y_connect_button,
                self.y_disconnect_button,
                self.y_initialize_button,
                self.move_y_spin,
                self.move_y_button,
                self.y_zero_spin,
                self.use_current_y_button,
                self.y_cor_spin,
                self.y_cor_button,
                self.y_controller_combo,
                self.y_actuator_combo,
                self.y_axis_spin,
                self.y_scale_spin,
                self.y_port_combo,
                self.y_port_refresh_button,
            ),
        }
        for widget in widgets_by_axis[axis]:
            widget.setEnabled(enabled)

    def _set_device_initializing(self, running: bool):
        for widget in (
            self.connect_button,
            self.disconnect_button,
            self.read_status_button,
            self.lockin_connect_button,
            self.lockin_disconnect_button,
            self.t_initialize_button,
            self.t_connect_button,
            self.t_disconnect_button,
            self.move_t_button,
            self.t_cor_button,
            self.use_current_t_button,
            self.x_initialize_button,
            self.x_connect_button,
            self.x_disconnect_button,
            self.move_x_button,
            self.x_cor_button,
            self.use_current_x_button,
            self.y_initialize_button,
            self.y_connect_button,
            self.y_disconnect_button,
            self.move_y_button,
            self.y_cor_button,
            self.use_current_y_button,
            self.trkr_tc_button,
            self.srkr_tc_button,
            self.strkr_tc_button,
            self.srkr_2d_tc_button,
        ):
            widget.setEnabled(not running)

    def _set_move_running(self, running: bool):
        if running:
            self.load_button.setEnabled(False)
            self.save_button.setEnabled(False)
            self.connect_button.setEnabled(False)
            self.disconnect_button.setEnabled(False)
            self.read_status_button.setEnabled(True)
            for axis in ("t", "x", "y"):
                self._set_motion_axis_enabled(axis, False)
            if self.running_move_axis in {"t", "x", "y"}:
                self.position_labels[self.running_move_axis].setText("Moving...")
            return

        self.read_status_button.setEnabled(True)
        self._set_running(self.thread is not None)
        if self.device_command_active:
            self._set_device_initializing(True)

    def clear_plot(self):
        self.rows_by_mode[self._measurement_name()].clear()
        self.point_text_by_mode[self._measurement_name()] = "-"
        self.eta_text_by_mode[self._measurement_name()] = "-"
        self.point_label.setText("-")
        self.eta_label.setText("-")
        self.snapshot_table.setRowCount(0)
        self._update_curves()
        self.append_log("Cleared plot data.")

    def save_rows(self):
        rows = self.rows_by_mode[self._measurement_name()]
        if not rows:
            QtWidgets.QMessageBox.information(self, "No Data", "No rows to save.")
            return
        path = self._output_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=fields_for_rows(rows))
            writer.writeheader()
            writer.writerows(output_rows(rows))
        self.append_log(f"Saved {len(rows)} rows to {path}")

    def _current_2d_axes(self, mode: str) -> tuple[str, str]:
        rows = self.rows_by_mode.get(mode, [])
        if rows:
            return _valid_scan2d_axes(mode, str(rows[-1].get("fast_axis", "")), str(rows[-1].get("slow_axis", "")))
        if mode == "strkr":
            return _valid_scan2d_axes(
                mode,
                self.strkr_fast_axis_combo.currentText(),
                self.strkr_slow_axis_combo.currentText(),
            )
        return _valid_scan2d_axes(
            mode,
            self.srkr_2d_fast_axis_combo.currentText(),
            self.srkr_2d_slow_axis_combo.currentText(),
        )

    def _clear_scan2d_plots(self) -> None:
        for curve in self.scan2d_line_curves.values():
            curve.setData([], [])
        for image in self.scan2d_heatmaps.values():
            image.clear()

    def _refresh_plot_labels(self):
        view = signal_view_config(self.signal_mode_combo.currentText(), self._voltage_unit)
        if hasattr(self, "signal_title_labels"):
            self.signal_title_labels["X"].setText("X")
            self.signal_title_labels["Y"].setText("Y")
            self.signal_title_labels["R"].setText("R")
            self.signal_title_labels["Theta"].setText("Theta (deg)")
        self.plot1.setLabel("left", view.title1, units=view.unit1)
        self.plot2.setLabel("left", view.title2, units=view.unit2)
        mode = self._measurement_name() if hasattr(self, "measurement_tabs") else "trkr"
        if mode == "srkr":
            self.plot_stack.setCurrentWidget(self.srkr_plot_widget)
        elif mode in {"strkr", "srkr_2d"}:
            self.plot_stack.setCurrentWidget(self.scan2d_plot_widget)
        else:
            self.plot_stack.setCurrentWidget(self.standard_plot_widget)
        if mode == "signal_monitor":
            self.plot1.setLabel("bottom", "elapsed", units="s")
            self.plot2.setLabel("bottom", "elapsed", units="s")
            self.plot1.setLabel("top", "")
            self.plot2.setLabel("top", "")
            self.plot1.getAxis("top").setTicks([])
            self.plot2.getAxis("top").setTicks([])
        elif mode == "trkr":
            self.plot1.setLabel("bottom", "t_cor", units="ps")
            self.plot2.setLabel("bottom", "t_cor", units="ps")
            self.plot1.setLabel("top", "t", units="ps")
            self.plot2.setLabel("top", "t", units="ps")
        elif mode == "srkr":
            for axis in ("x", "y"):
                for signal_index, title, unit in (
                    (1, view.title1, view.unit1),
                    (2, view.title2, view.unit2),
                ):
                    plot = self.srkr_plots[(axis, signal_index)]
                    plot.setLabel("bottom", f"{axis}_cor", units="um")
                    plot.setLabel("top", axis, units="um")
                    plot.setLabel("left", title, units=unit)
        else:
            fast_axis, slow_axis = self._current_2d_axes(mode)
            for signal_index, title, unit in (
                (1, view.title1, view.unit1),
                (2, view.title2, view.unit2),
            ):
                self.scan2d_line_plots[signal_index].setLabel("bottom", f"{fast_axis}_cor", units=_axis_unit(fast_axis))
                self.scan2d_line_plots[signal_index].setLabel("left", title, units=unit)
                self.scan2d_heatmap_plots[signal_index].setLabel("bottom", f"{fast_axis}_cor", units=_axis_unit(fast_axis))
                self.scan2d_heatmap_plots[signal_index].setLabel("left", f"{slow_axis}_cor", units=_axis_unit(slow_axis))
                self.scan2d_heatmap_plots[signal_index].getViewBox().setAspectLocked(
                    scan2d_uses_equal_spatial_units(fast_axis, slow_axis),
                    ratio=1.0,
                )

    def _update_curves(self):
        self._refresh_plot_labels()
        measurement = self._measurement_name()
        rows = self.rows_by_mode[measurement]
        if not rows:
            self.curve1.setData([], [])
            self.curve2.setData([], [])
            for curve in self.srkr_curves.values():
                curve.setData([], [])
            self._clear_scan2d_plots()
            for plot in self.srkr_plots.values():
                plot.getAxis("top").setTicks([])
            return
        view = signal_view_config(self.signal_mode_combo.currentText(), self._voltage_unit)
        if measurement == "signal_monitor":
            x_values = [row["elapsed_s"] for row in rows]
        elif measurement == "trkr":
            x_values = [row.get("t_cor_ps", row["t_ps"] - self.t_zero_spin.value()) for row in rows]
            raw_values = [row["t_ps"] for row in rows]
            ticks = _axis_ticks(x_values, raw_values)
            self.plot1.getAxis("top").setTicks([ticks])
            self.plot2.getAxis("top").setTicks([ticks])
        elif measurement == "srkr":
            self._update_srkr_curves(rows, view)
            return
        else:
            self._update_scan2d_plots(rows, view)
            return
        scale1 = 1.0 if view.signal1_key == "Theta_deg" else self._voltage_scale
        scale2 = 1.0 if view.signal2_key == "Theta_deg" else self._voltage_scale
        self.curve1.setData(x_values, [row[view.signal1_key] * scale1 for row in rows])
        self.curve2.setData(x_values, [row[view.signal2_key] * scale2 for row in rows])

    def _update_srkr_curves(self, rows: list[dict[str, Any]], view):
        for axis in ("x", "y"):
            axis_rows = [row for row in rows if row.get("fast_axis") == axis and row.get(f"{axis}_cor_um") is not None]
            x_values = [row[f"{axis}_cor_um"] for row in axis_rows]
            raw_values = [row[f"{axis}_um"] for row in axis_rows]
            ticks = _axis_ticks(x_values, raw_values)
            for signal_index, signal_key in ((1, view.signal1_key), (2, view.signal2_key)):
                scale = 1.0 if signal_key == "Theta_deg" else self._voltage_scale
                self.srkr_curves[(axis, signal_index)].setData(x_values, [row[signal_key] * scale for row in axis_rows])
                self.srkr_plots[(axis, signal_index)].getAxis("top").setTicks([ticks])

    def _update_scan2d_plots(self, rows: list[dict[str, Any]], view):
        fast_axis, slow_axis = _valid_scan2d_axes(
            self._measurement_name(),
            str(rows[-1].get("fast_axis", "")),
            str(rows[-1].get("slow_axis", "")),
        )
        fast_key = _axis_cor_key(fast_axis)
        slow_target_key = axis_target_key(slow_axis)
        current_slow = rows[-1].get(slow_target_key)
        line_rows = [row for row in rows if row.get(slow_target_key) == current_slow and row.get(fast_key) is not None]
        fast_values = [row[fast_key] for row in line_rows]
        for signal_index, signal_key in ((1, view.signal1_key), (2, view.signal2_key)):
            scale = 1.0 if signal_key == "Theta_deg" else self._voltage_scale
            self.scan2d_line_curves[signal_index].setData(
                fast_values,
                [row[signal_key] * scale for row in line_rows],
            )
            self._set_scan2d_heatmap(signal_index, signal_key, scale, rows, fast_axis, slow_axis)

    def _set_scan2d_heatmap(
        self,
        signal_index: int,
        signal_key: str,
        scale: float,
        rows: list[dict[str, Any]],
        fast_axis: str,
        slow_axis: str,
    ):
        fast_target_key = axis_target_key(fast_axis)
        slow_target_key = axis_target_key(slow_axis)
        fast_values = _unique_values(row.get(fast_target_key) for row in rows)
        slow_values = _unique_values(row.get(slow_target_key) for row in rows)
        if not fast_values or not slow_values:
            self.scan2d_heatmaps[signal_index].clear()
            return
        fast_lookup = {value: index for index, value in enumerate(fast_values)}
        slow_lookup = {value: index for index, value in enumerate(slow_values)}
        image = np.full((len(fast_values), len(slow_values)), np.nan)
        for row in rows:
            fast_value = row.get(fast_target_key)
            slow_value = row.get(slow_target_key)
            if fast_value in fast_lookup and slow_value in slow_lookup and row.get(signal_key) is not None:
                image[fast_lookup[fast_value], slow_lookup[slow_value]] = float(row[signal_key]) * scale
        normalized = _normalized_by_abs_max(image)
        self.scan2d_heatmaps[signal_index].setLookupTable(RDBU_R_LUT)
        self.scan2d_heatmaps[signal_index].setImage(normalized, autoLevels=False, levels=(-1.0, 1.0))
        if len(fast_values) > 1:
            width = float(max(fast_values) - min(fast_values))
        else:
            width = 1.0
        if len(slow_values) > 1:
            height = float(max(slow_values) - min(slow_values))
        else:
            height = 1.0
        self.scan2d_heatmaps[signal_index].setRect(
            QtCore.QRectF(float(min(fast_values)), float(min(slow_values)), width, height)
        )
        self.scan2d_heatmap_plots[signal_index].getViewBox().setAspectLocked(
            scan2d_uses_equal_spatial_units(fast_axis, slow_axis),
            ratio=1.0,
        )

    def _request_async_shutdown(self):
        if self._shutdown_requested:
            return
        self._shutdown_requested = True
        self.status_label.setText("closing")
        self.append_log("Closing: stopping workers and disconnecting devices.")
        self.centralWidget().setEnabled(False)
        if hasattr(self, "live_timer"):
            self.live_timer.stop()
        self.stop_measurement()
        QtCore.QTimer.singleShot(0, self._drain_before_shutdown)

    def _drain_before_shutdown(self):
        if self.thread is not None or self.move_thread is not None or self.device_command_active:
            self.stop_measurement()
            QtCore.QTimer.singleShot(100, self._drain_before_shutdown)
            return
        if self.experiment is None:
            self._shutdown_complete = True
            QtCore.QTimer.singleShot(0, self.close)
            return
        self._start_device_command(
            "shutdown_disconnect_all",
            label="Shutdown disconnect",
            allow_during_shutdown=True,
        )

    def _close_worker_threads(self):
        for thread in (self.thread, self.move_thread, self.live_thread, self.resource_thread, self.device_thread):
            if thread is not None:
                thread.quit()
                thread.wait(2000)

    def closeEvent(self, event):
        if not self._shutdown_complete:
            event.ignore()
            self._request_async_shutdown()
            return
        self._close_worker_threads()
        self._restore_log_streams()
        super().closeEvent(event)

    def resizeEvent(self, event):
        super().resizeEvent(event)
        self._apply_panel_sizes()


def main():
    app = QtWidgets.QApplication(sys.argv)
    window = TRKRGui()
    window.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
