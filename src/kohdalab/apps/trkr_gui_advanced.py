"""Advanced TRKR/SRKR GUI with raw/interface coordinate controls.

Use ``kohdalab.apps.trkr_gui`` for routine measurement-unit operation. This
module is kept for reference while the everyday GUI stays simpler.
"""

from __future__ import annotations

import csv
import json
import sys
import time
from copy import deepcopy
from datetime import datetime
from pathlib import Path

from PySide6 import QtCore, QtWidgets
import pyqtgraph as pg
from serial.tools import list_ports

from kohdalab.api import Experiment, MeasurementPoint, Position
from kohdalab.api.config import normalize_delay_stage_name
from kohdalab.api.conversion import actuator_pos_to_sample_um, sample_um_to_actuator_pos
from kohdalab.api.devices import (
    disconnect_delay_stage,
    disconnect_lockin,
    disconnect_scanner,
    list_actuators,
    list_stages,
)
from kohdalab.apps.trkr_gui_config import (
    GuiConfigSnapshot,
    build_measurement_config,
    build_saved_config,
    extract_loaded_gui_config,
    first_number as _first_number,
    output_settings_from_measurement as _output_settings_from_measurement,
    scanner_scale_key as _scanner_scale_key,
    scanner_scale_value as _scanner_scale_value,
    zero_um_from_config as _zero_um_from_config,
)
from kohdalab.apps.trkr_gui_coordinates import (
    coordinate_correction_enabled,
    delay_stage_label_for_coordinate,
    delay_stage_unit_for_coordinate,
    normalize_coordinate,
    scanner_label_for_coordinate,
    scanner_unit_for_coordinate,
)
from kohdalab.apps.trkr_gui_devices import (
    corrected_target,
    delay_stage_key,
    device_ref,
    lockin_key,
    scanner_key,
    scanner_keys,
    single_instrument_config,
    xy_scanner_config,
)
from kohdalab.apps.trkr_gui_output import (
    build_output_path,
    normalize_output_settings,
    output_settings_from_fields,
)
from kohdalab.apps.trkr_gui_measurement import (
    signal_monitor_plan,
    srkr_plan,
    trkr_plan,
)
from kohdalab.apps.trkr_gui_plot import (
    sample_axis_ticks,
    signal_monitor_top_labels,
    srkr_plot_series,
    standard_plot_series,
    trkr_top_labels,
)
from kohdalab.apps.trkr_gui_signal import (
    lockin_display_from_settings,
    overload_display_from_status,
    signal_view_config,
)
from kohdalab.api.measurement_rows import output_rows
from kohdalab.apps.trkr_gui_snapshot import format_snapshot_value
from kohdalab.interfaces.lockin import list_visa_resources

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
CONFIG_DIR = PACKAGE_ROOT / "config"
DEFAULT_CONFIG_PATH = CONFIG_DIR / "trkr_config_kikuchi.json"
LOCKIN_MODELS = ["SR7265", "SR830", "LI5640", "SR5210"]


def _add_coordinate_items(combo: QtWidgets.QComboBox) -> None:
    combo.clear()
    combo.addItem("measurement", "measurement")
    combo.addItem("interface", "interface")
    combo.addItem("instrument", "instrument")


def _coordinate_value(combo: QtWidgets.QComboBox) -> str:
    return str(combo.currentData() or combo.currentText()).strip().lower()


def _set_coordinate_value(combo: QtWidgets.QComboBox, value: str | None) -> None:
    normalized = normalize_coordinate(value)
    index = combo.findData(normalized)
    if index < 0:
        index = combo.findText(normalized)
    if index >= 0:
        combo.setCurrentIndex(index)


def _set_combo_text(combo: QtWidgets.QComboBox, text: str) -> None:
    index = combo.findText(text)
    if index < 0 and text:
        combo.insertItem(0, text)
        index = 0
    if index >= 0:
        combo.setCurrentIndex(index)


def _replace_combo_items(combo: QtWidgets.QComboBox, items: list[str], current: str | None = None) -> None:
    current_text = current if current is not None else combo.currentText()
    combo.blockSignals(True)
    combo.clear()
    combo.addItems(items)
    _set_combo_text(combo, current_text)
    combo.blockSignals(False)


def _scanner_sample_um(scanner) -> float:
    return actuator_pos_to_sample_um(scanner.config, scanner.get_pos_unit(), _scanner_control_pos(scanner))


def _scanner_control_pos(scanner) -> float:
    unit = scanner.get_pos_unit().strip().lower()
    if unit == "mm":
        return scanner.get_pos_mm()
    if unit == "deg":
        return scanner.get_pos_deg()
    raise ValueError(f"Unsupported scanner control unit: {scanner.get_pos_unit()}")


def _scanner_move_control_pos(scanner, value: float) -> float:
    unit = scanner.get_pos_unit().strip().lower()
    if unit == "mm":
        return scanner.move_pos_mm(value)
    if unit == "deg":
        return scanner.move_pos_deg(value)
    raise ValueError(f"Unsupported scanner control unit: {scanner.get_pos_unit()}")


def _scanner_move_sample_um(scanner, sample_um: float) -> float:
    target_pos = sample_um_to_actuator_pos(scanner.config, scanner.get_pos_unit(), sample_um)
    _scanner_move_control_pos(scanner, target_pos)
    return _scanner_sample_um(scanner)


class DeviceConfigTab(QtWidgets.QWidget):
    def __init__(
        self,
        *,
        title_fields: list[tuple[str, str, QtWidgets.QWidget]],
        port_combo: QtWidgets.QComboBox | None = None,
        refresh_button: QtWidgets.QPushButton | None = None,
    ):
        super().__init__()
        layout = QtWidgets.QFormLayout(self)
        for label, _, widget in title_fields:
            layout.addRow(label, widget)

        if port_combo is not None and refresh_button is not None:
            port_row = QtWidgets.QHBoxLayout()
            port_row.addWidget(port_combo, 1)
            port_row.addWidget(refresh_button)
            layout.addRow("Port", port_row)


class CleanDoubleSpinBox(QtWidgets.QDoubleSpinBox):
    def textFromValue(self, value: float) -> str:
        text = f"{value:.{self.decimals()}f}"
        text = text.rstrip("0").rstrip(".")
        if text == "-0":
            return "0"
        return text


class FixedDecimalSpinBox(QtWidgets.QDoubleSpinBox):
    def textFromValue(self, value: float) -> str:
        return f"{value:.{self.decimals()}f}"


class DeviceControlPanel(QtWidgets.QWidget):
    def __init__(
        self,
        config_widget: QtWidgets.QWidget,
        *,
        has_initialize: bool = True,
    ):
        super().__init__()
        self.connect_button = QtWidgets.QPushButton("Connect")
        self.disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.initialize_button = QtWidgets.QPushButton("Initialize")
        self.settings_button = QtWidgets.QPushButton("Settings")

        button_row = QtWidgets.QHBoxLayout()
        button_row.addWidget(self.connect_button)
        button_row.addWidget(self.disconnect_button)
        if has_initialize:
            button_row.addWidget(self.initialize_button)
        else:
            self.initialize_button.hide()
        button_row.addWidget(self.settings_button)

        layout = QtWidgets.QVBoxLayout(self)
        layout.addWidget(config_widget, 1)
        layout.addLayout(button_row)


class CollapsibleSection(QtWidgets.QWidget):
    def __init__(self, title: str, content: QtWidgets.QWidget, *, expanded: bool = False):
        super().__init__()
        self.toggle_button = QtWidgets.QToolButton()
        self.toggle_button.setText(title)
        self.toggle_button.setCheckable(True)
        self.toggle_button.setChecked(expanded)
        self.toggle_button.setToolButtonStyle(QtCore.Qt.ToolButtonTextBesideIcon)
        self.toggle_button.setArrowType(
            QtCore.Qt.DownArrow if expanded else QtCore.Qt.RightArrow
        )
        self.content = content
        self.content.setVisible(expanded)

        layout = QtWidgets.QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.addWidget(self.toggle_button)
        layout.addWidget(self.content)

        self.toggle_button.toggled.connect(self._toggle)

    def _toggle(self, expanded: bool):
        self.toggle_button.setArrowType(
            QtCore.Qt.DownArrow if expanded else QtCore.Qt.RightArrow
        )
        self.content.setVisible(expanded)


class TrkrWorker(QtCore.QObject):
    point_ready = QtCore.Signal(object)
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        config: dict,
        scan_points: list[float | int],
        coordinate: str,
        wait_s: float,
        output_path: str | None,
        return_to_zero: bool = True,
    ):
        super().__init__()
        self.config = config
        self.scan_points = scan_points
        self.coordinate = coordinate
        self.wait_s = wait_s
        self.output_path = Path(output_path) if output_path else None
        self.return_to_zero = return_to_zero
        self._running = False
        self.rows: list[dict] = []

    @QtCore.Slot()
    def run(self):
        try:
            self._running = True
            experiment = Experiment(self.config)
            self.rows = experiment.run_trkr(
                scan_points=self.scan_points,
                coordinate=self.coordinate,
                wait_s=self.wait_s,
                output=str(self.output_path) if self.output_path else None,
                return_to_zero=self.return_to_zero,
                on_status=self.status_changed.emit,
                on_point=self.point_ready.emit,
                should_continue=lambda: self._running,
            )
            self.finished.emit(self.rows)
        except Exception as e:
            self.error_occurred.emit(str(e))
            self.finished.emit(self.rows)

    @QtCore.Slot()
    def stop(self):
        self._running = False


class SignalMonitorWorker(QtCore.QObject):
    point_ready = QtCore.Signal(object)
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        config: dict,
        output_path: str | None,
        interval_s: float,
        n_points: int,
    ):
        super().__init__()
        self.config = config
        self.output_path = Path(output_path) if output_path else None
        self.interval_s = interval_s
        self.n_points = n_points
        self._running = False
        self.rows: list[dict] = []

    @QtCore.Slot()
    def run(self):
        try:
            self._running = True
            experiment = Experiment(self.config)
            self.rows = experiment.run_signal_monitor(
                output=str(self.output_path) if self.output_path else None,
                interval_s=self.interval_s,
                n_points=self.n_points,
                on_status=self.status_changed.emit,
                on_point=self.point_ready.emit,
                should_continue=lambda: self._running,
            )
            self.finished.emit(self.rows)
        except Exception as e:
            self.error_occurred.emit(str(e))
            self.finished.emit(self.rows)

    @QtCore.Slot()
    def stop(self):
        self._running = False


class SrkrWorker(QtCore.QObject):
    point_ready = QtCore.Signal(object)
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(
        self,
        *,
        config: dict,
        scan_axis_name: str,
        coordinate: str,
        scan_points: list[float | int],
        zero: float | int | dict[str, float | int],
        wait_s: float,
        output_path: str | None,
        return_to_zero: bool = True,
    ):
        super().__init__()
        self.config = config
        self.scan_axis_name = scan_axis_name
        self.coordinate = coordinate
        self.scan_points = scan_points
        self.zero = zero
        self.wait_s = wait_s
        self.output_path = Path(output_path) if output_path else None
        self.return_to_zero = return_to_zero
        self._running = False
        self.rows: list[dict] = []

    @QtCore.Slot()
    def run(self):
        try:
            self._running = True
            experiment = Experiment(self.config)
            self.rows = experiment.run_srkr(
                axis=self.scan_axis_name,
                coordinate=self.coordinate,
                scan_points=self.scan_points,
                wait_s=self.wait_s,
                output=str(self.output_path) if self.output_path else None,
                return_to_zero=self.return_to_zero,
                on_status=self.status_changed.emit,
                on_point=self.point_ready.emit,
                should_continue=lambda: self._running,
            )
            self.finished.emit(self.rows)
        except Exception as e:
            self.error_occurred.emit(str(e))
            self.finished.emit(self.rows)

    @QtCore.Slot()
    def stop(self):
        self._running = False


class DelayStageInitializeWorker(QtCore.QObject):
    finished = QtCore.Signal(object)
    error_occurred = QtCore.Signal(str)
    status_changed = QtCore.Signal(str)

    def __init__(self, *, config: dict, ref: str):
        super().__init__()
        self.config = config
        self.ref = ref

    @QtCore.Slot()
    def run(self):
        try:
            experiment = Experiment(self.config)
            info = experiment.initialize_delay_stage(self.ref, on_status=self.status_changed.emit)
            self.finished.emit(info)
        except Exception as e:
            self.error_occurred.emit(str(e))


class XYInitializeWorker(QtCore.QObject):
    finished = QtCore.Signal(object)
    error_occurred = QtCore.Signal(str)
    status_changed = QtCore.Signal(str)

    def __init__(self, *, config: dict, x_ref: str, y_ref: str):
        super().__init__()
        self.config = config
        self.x_ref = x_ref
        self.y_ref = y_ref

    @QtCore.Slot()
    def run(self):
        try:
            on_status = self.status_changed.emit
            on_status("xy initializing")
            experiment = Experiment(self.config)
            x_info = experiment.initialize_scanner("x", self.x_ref, on_status=on_status)
            y_info = experiment.initialize_scanner("y", self.y_ref, on_status=on_status)
            self.finished.emit({"x": x_info, "y": y_info})
        except Exception as e:
            self.error_occurred.emit(str(e))


class TrkrWindow(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("KohdaLab TRKR Advanced")
        self.resize(1500, 900)

        self._thread = None
        self._worker = None
        self._device_thread = None
        self._device_worker = None
        self._rows_by_measurement: dict[str, list[dict]] = {
            "signal_monitor": [],
            "TRKR": [],
            "SRKR": [],
        }
        self._loaded_config: dict = {}
        self._output_settings_by_measurement: dict[str, dict[str, object]] = {
            "signal_monitor": {
                "output_dir": str(Path.cwd()),
                "filename": "signal_monitor_run",
                "auto_timestamp_suffix": True,
            },
            "TRKR": {
                "output_dir": str(Path.cwd()),
                "filename": "trkr_run",
                "auto_timestamp_suffix": True,
            },
            "SRKR": {
                "output_dir": str(Path.cwd()),
                "filename": "srkr_run",
                "auto_timestamp_suffix": True,
            },
        }
        self._voltage_scale = 1.0
        self._voltage_unit = "V"
        self._t_zero_ps_current = 0.0
        self._current_t_ps: float | None = None
        self._current_stage_mm: float | None = None
        self._current_stage_pulse: float | None = None
        self._x_zero_um_current: float | None = None
        self._y_zero_um_current: float | None = None
        self._current_x_um: float | None = None
        self._current_y_um: float | None = None
        self._current_scanner_pos = {"x": None, "y": None}
        self._current_scanner_units = {"x": "mm", "y": "mm"}
        self._last_wait_source = "manual"
        self._srkr_corrected_origins = {"x": 0.0, "y": 0.0}
        self._srkr_targets = {"x": 0.0, "y": 0.0}
        self._srkr_corrected_targets = {"x": 0.0, "y": 0.0}
        self._srkr_active_axis = "x"
        self._status_timer = QtCore.QTimer(self)
        self._status_timer.setInterval(250)
        self._last_live_signal_refresh_perf = 0.0
        self._last_live_position_refresh_perf = 0.0
        self._last_live_settings_refresh_perf = 0.0
        self._last_curve_update_perf = 0.0
        self._curve_update_interval_s = 0.2
        self._lockin_connected = False
        self._t_connected = False
        self._xy_connected = False
        self._experiment: Experiment | None = None
        self._instrument_refs = {
            "lockin": {"signal_monitor": "main", "TRKR": "main", "SRKR": "main"},
            "scanner": {"x": "x", "y": "y"},
            "delay_stage": {"TRKR": "t", "move_abs_t": "t"},
        }

        pg.setConfigOptions(antialias=True)

        self.config_path_edit = QtWidgets.QLineEdit()
        self.config_path_edit.setText(str(DEFAULT_CONFIG_PATH))
        self.config_browse_button = QtWidgets.QPushButton("Browse")
        self.config_load_button = QtWidgets.QPushButton("Load")
        self.config_save_button = QtWidgets.QPushButton("Save")

        self.lockin_model_combo = QtWidgets.QComboBox()
        self.lockin_model_combo.addItems(LOCKIN_MODELS)
        self.lockin_resource_combo = QtWidgets.QComboBox()
        self.lockin_resource_combo.setEditable(True)
        self.lockin_refresh_button = QtWidgets.QPushButton("Refresh")

        self.xy_controller_combo = QtWidgets.QComboBox()
        self.xy_controller_combo.addItems(["CONEXCC", "CONEXAGAP"])
        self.xy_shared_port_check = QtWidgets.QCheckBox("Use shared port for X/Y")
        self.xy_shared_port_combo = QtWidgets.QComboBox()
        self.xy_shared_port_combo.setEditable(True)
        self.xy_shared_port_refresh_button = QtWidgets.QPushButton("Refresh")
        actuator_items = list_actuators(self.xy_controller_combo.currentText())
        self.x_actuator_combo = QtWidgets.QComboBox()
        self.x_actuator_combo.setEditable(True)
        self.x_actuator_combo.addItems(actuator_items)
        _set_combo_text(self.x_actuator_combo, "TRA12CC")
        self.x_axis_combo = QtWidgets.QComboBox()
        self.x_axis_combo.setEditable(False)
        self.x_scale_spin = CleanDoubleSpinBox()
        self.x_scale_spin.setRange(0.000001, 1000.0)
        self.x_scale_spin.setDecimals(6)
        self.x_scale_spin.setValue(582.0)
        self.x_port_combo = QtWidgets.QComboBox()
        self.x_port_combo.setEditable(True)
        self.x_port_refresh_button = QtWidgets.QPushButton("Refresh")

        self.y_actuator_combo = QtWidgets.QComboBox()
        self.y_actuator_combo.setEditable(True)
        self.y_actuator_combo.addItems(actuator_items)
        _set_combo_text(self.y_actuator_combo, "TRA12CC")
        self.y_axis_combo = QtWidgets.QComboBox()
        self.y_axis_combo.setEditable(False)
        self.y_scale_spin = CleanDoubleSpinBox()
        self.y_scale_spin.setRange(0.000001, 1000.0)
        self.y_scale_spin.setDecimals(6)
        self.y_scale_spin.setValue(412.0)
        self.y_port_combo = QtWidgets.QComboBox()
        self.y_port_combo.setEditable(True)
        self.y_port_refresh_button = QtWidgets.QPushButton("Refresh")

        self.t_controller_combo = QtWidgets.QComboBox()
        self.t_controller_combo.addItems(["SHOT302GS", "GSC01"])
        self.t_stage_combo = QtWidgets.QComboBox()
        self.t_stage_combo.setEditable(True)
        self.t_stage_combo.addItems(list_stages(self.t_controller_combo.currentText()))
        self.t_direction_spin = QtWidgets.QSpinBox()
        self.t_direction_spin.setRange(0, 1)
        self.t_direction_spin.setValue(0)
        self.t_port_combo = QtWidgets.QComboBox()
        self.t_port_combo.setEditable(True)
        self.t_port_refresh_button = QtWidgets.QPushButton("Refresh")

        self.device_tabs = QtWidgets.QTabWidget()
        self.measurement_tabs = QtWidgets.QTabWidget()

        self._configure_xy_axis_widgets()

        self.scan_min_spin = CleanDoubleSpinBox()
        self.scan_min_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.scan_min_spin.setDecimals(3)
        self.scan_min_spin.setSingleStep(1.0)
        self.scan_min_spin.setValue(0.0)
        self.scan_max_spin = CleanDoubleSpinBox()
        self.scan_max_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.scan_max_spin.setDecimals(3)
        self.scan_max_spin.setSingleStep(1.0)
        self.scan_max_spin.setValue(100.0)
        self.scan_step_spin = CleanDoubleSpinBox()
        self.scan_step_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.scan_step_spin.setDecimals(3)
        self.scan_step_spin.setSingleStep(1.0)
        self.scan_step_spin.setValue(10.0)
        self.trkr_coordinate_combo = QtWidgets.QComboBox()
        _add_coordinate_items(self.trkr_coordinate_combo)
        self.move_t_coordinate_combo = QtWidgets.QComboBox()
        _add_coordinate_items(self.move_t_coordinate_combo)
        self.wait_s_spin = FixedDecimalSpinBox()
        self.wait_s_spin.setRange(0.0, 120.0)
        self.wait_s_spin.setDecimals(1)
        self.wait_s_spin.setSingleStep(0.1)
        self.wait_s_spin.setValue(1.0)
        self.trkr_return_to_zero_check = QtWidgets.QCheckBox("Return to zero")
        self.trkr_return_to_zero_check.setChecked(True)
        self.t_zero_spin = CleanDoubleSpinBox()
        self.t_zero_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.t_zero_spin.setDecimals(3)
        self.t_zero_spin.setSingleStep(1.0)
        self.t_zero_spin.setValue(0.0)
        self.t_zero_current_button = QtWidgets.QPushButton("Use Current")
        self.wait_default_button = QtWidgets.QPushButton("Use TC x 4")
        self.move_t_spin = CleanDoubleSpinBox()
        self.move_t_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.move_t_spin.setDecimals(3)
        self.move_t_spin.setSingleStep(1.0)
        self.move_t_button = QtWidgets.QPushButton("Move")
        self.move_t_corrected_spin = CleanDoubleSpinBox()
        self.move_t_corrected_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.move_t_corrected_spin.setDecimals(3)
        self.move_t_corrected_spin.setSingleStep(1.0)
        self.move_t_corrected_button = QtWidgets.QPushButton("Move")
        self.signal_monitor_interval_spin = FixedDecimalSpinBox()
        self.signal_monitor_interval_spin.setRange(0.1, 86400.0)
        self.signal_monitor_interval_spin.setDecimals(1)
        self.signal_monitor_interval_spin.setSingleStep(0.1)
        self.signal_monitor_interval_spin.setValue(1.0)
        self.signal_monitor_points_spin = QtWidgets.QSpinBox()
        self.signal_monitor_points_spin.setRange(1, 10_000_000)
        self.signal_monitor_points_spin.setValue(60)
        self.srkr_min_spin = CleanDoubleSpinBox()
        self.srkr_min_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_min_spin.setDecimals(3)
        self.srkr_min_spin.setSingleStep(1.0)
        self.srkr_min_spin.setValue(-30.0)
        self.srkr_max_spin = CleanDoubleSpinBox()
        self.srkr_max_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_max_spin.setDecimals(3)
        self.srkr_max_spin.setSingleStep(1.0)
        self.srkr_max_spin.setValue(30.0)
        self.srkr_step_spin = CleanDoubleSpinBox()
        self.srkr_step_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_step_spin.setDecimals(3)
        self.srkr_step_spin.setSingleStep(1.0)
        self.srkr_step_spin.setValue(1.0)
        self.srkr_axis_combo = QtWidgets.QComboBox()
        self.srkr_axis_combo.addItems(["x", "y"])
        self.srkr_coordinate_combo = QtWidgets.QComboBox()
        _add_coordinate_items(self.srkr_coordinate_combo)
        self.scanner_coordinate_combos: dict[str, QtWidgets.QComboBox] = {}
        for axis in ("x", "y"):
            combo = QtWidgets.QComboBox()
            _add_coordinate_items(combo)
            self.scanner_coordinate_combos[axis] = combo
        self.srkr_offset_spin = CleanDoubleSpinBox()
        self.srkr_offset_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_offset_spin.setDecimals(3)
        self.srkr_offset_spin.setSingleStep(1.0)
        self.srkr_offset_spin.setValue(0.0)
        self.srkr_current_button = QtWidgets.QPushButton("Use Current")
        self.srkr_move_spin = CleanDoubleSpinBox()
        self.srkr_move_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_move_spin.setDecimals(3)
        self.srkr_move_spin.setSingleStep(1.0)
        self.srkr_move_button = QtWidgets.QPushButton("Move")
        self.srkr_corrected_move_spin = CleanDoubleSpinBox()
        self.srkr_corrected_move_spin.setRange(-1_000_000.0, 1_000_000.0)
        self.srkr_corrected_move_spin.setDecimals(3)
        self.srkr_corrected_move_spin.setSingleStep(1.0)
        self.srkr_corrected_move_button = QtWidgets.QPushButton("Move")
        self.scanner_move_spins: dict[str, CleanDoubleSpinBox] = {}
        self.scanner_move_buttons: dict[str, QtWidgets.QPushButton] = {}
        self.scanner_offset_spins: dict[str, CleanDoubleSpinBox] = {}
        self.scanner_offset_buttons: dict[str, QtWidgets.QPushButton] = {}
        self.scanner_corrected_move_spins: dict[str, CleanDoubleSpinBox] = {}
        self.scanner_corrected_move_buttons: dict[str, QtWidgets.QPushButton] = {}
        for axis in ("x", "y"):
            move_spin = CleanDoubleSpinBox()
            move_spin.setRange(-1_000_000.0, 1_000_000.0)
            move_spin.setDecimals(3)
            move_spin.setSingleStep(1.0)
            self.scanner_move_spins[axis] = move_spin
            self.scanner_move_buttons[axis] = QtWidgets.QPushButton("Move")

            offset_spin = CleanDoubleSpinBox()
            offset_spin.setRange(-1_000_000.0, 1_000_000.0)
            offset_spin.setDecimals(3)
            offset_spin.setSingleStep(1.0)
            self.scanner_offset_spins[axis] = offset_spin
            self.scanner_offset_buttons[axis] = QtWidgets.QPushButton("Use Current")

            corrected_spin = CleanDoubleSpinBox()
            corrected_spin.setRange(-1_000_000.0, 1_000_000.0)
            corrected_spin.setDecimals(3)
            corrected_spin.setSingleStep(1.0)
            self.scanner_corrected_move_spins[axis] = corrected_spin
            self.scanner_corrected_move_buttons[axis] = QtWidgets.QPushButton("Move")
        self.srkr_wait_spin = FixedDecimalSpinBox()
        self.srkr_wait_spin.setRange(0.0, 120.0)
        self.srkr_wait_spin.setDecimals(1)
        self.srkr_wait_spin.setSingleStep(0.1)
        self.srkr_wait_spin.setValue(1.0)
        self.srkr_return_to_zero_check = QtWidgets.QCheckBox("Return to zero")
        self.srkr_return_to_zero_check.setChecked(True)
        self.srkr_wait_default_button = QtWidgets.QPushButton("Use TC x 4")

        self.output_dir_edit = QtWidgets.QLineEdit(str(Path.cwd()))
        self.output_name_edit = QtWidgets.QLineEdit("trkr_run")
        self.output_browse_button = QtWidgets.QPushButton("Browse")
        self.auto_suffix_check = QtWidgets.QCheckBox("Auto timestamp suffix")
        self.auto_suffix_check.setChecked(True)

        self.connect_all_button = QtWidgets.QPushButton("Connect All")
        self.disconnect_all_button = QtWidgets.QPushButton("Disconnect All")
        self.start_button = QtWidgets.QPushButton("Start")
        self.stop_button = QtWidgets.QPushButton("Stop")
        self.stop_button.setEnabled(False)
        self.clear_button = QtWidgets.QPushButton("Clear Plot")
        self.save_button = QtWidgets.QPushButton("Save Now")

        self.status_label = QtWidgets.QLabel("idle")
        self.current_scan_axis_label = QtWidgets.QLabel("-")
        self.current_point_label = QtWidgets.QLabel("-")
        self.current_overload_label = QtWidgets.QLabel("-")
        self.current_sensitivity_label = QtWidgets.QLabel("-")
        self.current_tc_label = QtWidgets.QLabel("-")
        self.current_freq_label = QtWidgets.QLabel("-")
        self.current_t_label = QtWidgets.QLabel("-")
        self.current_t_offset_label = QtWidgets.QLabel("-")
        self.current_t_cor_label = QtWidgets.QLabel("-")
        self.current_x_label = QtWidgets.QLabel("-")
        self.current_x_offset_label = QtWidgets.QLabel("-")
        self.current_y_label = QtWidgets.QLabel("-")
        self.current_x_cor_label = QtWidgets.QLabel("-")
        self.current_y_offset_label = QtWidgets.QLabel("-")
        self.current_y_cor_label = QtWidgets.QLabel("-")
        self.current_signal1_label = QtWidgets.QLabel("-")
        self.current_signal2_label = QtWidgets.QLabel("-")
        self.current_signal3_label = QtWidgets.QLabel("-")
        self.current_signal4_label = QtWidgets.QLabel("-")
        self.status_x_signal_title = QtWidgets.QLabel("X (V)")
        self.status_r_signal_title = QtWidgets.QLabel("R (V)")
        self.status_y_signal_title = QtWidgets.QLabel("Y (V)")
        self.status_theta_signal_title = QtWidgets.QLabel("Theta (deg)")
        self.status_t_title = QtWidgets.QLabel("t (ps)")
        self.status_x_title = QtWidgets.QLabel("x (um)")
        self.status_y_title = QtWidgets.QLabel("y (um)")
        self.move_t_live_title = QtWidgets.QLabel("t (ps)")
        self.scanner_live_titles: dict[str, QtWidgets.QLabel] = {
            "x": QtWidgets.QLabel("x (um)"),
            "y": QtWidgets.QLabel("y (um)"),
        }
        self.signal_mode_combo = QtWidgets.QComboBox()
        self.signal_mode_combo.addItems(["X / Y", "R / Theta"])

        self.snapshot_keys: list[str] = []
        self.snapshot_labels: list[str] = []
        self.snapshot_table = QtWidgets.QTableWidget(0, 2)
        self.snapshot_table.setHorizontalHeaderLabels(["Field", "Value"])
        self.snapshot_table.verticalHeader().setVisible(False)
        self.snapshot_table.horizontalHeader().setStretchLastSection(True)
        self.snapshot_table.horizontalHeader().setSectionResizeMode(
            0, QtWidgets.QHeaderView.ResizeToContents
        )
        self.snapshot_table.setEditTriggers(QtWidgets.QAbstractItemView.NoEditTriggers)
        self._rebuild_snapshot_table()

        self.x_plot = pg.PlotWidget()
        self.x_plot.addLegend()
        self.x_plot.showGrid(x=True, y=True, alpha=0.25)
        self.x_plot.setLabel("left", "X", units="V")
        self.x_plot.setLabel("bottom", "t", units="ps")
        self.x_curve = self.x_plot.plot(pen=pg.mkPen("#1f77b4", width=2), name="X")
        self.x_plot.showAxis("top")
        self.x_plot.setLabel("top", "t_cor", units="ps")
        self.x_plot.getAxis("top").setStyle(showValues=True)

        self.y_plot = pg.PlotWidget()
        self.y_plot.addLegend()
        self.y_plot.showGrid(x=True, y=True, alpha=0.25)
        self.y_plot.setLabel("left", "Y", units="V")
        self.y_plot.setLabel("bottom", "t", units="ps")
        self.y_curve = self.y_plot.plot(pen=pg.mkPen("#d62728", width=2), name="Y")
        self.y_plot.showAxis("top")
        self.y_plot.setLabel("top", "t_cor", units="ps")
        self.y_plot.getAxis("top").setStyle(showValues=True)

        self.srkr_x_x_plot = pg.PlotWidget()
        self.srkr_x_x_plot.showGrid(x=True, y=True, alpha=0.25)
        self.srkr_x_x_plot.setLabel("left", "X", units="V")
        self.srkr_x_x_plot.setLabel("bottom", "x", units="um")
        self.srkr_x_x_plot.showAxis("top")
        self.srkr_x_x_plot.setLabel("top", "x_cor", units="um")
        self.srkr_x_x_plot.getAxis("top").setStyle(showValues=True)
        self.srkr_x_x_curve = self.srkr_x_x_plot.plot(pen=pg.mkPen("#1f77b4", width=2), name="X")

        self.srkr_x_y_plot = pg.PlotWidget()
        self.srkr_x_y_plot.showGrid(x=True, y=True, alpha=0.25)
        self.srkr_x_y_plot.setLabel("left", "Y", units="V")
        self.srkr_x_y_plot.setLabel("bottom", "x", units="um")
        self.srkr_x_y_plot.showAxis("top")
        self.srkr_x_y_plot.setLabel("top", "x_cor", units="um")
        self.srkr_x_y_plot.getAxis("top").setStyle(showValues=True)
        self.srkr_x_y_curve = self.srkr_x_y_plot.plot(pen=pg.mkPen("#d62728", width=2), name="Y")

        self.srkr_y_x_plot = pg.PlotWidget()
        self.srkr_y_x_plot.showGrid(x=True, y=True, alpha=0.25)
        self.srkr_y_x_plot.setLabel("left", "X", units="V")
        self.srkr_y_x_plot.setLabel("bottom", "y", units="um")
        self.srkr_y_x_plot.showAxis("top")
        self.srkr_y_x_plot.setLabel("top", "y_cor", units="um")
        self.srkr_y_x_plot.getAxis("top").setStyle(showValues=True)
        self.srkr_y_x_curve = self.srkr_y_x_plot.plot(pen=pg.mkPen("#1f77b4", width=2), name="X")

        self.srkr_y_y_plot = pg.PlotWidget()
        self.srkr_y_y_plot.showGrid(x=True, y=True, alpha=0.25)
        self.srkr_y_y_plot.setLabel("left", "Y", units="V")
        self.srkr_y_y_plot.setLabel("bottom", "y", units="um")
        self.srkr_y_y_plot.showAxis("top")
        self.srkr_y_y_plot.setLabel("top", "y_cor", units="um")
        self.srkr_y_y_plot.getAxis("top").setStyle(showValues=True)
        self.srkr_y_y_curve = self.srkr_y_y_plot.plot(pen=pg.mkPen("#d62728", width=2), name="Y")

        self.event_log = QtWidgets.QPlainTextEdit()
        self.event_log.setReadOnly(True)
        self.event_log.setMaximumBlockCount(1000)

        self._build_device_tabs()
        self.devices_section = CollapsibleSection("Devices", self.device_tabs, expanded=False)
        self._build_layout()
        self._configure_spin_boxes()
        self._connect_signals()
        self._refresh_move_abs_coordinate_ui()
        self._refresh_measurement_view()
        self.refresh_all_ports()
        self._status_timer.start()

    def _configure_spin_boxes(self):
        for spinbox in self.findChildren(QtWidgets.QAbstractSpinBox):
            spinbox.setButtonSymbols(QtWidgets.QAbstractSpinBox.NoButtons)

    def _build_device_tabs(self):
        lockin_config = DeviceConfigTab(
            title_fields=[
                ("Model", "model", self.lockin_model_combo),
            ]
        )
        lockin_layout = lockin_config.layout()
        lockin_row = QtWidgets.QHBoxLayout()
        lockin_row.addWidget(self.lockin_resource_combo, 1)
        lockin_row.addWidget(self.lockin_refresh_button)
        lockin_layout.addRow("Resource", lockin_row)
        self.lockin_panel = DeviceControlPanel(lockin_config, has_initialize=False)

        t_config = DeviceConfigTab(
            title_fields=[
                ("Controller", "controller", self.t_controller_combo),
                ("Stage", "stage", self.t_stage_combo),
                ("Direction", "direction", self.t_direction_spin),
            ],
            port_combo=self.t_port_combo,
            refresh_button=self.t_port_refresh_button,
        )
        self.t_panel = DeviceControlPanel(t_config, has_initialize=True)

        self.device_tabs.addTab(self.lockin_panel, "Lock-in")
        xy_inner = QtWidgets.QWidget()
        xy_layout = QtWidgets.QVBoxLayout(xy_inner)
        xy_common = QtWidgets.QFormLayout()
        xy_common.addRow("Controller", self.xy_controller_combo)
        shared_port_row = QtWidgets.QHBoxLayout()
        shared_port_row.addWidget(self.xy_shared_port_combo, 1)
        shared_port_row.addWidget(self.xy_shared_port_refresh_button)
        shared_port_row.addWidget(self.xy_shared_port_check)
        xy_common.addRow("Shared Port", shared_port_row)
        xy_layout.addLayout(xy_common)
        x_group = QtWidgets.QGroupBox("X")
        x_group.setMinimumWidth(280)
        x_group_layout = QtWidgets.QFormLayout(x_group)
        x_group_layout.addRow("Axis", self.x_axis_combo)
        x_group_layout.addRow("Actuator", self.x_actuator_combo)
        x_group_layout.addRow("sample um / unit", self.x_scale_spin)
        x_port_row = QtWidgets.QHBoxLayout()
        x_port_row.addWidget(self.x_port_combo, 1)
        x_port_row.addWidget(self.x_port_refresh_button)
        x_group_layout.addRow("Port", x_port_row)
        y_group = QtWidgets.QGroupBox("Y")
        y_group.setMinimumWidth(280)
        y_group_layout = QtWidgets.QFormLayout(y_group)
        y_group_layout.addRow("Axis", self.y_axis_combo)
        y_group_layout.addRow("Actuator", self.y_actuator_combo)
        y_group_layout.addRow("sample um / unit", self.y_scale_spin)
        y_port_row = QtWidgets.QHBoxLayout()
        y_port_row.addWidget(self.y_port_combo, 1)
        y_port_row.addWidget(self.y_port_refresh_button)
        y_group_layout.addRow("Port", y_port_row)
        xy_layout.addWidget(x_group)
        xy_layout.addWidget(y_group)
        xy_layout.addStretch(1)
        xy_scroll = QtWidgets.QScrollArea()
        xy_scroll.setWidgetResizable(True)
        xy_scroll.setFrameShape(QtWidgets.QFrame.NoFrame)
        xy_scroll.setHorizontalScrollBarPolicy(QtCore.Qt.ScrollBarAlwaysOff)
        xy_scroll.setMinimumHeight(380)
        xy_scroll.setWidget(xy_inner)

        self.xy_panel = DeviceControlPanel(xy_scroll, has_initialize=True)
        self.device_tabs.addTab(self.t_panel, "Delay Stage")
        self.device_tabs.addTab(self.xy_panel, "XY Scanner")

    def _build_layout(self):
        def _status_move_row(
            label: str | QtWidgets.QLabel,
            value_label: QtWidgets.QLabel,
            editor: QtWidgets.QWidget,
            button: QtWidgets.QPushButton,
        ) -> QtWidgets.QHBoxLayout:
            row = QtWidgets.QHBoxLayout()
            row.setContentsMargins(0, 0, 0, 0)
            row.setSpacing(8)

            live_widget = QtWidgets.QWidget()
            live_widget.setSizePolicy(QtWidgets.QSizePolicy.Ignored, QtWidgets.QSizePolicy.Preferred)
            live_layout = QtWidgets.QHBoxLayout(live_widget)
            live_layout.setContentsMargins(0, 0, 0, 0)
            live_layout.setSpacing(6)
            title_label = label if isinstance(label, QtWidgets.QLabel) else QtWidgets.QLabel(label)
            title_label.setMinimumWidth(0)
            title_label.setSizePolicy(QtWidgets.QSizePolicy.Ignored, QtWidgets.QSizePolicy.Preferred)
            live_layout.addWidget(title_label, 1)
            value_label.setMinimumWidth(72)
            value_label.setAlignment(QtCore.Qt.AlignRight | QtCore.Qt.AlignVCenter)
            live_layout.addWidget(value_label, 1)

            move_widget = QtWidgets.QWidget()
            move_widget.setSizePolicy(QtWidgets.QSizePolicy.Ignored, QtWidgets.QSizePolicy.Preferred)
            move_layout = QtWidgets.QHBoxLayout(move_widget)
            move_layout.setContentsMargins(0, 0, 0, 0)
            move_layout.setSpacing(6)
            move_layout.addWidget(editor, 1)
            move_layout.addWidget(button, 1)

            row.addWidget(live_widget)
            row.addWidget(move_widget)
            row.setStretch(0, 1)
            row.setStretch(1, 1)
            return row

        config_row = QtWidgets.QHBoxLayout()
        config_row.addWidget(self.config_path_edit, 1)
        config_row.addWidget(self.config_browse_button)
        config_row.addWidget(self.config_load_button)
        config_row.addWidget(self.config_save_button)

        session_button_row = QtWidgets.QHBoxLayout()
        session_button_row.addWidget(self.connect_all_button)
        session_button_row.addWidget(self.disconnect_all_button)

        output_dir_row = QtWidgets.QHBoxLayout()
        output_dir_row.addWidget(self.output_dir_edit, 1)
        output_dir_row.addWidget(self.output_browse_button)

        signal_mode_row = QtWidgets.QHBoxLayout()
        signal_mode_row.setContentsMargins(0, 0, 0, 0)
        signal_mode_row.addWidget(QtWidgets.QLabel("Signal Mode"))
        signal_mode_row.addWidget(self.signal_mode_combo, 1)

        scan_range_grid = QtWidgets.QGridLayout()
        scan_range_grid.setHorizontalSpacing(6)
        scan_range_grid.setContentsMargins(0, 0, 0, 0)
        scan_range_grid.addWidget(QtWidgets.QLabel("Min"), 0, 0)
        scan_range_grid.addWidget(self.scan_min_spin, 0, 1)
        scan_range_grid.addWidget(QtWidgets.QLabel("Max"), 0, 2)
        scan_range_grid.addWidget(self.scan_max_spin, 0, 3)
        scan_range_grid.addWidget(QtWidgets.QLabel("Step"), 0, 4)
        scan_range_grid.addWidget(self.scan_step_spin, 0, 5)
        scan_range_widget = QtWidgets.QWidget()
        scan_range_widget.setLayout(scan_range_grid)
        scan_range_widget.setMinimumWidth(300)

        wait_row = QtWidgets.QHBoxLayout()
        wait_row.setContentsMargins(0, 0, 0, 0)
        wait_row.addWidget(self.wait_s_spin, 1)
        wait_row.addWidget(self.wait_default_button)

        move_t_row = _status_move_row(self.move_t_live_title, self.current_t_label, self.move_t_spin, self.move_t_button)
        t_offset_row = _status_move_row("offset (ps)", self.current_t_offset_label, self.t_zero_spin, self.t_zero_current_button)
        move_t_corrected_row = _status_move_row("t_cor (ps)", self.current_t_cor_label, self.move_t_corrected_spin, self.move_t_corrected_button)

        trkr_axis_combo = QtWidgets.QComboBox()
        trkr_axis_combo.addItem("t")

        trkr_move_widget = QtWidgets.QWidget()
        trkr_move_layout = QtWidgets.QVBoxLayout(trkr_move_widget)
        trkr_move_layout.setContentsMargins(0, 0, 0, 0)
        trkr_move_layout.addLayout(move_t_row)
        trkr_move_layout.addLayout(t_offset_row)
        trkr_move_layout.addLayout(move_t_corrected_row)

        trkr_scan_group = QtWidgets.QGroupBox("Scan (corrected)")
        trkr_scan_form = QtWidgets.QFormLayout(trkr_scan_group)
        trkr_scan_form.addRow("Coordinate", self.trkr_coordinate_combo)
        trkr_scan_form.addRow("Wait time (s)", wait_row)
        trkr_scan_form.addRow("", self.trkr_return_to_zero_check)
        trkr_scan_form.addRow("Range", scan_range_widget)

        trkr_tab = QtWidgets.QWidget()
        trkr_layout = QtWidgets.QVBoxLayout(trkr_tab)
        trkr_layout.setSpacing(10)
        trkr_form = QtWidgets.QFormLayout()
        trkr_form.addRow("Axis", trkr_axis_combo)
        trkr_layout.addLayout(trkr_form)
        trkr_layout.addWidget(trkr_scan_group)

        srkr_range_grid = QtWidgets.QGridLayout()
        srkr_range_grid.setHorizontalSpacing(6)
        srkr_range_grid.setContentsMargins(0, 0, 0, 0)
        srkr_range_grid.addWidget(QtWidgets.QLabel("Min"), 0, 0)
        srkr_range_grid.addWidget(self.srkr_min_spin, 0, 1)
        srkr_range_grid.addWidget(QtWidgets.QLabel("Max"), 0, 2)
        srkr_range_grid.addWidget(self.srkr_max_spin, 0, 3)
        srkr_range_grid.addWidget(QtWidgets.QLabel("Step"), 0, 4)
        srkr_range_grid.addWidget(self.srkr_step_spin, 0, 5)
        srkr_range_widget = QtWidgets.QWidget()
        srkr_range_widget.setLayout(srkr_range_grid)
        srkr_range_widget.setMinimumWidth(300)

        srkr_move_row = QtWidgets.QHBoxLayout()
        srkr_move_row.setContentsMargins(0, 0, 0, 0)
        srkr_move_row.addWidget(self.srkr_move_spin, 1)
        srkr_move_row.addWidget(self.srkr_move_button)
        srkr_corrected_move_row = QtWidgets.QHBoxLayout()
        srkr_corrected_move_row.setContentsMargins(0, 0, 0, 0)
        srkr_corrected_move_row.addWidget(self.srkr_corrected_move_spin, 1)
        srkr_corrected_move_row.addWidget(self.srkr_corrected_move_button)

        srkr_offset_row = QtWidgets.QHBoxLayout()
        srkr_offset_row.setContentsMargins(0, 0, 0, 0)
        srkr_offset_row.addWidget(self.srkr_offset_spin, 1)
        srkr_offset_row.addWidget(self.srkr_current_button)

        srkr_wait_row = QtWidgets.QHBoxLayout()
        srkr_wait_row.setContentsMargins(0, 0, 0, 0)
        srkr_wait_row.addWidget(self.srkr_wait_spin, 1)
        srkr_wait_row.addWidget(self.srkr_wait_default_button)

        self.srkr_move_widget = QtWidgets.QWidget()
        self.srkr_move_form = QtWidgets.QFormLayout(self.srkr_move_widget)
        self.srkr_move_form.setContentsMargins(8, 6, 8, 6)
        self.srkr_move_row_container = QtWidgets.QWidget()
        self.srkr_move_row_container.setLayout(srkr_move_row)
        self.srkr_move_form.addRow("x (coordinate)", self.srkr_move_row_container)
        self.srkr_offset_row_container = QtWidgets.QWidget()
        self.srkr_offset_row_container.setLayout(srkr_offset_row)
        self.srkr_move_form.addRow("x_offset (um)", self.srkr_offset_row_container)
        self.srkr_corrected_move_row_container = QtWidgets.QWidget()
        self.srkr_corrected_move_row_container.setLayout(srkr_corrected_move_row)
        self.srkr_move_form.addRow("x_cor (um)", self.srkr_corrected_move_row_container)

        srkr_scan_group = QtWidgets.QGroupBox("Scan (corrected)")
        srkr_scan_form = QtWidgets.QFormLayout(srkr_scan_group)
        srkr_scan_form.addRow("Coordinate", self.srkr_coordinate_combo)
        srkr_scan_form.addRow("Wait time (s)", srkr_wait_row)
        srkr_scan_form.addRow("", self.srkr_return_to_zero_check)
        srkr_scan_form.addRow("Range", srkr_range_widget)

        srkr_tab = QtWidgets.QWidget()
        srkr_layout = QtWidgets.QVBoxLayout(srkr_tab)
        srkr_layout.setSpacing(10)
        srkr_form = QtWidgets.QFormLayout()
        srkr_form.addRow("Axis", self.srkr_axis_combo)
        srkr_layout.addLayout(srkr_form)
        srkr_layout.addWidget(srkr_scan_group)
        signal_monitor_tab = QtWidgets.QWidget()
        signal_monitor_form = QtWidgets.QFormLayout(signal_monitor_tab)
        signal_monitor_form.addRow("Interval (s)", self.signal_monitor_interval_spin)
        signal_monitor_form.addRow("Points", self.signal_monitor_points_spin)

        measurement_group = QtWidgets.QGroupBox("Measurement")
        measurement_layout = QtWidgets.QVBoxLayout(measurement_group)
        measurement_layout.addLayout(signal_mode_row)
        self.measurement_tabs.addTab(signal_monitor_tab, "Signal Monitor")
        self.measurement_tabs.addTab(trkr_tab, "TRKR")
        self.measurement_tabs.addTab(srkr_tab, "SRKR")
        measurement_layout.addWidget(self.measurement_tabs)
        common_output_form = QtWidgets.QFormLayout()
        common_output_form.addRow("Output Dir", output_dir_row)
        base_name_row = QtWidgets.QHBoxLayout()
        base_name_row.setContentsMargins(0, 0, 0, 0)
        base_name_row.addWidget(self.output_name_edit, 1)
        base_name_row.addWidget(self.auto_suffix_check)
        common_output_form.addRow("Base Filename", base_name_row)
        measurement_layout.addLayout(common_output_form)
        top_buttons = QtWidgets.QHBoxLayout()
        top_buttons.addWidget(self.start_button)
        top_buttons.addWidget(self.stop_button)
        measurement_layout.addLayout(top_buttons)
        secondary_buttons = QtWidgets.QHBoxLayout()
        secondary_buttons.addWidget(self.clear_button)
        secondary_buttons.addWidget(self.save_button)
        measurement_layout.addLayout(secondary_buttons)

        def _group(title: str) -> tuple[QtWidgets.QGroupBox, QtWidgets.QVBoxLayout]:
            box = QtWidgets.QGroupBox(title)
            layout = QtWidgets.QVBoxLayout(box)
            layout.setSpacing(8)
            return box, layout

        def _section_title(text: str) -> QtWidgets.QLabel:
            label = QtWidgets.QLabel(text)
            font = label.font()
            font.setBold(True)
            label.setFont(font)
            return label

        def _coordinate_row(combo: QtWidgets.QComboBox, *, measurement_unit: str, control_unit: str, device_unit: str) -> QtWidgets.QWidget:
            widget = QtWidgets.QWidget()
            layout = QtWidgets.QHBoxLayout(widget)
            layout.setContentsMargins(0, 0, 0, 0)
            layout.addWidget(QtWidgets.QLabel("Coordinate"))
            layout.addWidget(combo, 1)
            return widget

        session_group, session_layout = _group("Session")
        session_layout.addLayout(config_row)
        session_layout.addLayout(session_button_row)

        lockin_resource_row = QtWidgets.QHBoxLayout()
        lockin_resource_row.setContentsMargins(0, 0, 0, 0)
        lockin_resource_row.addWidget(self.lockin_resource_combo, 1)
        lockin_resource_row.addWidget(self.lockin_refresh_button)
        lockin_form = QtWidgets.QFormLayout()
        lockin_form.addRow("Model", self.lockin_model_combo)
        lockin_form.addRow("Resource", lockin_resource_row)
        lockin_buttons = QtWidgets.QHBoxLayout()
        lockin_buttons.addWidget(self.lockin_panel.connect_button)
        lockin_buttons.addWidget(self.lockin_panel.disconnect_button)
        lockin_status_grid = QtWidgets.QGridLayout()
        lockin_status_grid.addWidget(QtWidgets.QLabel("Overload"), 0, 0)
        lockin_status_grid.addWidget(self.current_overload_label, 0, 1)
        lockin_status_grid.addWidget(self.status_x_signal_title, 0, 2)
        lockin_status_grid.addWidget(self.current_signal1_label, 0, 3)
        lockin_status_grid.addWidget(QtWidgets.QLabel("Sensitivity"), 1, 0)
        lockin_status_grid.addWidget(self.current_sensitivity_label, 1, 1)
        lockin_status_grid.addWidget(self.status_y_signal_title, 1, 2)
        lockin_status_grid.addWidget(self.current_signal2_label, 1, 3)
        lockin_status_grid.addWidget(QtWidgets.QLabel("Time Constant"), 2, 0)
        lockin_status_grid.addWidget(self.current_tc_label, 2, 1)
        lockin_status_grid.addWidget(self.status_r_signal_title, 2, 2)
        lockin_status_grid.addWidget(self.current_signal3_label, 2, 3)
        lockin_status_grid.addWidget(QtWidgets.QLabel("Ref. Freq."), 3, 0)
        lockin_status_grid.addWidget(self.current_freq_label, 3, 1)
        lockin_status_grid.addWidget(self.status_theta_signal_title, 3, 2)
        lockin_status_grid.addWidget(self.current_signal4_label, 3, 3)
        lockin_device_widget = QtWidgets.QWidget()
        lockin_device_layout = QtWidgets.QVBoxLayout(lockin_device_widget)
        lockin_device_layout.setContentsMargins(0, 0, 0, 0)
        lockin_device_layout.addLayout(lockin_form)
        lockin_device_layout.addLayout(lockin_buttons)
        lockin_device_section = CollapsibleSection("Device", lockin_device_widget, expanded=False)
        lockin_widget = QtWidgets.QWidget()
        lockin_layout = QtWidgets.QVBoxLayout(lockin_widget)
        lockin_layout.setContentsMargins(0, 8, 0, 8)
        lockin_layout.setSpacing(8)
        lockin_layout.addWidget(_section_title("Lock-in"))
        lockin_layout.addWidget(lockin_device_section)
        lockin_layout.addWidget(QtWidgets.QLabel("Live Status"))
        lockin_layout.addLayout(lockin_status_grid)

        t_port_row = QtWidgets.QHBoxLayout()
        t_port_row.setContentsMargins(0, 0, 0, 0)
        t_port_row.addWidget(self.t_port_combo, 1)
        t_port_row.addWidget(self.t_port_refresh_button)
        delay_stage_form = QtWidgets.QFormLayout()
        delay_stage_form.addRow("Controller", self.t_controller_combo)
        delay_stage_form.addRow("Stage", self.t_stage_combo)
        delay_stage_form.addRow("Direction", self.t_direction_spin)
        delay_stage_form.addRow("Port", t_port_row)
        delay_stage_buttons = QtWidgets.QHBoxLayout()
        delay_stage_buttons.addWidget(self.t_panel.connect_button)
        delay_stage_buttons.addWidget(self.t_panel.disconnect_button)
        delay_stage_buttons.addWidget(self.t_panel.initialize_button)
        delay_stage_device_widget = QtWidgets.QWidget()
        delay_stage_device_layout = QtWidgets.QVBoxLayout(delay_stage_device_widget)
        delay_stage_device_layout.setContentsMargins(0, 0, 0, 0)
        delay_stage_device_layout.addLayout(delay_stage_form)
        delay_stage_device_layout.addLayout(delay_stage_buttons)
        delay_stage_device_layout.addWidget(
            _coordinate_row(
                self.move_t_coordinate_combo,
                measurement_unit="ps",
                control_unit="mm",
                device_unit="pulse",
            )
        )
        delay_stage_device_section = CollapsibleSection("Device", delay_stage_device_widget, expanded=False)
        delay_stage_widget = QtWidgets.QWidget()
        delay_stage_layout = QtWidgets.QVBoxLayout(delay_stage_widget)
        delay_stage_layout.setContentsMargins(0, 8, 0, 8)
        delay_stage_layout.setSpacing(8)
        delay_stage_layout.addWidget(_section_title("Delay Stage"))
        delay_stage_layout.addWidget(delay_stage_device_section)
        delay_stage_layout.addWidget(trkr_move_widget)

        xy_shared_row = QtWidgets.QHBoxLayout()
        xy_shared_row.setContentsMargins(0, 0, 0, 0)
        xy_shared_row.addWidget(self.xy_shared_port_combo, 1)
        xy_shared_row.addWidget(self.xy_shared_port_refresh_button)
        xy_shared_row.addWidget(self.xy_shared_port_check)
        xy_buttons = QtWidgets.QHBoxLayout()
        xy_buttons.addWidget(self.xy_panel.connect_button)
        xy_buttons.addWidget(self.xy_panel.disconnect_button)
        xy_buttons.addWidget(self.xy_panel.initialize_button)

        scanner_x_form = QtWidgets.QFormLayout()
        scanner_x_form.addRow("Controller", self.xy_controller_combo)
        scanner_x_form.addRow("Axis", self.x_axis_combo)
        scanner_x_form.addRow("Actuator", self.x_actuator_combo)
        scanner_x_form.addRow("sample um / unit", self.x_scale_spin)
        x_port_row = QtWidgets.QHBoxLayout()
        x_port_row.setContentsMargins(0, 0, 0, 0)
        x_port_row.addWidget(self.x_port_combo, 1)
        x_port_row.addWidget(self.x_port_refresh_button)
        scanner_x_form.addRow("Port", x_port_row)
        scanner_x_form.addRow("Shared Port", xy_shared_row)
        scanner_y_form = QtWidgets.QFormLayout()
        scanner_y_form.addRow("Controller", QtWidgets.QLabel("shared"))
        scanner_y_form.addRow("Axis", self.y_axis_combo)
        scanner_y_form.addRow("Actuator", self.y_actuator_combo)
        scanner_y_form.addRow("sample um / unit", self.y_scale_spin)
        y_port_row = QtWidgets.QHBoxLayout()
        y_port_row.setContentsMargins(0, 0, 0, 0)
        y_port_row.addWidget(self.y_port_combo, 1)
        y_port_row.addWidget(self.y_port_refresh_button)
        scanner_y_form.addRow("Port", y_port_row)
        def _scanner_section(
            *,
            title: str,
            axis: str,
            device_form: QtWidgets.QFormLayout,
            value_label: QtWidgets.QLabel,
            offset_label: QtWidgets.QLabel,
            cor_label: QtWidgets.QLabel,
        ) -> QtWidgets.QWidget:
            device_widget = QtWidgets.QWidget()
            device_layout = QtWidgets.QVBoxLayout(device_widget)
            device_layout.setContentsMargins(0, 0, 0, 0)
            device_layout.addLayout(device_form)
            if axis == "x":
                device_layout.addLayout(xy_buttons)
            device_layout.addWidget(
                _coordinate_row(
                    self.scanner_coordinate_combos[axis],
                    measurement_unit="um",
                    control_unit="mm/deg",
                    device_unit="mm/deg",
                )
            )
            device_section = CollapsibleSection("Device", device_widget, expanded=False)
            move_row = _status_move_row(self.scanner_live_titles[axis], value_label, self.scanner_move_spins[axis], self.scanner_move_buttons[axis])
            offset_row = _status_move_row("offset (um)", offset_label, self.scanner_offset_spins[axis], self.scanner_offset_buttons[axis])
            corrected_row = _status_move_row(f"{axis}_cor (um)", cor_label, self.scanner_corrected_move_spins[axis], self.scanner_corrected_move_buttons[axis])
            move_layout = QtWidgets.QVBoxLayout()
            move_layout.addLayout(move_row)
            move_layout.addLayout(offset_row)
            move_layout.addLayout(corrected_row)
            widget = QtWidgets.QWidget()
            layout = QtWidgets.QVBoxLayout(widget)
            layout.setContentsMargins(0, 8, 0, 8)
            layout.setSpacing(8)
            layout.addWidget(_section_title(title))
            layout.addWidget(device_section)
            layout.addLayout(move_layout)
            return widget

        scanner_x_widget = _scanner_section(
            title="Scanner X",
            axis="x",
            device_form=scanner_x_form,
            value_label=self.current_x_label,
            offset_label=self.current_x_offset_label,
            cor_label=self.current_x_cor_label,
        )
        scanner_y_widget = _scanner_section(
            title="Scanner Y",
            axis="y",
            device_form=scanner_y_form,
            value_label=self.current_y_label,
            offset_label=self.current_y_offset_label,
            cor_label=self.current_y_cor_label,
        )

        left_panel = QtWidgets.QWidget()
        left_panel.setMinimumWidth(384)
        left_layout = QtWidgets.QVBoxLayout(left_panel)
        left_layout.addWidget(session_group)
        left_layout.addWidget(lockin_widget)
        left_layout.addWidget(delay_stage_widget)
        left_layout.addWidget(scanner_x_widget)
        left_layout.addWidget(scanner_y_widget)
        left_layout.addStretch(1)

        self.plot_splitter = QtWidgets.QSplitter(QtCore.Qt.Vertical)
        self.plot_splitter.addWidget(self.x_plot)
        self.plot_splitter.addWidget(self.y_plot)
        self.plot_splitter.setStretchFactor(0, 3)
        self.plot_splitter.setStretchFactor(1, 1)
        self.plot_splitter.setSizes([3, 1])

        self.srkr_plot_widget = QtWidgets.QWidget()
        self.srkr_plot_layout = QtWidgets.QGridLayout(self.srkr_plot_widget)
        self.srkr_plot_layout.setContentsMargins(8, 8, 8, 8)
        self.srkr_plot_layout.setHorizontalSpacing(10)
        self.srkr_plot_layout.setVerticalSpacing(10)
        self.srkr_plot_layout.addWidget(self.srkr_x_x_plot, 0, 0)
        self.srkr_plot_layout.addWidget(self.srkr_y_x_plot, 0, 1)
        self.srkr_plot_layout.addWidget(self.srkr_x_y_plot, 1, 0)
        self.srkr_plot_layout.addWidget(self.srkr_y_y_plot, 1, 1)
        self.srkr_plot_layout.setColumnStretch(0, 1)
        self.srkr_plot_layout.setColumnStretch(1, 1)
        self.srkr_plot_layout.setRowStretch(0, 3)
        self.srkr_plot_layout.setRowStretch(1, 1)

        self.plot_stack = QtWidgets.QStackedWidget()
        self.plot_stack.addWidget(self.plot_splitter)
        self.plot_stack.addWidget(self.srkr_plot_widget)

        left_scroll = QtWidgets.QScrollArea()
        left_scroll.setWidgetResizable(True)
        left_scroll.setHorizontalScrollBarPolicy(QtCore.Qt.ScrollBarAlwaysOff)
        left_scroll.setWidget(left_panel)
        left_scroll.setMinimumWidth(384)

        center_panel = QtWidgets.QWidget()
        center_layout = QtWidgets.QVBoxLayout(center_panel)
        center_layout.addWidget(measurement_group, 0)
        center_layout.addWidget(self.plot_stack, 1)

        central = QtWidgets.QWidget()
        layout = QtWidgets.QHBoxLayout(central)
        main_splitter = QtWidgets.QSplitter(QtCore.Qt.Horizontal)
        main_splitter.addWidget(left_scroll)
        main_splitter.addWidget(center_panel)
        main_splitter.setStretchFactor(0, 0)
        main_splitter.setStretchFactor(1, 1)
        main_splitter.setSizes([max(384, int(self.width() * 0.20)), max(1, int(self.width() * 0.80))])
        layout.addWidget(main_splitter, 1)
        self.setCentralWidget(central)

    def _harmonize_form_label_widths(self, *forms: QtWidgets.QFormLayout):
        labels = []
        for form in forms:
            for row in range(form.rowCount()):
                item = form.itemAt(row, QtWidgets.QFormLayout.LabelRole)
                if item is None:
                    continue
                widget = item.widget()
                if isinstance(widget, QtWidgets.QLabel):
                    labels.append(widget)
        if not labels:
            return
        width = max(label.sizeHint().width() for label in labels)
        for label in labels:
            label.setMinimumWidth(width)

    def _snapshot_config(self) -> tuple[list[str], list[str]]:
        measurement_name = self._measurement_name()
        if measurement_name == "SRKR":
            keys = [
                "scan_axis",
                "x_um",
                "x_cor_um",
                "x_scanner_mm",
                "y_um",
                "y_cor_um",
                "y_scanner_mm",
                "X_V",
                "Y_V",
                "R_V",
                "Theta_deg",
            ]
        elif measurement_name == "signal_monitor":
            keys = [
                "elapsed_s",
                "X_V",
                "Y_V",
                "R_V",
                "Theta_deg",
            ]
        else:
            keys = [
                "scan_axis",
                "t_ps",
                "t_cor_ps",
                "delay_stage_mm",
                "delay_stage_pulse",
                "X_V",
                "Y_V",
                "R_V",
                "Theta_deg",
            ]
        return keys, list(keys)

    def _rebuild_snapshot_table(self):
        self.snapshot_keys, self.snapshot_labels = self._snapshot_config()
        self.snapshot_table.setRowCount(len(self.snapshot_keys))
        for row, label in enumerate(self.snapshot_labels):
            self.snapshot_table.setItem(row, 0, QtWidgets.QTableWidgetItem(label))
            self.snapshot_table.setItem(row, 1, QtWidgets.QTableWidgetItem("-"))

    def _connect_signals(self):
        self.config_browse_button.clicked.connect(self.choose_config_file)
        self.config_load_button.clicked.connect(self.load_config_file)
        self.config_save_button.clicked.connect(self.save_config_file)
        self.output_browse_button.clicked.connect(self.choose_output_dir)
        self.connect_all_button.clicked.connect(self.test_connections)
        self.disconnect_all_button.clicked.connect(self.disconnect_all_devices)
        self.start_button.clicked.connect(self.start_measurement)
        self.stop_button.clicked.connect(self.stop_measurement)
        self.clear_button.clicked.connect(self.clear_plot)
        self.save_button.clicked.connect(self.save_rows_now)
        self.wait_default_button.clicked.connect(self.set_wait_from_lockin)
        self.t_zero_current_button.clicked.connect(self.set_t_zero_from_current)
        self.move_t_button.clicked.connect(self.move_t_absolute)
        self.move_t_corrected_button.clicked.connect(self.move_t_corrected)
        self.move_t_coordinate_combo.currentTextChanged.connect(self._refresh_move_abs_coordinate_ui)
        self.srkr_move_button.clicked.connect(self.move_srkr_absolute)
        self.srkr_corrected_move_button.clicked.connect(self.move_srkr_corrected)
        self.srkr_current_button.clicked.connect(self.set_srkr_offset_from_current)
        self.srkr_wait_default_button.clicked.connect(self.set_srkr_wait_from_lockin)
        self.srkr_axis_combo.currentTextChanged.connect(self._handle_srkr_axis_change)
        self.srkr_coordinate_combo.currentTextChanged.connect(self._sync_srkr_axis_ui)
        self.srkr_offset_spin.valueChanged.connect(self._sync_srkr_offset_value_from_spin)
        self.srkr_offset_spin.editingFinished.connect(self._sync_srkr_offset_value_from_spin)
        self.srkr_move_spin.valueChanged.connect(self._sync_srkr_move_value_from_spin)
        self.srkr_move_spin.editingFinished.connect(self._sync_srkr_move_value_from_spin)
        self.srkr_corrected_move_spin.valueChanged.connect(self._sync_srkr_corrected_move_value_from_spin)
        self.srkr_corrected_move_spin.editingFinished.connect(self._sync_srkr_corrected_move_value_from_spin)
        for axis in ("x", "y"):
            self.scanner_move_buttons[axis].clicked.connect(
                lambda checked=False, axis=axis: self.move_scanner_absolute(axis)
            )
            self.scanner_offset_buttons[axis].clicked.connect(
                lambda checked=False, axis=axis: self.set_scanner_offset_from_current(axis)
            )
            self.scanner_corrected_move_buttons[axis].clicked.connect(
                lambda checked=False, axis=axis: self.move_scanner_corrected(axis)
            )
            self.scanner_offset_spins[axis].valueChanged.connect(
                lambda _value, axis=axis: self._sync_scanner_offset_from_spin(axis)
            )
            self.scanner_offset_spins[axis].editingFinished.connect(
                lambda axis=axis: self._sync_scanner_offset_from_spin(axis)
            )
            self.scanner_move_spins[axis].valueChanged.connect(
                lambda _value, axis=axis: self._sync_scanner_move_from_spin(axis)
            )
            self.scanner_move_spins[axis].editingFinished.connect(
                lambda axis=axis: self._sync_scanner_move_from_spin(axis)
            )
            self.scanner_corrected_move_spins[axis].valueChanged.connect(
                lambda _value, axis=axis: self._sync_scanner_corrected_move_from_spin(axis)
            )
            self.scanner_corrected_move_spins[axis].editingFinished.connect(
                lambda axis=axis: self._sync_scanner_corrected_move_from_spin(axis)
            )
            self.scanner_coordinate_combos[axis].currentTextChanged.connect(
                self._refresh_move_abs_coordinate_ui
            )
        self.signal_mode_combo.currentTextChanged.connect(self._refresh_signal_view)
        self.measurement_tabs.currentChanged.connect(self._refresh_measurement_view)
        self._status_timer.timeout.connect(self.refresh_live_status)
        self.lockin_panel.connect_button.clicked.connect(self.connect_lockin_only)
        self.xy_panel.connect_button.clicked.connect(self.connect_xy_only)
        self.t_panel.connect_button.clicked.connect(self.connect_t_only)
        self.lockin_panel.disconnect_button.clicked.connect(self.disconnect_lockin_only)
        self.xy_panel.disconnect_button.clicked.connect(self.disconnect_xy_only)
        self.t_panel.disconnect_button.clicked.connect(self.disconnect_t_only)
        self.xy_panel.initialize_button.clicked.connect(self.initialize_xy)
        self.t_panel.initialize_button.clicked.connect(self.initialize_t)
        self.lockin_panel.settings_button.clicked.connect(self.open_settings_placeholder)
        self.xy_panel.settings_button.clicked.connect(self.open_settings_placeholder)
        self.t_panel.settings_button.clicked.connect(self.open_settings_placeholder)
        self.lockin_refresh_button.clicked.connect(self.refresh_lockin_resources)
        self.xy_controller_combo.currentTextChanged.connect(self._update_xy_port_mode)
        self.x_actuator_combo.currentTextChanged.connect(self._refresh_move_abs_coordinate_ui)
        self.y_actuator_combo.currentTextChanged.connect(self._refresh_move_abs_coordinate_ui)
        self.t_controller_combo.currentTextChanged.connect(self._apply_delay_stage_controller_defaults)
        self.xy_shared_port_check.toggled.connect(self._update_xy_port_mode)
        self.xy_shared_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.x_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.y_port_refresh_button.clicked.connect(self.refresh_all_ports)
        self.t_port_refresh_button.clicked.connect(self.refresh_all_ports)

    def append_log(self, message: str):
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.event_log.appendPlainText(f"[{timestamp}] {message}")

    def _refresh_port_combo(self, combo: QtWidgets.QComboBox, current_port: str):
        combo.clear()
        ports = sorted(list_ports.comports(), key=lambda port: port.device)
        for port in ports:
            label = port.device
            if port.description and port.description != "n/a":
                label = f"{port.device} - {port.description}"
            combo.addItem(label, port.device)
        if not ports:
            if current_port:
                combo.addItem(current_port, current_port)
            return
        index = combo.findData(current_port)
        if index >= 0:
            combo.setCurrentIndex(index)
        elif current_port:
            combo.addItem(current_port, current_port)
            combo.setCurrentIndex(combo.count() - 1)

    def _set_combo_value(self, combo: QtWidgets.QComboBox, value: str):
        text = (value or "").strip()
        if not text:
            return

        index = combo.findData(text)
        if index < 0:
            index = combo.findText(text)
        if index >= 0:
            combo.setCurrentIndex(index)
            return

        combo.addItem(text, text)
        combo.setCurrentIndex(combo.count() - 1)

    def _is_agap_controller(self) -> bool:
        return self.xy_controller_combo.currentText().strip().upper() == "CONEXAGAP"

    def _axis_options_for_controller(self) -> list[str]:
        if self._is_agap_controller():
            return ["U", "V"]
        return [str(index) for index in range(1, 9)]

    def _selected_axis_value(self, combo: QtWidgets.QComboBox):
        text = combo.currentText().strip()
        if self._is_agap_controller():
            return text.upper() or "U"
        try:
            return int(text)
        except ValueError:
            return 1

    def _set_axis_value(self, combo: QtWidgets.QComboBox, value):
        text = str(value).strip().upper() if self._is_agap_controller() else str(int(value))
        index = combo.findText(text)
        if index >= 0:
            combo.setCurrentIndex(index)
            return
        combo.addItem(text)
        combo.setCurrentIndex(combo.count() - 1)

    def _configure_xy_axis_widgets(self):
        current_x = self.x_axis_combo.currentText().strip() or "1"
        current_y = self.y_axis_combo.currentText().strip() or "1"
        options = self._axis_options_for_controller()

        self.x_axis_combo.blockSignals(True)
        self.y_axis_combo.blockSignals(True)
        self.x_axis_combo.clear()
        self.y_axis_combo.clear()
        self.x_axis_combo.addItems(options)
        self.y_axis_combo.addItems(options)

        if self._is_agap_controller():
            self._set_axis_value(self.x_axis_combo, current_x if current_x in {"U", "V"} else "U")
            self._set_axis_value(self.y_axis_combo, current_y if current_y in {"U", "V"} else "V")
        else:
            self._set_axis_value(self.x_axis_combo, current_x if current_x.isdigit() else 1)
            self._set_axis_value(self.y_axis_combo, current_y if current_y.isdigit() else 2)

        self.x_axis_combo.blockSignals(False)
        self.y_axis_combo.blockSignals(False)

    def _apply_xy_controller_defaults(self):
        actuator_items = list_actuators(self.xy_controller_combo.currentText())
        default_actuator = actuator_items[0] if actuator_items else ""
        for combo in (self.x_actuator_combo, self.y_actuator_combo):
            current = combo.currentText().strip()
            if current not in actuator_items:
                current = default_actuator
            _replace_combo_items(combo, actuator_items, current)

    def _apply_delay_stage_controller_defaults(self):
        stage_items = list_stages(self.t_controller_combo.currentText())
        current = normalize_delay_stage_name(self.t_stage_combo.currentText().strip() or None) or ""
        if current not in stage_items:
            current = stage_items[0] if stage_items else ""
        _replace_combo_items(self.t_stage_combo, stage_items, current)

    def refresh_lockin_resources(self):
        current = self.lockin_resource_combo.currentText().strip()
        self.lockin_resource_combo.clear()
        try:
            resources = list_visa_resources()
        except Exception as e:
            self.lockin_resource_combo.addItem(current)
            self.append_log(f"VISA resource refresh failed: {e}")
            return

        for resource in resources:
            self.lockin_resource_combo.addItem(resource, resource)

        index = self.lockin_resource_combo.findData(current)
        if index >= 0:
            self.lockin_resource_combo.setCurrentIndex(index)
        elif current:
            self.lockin_resource_combo.setEditText(current)
        self.append_log("VISA resource list refreshed.")

    def refresh_all_ports(self):
        self.refresh_lockin_resources()
        self._refresh_port_combo(self.xy_shared_port_combo, self._selected_port(self.xy_shared_port_combo))
        self._refresh_port_combo(self.x_port_combo, self._selected_port(self.x_port_combo))
        self._refresh_port_combo(self.y_port_combo, self._selected_port(self.y_port_combo))
        self._refresh_port_combo(self.t_port_combo, self._selected_port(self.t_port_combo))
        self._update_xy_port_mode()
        self.append_log("COM port list refreshed.")

    def _update_xy_port_mode(self):
        self._apply_xy_controller_defaults()
        self._configure_xy_axis_widgets()
        shared = self.xy_shared_port_check.isChecked()
        self.xy_shared_port_combo.setEnabled(shared)
        self.xy_shared_port_refresh_button.setEnabled(shared)
        self.x_port_combo.setEnabled(not shared)
        self.x_port_refresh_button.setEnabled(not shared)
        self.y_port_combo.setEnabled(not shared)
        self.y_port_refresh_button.setEnabled(not shared)
        axis_enabled = shared or self._is_agap_controller()
        self.x_axis_combo.setEnabled(axis_enabled)
        self.y_axis_combo.setEnabled(axis_enabled)
        if not shared and not self._is_agap_controller():
            self._set_axis_value(self.x_axis_combo, 1)
            self._set_axis_value(self.y_axis_combo, 1)
        self._refresh_move_abs_coordinate_ui()
        self._sync_srkr_axis_ui()

    def _selected_port(self, combo: QtWidgets.QComboBox) -> str:
        text = combo.currentText().strip()
        if text:
            if " - " in text:
                return text.split(" - ", 1)[0].strip()
            return text
        data = combo.currentData()
        return str(data or "")

    def choose_config_file(self):
        path_str, _ = QtWidgets.QFileDialog.getOpenFileName(
            self,
            "Load Config",
            str(CONFIG_DIR if CONFIG_DIR.exists() else Path.cwd()),
            "JSON Files (*.json);;All Files (*)",
        )
        if path_str:
            self.config_path_edit.setText(path_str)

    def load_config_file(self):
        path_str = self.config_path_edit.text().strip()
        if not path_str:
            QtWidgets.QMessageBox.warning(self, "Config Required", "Choose a config file.")
            return

        path = Path(path_str)
        data = json.loads(path.read_text(encoding="utf-8"))
        self._loaded_config = data

        loaded = extract_loaded_gui_config(data)
        self._instrument_refs = loaded.instrument_refs
        lockin_config = loaded.lockin_config
        x_config = loaded.x_config
        y_config = loaded.y_config
        t_config = loaded.t_config
        trkr_config = loaded.trkr_config
        signal_monitor_config = loaded.signal_monitor_config
        srkr_config = loaded.srkr_config
        move_abs_measurement = loaded.move_abs_config
        shared_port = loaded.shared_xy_port

        self.lockin_model_combo.setCurrentText(lockin_config.get("model", self.lockin_model_combo.currentText()))
        self._set_combo_value(self.lockin_resource_combo, lockin_config.get("resource", ""))
        self.xy_controller_combo.setCurrentText(x_config.get("controller", self.xy_controller_combo.currentText()))
        self._apply_xy_controller_defaults()
        self.xy_shared_port_check.setChecked(bool(shared_port))
        self._set_combo_value(self.xy_shared_port_combo, x_config.get("port", "") if shared_port else "")
        _set_combo_text(self.x_actuator_combo, x_config.get("actuator", self.x_actuator_combo.currentText()))
        self._set_axis_value(self.x_axis_combo, x_config.get("axis", self.x_axis_combo.currentText() or 1))
        self.x_scale_spin.setValue(
            _scanner_scale_value(x_config, self.x_scale_spin.value())
        )
        self._set_combo_value(self.x_port_combo, x_config.get("port", ""))
        _set_combo_text(self.y_actuator_combo, y_config.get("actuator", self.y_actuator_combo.currentText()))
        self._set_axis_value(self.y_axis_combo, y_config.get("axis", self.y_axis_combo.currentText() or 1))
        self.y_scale_spin.setValue(
            _scanner_scale_value(y_config, self.y_scale_spin.value())
        )
        self._set_combo_value(self.y_port_combo, y_config.get("port", ""))
        self.t_controller_combo.setCurrentText(t_config.get("controller", self.t_controller_combo.currentText()))
        self._apply_delay_stage_controller_defaults()
        _set_combo_text(self.t_stage_combo, normalize_delay_stage_name(t_config.get("stage", self.t_stage_combo.currentText())) or "")
        self.t_direction_spin.setValue(int(t_config.get("direction", self.t_direction_spin.value())))
        self._set_combo_value(self.t_port_combo, t_config.get("port", ""))
        trkr_scan = trkr_config.get("scan", {}) if isinstance(trkr_config.get("scan", {}), dict) else {}
        self.scan_min_spin.setValue(float(trkr_scan.get("min", trkr_config.get("t_min_ps", self.scan_min_spin.value()))))
        self.scan_max_spin.setValue(float(trkr_scan.get("max", trkr_config.get("t_max_ps", self.scan_max_spin.value()))))
        self.scan_step_spin.setValue(float(trkr_scan.get("step", trkr_config.get("t_step_ps", self.scan_step_spin.value()))))
        _set_coordinate_value(self.trkr_coordinate_combo, trkr_config.get("coordinate"))
        _set_coordinate_value(self.move_t_coordinate_combo, _coordinate_value(self.trkr_coordinate_combo))
        trkr_zero = trkr_config.get("zero", {})
        shared_zero = move_abs_measurement.get("zero", {})
        self.t_zero_spin.setValue(
            float(
                trkr_config.get(
                    "t_zero_ps",
                    trkr_zero.get("t_ps", shared_zero.get("t_ps", self.t_zero_spin.value()) if isinstance(shared_zero, dict) else self.t_zero_spin.value()),
                )
            )
        )
        self._set_status_value(self.current_t_offset_label, self.t_zero_spin.value())
        self.wait_s_spin.setValue(float(trkr_config.get("wait_s", self.wait_s_spin.value())))
        self.trkr_return_to_zero_check.setChecked(bool(trkr_config.get("return_to_zero", True)))
        self._output_settings_by_measurement["TRKR"] = _output_settings_from_measurement(
            trkr_config, self._output_settings_by_measurement["TRKR"], self.output_dir_edit.text().strip() or str(Path.cwd())
        )
        self.signal_monitor_interval_spin.setValue(float(signal_monitor_config.get("interval_s", self.signal_monitor_interval_spin.value())))
        self.signal_monitor_points_spin.setValue(int(signal_monitor_config.get("n_points", self.signal_monitor_points_spin.value())))
        self._output_settings_by_measurement["signal_monitor"] = _output_settings_from_measurement(
            signal_monitor_config, self._output_settings_by_measurement["signal_monitor"], self.output_dir_edit.text().strip() or str(Path.cwd())
        )
        srkr_zero = srkr_config.get("zero", shared_zero if isinstance(shared_zero, dict) else {})
        srkr_scan = srkr_config.get("scan", {}) if isinstance(srkr_config.get("scan", {}), dict) else {}
        srkr_x_config = srkr_config.get("x", srkr_config)
        srkr_y_config = srkr_config.get("y", srkr_config)
        x_zero_default = _zero_um_from_config(srkr_zero, "x", self._srkr_corrected_origins["x"])
        y_zero_default = _zero_um_from_config(srkr_zero, "y", self._srkr_corrected_origins["y"])
        self.srkr_min_spin.setValue(float(srkr_scan.get("min", srkr_config.get("minimum", srkr_x_config.get("minimum", self.srkr_min_spin.value())))))
        self.srkr_max_spin.setValue(float(srkr_scan.get("max", srkr_config.get("maximum", srkr_x_config.get("maximum", self.srkr_max_spin.value())))))
        self.srkr_step_spin.setValue(float(srkr_scan.get("step", srkr_config.get("step", srkr_x_config.get("step", self.srkr_step_spin.value())))))
        _set_coordinate_value(self.srkr_coordinate_combo, srkr_config.get("coordinate"))
        self.srkr_axis_combo.setCurrentText(str(srkr_scan.get("axis", self.srkr_axis_combo.currentText())).lower())
        for combo in self.scanner_coordinate_combos.values():
            _set_coordinate_value(combo, _coordinate_value(self.srkr_coordinate_combo))
        self._srkr_corrected_origins["x"] = _first_number(
            srkr_x_config.get("corrected_origin"),
            srkr_x_config.get("offset"),
            srkr_x_config.get("center"),
            srkr_x_config.get("zero"),
            x_zero_default,
            default=x_zero_default,
        )
        self._srkr_corrected_origins["y"] = _first_number(
            srkr_y_config.get("corrected_origin"),
            srkr_y_config.get("offset"),
            srkr_y_config.get("center"),
            srkr_y_config.get("zero"),
            y_zero_default,
            default=y_zero_default,
        )
        self._sync_srkr_axis_ui()
        self._apply_srkr_offsets_to_status()
        self.srkr_wait_spin.setValue(float(srkr_config.get("wait_s", self.srkr_wait_spin.value())))
        self.srkr_return_to_zero_check.setChecked(bool(srkr_config.get("return_to_zero", True)))
        self._output_settings_by_measurement["SRKR"] = _output_settings_from_measurement(
            srkr_config, self._output_settings_by_measurement["SRKR"], self.output_dir_edit.text().strip() or str(Path.cwd())
        )
        self._apply_output_settings(self._measurement_name())
        self._update_xy_port_mode()
        self._refresh_measurement_view()
        self.append_log(f"Loaded config: {path}")

    def save_config_file(self):
        self._save_current_output_settings()
        current_path = self.config_path_edit.text().strip()
        if current_path:
            path = Path(current_path)
        else:
            default_path = DEFAULT_CONFIG_PATH
            path_str, _ = QtWidgets.QFileDialog.getSaveFileName(
                self,
                "Save Config",
                str(default_path),
                "JSON Files (*.json)",
            )
            if not path_str:
                return
            path = Path(path_str)
            self.config_path_edit.setText(str(path))

        config = self._collect_config()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(config, indent=2), encoding="utf-8")
        self.append_log(f"Saved config: {path}")

    def choose_output_dir(self):
        path_str = QtWidgets.QFileDialog.getExistingDirectory(
            self,
            "Choose Output Directory",
            self.output_dir_edit.text().strip() or str(Path.cwd()),
        )
        if path_str:
            self.output_dir_edit.setText(path_str)

    def _lockin_config(self) -> dict:
        resource = self.lockin_resource_combo.currentText().strip()
        if not resource:
            raise ValueError("Lock-in resource is empty.")
        return {
            "model": self.lockin_model_combo.currentText().strip(),
            "resource": resource,
        }

    def _lockin_ref_name(self, measurement_name: str | None = None) -> str:
        measurement_name = measurement_name or self._measurement_name()
        return lockin_key(self._instrument_refs, measurement_name)

    def _scanner_config(self, axis: str) -> dict:
        shared = self.xy_shared_port_check.isChecked()
        if axis == "x":
            actuator = self.x_actuator_combo
            axis_combo = self.x_axis_combo
            scale_spin = self.x_scale_spin
            port_combo = self.xy_shared_port_combo if shared else self.x_port_combo
            source = self._loaded_config.get("instruments", {}).get("scanner", {}).get(self._instrument_refs["scanner"]["x"])
            if source is None:
                source = (
                    self._loaded_config.get("xy_scanner", {}).get("x")
                    or self._loaded_config.get("x_scanner")
                    or self._loaded_config.get("scanner1")
                )
        else:
            actuator = self.y_actuator_combo
            axis_combo = self.y_axis_combo
            scale_spin = self.y_scale_spin
            port_combo = self.xy_shared_port_combo if shared else self.y_port_combo
            source = self._loaded_config.get("instruments", {}).get("scanner", {}).get(self._instrument_refs["scanner"]["y"])
            if source is None:
                source = (
                    self._loaded_config.get("xy_scanner", {}).get("y")
                    or self._loaded_config.get("y_scanner")
                    or self._loaded_config.get("scanner2")
                )

        port = self._selected_port(port_combo)
        if not port:
            raise ValueError(f"{axis.upper()} scanner port is empty.")

        config = {
            "controller": self.xy_controller_combo.currentText().strip(),
            "actuator": actuator.currentText().strip(),
            "port": port,
            "axis": self._selected_axis_value(axis_combo) if (shared or self._is_agap_controller()) else 1,
            _scanner_scale_key({"actuator": actuator.currentText().strip()}): scale_spin.value(),
        }
        if isinstance(source, dict):
            merged = dict(source)
            merged.update(config)
            return merged
        return config

    def _delay_stage_config(self) -> dict:
        port = self._selected_port(self.t_port_combo)
        if not port:
            raise ValueError("Delay stage port is empty.")

        config = {
            "controller": self.t_controller_combo.currentText().strip(),
            "stage": normalize_delay_stage_name(self.t_stage_combo.currentText().strip() or None),
            "port": port,
            "direction": self.t_direction_spin.value(),
        }

        source = self._loaded_config.get("instruments", {}).get("delay_stage", {}).get(self._instrument_refs["delay_stage"]["TRKR"])
        if source is None:
            source = self._loaded_config.get("delay_stage") or self._loaded_config.get("delayline")
        if isinstance(source, dict):
            merged = dict(source)
            merged.update({key: value for key, value in config.items() if value is not None})
            return merged
        return config

    def _output_path(self) -> Path:
        return build_output_path(self._current_output_settings())

    def _current_output_settings(self) -> dict:
        return output_settings_from_fields(
            output_dir=self.output_dir_edit.text(),
            filename=self.output_name_edit.text(),
            auto_timestamp_suffix=self.auto_suffix_check.isChecked(),
            default_dir=Path.cwd(),
        )

    def _save_current_output_settings(self, measurement_name: str | None = None):
        self._output_settings_by_measurement[measurement_name or self._measurement_name()] = self._current_output_settings()

    def _apply_output_settings(self, measurement_name: str):
        settings = normalize_output_settings(
            self._output_settings_by_measurement.get(measurement_name),
            default_dir=Path.cwd(),
        )
        self.output_dir_edit.setText(str(settings.get("output_dir", str(Path.cwd()))))
        self.output_name_edit.setText(str(settings.get("filename", "trkr_run")))
        self.auto_suffix_check.setChecked(bool(settings.get("auto_timestamp_suffix", True)))

    def _config_snapshot(self) -> GuiConfigSnapshot:
        return GuiConfigSnapshot(
            instrument_refs=self._instrument_refs,
            lockin_config=self._lockin_config(),
            scanner_configs={
                "x": self._scanner_config("x"),
                "y": self._scanner_config("y"),
            },
            delay_stage_config=self._delay_stage_config(),
            move_t_coordinate=_coordinate_value(self.move_t_coordinate_combo),
            trkr_coordinate=_coordinate_value(self.trkr_coordinate_combo),
            srkr_coordinate=_coordinate_value(self.srkr_coordinate_combo),
            t_zero_ps=self.t_zero_spin.value(),
            x_zero_um=self._srkr_corrected_origins["x"],
            y_zero_um=self._srkr_corrected_origins["y"],
            trkr_scan={
                "min": self.scan_min_spin.value(),
                "max": self.scan_max_spin.value(),
                "step": self.scan_step_spin.value(),
            },
            trkr_wait_s=self.wait_s_spin.value(),
            trkr_return_to_zero=self.trkr_return_to_zero_check.isChecked(),
            signal_monitor_interval_s=self.signal_monitor_interval_spin.value(),
            signal_monitor_n_points=self.signal_monitor_points_spin.value(),
            srkr_axis=self.srkr_axis_combo.currentText().lower(),
            srkr_scan={
                "min": self.srkr_min_spin.value(),
                "max": self.srkr_max_spin.value(),
                "step": self.srkr_step_spin.value(),
            },
            srkr_wait_s=self.srkr_wait_spin.value(),
            srkr_return_to_zero=self.srkr_return_to_zero_check.isChecked(),
            output_settings=self._output_settings_by_measurement,
        )

    def _collect_config(self) -> dict:
        return build_saved_config(self._config_snapshot())

    def _api_config_for_measurement(self, measurement_name: str) -> dict:
        return build_measurement_config(self._config_snapshot(), measurement_name)

    def _measurement_name(self) -> str:
        label = self.measurement_tabs.tabText(self.measurement_tabs.currentIndex())
        if label == "Signal Monitor":
            return "signal_monitor"
        return label

    def _measurement_label(self) -> str:
        measurement_name = self._measurement_name()
        if measurement_name == "signal_monitor":
            return "Signal Monitor"
        return measurement_name

    def _current_rows(self) -> list[dict]:
        return self._rows_by_measurement.setdefault(self._measurement_name(), [])

    def _ensure_experiment(self) -> Experiment:
        config = self._collect_config()
        if self._experiment is None:
            self._experiment = Experiment(config)
        else:
            self._experiment.config = config
        return self._experiment

    def _ensure_partial_experiment(self) -> Experiment:
        if self._experiment is not None:
            config = deepcopy(self._experiment.config)
        else:
            config = deepcopy(self._loaded_config) if isinstance(self._loaded_config, dict) else {}
        config.setdefault("instruments", {})
        config.setdefault("measurements", {})
        if self._experiment is None:
            self._experiment = Experiment(config)
        else:
            self._experiment.config = config
        return self._experiment

    def _set_experiment_instrument_config(self, kind: str, key: str, config: dict) -> Experiment:
        experiment = self._ensure_partial_experiment()
        instruments = experiment.config.setdefault("instruments", {})
        instruments.setdefault(kind, {})
        instruments[kind][key] = config
        return experiment

    def _apply_position_model(self, position: Position, *, reset_xy_zero: bool = False):
        self._update_position_status(
            t_ps=position.t_ps,
            stage_mm=position.delay_stage_mm,
            stage_pulse=position.delay_stage_pulse,
            x_um=position.x_um,
            y_um=position.y_um,
            x_pos=position.scanner_x_value,
            y_pos=position.scanner_y_value,
            x_unit=position.scanner_x_unit,
            y_unit=position.scanner_y_unit,
            reset_xy_zero=reset_xy_zero,
        )

    def _lockin_handle(self, measurement_name: str = "TRKR"):
        if self._experiment is None:
            return None
        key = self._lockin_ref_name(measurement_name)
        return self._experiment.session.lockins.get(key) or next(iter(self._experiment.session.lockins.values()), None)

    def _delay_stage_handle(self):
        if self._experiment is None:
            return None
        key = delay_stage_key(self._instrument_refs)
        return self._experiment.session.delay_stages.get(key)

    def _scanner_handle(self, axis: str):
        if self._experiment is None:
            return None
        key = scanner_key(self._instrument_refs, axis)
        return self._experiment.session.scanners.get(key)

    def _connected_refs(self) -> dict[str, bool]:
        if self._experiment is None:
            return {}
        return self._experiment.connected_devices()

    def _has_connected_lockin(self) -> bool:
        return any(ref.startswith("lockin.") and connected for ref, connected in self._connected_refs().items())

    def _has_connected_delay_stage(self) -> bool:
        return any(ref.startswith("delay_stage.") and connected for ref, connected in self._connected_refs().items())

    def _has_connected_scanner(self) -> bool:
        return any(ref.startswith("scanner.") and connected for ref, connected in self._connected_refs().items())

    def _sync_connected_flags_from_experiment(self):
        self._lockin_connected = self._has_connected_lockin()
        self._t_connected = self._has_connected_delay_stage()
        self._xy_connected = self._has_connected_scanner()

    def _delay_stage_unit_for_coordinate(self, coordinate: str) -> str:
        return delay_stage_unit_for_coordinate(coordinate)

    def _scanner_unit_for_coordinate(self, axis: str, coordinate: str) -> str:
        selected = self.x_actuator_combo.currentText() if axis == "x" else self.y_actuator_combo.currentText()
        scanner = self._scanner_handle(axis)
        connected_unit = None
        if scanner is not None:
            connected_unit = self._current_scanner_units.get(axis) or scanner.get_pos_unit()
        return scanner_unit_for_coordinate(coordinate, actuator=selected, connected_unit=connected_unit)

    def _scanner_label_for_coordinate(self, axis: str, coordinate: str) -> str:
        selected = self.x_actuator_combo.currentText() if axis == "x" else self.y_actuator_combo.currentText()
        scanner = self._scanner_handle(axis)
        connected_unit = None
        if scanner is not None:
            connected_unit = self._current_scanner_units.get(axis) or scanner.get_pos_unit()
        return scanner_label_for_coordinate(axis, coordinate, actuator=selected, connected_unit=connected_unit)

    def _delay_stage_label_for_coordinate(self, coordinate: str) -> str:
        return delay_stage_label_for_coordinate(coordinate)

    def _coordinate_correction_enabled(self, coordinate: str) -> bool:
        return coordinate_correction_enabled(coordinate)

    def _delay_stage_status_value_for_coordinate(self, coordinate: str) -> float | None:
        coordinate = coordinate.strip().lower()
        if coordinate in {"interface", "control"}:
            return self._current_stage_mm
        if coordinate in {"instrument", "device"}:
            return self._current_stage_pulse
        return self._current_t_ps

    def _scanner_status_value_for_coordinate(self, axis: str, coordinate: str) -> float | None:
        coordinate = coordinate.strip().lower()
        if coordinate in {"interface", "instrument", "control", "device"}:
            return self._current_scanner_pos.get(axis)
        return self._current_x_um if axis == "x" else self._current_y_um

    def _refresh_delay_stage_cache_if_needed(self, coordinate: str, *, force: bool = False) -> None:
        coordinate = coordinate.strip().lower()
        has_value = (
            (coordinate == "measurement" and self._current_t_ps is not None)
            or (coordinate in {"interface", "control"} and self._current_stage_mm is not None)
            or (coordinate in {"instrument", "device"} and self._current_stage_pulse is not None)
        )
        delay_stage = self._delay_stage_handle()
        if (has_value and not force) or delay_stage is None:
            return
        try:
            self._current_t_ps = float(delay_stage.get_delay_ps())
            self._current_stage_mm = float(delay_stage.get_pos_mm())
            self._current_stage_pulse = float(delay_stage.get_pulse())
        except Exception:
            return

    def _refresh_scanner_cache_if_needed(self, axis: str, coordinate: str, *, force: bool = False) -> None:
        coordinate = coordinate.strip().lower()
        current_um = self._current_x_um if axis == "x" else self._current_y_um
        has_value = (
            (coordinate == "measurement" and current_um is not None)
            or (coordinate in {"interface", "instrument", "control", "device"} and self._current_scanner_pos.get(axis) is not None)
        )
        scanner = self._scanner_handle(axis)
        if (has_value and not force) or scanner is None:
            return
        try:
            control_pos = _scanner_control_pos(scanner)
            sample_um = _scanner_sample_um(scanner)
            unit = scanner.get_pos_unit()
        except Exception:
            return
        self._current_scanner_pos[axis] = float(control_pos)
        self._current_scanner_units[axis] = unit
        if axis == "x":
            self._current_x_um = float(sample_um)
        else:
            self._current_y_um = float(sample_um)

    def _refresh_move_abs_coordinate_ui(self, *_args):
        force_refresh = bool(_args)
        t_coordinate = _coordinate_value(self.move_t_coordinate_combo)
        self._refresh_delay_stage_cache_if_needed(t_coordinate, force=force_refresh)
        self.move_t_live_title.setText(self._delay_stage_label_for_coordinate(t_coordinate))
        self._set_status_value(
            self.current_t_label,
            self._delay_stage_status_value_for_coordinate(t_coordinate),
        )

        for axis in ("x", "y"):
            coordinate = _coordinate_value(self.scanner_coordinate_combos[axis])
            self._refresh_scanner_cache_if_needed(axis, coordinate, force=force_refresh)
            self.scanner_live_titles[axis].setText(self._scanner_label_for_coordinate(axis, coordinate))
            self._set_status_value(
                self.current_x_label if axis == "x" else self.current_y_label,
                self._scanner_status_value_for_coordinate(axis, coordinate),
            )
        self._apply_coordinate_edit_locks()

    def _sync_srkr_axis_ui(self):
        axis = self.srkr_axis_combo.currentText().lower()
        if self._measurement_name() == "SRKR":
            self.current_scan_axis_label.setText(axis)
        coordinate = _coordinate_value(self.srkr_coordinate_combo)
        move_label = self._scanner_label_for_coordinate(axis, coordinate)
        offset_label = "x_offset (um)" if axis == "x" else "y_offset (um)"
        corrected_label = "x_cor (um)" if axis == "x" else "y_cor (um)"
        self.srkr_move_form.labelForField(self.srkr_move_row_container).setText(move_label)
        self.srkr_move_form.labelForField(self.srkr_offset_row_container).setText(offset_label)
        self.srkr_move_form.labelForField(self.srkr_corrected_move_row_container).setText(corrected_label)
        self.srkr_offset_spin.blockSignals(True)
        self.srkr_offset_spin.setValue(self._srkr_corrected_origins[axis])
        self.srkr_offset_spin.blockSignals(False)
        self.srkr_move_spin.blockSignals(True)
        self.srkr_move_spin.setValue(self._srkr_targets[axis])
        self.srkr_move_spin.blockSignals(False)
        self.srkr_corrected_move_spin.blockSignals(True)
        self.srkr_corrected_move_spin.setValue(self._srkr_corrected_targets[axis])
        self.srkr_corrected_move_spin.blockSignals(False)
        self._sync_scanner_axis_widgets()
        self._apply_coordinate_edit_locks()

    def _sync_scanner_axis_widgets(self):
        for axis in ("x", "y"):
            self.scanner_offset_spins[axis].blockSignals(True)
            self.scanner_offset_spins[axis].setValue(self._srkr_corrected_origins[axis])
            self.scanner_offset_spins[axis].blockSignals(False)
            self.scanner_move_spins[axis].blockSignals(True)
            self.scanner_move_spins[axis].setValue(self._srkr_targets[axis])
            self.scanner_move_spins[axis].blockSignals(False)
            self.scanner_corrected_move_spins[axis].blockSignals(True)
            self.scanner_corrected_move_spins[axis].setValue(self._srkr_corrected_targets[axis])
            self.scanner_corrected_move_spins[axis].blockSignals(False)
            coordinate = _coordinate_value(self.scanner_coordinate_combos[axis])
            self.scanner_live_titles[axis].setText(self._scanner_label_for_coordinate(axis, coordinate))

    def _apply_coordinate_edit_locks(self):
        idle = self._thread is None and self._device_thread is None
        t_correction_enabled = idle and self._coordinate_correction_enabled(_coordinate_value(self.move_t_coordinate_combo))
        for widget in (
            self.t_zero_spin,
            self.t_zero_current_button,
            self.move_t_corrected_spin,
            self.move_t_corrected_button,
        ):
            widget.setEnabled(t_correction_enabled)

        srkr_correction_enabled = idle and self._coordinate_correction_enabled(_coordinate_value(self.srkr_coordinate_combo))
        for widget in (
            self.srkr_offset_spin,
            self.srkr_current_button,
            self.srkr_corrected_move_spin,
            self.srkr_corrected_move_button,
        ):
            widget.setEnabled(srkr_correction_enabled)

        for axis in ("x", "y"):
            correction_enabled = idle and self._coordinate_correction_enabled(_coordinate_value(self.scanner_coordinate_combos[axis]))
            for widget in (
                self.scanner_offset_spins[axis],
                self.scanner_offset_buttons[axis],
                self.scanner_corrected_move_spins[axis],
                self.scanner_corrected_move_buttons[axis],
            ):
                widget.setEnabled(correction_enabled)

    def _apply_srkr_offsets_to_status(self):
        self._x_zero_um_current = self._srkr_corrected_origins["x"]
        self._y_zero_um_current = self._srkr_corrected_origins["y"]

    def _store_srkr_axis_values(self, axis: str):
        self.srkr_offset_spin.interpretText()
        self.srkr_move_spin.interpretText()
        self.srkr_corrected_move_spin.interpretText()
        self._srkr_corrected_origins[axis] = self.srkr_offset_spin.value()
        self._srkr_targets[axis] = self.srkr_move_spin.value()
        self._srkr_corrected_targets[axis] = self.srkr_corrected_move_spin.value()
        self._apply_srkr_offsets_to_status()

    def _handle_srkr_axis_change(self, axis_text: str):
        self._store_srkr_axis_values(self._srkr_active_axis)
        self._srkr_active_axis = axis_text.lower()
        self._sync_srkr_axis_ui()

    def _sync_srkr_offset_value_from_spin(self):
        axis = self.srkr_axis_combo.currentText().lower()
        self._srkr_corrected_origins[axis] = self.srkr_offset_spin.value()
        self._sync_scanner_axis_widgets()
        self._apply_srkr_offsets_to_status()
        x_scanner = self._scanner_handle("x")
        y_scanner = self._scanner_handle("y")
        if x_scanner is not None or y_scanner is not None:
            self._update_position_status(
                x_um=_scanner_sample_um(x_scanner) if x_scanner is not None else None,
                y_um=_scanner_sample_um(y_scanner) if y_scanner is not None else None,
            )

    def _sync_srkr_move_value_from_spin(self):
        axis = self.srkr_axis_combo.currentText().lower()
        self._srkr_targets[axis] = self.srkr_move_spin.value()
        self._sync_scanner_axis_widgets()

    def _sync_srkr_corrected_move_value_from_spin(self):
        axis = self.srkr_axis_combo.currentText().lower()
        self._srkr_corrected_targets[axis] = self.srkr_corrected_move_spin.value()
        self._sync_scanner_axis_widgets()

    def _sync_scanner_offset_from_spin(self, axis: str):
        axis = axis.lower()
        self._srkr_corrected_origins[axis] = self.scanner_offset_spins[axis].value()
        if self.srkr_axis_combo.currentText().lower() == axis:
            self.srkr_offset_spin.blockSignals(True)
            self.srkr_offset_spin.setValue(self._srkr_corrected_origins[axis])
            self.srkr_offset_spin.blockSignals(False)
        self._apply_srkr_offsets_to_status()
        x_scanner = self._scanner_handle("x")
        y_scanner = self._scanner_handle("y")
        if x_scanner is not None or y_scanner is not None:
            self._update_position_status(
                x_um=_scanner_sample_um(x_scanner) if x_scanner is not None else None,
                y_um=_scanner_sample_um(y_scanner) if y_scanner is not None else None,
            )

    def _sync_scanner_move_from_spin(self, axis: str):
        axis = axis.lower()
        self._srkr_targets[axis] = self.scanner_move_spins[axis].value()
        if self.srkr_axis_combo.currentText().lower() == axis:
            self.srkr_move_spin.blockSignals(True)
            self.srkr_move_spin.setValue(self._srkr_targets[axis])
            self.srkr_move_spin.blockSignals(False)

    def _sync_scanner_corrected_move_from_spin(self, axis: str):
        axis = axis.lower()
        self._srkr_corrected_targets[axis] = self.scanner_corrected_move_spins[axis].value()
        if self.srkr_axis_combo.currentText().lower() == axis:
            self.srkr_corrected_move_spin.blockSignals(True)
            self.srkr_corrected_move_spin.setValue(self._srkr_corrected_targets[axis])
            self.srkr_corrected_move_spin.blockSignals(False)

    def _set_srkr_offsets(self, *, x_um: float | None = None, y_um: float | None = None):
        if x_um is not None:
            self._srkr_corrected_origins["x"] = x_um
        if y_um is not None:
            self._srkr_corrected_origins["y"] = y_um
        self._apply_srkr_offsets_to_status()
        self._sync_srkr_axis_ui()

    def test_connections(self):
        try:
            experiment = self._ensure_experiment()
            experiment.connect_all()
            self._sync_connected_flags_from_experiment()
            lockin = self._lockin_handle("TRKR")
            x_scanner = self._scanner_handle("x")
            y_scanner = self._scanner_handle("y")
            delay_stage = self._delay_stage_handle()
            if lockin is None or x_scanner is None or y_scanner is None or delay_stage is None:
                raise RuntimeError("Failed to connect one or more required devices.")
            lockin_ref = device_ref("lockin", self._lockin_ref_name("TRKR"))
            live_data = experiment.read_lockin_signal(lockin_ref)
            self._update_voltage_display_from_settings(experiment.read_lockin_settings(lockin_ref))
            self._refresh_overload_display(experiment, lockin_ref)
            self._t_zero_ps_current = self.t_zero_spin.value()
            wait_s = experiment.lockin_wait_time(lockin_ref, multiplier=4.0)
            self.wait_s_spin.setValue(wait_s)
            self.srkr_wait_spin.setValue(wait_s)
            self.status_label.setText("connected")
            x_um = _scanner_sample_um(x_scanner)
            y_um = _scanner_sample_um(y_scanner)
            self._update_position_status(
                t_ps=delay_stage.get_delay_ps(),
                x_um=x_um,
                y_um=y_um,
                reset_xy_zero=True,
            )
            self._set_srkr_offsets(x_um=x_um, y_um=y_um)
            self._update_signal_status(live_data)
            self.append_log(
                f"Devices connected: X={x_um:.3f} um ({_scanner_control_pos(x_scanner):.6f} {x_scanner.get_pos_unit()}), "
                f"Y={y_um:.3f} um ({_scanner_control_pos(y_scanner):.6f} {y_scanner.get_pos_unit()}), "
                f"T={delay_stage.get_delay_ps():.6f} ps"
            )
        except Exception as e:
            self.append_log(f"Connection error: {e}")
            QtWidgets.QMessageBox.critical(self, "Connection Error", str(e))

    def connect_lockin_only(self):
        try:
            key = self._lockin_ref_name()
            experiment = self._set_experiment_instrument_config("lockin", key, self._lockin_config())
            experiment.connect_device(device_ref("lockin", key))
            self._sync_connected_flags_from_experiment()
            self._update_voltage_display_from_settings(experiment.read_lockin_settings(device_ref("lockin", key)))
            self._refresh_overload_display(experiment, device_ref("lockin", key))
            wait_s = experiment.lockin_wait_time(device_ref("lockin", key), multiplier=4.0)
            self.wait_s_spin.setValue(wait_s)
            self.srkr_wait_spin.setValue(wait_s)
            live_data = experiment.read_lockin_signal(device_ref("lockin", key))
            self._update_signal_status(live_data)
            self.append_log("Lock-in connected.")
        except Exception as e:
            self.append_log(f"Lock-in connection error: {e}")
            QtWidgets.QMessageBox.warning(self, "Lock-in Error", str(e))

    def connect_xy_only(self):
        try:
            x_key, y_key = scanner_keys(self._instrument_refs)
            experiment = self._set_experiment_instrument_config("scanner", x_key, self._scanner_config("x"))
            self._set_experiment_instrument_config("scanner", y_key, self._scanner_config("y"))
            x_scanner = experiment.connect_device(device_ref("scanner", x_key))
            y_scanner = experiment.connect_device(device_ref("scanner", y_key))
            self._sync_connected_flags_from_experiment()
            x_um = _scanner_sample_um(x_scanner)
            y_um = _scanner_sample_um(y_scanner)
            self._update_position_status(
                x_um=x_um,
                y_um=y_um,
                reset_xy_zero=True,
            )
            self._set_srkr_offsets(x_um=x_um, y_um=y_um)
            self.append_log(
                f"XY scanners connected: X={x_um:.3f} um ({_scanner_control_pos(x_scanner):.6f} {x_scanner.get_pos_unit()}), "
                f"Y={y_um:.3f} um ({_scanner_control_pos(y_scanner):.6f} {y_scanner.get_pos_unit()})"
            )
        except Exception as e:
            self.append_log(f"XY scanner connection error: {e}")
            QtWidgets.QMessageBox.warning(self, "XY Scanner Error", str(e))

    def connect_t_only(self):
        try:
            key = delay_stage_key(self._instrument_refs)
            experiment = self._set_experiment_instrument_config("delay_stage", key, self._delay_stage_config())
            delay_stage = experiment.connect_device(device_ref("delay_stage", key))
            self._sync_connected_flags_from_experiment()
            current_t_ps = delay_stage.get_delay_ps()
            self.t_zero_spin.setValue(current_t_ps)
            self._t_zero_ps_current = current_t_ps
            self._update_position_status(
                t_ps=current_t_ps,
                stage_mm=delay_stage.get_pos_mm(),
                stage_pulse=delay_stage.get_pulse(),
            )
            self.append_log(f"T scanner connected: T={current_t_ps:.6f} ps")
        except Exception as e:
            self.append_log(f"T scanner connection error: {e}")
            QtWidgets.QMessageBox.warning(self, "Delay Stage Error", str(e))

    def disconnect_all_devices(self):
        if self._experiment is not None:
            self._experiment.disconnect_all()
        else:
            disconnect_lockin()
            disconnect_scanner()
            disconnect_delay_stage()
        self._x_zero_um_current = None
        self._y_zero_um_current = None
        self._sync_connected_flags_from_experiment()
        self.status_label.setText("disconnected")
        self.current_x_offset_label.setText("-")
        self.current_x_cor_label.setText("-")
        self.current_y_offset_label.setText("-")
        self.current_y_cor_label.setText("-")
        self.append_log("All devices disconnected.")

    def disconnect_lockin_only(self):
        try:
            if self._experiment is not None:
                self._experiment.disconnect_device(device_ref("lockin", self._lockin_ref_name()))
            else:
                disconnect_lockin(self._lockin_config())
            self._sync_connected_flags_from_experiment()
            self.append_log("Lock-in disconnected.")
        except Exception as e:
            self.append_log(f"Lock-in disconnect error: {e}")

    def disconnect_xy_only(self):
        try:
            if self._experiment is not None:
                x_key, y_key = scanner_keys(self._instrument_refs)
                self._experiment.disconnect_device(device_ref("scanner", x_key))
                self._experiment.disconnect_device(device_ref("scanner", y_key))
            else:
                disconnect_scanner(self._scanner_config("x"))
                disconnect_scanner(self._scanner_config("y"))
            self._x_zero_um_current = None
            self._y_zero_um_current = None
            self._sync_connected_flags_from_experiment()
            self.current_x_offset_label.setText("-")
            self.current_x_cor_label.setText("-")
            self.current_y_offset_label.setText("-")
            self.current_y_cor_label.setText("-")
            self.append_log("XY scanners disconnected.")
        except Exception as e:
            self.append_log(f"XY disconnect error: {e}")

    def disconnect_t_only(self):
        try:
            if self._experiment is not None:
                self._experiment.disconnect_device(device_ref("delay_stage", delay_stage_key(self._instrument_refs)))
            else:
                disconnect_delay_stage(self._delay_stage_config())
            self._sync_connected_flags_from_experiment()
            self.append_log("T scanner disconnected.")
        except Exception as e:
            self.append_log(f"T disconnect error: {e}")

    def initialize_xy(self):
        if self._device_thread is not None:
            return
        try:
            x_key, y_key = scanner_keys(self._instrument_refs)
            config = xy_scanner_config(self._instrument_refs, self._scanner_config("x"), self._scanner_config("y"))
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
            return

        self._device_thread = QtCore.QThread(self)
        self._device_worker = XYInitializeWorker(
            config=config,
            x_ref=device_ref("scanner", x_key),
            y_ref=device_ref("scanner", y_key),
        )
        self._device_worker.moveToThread(self._device_thread)
        self._device_thread.started.connect(self._device_worker.run)
        self._device_worker.status_changed.connect(self.handle_status)
        self._device_worker.finished.connect(self.handle_xy_initialized)
        self._device_worker.error_occurred.connect(self.handle_xy_initialize_error)
        self._device_worker.finished.connect(self._device_thread.quit)
        self._device_worker.finished.connect(self._device_worker.deleteLater)
        self._device_worker.error_occurred.connect(self._device_thread.quit)
        self._device_worker.error_occurred.connect(self._device_worker.deleteLater)
        self._device_thread.finished.connect(self._device_thread.deleteLater)
        self._device_thread.finished.connect(self._cleanup_device_thread)

        self._set_device_busy_state(True)
        self.append_log("XY initialize started.")
        self._device_thread.start()

    def initialize_t(self):
        if self._device_thread is not None:
            return
        try:
            key = delay_stage_key(self._instrument_refs)
            config = single_instrument_config("delay_stage", key, self._delay_stage_config())
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
            return

        self._device_thread = QtCore.QThread(self)
        self._device_worker = DelayStageInitializeWorker(config=config, ref=device_ref("delay_stage", key))
        self._device_worker.moveToThread(self._device_thread)
        self._device_thread.started.connect(self._device_worker.run)
        self._device_worker.status_changed.connect(self.handle_status)
        self._device_worker.finished.connect(self.handle_delay_stage_initialized)
        self._device_worker.error_occurred.connect(self.handle_delay_stage_initialize_error)
        self._device_worker.finished.connect(self._device_thread.quit)
        self._device_worker.finished.connect(self._device_worker.deleteLater)
        self._device_worker.error_occurred.connect(self._device_thread.quit)
        self._device_worker.error_occurred.connect(self._device_worker.deleteLater)
        self._device_thread.finished.connect(self._device_thread.deleteLater)
        self._device_thread.finished.connect(self._cleanup_device_thread)

        self._set_device_busy_state(True)
        self.append_log("Delay stage initialize started.")
        self._device_thread.start()

    def open_settings_placeholder(self):
        self.append_log("Settings window is not implemented yet.")

    def set_srkr_wait_from_lockin(self):
        try:
            key = self._lockin_ref_name("SRKR")
            experiment = self._set_experiment_instrument_config("lockin", key, self._lockin_config())
            if key not in experiment.lockins:
                experiment.connect_device(device_ref("lockin", key))
            self._sync_connected_flags_from_experiment()
            wait_s = experiment.lockin_wait_time(device_ref("lockin", key), multiplier=4.0)
            self.srkr_wait_spin.setValue(wait_s)
            self.append_log(f"SRKR wait set from lock-in TC x 4: {wait_s:.3f} s")
        except Exception as e:
            self.append_log(f"Failed to read SRKR lock-in TC: {e}")
            QtWidgets.QMessageBox.warning(self, "Lock-in Error", str(e))

    def set_scanner_offset_from_current(self, axis: str):
        try:
            axis = axis.lower()
            key = scanner_key(self._instrument_refs, axis)
            experiment = self._set_experiment_instrument_config("scanner", key, self._scanner_config(axis))
            if key not in experiment.scanners:
                experiment.connect_device(device_ref("scanner", key))
            self._sync_connected_flags_from_experiment()
            position = experiment.read_position()
            value = position.x_um if axis == "x" else position.y_um
            if value is None:
                raise RuntimeError(f"{axis.upper()} scanner position is not available.")
            self._srkr_corrected_origins[axis] = float(value)
            self._sync_srkr_axis_ui()
            self.append_log(f"SRKR {axis.upper()} origin set from current {axis.upper()}: {value:.3f} um")
        except Exception as e:
            self.append_log(f"SRKR origin read error: {e}")
            QtWidgets.QMessageBox.warning(self, "SRKR Raw Origin Error", str(e))

    def set_srkr_offset_from_current(self):
        self.set_scanner_offset_from_current(self.srkr_axis_combo.currentText().lower())

    def _clear_srkr_axis_data(self, axis: str):
        srkr_rows = self._rows_by_measurement.setdefault("SRKR", [])
        srkr_rows[:] = [row for row in srkr_rows if row.get("scan_axis") != axis]
        if axis == "x":
            self.srkr_x_x_plot.getAxis("top").setTicks([])
            self.srkr_x_y_plot.getAxis("top").setTicks([])
            self.srkr_x_x_curve.setData([], [])
            self.srkr_x_y_curve.setData([], [])
        else:
            self.srkr_y_x_plot.getAxis("top").setTicks([])
            self.srkr_y_y_plot.getAxis("top").setTicks([])
            self.srkr_y_x_curve.setData([], [])
            self.srkr_y_y_curve.setData([], [])

    def move_scanner_absolute(self, axis: str):
        try:
            axis = axis.lower()
            self.scanner_move_spins[axis].interpretText()
            self._sync_scanner_move_from_spin(axis)
            key = scanner_key(self._instrument_refs, axis)
            experiment = self._set_experiment_instrument_config("scanner", key, self._scanner_config(axis))
            coordinate = _coordinate_value(self.scanner_coordinate_combos[axis])
            target = self._srkr_targets[axis]
            position = experiment.move_scanner(axis, target, coordinate=coordinate)
            self._sync_connected_flags_from_experiment()
            scanner = self._scanner_handle(axis)
            moved_um = position.x_um if axis == "x" else position.y_um
            if moved_um is None:
                raise RuntimeError(f"{axis.upper()} scanner did not report a measurement position.")
            self._apply_position_model(position)
            control_text = ""
            if scanner is not None:
                control_text = f" ({_scanner_control_pos(scanner):.6f} {scanner.get_pos_unit()})"
            self.append_log(
                f"Moved {axis.upper()} to {moved_um:.3f} um{control_text}"
            )
        except Exception as e:
            self.append_log(f"SRKR move error: {e}")
            QtWidgets.QMessageBox.warning(self, "SRKR Move Error", str(e))

    def move_srkr_absolute(self):
        self.srkr_move_spin.interpretText()
        self._sync_srkr_move_value_from_spin()
        self.move_scanner_absolute(self.srkr_axis_combo.currentText().lower())

    def move_scanner_corrected(self, axis: str):
        try:
            axis = axis.lower()
            self.scanner_offset_spins[axis].interpretText()
            self._sync_scanner_offset_from_spin(axis)
            self.scanner_corrected_move_spins[axis].interpretText()
            self._sync_scanner_corrected_move_from_spin(axis)
            key = scanner_key(self._instrument_refs, axis)
            experiment = self._set_experiment_instrument_config("scanner", key, self._scanner_config(axis))
            corrected_um = self._srkr_corrected_targets[axis]
            target_um = corrected_target(self._srkr_corrected_origins[axis], corrected_um)
            position = experiment.move_scanner(axis, target_um, coordinate="measurement")
            self._sync_connected_flags_from_experiment()
            scanner = self._scanner_handle(axis)
            moved_um = position.x_um if axis == "x" else position.y_um
            if moved_um is None:
                raise RuntimeError(f"{axis.upper()} scanner did not report a measurement position.")
            self._apply_position_model(position)
            control_text = ""
            if scanner is not None:
                control_text = f" ({_scanner_control_pos(scanner):.6f} {scanner.get_pos_unit()})"
            self.append_log(
                f"Moved {axis.upper()} cor to {corrected_um:.3f} um{control_text}"
            )
        except Exception as e:
            self.append_log(f"SRKR cor move error: {e}")
            QtWidgets.QMessageBox.warning(self, "SRKR Cor Move Error", str(e))

    def move_srkr_corrected(self):
        self.srkr_offset_spin.interpretText()
        self._sync_srkr_offset_value_from_spin()
        self.srkr_corrected_move_spin.interpretText()
        self._sync_srkr_corrected_move_value_from_spin()
        self.move_scanner_corrected(self.srkr_axis_combo.currentText().lower())

    def start_measurement(self):
        if self._thread is not None:
            return

        measurement_name = self._measurement_name()
        measurement_label = self._measurement_label()
        try:
            output_path = self._output_path()
        except Exception as e:
            QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
            return

        if measurement_name == "SRKR":
            self._clear_srkr_axis_data(self.srkr_axis_combo.currentText().lower())
        else:
            self._current_rows().clear()
        self._last_curve_update_perf = 0.0
        self._update_curves()

        self._thread = QtCore.QThread(self)
        if measurement_name == "signal_monitor":
            try:
                plan = signal_monitor_plan(
                    interval_s=self.signal_monitor_interval_spin.value(),
                    n_points=self.signal_monitor_points_spin.value(),
                )
                self._t_zero_ps_current = self.t_zero_spin.value()
                self._worker = SignalMonitorWorker(
                    config=self._api_config_for_measurement("signal_monitor"),
                    output_path=str(output_path),
                    interval_s=plan.interval_s,
                    n_points=plan.n_points,
                )
                summary = plan.summary
            except Exception as e:
                QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
                self._thread.deleteLater()
                self._thread = None
                return
        elif measurement_name == "SRKR":
            try:
                plan = srkr_plan(
                    axis=self.srkr_axis_combo.currentText(),
                    minimum_um=self.srkr_min_spin.value(),
                    maximum_um=self.srkr_max_spin.value(),
                    step_um=self.srkr_step_spin.value(),
                    zero_by_axis=self._srkr_corrected_origins,
                    coordinate=_coordinate_value(self.srkr_coordinate_combo),
                )
                self._worker = SrkrWorker(
                    config=self._api_config_for_measurement("SRKR"),
                    scan_axis_name=plan.axis,
                    coordinate=plan.coordinate,
                    scan_points=plan.scan_points,
                    zero=plan.zero,
                    wait_s=self.srkr_wait_spin.value(),
                    output_path=str(output_path),
                    return_to_zero=self.srkr_return_to_zero_check.isChecked(),
                )
                summary = plan.summary
            except Exception as e:
                QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
                self._thread.deleteLater()
                self._thread = None
                return
        else:
            try:
                plan = trkr_plan(
                    minimum_ps=self.scan_min_spin.value(),
                    maximum_ps=self.scan_max_spin.value(),
                    step_ps=self.scan_step_spin.value(),
                    t_zero_ps=self.t_zero_spin.value(),
                    coordinate=_coordinate_value(self.trkr_coordinate_combo),
                )
                self._t_zero_ps_current = plan.t_zero_ps
                self._worker = TrkrWorker(
                    config=self._api_config_for_measurement("TRKR"),
                    scan_points=plan.scan_points,
                    coordinate=plan.coordinate,
                    wait_s=self.wait_s_spin.value(),
                    output_path=str(output_path),
                    return_to_zero=self.trkr_return_to_zero_check.isChecked(),
                )
                summary = plan.summary
            except Exception as e:
                QtWidgets.QMessageBox.warning(self, "Invalid Settings", str(e))
                self._thread.deleteLater()
                self._thread = None
                return

        self._worker.moveToThread(self._thread)
        self._thread.started.connect(self._worker.run)
        self._worker.point_ready.connect(self.handle_point)
        self._worker.status_changed.connect(self.handle_status)
        self._worker.error_occurred.connect(self.handle_error)
        self._worker.finished.connect(self.handle_finished)
        self._worker.finished.connect(self._thread.quit)
        self._worker.finished.connect(self._worker.deleteLater)
        self._thread.finished.connect(self._thread.deleteLater)
        self._thread.finished.connect(self._cleanup_thread)

        self._set_running_state(True)
        self.append_log(f"Measurement started [{measurement_label}]: {summary} -> {output_path}")
        self._thread.start()

    def stop_measurement(self):
        if self._worker is not None:
            self._worker.stop()
            self.append_log("Stop requested.")

    def clear_plot(self):
        self._current_rows().clear()
        self._update_curves()
        self.current_scan_axis_label.setText("-")
        self.current_point_label.setText("-")
        for row in range(self.snapshot_table.rowCount()):
            self.snapshot_table.item(row, 1).setText("-")
        self.append_log("Cleared in-memory plot data.")

    def save_rows_now(self):
        rows = self._current_rows()
        if not rows:
            QtWidgets.QMessageBox.information(self, "No Data", "There is no in-memory data to save.")
            return
        path = self._output_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("w", newline="", encoding="utf-8") as f:
            writer = csv.DictWriter(f, fieldnames=list(rows[0].keys()))
            writer.writeheader()
            writer.writerows(output_rows(rows))
        self.append_log(f"Saved {len(rows)} rows to {path}")

    def set_wait_from_lockin(self):
        try:
            key = self._lockin_ref_name()
            experiment = self._set_experiment_instrument_config("lockin", key, self._lockin_config())
            if key not in experiment.lockins:
                experiment.connect_device(device_ref("lockin", key))
            self._sync_connected_flags_from_experiment()
            wait_s = experiment.lockin_wait_time(device_ref("lockin", key), multiplier=4.0)
            self.wait_s_spin.setValue(wait_s)
            self.append_log(f"Wait set from lock-in TC x 4: {wait_s:.3f} s")
        except Exception as e:
            self.append_log(f"Failed to read lock-in TC: {e}")
            QtWidgets.QMessageBox.warning(self, "Lock-in Error", str(e))

    def set_t_zero_from_current(self):
        try:
            key = delay_stage_key(self._instrument_refs)
            experiment = self._set_experiment_instrument_config("delay_stage", key, self._delay_stage_config())
            if key not in experiment.delay_stages:
                experiment.connect_device(device_ref("delay_stage", key))
            self._sync_connected_flags_from_experiment()
            position = experiment.read_position()
            current_t_ps = position.t_ps
            if current_t_ps is None:
                raise RuntimeError("Delay stage position is not available.")
            self.t_zero_spin.setValue(current_t_ps)
            self._t_zero_ps_current = current_t_ps
            self._apply_position_model(position)
            self.append_log(f"T origin set from current delay: {current_t_ps:.3f} ps")
        except Exception as e:
            self.append_log(f"Failed to read current T delay: {e}")
            QtWidgets.QMessageBox.warning(self, "T Raw Origin Error", str(e))

    def move_t_absolute(self):
        try:
            key = delay_stage_key(self._instrument_refs)
            experiment = self._set_experiment_instrument_config("delay_stage", key, self._delay_stage_config())
            coordinate = _coordinate_value(self.move_t_coordinate_combo)
            target = self.move_t_spin.value()
            position = experiment.move_delay_stage(target, coordinate=coordinate)
            self._sync_connected_flags_from_experiment()
            moved_t_ps = position.t_ps
            if moved_t_ps is None:
                raise RuntimeError("Delay stage did not report t_ps.")
            self._apply_position_model(position)
            self.append_log(f"Moved T to {moved_t_ps:.6f} ps")
        except Exception as e:
            self.append_log(f"T move error: {e}")
            QtWidgets.QMessageBox.warning(self, "T Move Error", str(e))

    def move_t_corrected(self):
        try:
            key = delay_stage_key(self._instrument_refs)
            experiment = self._set_experiment_instrument_config("delay_stage", key, self._delay_stage_config())
            corrected_ps = self.move_t_corrected_spin.value()
            target_ps = corrected_target(self.t_zero_spin.value(), corrected_ps)
            position = experiment.move_delay_stage(target_ps, coordinate="measurement")
            self._sync_connected_flags_from_experiment()
            moved_t_ps = position.t_ps
            if moved_t_ps is None:
                raise RuntimeError("Delay stage did not report t_ps.")
            self._apply_position_model(position)
            self.append_log(f"Moved T cor to {corrected_ps:.6f} ps")
        except Exception as e:
            self.append_log(f"T cor move error: {e}")
            QtWidgets.QMessageBox.warning(self, "T Cor Move Error", str(e))

    def refresh_live_status(self):
        if self._thread is not None:
            return

        if self._experiment is None:
            return

        now = time.perf_counter()

        if self._has_connected_lockin():
            try:
                key = self._lockin_ref_name()
                if now - self._last_live_settings_refresh_perf >= 5.0:
                    self._update_voltage_display_from_settings(self._experiment.read_lockin_settings(device_ref("lockin", key)))
                    self._refresh_overload_display(self._experiment, device_ref("lockin", key))
                    self._last_live_settings_refresh_perf = now
                if now - self._last_live_signal_refresh_perf >= 0.5:
                    self._update_signal_status(self._experiment.read_lockin_signal(device_ref("lockin", key)))
                    self._last_live_signal_refresh_perf = now
            except Exception:
                self._experiment.session.lockins.pop(key, None)
                self._sync_connected_flags_from_experiment()

        if now - self._last_live_position_refresh_perf < 0.5:
            return
        self._last_live_position_refresh_perf = now

        if self._has_connected_delay_stage() or self._has_connected_scanner():
            try:
                self._apply_position_model(self._experiment.read_position())
            except Exception:
                self._sync_connected_flags_from_experiment()

    @QtCore.Slot(object)
    def handle_point(self, point: MeasurementPoint):
        self._current_rows().append(point.row)
        self._t_zero_ps_current = self.t_zero_spin.value()
        self.current_point_label.setText(f"{point.index}/{point.total_points}")
        self.current_scan_axis_label.setText(str(point.row.get("scan_axis", "-")))
        self._update_position_status(
            t_ps=point.row.get("t_ps"),
            stage_mm=point.row.get("delay_stage_mm"),
            stage_pulse=point.row.get("delay_stage_pulse"),
            x_um=point.row.get("x_um"),
            y_um=point.row.get("y_um"),
            x_pos=point.row.get("x_scanner_mm", point.row.get("x_scanner_deg")),
            y_pos=point.row.get("y_scanner_mm", point.row.get("y_scanner_deg")),
            x_unit="deg" if point.row.get("x_scanner_deg") is not None else ("mm" if point.row.get("x_scanner_mm") is not None else None),
            y_unit="deg" if point.row.get("y_scanner_deg") is not None else ("mm" if point.row.get("y_scanner_mm") is not None else None),
        )
        self._update_signal_status(point.row)
        self._update_snapshot(point.row)
        now = time.perf_counter()
        if (
            point.index == point.total_points
            or now - self._last_curve_update_perf >= self._curve_update_interval_s
        ):
            self._update_curves()
            self._last_curve_update_perf = now

    @QtCore.Slot(str)
    def handle_status(self, status: str):
        self.status_label.setText(status)

    @QtCore.Slot(str)
    def handle_error(self, message: str):
        self.append_log(f"Error: {message}")
        QtWidgets.QMessageBox.critical(self, "TRKR Error", message)

    @QtCore.Slot(object)
    def handle_delay_stage_initialized(self, info: object):
        info_dict = dict(info)
        key = delay_stage_key(self._instrument_refs)
        experiment = self._set_experiment_instrument_config("delay_stage", key, self._delay_stage_config())
        experiment.connect_device(device_ref("delay_stage", key))
        self._sync_connected_flags_from_experiment()
        current_t_ps = float(info_dict["delay_ps"])
        self.t_zero_spin.setValue(current_t_ps)
        self._t_zero_ps_current = current_t_ps
        self._update_position_status(
            t_ps=current_t_ps,
            stage_mm=info_dict.get("pos_mm"),
            stage_pulse=info_dict.get("pulse"),
        )
        self.status_label.setText("connected")
        self.append_log(
            "Delay stage initialized: "
            f"ready={info_dict['ready']}, pos_mm={info_dict['pos_mm']:.6f}, "
            f"pulse={info_dict['pulse']}, delay={current_t_ps:.6f} ps"
        )

    @QtCore.Slot(object)
    def handle_xy_initialized(self, _info: object):
        x_key, y_key = scanner_keys(self._instrument_refs)
        experiment = self._set_experiment_instrument_config("scanner", x_key, self._scanner_config("x"))
        self._set_experiment_instrument_config("scanner", y_key, self._scanner_config("y"))
        x_scanner = experiment.connect_device(device_ref("scanner", x_key))
        y_scanner = experiment.connect_device(device_ref("scanner", y_key))
        self._sync_connected_flags_from_experiment()
        x_um = _scanner_sample_um(x_scanner)
        y_um = _scanner_sample_um(y_scanner)
        self._update_position_status(
            x_um=x_um,
            y_um=y_um,
            reset_xy_zero=True,
        )
        self._set_srkr_offsets(x_um=x_um, y_um=y_um)
        self.status_label.setText("connected")
        self.append_log(
            "XY initialized: "
            f"X={x_um:.3f} um ({_scanner_control_pos(x_scanner):.6f} {x_scanner.get_pos_unit()}), "
            f"Y={y_um:.3f} um ({_scanner_control_pos(y_scanner):.6f} {y_scanner.get_pos_unit()})"
        )

    @QtCore.Slot(str)
    def handle_xy_initialize_error(self, message: str):
        self.append_log(f"XY initialize error: {message}")
        self.status_label.setText("idle")
        QtWidgets.QMessageBox.warning(self, "XY Initialize Error", message)

    @QtCore.Slot(str)
    def handle_delay_stage_initialize_error(self, message: str):
        self.append_log(f"Delay stage initialize error: {message}")
        self.status_label.setText("idle")
        QtWidgets.QMessageBox.warning(self, "Delay Stage Initialize Error", message)

    @QtCore.Slot(object)
    def handle_finished(self, rows):
        self._update_curves()
        self.append_log(f"Measurement finished. {len(rows)} points collected.")

    def _update_snapshot(self, row: dict):
        for idx, key in enumerate(self.snapshot_keys):
            value = row.get(key, "-")
            text = format_snapshot_value(key, value, voltage_scale=self._voltage_scale)
            self.snapshot_table.item(idx, 1).setText(text)

    def _update_curves(self):
        signal1_key, signal2_key, title1, title2, unit1, unit2 = self._signal_view_config()
        measurement_name = self._measurement_name()
        if measurement_name != "SRKR":
            self.x_curve.opts["name"] = title1
            self.y_curve.opts["name"] = title2
            self.x_plot.setLabel("left", title1, units=unit1)
            self.y_plot.setLabel("left", title2, units=unit2)

        if measurement_name == "signal_monitor":
            self.x_plot.setLabel("bottom", "elapsed time", units="s")
            self.y_plot.setLabel("bottom", "elapsed time", units="s")
            self.x_plot.setLabel("top", "Point")
            self.y_plot.setLabel("top", "Point")
            self.x_plot.getAxis("top").setStyle(showValues=True)
            self.y_plot.getAxis("top").setStyle(showValues=True)
        elif measurement_name == "SRKR":
            self.srkr_x_x_plot.setLabel("left", title1, units=unit1)
            self.srkr_x_y_plot.setLabel("left", title2, units=unit2)
            self.srkr_y_x_plot.setLabel("left", title1, units=unit1)
            self.srkr_y_y_plot.setLabel("left", title2, units=unit2)
            self.srkr_x_x_plot.setLabel("bottom", "x", units="um")
            self.srkr_x_x_plot.setLabel("top", "x_cor", units="um")
            self.srkr_x_y_plot.setLabel("bottom", "x", units="um")
            self.srkr_x_y_plot.setLabel("top", "x_cor", units="um")
            self.srkr_y_x_plot.setLabel("bottom", "y", units="um")
            self.srkr_y_x_plot.setLabel("top", "y_cor", units="um")
            self.srkr_y_y_plot.setLabel("bottom", "y", units="um")
            self.srkr_y_y_plot.setLabel("top", "y_cor", units="um")
            self.srkr_x_x_plot.getAxis("top").setStyle(showValues=True)
            self.srkr_x_y_plot.getAxis("top").setStyle(showValues=True)
            self.srkr_y_x_plot.getAxis("top").setStyle(showValues=True)
            self.srkr_y_y_plot.getAxis("top").setStyle(showValues=True)
        else:
            self.x_plot.setLabel("bottom", "t", units="ps")
            self.y_plot.setLabel("bottom", "t", units="ps")
            self.x_plot.setLabel("top", "t_cor", units="ps")
            self.y_plot.setLabel("top", "t_cor", units="ps")
            self.x_plot.getAxis("top").setStyle(showValues=True)
            self.y_plot.getAxis("top").setStyle(showValues=True)

        rows = self._current_rows()
        if not rows:
            if measurement_name == "SRKR":
                for plot_widget in (
                    self.srkr_x_x_plot,
                    self.srkr_x_y_plot,
                    self.srkr_y_x_plot,
                    self.srkr_y_y_plot,
                ):
                    plot_widget.getAxis("top").setTicks([])
                self.srkr_x_x_curve.setData([], [])
                self.srkr_x_y_curve.setData([], [])
                self.srkr_y_x_curve.setData([], [])
                self.srkr_y_y_curve.setData([], [])
            else:
                self.x_curve.setData([], [])
                self.y_curve.setData([], [])
                self._update_top_axis_ticks(self.x_plot, [])
                self._update_top_axis_ticks(self.y_plot, [])
            return

        if len(rows) > 5000:
            for curve in (
                self.x_curve,
                self.y_curve,
                self.srkr_x_x_curve,
                self.srkr_x_y_curve,
                self.srkr_y_x_curve,
                self.srkr_y_y_curve,
            ):
                if hasattr(curve, "setSkipFiniteCheck"):
                    curve.setSkipFiniteCheck(True)

        if measurement_name == "signal_monitor":
            series1, series2 = standard_plot_series(
                rows,
                measurement_name=measurement_name,
                signal1_key=signal1_key,
                signal2_key=signal2_key,
                voltage_scale=self._voltage_scale,
            )
            point_labels = signal_monitor_top_labels(len(series1.x))
            self._update_top_axis_ticks_from_pairs(self.x_plot, series1.x, point_labels)
            self._update_top_axis_ticks_from_pairs(self.y_plot, series1.x, point_labels)
            self.x_curve.setData(series1.x, series1.y)
            self.y_curve.setData(series2.x, series2.y)
            return
        elif measurement_name == "SRKR":
            series = srkr_plot_series(
                rows,
                signal1_key=signal1_key,
                signal2_key=signal2_key,
                voltage_scale=self._voltage_scale,
            )
            self._set_srkr_plot_data(
                self.srkr_x_x_plot,
                self.srkr_x_x_curve,
                positions=series.x_signal1.positions,
                cor_values=series.x_signal1.cor_values,
                signal_values=series.x_signal1.signal_values,
                bottom_label="x",
                top_label="x_cor",
                left_label=title1,
                left_unit=unit1,
            )
            self._set_srkr_plot_data(
                self.srkr_x_y_plot,
                self.srkr_x_y_curve,
                positions=series.x_signal2.positions,
                cor_values=series.x_signal2.cor_values,
                signal_values=series.x_signal2.signal_values,
                bottom_label="x",
                top_label="x_cor",
                left_label=title2,
                left_unit=unit2,
            )
            self._set_srkr_plot_data(
                self.srkr_y_x_plot,
                self.srkr_y_x_curve,
                positions=series.y_signal1.positions,
                cor_values=series.y_signal1.cor_values,
                signal_values=series.y_signal1.signal_values,
                bottom_label="y",
                top_label="y_cor",
                left_label=title1,
                left_unit=unit1,
            )
            self._set_srkr_plot_data(
                self.srkr_y_y_plot,
                self.srkr_y_y_curve,
                positions=series.y_signal2.positions,
                cor_values=series.y_signal2.cor_values,
                signal_values=series.y_signal2.signal_values,
                bottom_label="y",
                top_label="y_cor",
                left_label=title2,
                left_unit=unit2,
            )
        else:
            series1, series2 = standard_plot_series(
                rows,
                measurement_name=measurement_name,
                signal1_key=signal1_key,
                signal2_key=signal2_key,
                voltage_scale=self._voltage_scale,
            )
            self._update_top_axis_ticks(self.x_plot, series1.x)
            self._update_top_axis_ticks(self.y_plot, series1.x)
            self.x_curve.setData(series1.x, series1.y)
            self.y_curve.setData(series2.x, series2.y)

    def _update_top_axis_ticks(self, plot_widget: pg.PlotWidget, x_values: list[float]):
        axis = plot_widget.getAxis("top")
        if not x_values:
            axis.setTicks([])
            return
        ticks = sample_axis_ticks(x_values, trkr_top_labels(x_values, self._t_zero_ps_current))
        axis.setTicks([ticks])

    def _update_top_axis_ticks_from_pairs(
        self, plot_widget: pg.PlotWidget, positions: list[float], labels: list[str]
    ):
        axis = plot_widget.getAxis("top")
        if not positions:
            axis.setTicks([])
            return
        axis.setTicks([sample_axis_ticks(positions, labels)])

    def _set_standard_plot_ratio(self):
        total = max(self.plot_splitter.size().height(), self.plot_stack.size().height(), 400)
        top = max(int(total * 0.75), 1)
        bottom = max(total - top, 1)
        self.plot_splitter.setSizes([top, bottom])

    def _set_srkr_plot_ratio(self):
        self.srkr_plot_layout.setRowStretch(0, 3)
        self.srkr_plot_layout.setRowStretch(1, 1)
        self.srkr_plot_layout.setRowMinimumHeight(0, 0)
        self.srkr_plot_layout.setRowMinimumHeight(1, 0)

    def _set_srkr_plot_data(
        self,
        plot_widget: pg.PlotWidget,
        curve,
        *,
        positions: list[float],
        cor_values: list[float],
        signal_values: list[float],
        bottom_label: str,
        top_label: str,
        left_label: str,
        left_unit: str,
    ):
        plot_widget.setLabel("bottom", bottom_label, units="um")
        plot_widget.setLabel("top", top_label, units="um")
        plot_widget.setLabel("left", left_label, units=left_unit)
        self._update_top_axis_ticks_from_pairs(
            plot_widget,
            positions,
            [f"{value:.0f}" for value in cor_values],
        )
        curve.setData(positions, signal_values)

    def _update_voltage_display_from_settings(self, settings: dict):
        display = lockin_display_from_settings(settings)
        self._voltage_scale = display.voltage_scale
        self._voltage_unit = display.voltage_unit
        self.current_sensitivity_label.setText(display.sensitivity)
        self.current_tc_label.setText(display.time_constant)
        self.current_freq_label.setText(display.ref_freq)
        self.status_x_signal_title.setText(display.x_title)
        self.status_y_signal_title.setText(display.y_title)
        self.status_r_signal_title.setText(display.r_title)
        self.status_theta_signal_title.setText(display.theta_title)

    def _update_overload_display(self, status: dict | None):
        self.current_overload_label.setText(overload_display_from_status(status))

    def _refresh_overload_display(self, experiment: Experiment, ref: str):
        try:
            self._update_overload_display(experiment.read_lockin_overload(ref))
        except Exception:
            self.current_overload_label.setText("?")

    def _set_status_value(self, label: QtWidgets.QLabel, value: float | None):
        label.setText("-" if value is None else f"{value:.3f}")

    def _update_position_status(
        self,
        *,
        t_ps: float | None = None,
        stage_mm: float | None = None,
        stage_pulse: float | None = None,
        x_um: float | None = None,
        y_um: float | None = None,
        x_pos: float | None = None,
        y_pos: float | None = None,
        x_unit: str | None = None,
        y_unit: str | None = None,
        reset_xy_zero: bool = False,
    ):
        delay_stage = self._delay_stage_handle()
        x_scanner = self._scanner_handle("x")
        y_scanner = self._scanner_handle("y")

        if t_ps is not None and stage_mm is None and delay_stage is not None:
            try:
                stage_mm = float(delay_stage.get_pos_mm())
            except Exception:
                stage_mm = None
        if t_ps is not None and stage_pulse is None and delay_stage is not None:
            try:
                stage_pulse = float(delay_stage.get_pulse())
            except Exception:
                stage_pulse = None
        if x_um is not None and x_pos is None and x_scanner is not None:
            try:
                x_pos = _scanner_control_pos(x_scanner)
                x_unit = x_scanner.get_pos_unit()
            except Exception:
                x_pos = None
        if y_um is not None and y_pos is None and y_scanner is not None:
            try:
                y_pos = _scanner_control_pos(y_scanner)
                y_unit = y_scanner.get_pos_unit()
            except Exception:
                y_pos = None
        if stage_mm is not None:
            self._current_stage_mm = float(stage_mm)
        if stage_pulse is not None:
            self._current_stage_pulse = float(stage_pulse)
        if t_ps is not None:
            self._current_t_ps = float(t_ps)
            self._t_zero_ps_current = self.t_zero_spin.value()
            self._set_status_value(self.current_t_offset_label, self._t_zero_ps_current)
            self._set_status_value(self.current_t_cor_label, t_ps - self._t_zero_ps_current)

        if x_pos is not None:
            self._current_scanner_pos["x"] = float(x_pos)
        if y_pos is not None:
            self._current_scanner_pos["y"] = float(y_pos)
        if x_unit is not None:
            self._current_scanner_units["x"] = x_unit
        if y_unit is not None:
            self._current_scanner_units["y"] = y_unit

        if x_um is not None:
            self._current_x_um = float(x_um)
            if reset_xy_zero or self._x_zero_um_current is None:
                self._x_zero_um_current = x_um
            self._set_status_value(self.current_x_offset_label, self._x_zero_um_current)
            self._set_status_value(self.current_x_cor_label, x_um - self._x_zero_um_current)

        if y_um is not None:
            self._current_y_um = float(y_um)
            if reset_xy_zero or self._y_zero_um_current is None:
                self._y_zero_um_current = y_um
            self._set_status_value(self.current_y_offset_label, self._y_zero_um_current)
            self._set_status_value(self.current_y_cor_label, y_um - self._y_zero_um_current)
        self._refresh_move_abs_coordinate_ui()

    def _signal_view_config(self):
        view = signal_view_config(self.signal_mode_combo.currentText(), self._voltage_unit)
        return view.signal1_key, view.signal2_key, view.title1, view.title2, view.unit1, view.unit2

    def _update_signal_status(self, row: dict):
        x_value = row.get("X_V", row.get("X"))
        y_value = row.get("Y_V", row.get("Y"))
        r_value = row.get("R_V", row.get("R"))
        theta_value = row.get("Theta_deg", row.get("Theta"))
        if x_value is None or y_value is None or r_value is None or theta_value is None:
            return
        self.current_signal1_label.setText(f"{x_value * self._voltage_scale:.3f}")
        self.current_signal2_label.setText(f"{y_value * self._voltage_scale:.3f}")
        self.current_signal3_label.setText(f"{r_value * self._voltage_scale:.3f}")
        self.current_signal4_label.setText(f"{theta_value:.3f}")

    def _refresh_signal_view(self):
        _, _, title1, title2, unit1, unit2 = self._signal_view_config()
        self.x_plot.setLabel("left", title1, units=unit1)
        self.y_plot.setLabel("left", title2, units=unit2)
        rows = self._current_rows()
        if rows:
            self._update_signal_status(rows[-1])
        self._update_curves()

    def _refresh_measurement_view(self):
        current_index = self.measurement_tabs.currentIndex()
        previous_index = getattr(self, "_last_measurement_tab_index", current_index)
        if previous_index != current_index:
            previous_label = self.measurement_tabs.tabText(previous_index)
            previous_name = "signal_monitor" if previous_label == "Signal Monitor" else previous_label
            self._save_current_output_settings(previous_name)

        measurement_name = self._measurement_name()
        is_srkr = measurement_name == "SRKR"
        idle = self._thread is None
        self._apply_output_settings(measurement_name)
        self.plot_stack.setCurrentWidget(self.srkr_plot_widget if is_srkr else self.plot_splitter)
        self.plot_splitter.setOrientation(QtCore.Qt.Vertical)
        if is_srkr:
            self._set_srkr_plot_ratio()
        else:
            self._set_standard_plot_ratio()
        self.current_scan_axis_label.setText(
            "-" if measurement_name == "signal_monitor" else ("t" if measurement_name == "TRKR" else self.srkr_axis_combo.currentText().lower())
        )
        self._rebuild_snapshot_table()
        rows = self._current_rows()
        if rows:
            self._update_snapshot(rows[-1])
        self.start_button.setEnabled(idle)
        self.move_t_spin.setEnabled(idle)
        self.move_t_button.setEnabled(idle)
        self.move_t_corrected_spin.setEnabled(idle)
        self.move_t_corrected_button.setEnabled(idle)
        self.t_zero_spin.setEnabled(idle)
        self.t_zero_current_button.setEnabled(idle)
        self.srkr_axis_combo.setEnabled(idle)
        self.srkr_move_spin.setEnabled(idle)
        self.srkr_move_button.setEnabled(idle)
        self.srkr_corrected_move_spin.setEnabled(idle)
        self.srkr_corrected_move_button.setEnabled(idle)
        self.srkr_offset_spin.setEnabled(idle)
        self.srkr_current_button.setEnabled(idle)
        for axis in ("x", "y"):
            self.scanner_move_spins[axis].setEnabled(idle)
            self.scanner_move_buttons[axis].setEnabled(idle)
            self.scanner_offset_spins[axis].setEnabled(idle)
            self.scanner_offset_buttons[axis].setEnabled(idle)
            self.scanner_corrected_move_spins[axis].setEnabled(idle)
            self.scanner_corrected_move_buttons[axis].setEnabled(idle)
        self._refresh_signal_view()
        self._last_measurement_tab_index = current_index
        self._apply_coordinate_edit_locks()

    def _set_running_state(self, running: bool):
        measurement_name = self._measurement_name()
        t_move_enabled = not (running and measurement_name == "TRKR")
        scanner_move_enabled = not (running and measurement_name == "SRKR")
        self.connect_all_button.setEnabled(not running)
        self.disconnect_all_button.setEnabled(not running)
        self.start_button.setEnabled(not running)
        self.stop_button.setEnabled(running)
        self.config_path_edit.setEnabled(not running)
        self.config_browse_button.setEnabled(not running)
        self.config_load_button.setEnabled(not running)
        self.config_save_button.setEnabled(not running)
        self.device_tabs.setEnabled(not running)
        self.measurement_tabs.setEnabled(not running)
        self.scan_min_spin.setEnabled(not running)
        self.scan_max_spin.setEnabled(not running)
        self.scan_step_spin.setEnabled(not running)
        self.wait_s_spin.setEnabled(not running)
        self.wait_default_button.setEnabled(not running)
        self.trkr_return_to_zero_check.setEnabled(not running)
        self.t_zero_spin.setEnabled(t_move_enabled)
        self.move_t_spin.setEnabled(t_move_enabled)
        self.move_t_button.setEnabled(t_move_enabled)
        self.move_t_corrected_spin.setEnabled(t_move_enabled)
        self.move_t_corrected_button.setEnabled(t_move_enabled)
        self.signal_monitor_interval_spin.setEnabled(not running)
        self.signal_monitor_points_spin.setEnabled(not running)
        self.srkr_min_spin.setEnabled(not running)
        self.srkr_max_spin.setEnabled(not running)
        self.srkr_step_spin.setEnabled(not running)
        self.t_zero_current_button.setEnabled(t_move_enabled)
        self.srkr_axis_combo.setEnabled(scanner_move_enabled)
        self.srkr_move_spin.setEnabled(scanner_move_enabled)
        self.srkr_move_button.setEnabled(scanner_move_enabled)
        self.srkr_corrected_move_spin.setEnabled(scanner_move_enabled)
        self.srkr_corrected_move_button.setEnabled(scanner_move_enabled)
        self.srkr_offset_spin.setEnabled(scanner_move_enabled)
        self.srkr_current_button.setEnabled(scanner_move_enabled)
        for axis in ("x", "y"):
            self.scanner_move_spins[axis].setEnabled(scanner_move_enabled)
            self.scanner_move_buttons[axis].setEnabled(scanner_move_enabled)
            self.scanner_offset_spins[axis].setEnabled(scanner_move_enabled)
            self.scanner_offset_buttons[axis].setEnabled(scanner_move_enabled)
            self.scanner_corrected_move_spins[axis].setEnabled(scanner_move_enabled)
            self.scanner_corrected_move_buttons[axis].setEnabled(scanner_move_enabled)
        self.srkr_wait_spin.setEnabled(not running)
        self.srkr_wait_default_button.setEnabled(not running)
        self.srkr_return_to_zero_check.setEnabled(not running)
        self.output_dir_edit.setEnabled(not running)
        self.output_name_edit.setEnabled(not running)
        self.output_browse_button.setEnabled(not running)
        self.auto_suffix_check.setEnabled(not running)
        self._apply_coordinate_edit_locks()
        self.save_button.setEnabled(True)

    def _set_device_busy_state(self, busy: bool):
        if self._thread is not None:
            return
        self.connect_all_button.setEnabled(not busy)
        self.disconnect_all_button.setEnabled(not busy)
        self.start_button.setEnabled(not busy)
        self.xy_panel.connect_button.setEnabled(not busy)
        self.xy_panel.disconnect_button.setEnabled(not busy)
        self.xy_panel.initialize_button.setEnabled(not busy)
        self.xy_panel.settings_button.setEnabled(not busy)
        self.t_panel.connect_button.setEnabled(not busy)
        self.t_panel.disconnect_button.setEnabled(not busy)
        self.t_panel.initialize_button.setEnabled(not busy)
        self.t_panel.settings_button.setEnabled(not busy)
        self.t_zero_spin.setEnabled(not busy)
        self.t_zero_current_button.setEnabled(not busy)
        self.trkr_return_to_zero_check.setEnabled(not busy)
        self.move_t_spin.setEnabled(not busy)
        self.move_t_button.setEnabled(not busy)
        self.move_t_corrected_spin.setEnabled(not busy)
        self.move_t_corrected_button.setEnabled(not busy)
        self.srkr_axis_combo.setEnabled(not busy)
        self.srkr_move_spin.setEnabled(not busy)
        self.srkr_move_button.setEnabled(not busy)
        self.srkr_corrected_move_spin.setEnabled(not busy)
        self.srkr_corrected_move_button.setEnabled(not busy)
        self.srkr_offset_spin.setEnabled(not busy)
        self.srkr_current_button.setEnabled(not busy)
        self.srkr_return_to_zero_check.setEnabled(not busy)
        for axis in ("x", "y"):
            self.scanner_move_spins[axis].setEnabled(not busy)
            self.scanner_move_buttons[axis].setEnabled(not busy)
            self.scanner_offset_spins[axis].setEnabled(not busy)
            self.scanner_offset_buttons[axis].setEnabled(not busy)
            self.scanner_corrected_move_spins[axis].setEnabled(not busy)
            self.scanner_corrected_move_buttons[axis].setEnabled(not busy)
        self._apply_coordinate_edit_locks()

    @QtCore.Slot()
    def _cleanup_thread(self):
        self._worker = None
        self._thread = None
        self._set_running_state(False)

    @QtCore.Slot()
    def _cleanup_device_thread(self):
        self._device_worker = None
        self._device_thread = None
        self._set_device_busy_state(False)

    def closeEvent(self, event):
        self.stop_measurement()
        if self._thread is not None:
            self._thread.quit()
            self._thread.wait(2000)
        if self._device_thread is not None:
            self._device_thread.quit()
            self._device_thread.wait(2000)
        super().closeEvent(event)


def main():
    app = QtWidgets.QApplication(sys.argv)
    window = TrkrWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
