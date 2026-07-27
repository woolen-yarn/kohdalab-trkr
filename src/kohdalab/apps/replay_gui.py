from __future__ import annotations

import argparse
import math
import sys
import threading
from pathlib import Path
from typing import Any, cast

from PySide6 import QtCore, QtWidgets

from kohdalab.api.measurement_rows import axis_target_key
from kohdalab.api.models import MeasurementPoint
from kohdalab.api.status import (
    STATUS_READING_LOCKIN,
    STATUS_RUNNING,
    STATUS_SLOW_AXIS_READY,
    STATUS_STOPPED,
    STATUS_WAITING,
    moving_axis_status,
)
from kohdalab.apps.replay import (
    REPLAY_MEASUREMENTS,
    ReplayDataset,
    discover_replay_datasets,
    load_replay_csv,
    replay_axis_value,
)
from kohdalab.apps.trkr_gui import TRKRGui, _validated_measurement_point


TAB_INDEX = {
    "signal_monitor": 0,
    "trkr": 1,
    "srkr": 2,
    "strkr": 3,
    "srkr_2d": 4,
}
FAST_REPLAY_BATCH_SIZE = 16
FAST_REPLAY_FRAME_S = 1.0 / 60.0
LEFT_PANEL_VALUE_SAMPLES = ("-9999.999", "Moving...")
LOCKIN_SIGNAL_VALUE_WIDTH = 96
LOCKIN_SIGNAL_VALUE_SAMPLES = ("-999.999 deg", "-999.999 mV")
SNAPSHOT_FIELD_COLUMN_WIDTH = 100
LEFT_PANEL_WIDTH_MARGIN = 4
LEFT_PANEL_CONTENT_MARGIN = 6
MOTION_GRID_SPACING = 4
MOTION_SPIN_WIDTH = 88
MOTION_BUTTON_WIDTH = 80
USE_CURRENT_BUTTON_WIDTH = 89


class ReplayWorker(QtCore.QObject):
    cycle_started = QtCore.Signal(int)
    point_ready = QtCore.Signal(object)
    points_ready = QtCore.Signal(object)
    status_changed = QtCore.Signal(str)
    error_occurred = QtCore.Signal(str)
    finished = QtCore.Signal(object)

    def __init__(self, dataset: ReplayDataset, interval_s: float) -> None:
        super().__init__()
        if not math.isfinite(interval_s) or interval_s < 0:
            raise ValueError("Replay interval must be finite and non-negative.")
        self.dataset = dataset
        self.interval_s = float(interval_s)
        self._stop_event = threading.Event()
        self._point_rendered = threading.Event()

    def stop(self) -> None:
        self._stop_event.set()
        self._point_rendered.set()

    def acknowledge_point_rendered(self) -> None:
        self._point_rendered.set()

    def _wait(self, fraction: float) -> bool:
        return self._stop_event.wait(self.interval_s * fraction)

    def _status_phase(self, status: str, fraction: float) -> bool:
        self.status_changed.emit(status)
        return self._wait(fraction)

    def _emit_point(self, index: int, row: dict[str, Any]) -> bool:
        self._point_rendered.clear()
        self.point_ready.emit(
            MeasurementPoint(
                index=index,
                total_points=self.dataset.point_count,
                row=dict(row),
            )
        )
        self._point_rendered.wait()
        return self._stop_event.is_set()

    def _emit_points(self, points: list[MeasurementPoint]) -> bool:
        self._point_rendered.clear()
        self.points_ready.emit(points)
        self._point_rendered.wait()
        return self._stop_event.is_set()

    def _run_timed_cycle(self) -> bool:
        previous_slow: float | None = None
        for index, row in enumerate(self.dataset.rows, start=1):
            if self._stop_event.is_set():
                return True
            slow_moved = False
            if self.dataset.slow_axis is not None:
                slow = replay_axis_value(row, self.dataset.slow_axis)
                if slow != previous_slow:
                    if self._status_phase(
                        moving_axis_status(self.dataset.slow_axis), 0.075
                    ):
                        return True
                    self.status_changed.emit(STATUS_SLOW_AXIS_READY)
                    previous_slow = slow
                    slow_moved = True
            fast_motion_fraction = 0.075 if slow_moved else 0.15
            if self._status_phase(
                moving_axis_status(self.dataset.fast_axis),
                fast_motion_fraction,
            ):
                return True
            if self._status_phase(STATUS_WAITING, 0.60):
                return True
            if self._status_phase(STATUS_READING_LOCKIN, 0.05):
                return True
            if self._emit_point(index, row):
                return True
            if self._wait(0.20):
                return True
        return False

    def _run_fast_cycle(self) -> bool:
        previous_slow: float | None = None
        for start in range(0, self.dataset.point_count, FAST_REPLAY_BATCH_SIZE):
            if self._stop_event.is_set():
                return True
            batch_rows = self.dataset.rows[start : start + FAST_REPLAY_BATCH_SIZE]
            last_row = batch_rows[-1]
            if self.dataset.slow_axis is not None:
                slow = replay_axis_value(last_row, self.dataset.slow_axis)
                if slow != previous_slow:
                    self.status_changed.emit(moving_axis_status(self.dataset.slow_axis))
                    self.status_changed.emit(STATUS_SLOW_AXIS_READY)
                    previous_slow = slow
            self.status_changed.emit(moving_axis_status(self.dataset.fast_axis))
            self.status_changed.emit(STATUS_WAITING)
            self.status_changed.emit(STATUS_READING_LOCKIN)
            points = [
                MeasurementPoint(
                    index=start + offset,
                    total_points=self.dataset.point_count,
                    row=dict(row),
                )
                for offset, row in enumerate(batch_rows, start=1)
            ]
            if self._emit_points(points):
                return True
            if self._stop_event.wait(FAST_REPLAY_FRAME_S):
                return True
        return False

    @QtCore.Slot()
    def run(self) -> None:
        cycle = 0
        try:
            while not self._stop_event.is_set():
                cycle += 1
                self.cycle_started.emit(cycle)
                self.status_changed.emit(STATUS_RUNNING)
                stopped = (
                    self._run_fast_cycle()
                    if self.interval_s == 0.0
                    else self._run_timed_cycle()
                )
                if stopped:
                    break
        except Exception as error:
            self.error_occurred.emit(str(error))
        finally:
            self.status_changed.emit(STATUS_STOPPED)
            self.finished.emit([dict(row) for row in self.dataset.rows])


class ReplayGui(TRKRGui):
    """The production GUI layout backed by recorded CSV data instead of hardware."""

    def __init__(
        self,
        datasets: dict[str, ReplayDataset] | None = None,
        *,
        interval_s: float | None = None,
        initial_dir: str | Path = Path("demo_csv"),
    ) -> None:
        self.datasets = dict(datasets or {})
        self.selected_paths = {
            measurement: dataset.path for measurement, dataset in self.datasets.items()
        }
        self.initial_dir = Path(initial_dir).expanduser()
        self.virtual_connected = False
        self.replay_worker: ReplayWorker | None = None
        super().__init__()
        if interval_s is not None:
            for spin in (
                self.trkr_wait_spin,
                self.srkr_wait_spin,
                self.strkr_wait_spin,
                self.srkr_2d_wait_spin,
            ):
                spin.setValue(interval_s)
        self.setWindowTitle("KohdaLab TRKR — DEMO MODE — NO HARDWARE")
        self.demo_badge = QtWidgets.QLabel("  DEMO MODE · 実機未接続  ")
        self.demo_badge.setStyleSheet(
            "QLabel {"
            " background: #b00020;"
            " color: white;"
            " font-weight: bold;"
            " font-size: 14px;"
            " padding: 5px 12px;"
            " border-radius: 4px;"
            "}"
        )
        self.statusBar().addPermanentWidget(self.demo_badge)
        self.live_timer.stop()
        self.save_rows_button.setText("Load")
        self._stabilize_left_panel_value_widths()
        self._compact_left_panel_controls()
        self._stabilize_snapshot_column_widths()
        self._apply_panel_sizes()
        self.measurement_tabs.setTabEnabled(TAB_INDEX["signal_monitor"], False)
        for measurement in REPLAY_MEASUREMENTS:
            self.measurement_tabs.setTabEnabled(TAB_INDEX[measurement], True)
        first_measurement = next(iter(self.datasets), "trkr")
        self.measurement_tabs.setCurrentIndex(TAB_INDEX[first_measurement])
        self._rename_output_group()
        self._show_selected_path(first_measurement)
        if first_measurement in self.datasets:
            self._apply_replay_dataset_to_controls(first_measurement)
        self._apply_control_policy()
        self.status_label.setText("load config, then Connect All")
        self.append_log(
            "Replay mode: load a config, use virtual Connect All, then Browse and Load a CSV."
        )

    def refresh_all_ports(self) -> None:
        """Do not enumerate VISA or serial resources in replay mode."""

    def refresh_live_status(self) -> None:
        """Do not poll hardware in replay mode."""

    def _apply_panel_sizes(self) -> None:
        super()._apply_panel_sizes()
        content = cast(QtWidgets.QWidget, self.left_panel.widget())
        required_content_width = max(
            content.sizeHint().width(),
            content.minimumSizeHint().width(),
        )
        panel_chrome_width = (
            self.left_panel.verticalScrollBar().sizeHint().width()
            + (2 * self.left_panel.frameWidth())
            + LEFT_PANEL_WIDTH_MARGIN
        )
        required_panel_width = required_content_width + panel_chrome_width
        self.left_panel.setFixedWidth(
            max(self.left_panel.width(), required_panel_width)
        )

    def showEvent(self, event: Any) -> None:
        super().showEvent(event)
        self._apply_panel_sizes()

    def _rename_output_group(self) -> None:
        for group in self.output_run_widget.findChildren(QtWidgets.QGroupBox):
            if group.title() == "Output":
                group.setTitle("Replay CSV")

    def _stabilize_left_panel_value_widths(self) -> None:
        value_labels = (
            *self.position_labels.values(),
            *self.offset_labels.values(),
            *self.corrected_labels.values(),
        )
        width = (
            max(
                label.fontMetrics().horizontalAdvance(text)
                for label in value_labels
                for text in LEFT_PANEL_VALUE_SAMPLES
            )
            + 4
        )
        alignment = (
            QtCore.Qt.AlignmentFlag.AlignRight | QtCore.Qt.AlignmentFlag.AlignVCenter
        )
        for label in value_labels:
            label.setFixedWidth(width)
            label.setAlignment(alignment)
        signal_width = max(
            LOCKIN_SIGNAL_VALUE_WIDTH,
            max(
                label.fontMetrics().horizontalAdvance(text)
                for label in self.signal_labels.values()
                for text in LOCKIN_SIGNAL_VALUE_SAMPLES
            )
            + 8,
        )
        for label in self.signal_labels.values():
            label.setFixedWidth(signal_width)
            label.setAlignment(alignment)

    def _compact_left_panel_controls(self) -> None:
        content = cast(QtWidgets.QWidget, self.left_panel.widget())
        content_layout = cast(QtWidgets.QLayout, content.layout())
        content_layout.setContentsMargins(
            LEFT_PANEL_CONTENT_MARGIN,
            LEFT_PANEL_CONTENT_MARGIN,
            LEFT_PANEL_CONTENT_MARGIN,
            LEFT_PANEL_CONTENT_MARGIN,
        )
        for spin in (
            self.move_t_spin,
            self.move_x_spin,
            self.move_y_spin,
            self.t_zero_spin,
            self.x_zero_spin,
            self.y_zero_spin,
            self.t_cor_spin,
            self.x_cor_spin,
            self.y_cor_spin,
        ):
            spin.setFixedWidth(MOTION_SPIN_WIDTH)
        for button in (
            self.move_t_button,
            self.move_x_button,
            self.move_y_button,
            self.t_cor_button,
            self.x_cor_button,
            self.y_cor_button,
        ):
            button.setFixedWidth(MOTION_BUTTON_WIDTH)
        for button in (
            self.use_current_t_button,
            self.use_current_x_button,
            self.use_current_y_button,
        ):
            button.setFixedWidth(USE_CURRENT_BUTTON_WIDTH)
        motion_titles = {"Delay Stage", "Scanner X", "Scanner Y"}
        for group in self.left_panel.findChildren(QtWidgets.QGroupBox):
            if group.title() not in motion_titles:
                continue
            group_layout = cast(QtWidgets.QLayout, group.layout())
            for index in range(group_layout.count()):
                item = cast(QtWidgets.QLayoutItem, group_layout.itemAt(index))
                grid = item.layout()
                if isinstance(grid, QtWidgets.QGridLayout):
                    grid.setHorizontalSpacing(MOTION_GRID_SPACING)

    def _stabilize_snapshot_column_widths(self) -> None:
        header = self.snapshot_table.horizontalHeader()
        header.setSectionResizeMode(0, QtWidgets.QHeaderView.ResizeMode.Fixed)
        header.setSectionResizeMode(1, QtWidgets.QHeaderView.ResizeMode.Stretch)
        self.snapshot_table.setColumnWidth(0, SNAPSHOT_FIELD_COLUMN_WIDTH)

    def _update_snapshot(self, row: dict[str, Any]) -> None:
        super()._update_snapshot(row)
        self._stabilize_snapshot_column_widths()

    def _select_default_replay_csvs(self) -> bool:
        try:
            discovered = discover_replay_datasets(self.initial_dir)
        except (OSError, ValueError) as error:
            self.append_log(f"Demo CSV auto-selection skipped: {error}")
            return False

        self.datasets.clear()
        self.selected_paths = {
            measurement: dataset.path
            for measurement, dataset in discovered.items()
            if measurement in REPLAY_MEASUREMENTS
        }
        measurement = self._measurement_name()
        self._show_selected_path(measurement)
        self.append_log(
            "Selected default demo CSVs: "
            + ", ".join(
                f"{name.upper()}={self.selected_paths[name].name}"
                for name in REPLAY_MEASUREMENTS
                if name in self.selected_paths
            )
        )
        return True

    def _apply_control_policy(self) -> None:
        central = self.centralWidget()
        for widget_type in (
            QtWidgets.QAbstractButton,
            QtWidgets.QAbstractSpinBox,
            QtWidgets.QComboBox,
            QtWidgets.QLineEdit,
        ):
            for widget in central.findChildren(widget_type):
                widget.setEnabled(False)

        idle = self.measurement_thread is None
        measurement = self._measurement_name()
        selected = measurement in self.selected_paths
        loaded = measurement in self.datasets

        self.measurement_tabs.setEnabled(True)
        self.measurement_tabs.tabBar().setEnabled(idle)
        self.signal_mode_combo.setEnabled(True)
        self.right_panel_toggle.setEnabled(True)
        self.config_path.setEnabled(idle)
        self.browse_button.setEnabled(idle)
        self.load_button.setEnabled(idle)
        self.connect_button.setEnabled(idle and not self.virtual_connected)
        self.disconnect_button.setEnabled(idle and self.virtual_connected)
        self.output_dir_edit.setEnabled(True)
        self.output_dir_edit.setReadOnly(True)
        self.output_name_edit.setEnabled(True)
        self.output_name_edit.setReadOnly(True)
        self.output_browse_button.setEnabled(idle)
        self.save_rows_button.setEnabled(idle and selected)
        wait_spin = {
            "trkr": self.trkr_wait_spin,
            "srkr": self.srkr_wait_spin,
            "strkr": self.strkr_wait_spin,
            "srkr_2d": self.srkr_2d_wait_spin,
        }.get(measurement)
        if wait_spin is not None:
            wait_spin.setEnabled(idle)
            wait_spin.lineEdit().setEnabled(idle)
        self.start_button.setEnabled(idle and self.virtual_connected and loaded)
        self.stop_button.setEnabled(not idle)
        self.start_button.setToolTip(
            ""
            if self.virtual_connected and loaded
            else "Use Connect All and Load a replay CSV first."
        )

    def _refresh_measurement_availability(self) -> None:
        self._apply_control_policy()

    def _set_running(self, running: bool) -> None:
        self._apply_control_policy()

    def load_config_file(self) -> None:
        if self.measurement_thread is not None:
            return
        self.virtual_connected = False
        super().load_config_file()
        self._select_default_replay_csvs()
        self.status_label.setText("config loaded; click Connect All")
        self._apply_control_policy()

    def connect_all(self) -> None:
        if self.measurement_thread is not None:
            return
        self.virtual_connected = True
        for axis, spin in (
            ("t", self.t_zero_spin),
            ("x", self.x_zero_spin),
            ("y", self.y_zero_spin),
        ):
            self._set_position_value(axis, spin.value())
        self.overload_label.setText("OK")
        self.status_label.setText("connected (demo)")
        self.append_log(
            "Connected all configured devices in replay mode (no hardware I/O)."
        )
        self._apply_control_policy()

    def disconnect_all(self) -> None:
        if self.measurement_thread is not None:
            return
        self.virtual_connected = False
        self.status_label.setText("disconnected (demo)")
        self.append_log("Disconnected all replay devices.")
        self._apply_control_policy()

    def browse_output_dir(self) -> None:
        measurement = self._measurement_name()
        if measurement not in REPLAY_MEASUREMENTS:
            return
        selected = self.selected_paths.get(measurement)
        start = (
            selected.parent
            if selected is not None
            else self.initial_dir
            if self.initial_dir.is_dir()
            else Path.cwd()
        )
        path, _ = QtWidgets.QFileDialog.getOpenFileName(
            self,
            f"Select {measurement.upper()} Replay CSV",
            str(start),
            "CSV Files (*.csv)",
        )
        if not path:
            return
        selected_path = Path(path).expanduser().resolve()
        self.selected_paths[measurement] = selected_path
        self.datasets.pop(measurement, None)
        self._show_selected_path(measurement)
        self.status_label.setText("CSV selected; click Load")
        self.append_log(f"Selected {measurement.upper()} replay CSV: {selected_path}")
        self._apply_control_policy()

    def save_rows(self) -> None:
        measurement = self._measurement_name()
        path = self.selected_paths.get(measurement)
        if path is None:
            return
        try:
            dataset = load_replay_csv(path, expected_measurement=measurement)
            self.datasets[measurement] = dataset
            self.rows_by_mode[measurement].clear()
            self._apply_replay_dataset_to_controls(measurement)
            self.status_label.setText("CSV loaded")
            self.append_log(
                f"Loaded {dataset.point_count} {measurement.upper()} replay points."
            )
        except (OSError, ValueError) as error:
            self.datasets.pop(measurement, None)
            self.status_label.setText("CSV load error")
            QtWidgets.QMessageBox.warning(self, "Replay CSV Error", str(error))
        self._apply_control_policy()

    def _handle_measurement_tab_changed(self, index: int) -> None:
        super()._handle_measurement_tab_changed(index)
        measurement = self._measurement_name()
        self._show_selected_path(measurement)
        if measurement in self.datasets:
            self._apply_replay_dataset_to_controls(measurement)
        self._apply_control_policy()

    def _show_selected_path(self, measurement: str) -> None:
        path = self.selected_paths.get(measurement)
        if path is None:
            self.output_dir_edit.setText("")
            self.output_name_edit.setText("")
            return
        self.output_dir_edit.setText(str(path.parent))
        self.output_name_edit.setText(path.name)

    def _apply_replay_dataset_to_controls(self, measurement: str) -> None:
        dataset = self.datasets[measurement]
        self._show_selected_path(measurement)
        if measurement == "trkr":
            self._set_range_controls(
                self.trkr_min_spin,
                self.trkr_max_spin,
                self.trkr_step_spin,
                [replay_axis_value(row, "t") for row in dataset.rows],
            )
        elif measurement == "srkr":
            self.srkr_axis_combo.setCurrentText(dataset.fast_axis)
            self._set_range_controls(
                self.srkr_min_spin,
                self.srkr_max_spin,
                self.srkr_step_spin,
                [replay_axis_value(row, dataset.fast_axis) for row in dataset.rows],
            )
        elif measurement == "strkr":
            self.strkr_fast_axis_combo.setCurrentText(dataset.fast_axis)
            self.strkr_slow_axis_combo.setCurrentText(str(dataset.slow_axis))
            self._set_scan2d_range_controls(dataset)
        elif measurement == "srkr_2d":
            self.srkr_2d_fast_axis_combo.setCurrentText(dataset.fast_axis)
            self.srkr_2d_slow_axis_combo.setCurrentText(str(dataset.slow_axis))
            self._set_scan2d_range_controls(dataset)
        self._refresh_plot_labels()

    def _set_range_controls(
        self,
        minimum_spin: QtWidgets.QDoubleSpinBox,
        maximum_spin: QtWidgets.QDoubleSpinBox,
        step_spin: QtWidgets.QDoubleSpinBox,
        values: list[float],
    ) -> None:
        unique = sorted(set(values))
        minimum_spin.setValue(unique[0])
        maximum_spin.setValue(unique[-1])
        if len(unique) > 1:
            step_spin.setValue(
                min(
                    right - left
                    for left, right in zip(unique, unique[1:], strict=False)
                    if right > left
                )
            )

    def _set_scan2d_range_controls(self, dataset: ReplayDataset) -> None:
        if dataset.slow_axis is None:
            return
        role_spins = (
            self.strkr_role_spins
            if dataset.measurement == "strkr"
            else self.srkr_2d_role_spins
        )
        for role, axis in (
            ("fast_axis", dataset.fast_axis),
            ("slow_axis", dataset.slow_axis),
        ):
            spins = role_spins[role]
            self._set_range_controls(
                spins["min"],
                spins["max"],
                spins["step"],
                [replay_axis_value(row, axis) for row in dataset.rows],
            )

    def _enriched_point(self, point: MeasurementPoint) -> MeasurementPoint:
        row = dict(point.row)
        zero_by_axis = {
            "t": self.t_zero_spin.value(),
            "x": self.x_zero_spin.value(),
            "y": self.y_zero_spin.value(),
        }
        for axis in ("t", "x", "y"):
            raw_key = "t_ps" if axis == "t" else f"{axis}_um"
            corrected_key = "t_cor_ps" if axis == "t" else f"{axis}_cor_um"
            corrected = row.get(corrected_key)
            if corrected is None:
                corrected = row.get(axis_target_key(axis))
            if row.get(raw_key) is None and corrected is not None:
                row[raw_key] = float(corrected) + zero_by_axis[axis]
        return MeasurementPoint(
            index=point.index,
            total_points=point.total_points,
            row=row,
        )

    def _acknowledge_replay_render(self) -> None:
        if self.replay_worker is not None:
            self.replay_worker.acknowledge_point_rendered()

    def handle_point(self, payload: object) -> None:
        try:
            if isinstance(payload, MeasurementPoint):
                payload = self._enriched_point(payload)
            super().handle_point(payload)
        finally:
            self._acknowledge_replay_render()

    @QtCore.Slot(object)
    def handle_points(self, payload: object) -> None:
        measurement = self.running_measurement or self._measurement_name()
        try:
            if not isinstance(payload, list) or not payload:
                raise TypeError("replay point batch must be a non-empty list.")
            points = [
                _validated_measurement_point(
                    self._enriched_point(point)
                    if isinstance(point, MeasurementPoint)
                    else point,
                    measurement,
                )
                for point in payload
            ]
            self.rows_by_mode[measurement].extend(point.row for point in points)
            last = points[-1]
            self.point_text_by_mode[measurement] = f"{last.index}/{last.total_points}"
            self.eta_text_by_mode[measurement] = self._eta_text(
                measurement,
                last.index,
                last.total_points,
            )
            if measurement == self._measurement_name():
                self.point_label.setText(self.point_text_by_mode[measurement])
                self.eta_label.setText(self.eta_text_by_mode[measurement])
            self._apply_signal(last.row)
            self._update_position_from_row(last.row)
            self._update_snapshot(last.row)
            if measurement == self._measurement_name():
                self._update_curves()
        except (TypeError, ValueError, OverflowError) as error:
            if self.replay_worker is not None:
                self.replay_worker.stop()
            self.handle_error(f"Invalid replay point batch: {error}")
        finally:
            self._acknowledge_replay_render()

    @QtCore.Slot()
    def start_measurement(self) -> None:
        if self.measurement_thread is not None:
            return
        measurement = self._measurement_name()
        dataset = self.datasets.get(measurement)
        if not self.virtual_connected or dataset is None:
            self._apply_control_policy()
            return

        self._apply_replay_dataset_to_controls(measurement)
        self.rows_by_mode[measurement].clear()
        self.point_text_by_mode[measurement] = "-"
        self.eta_text_by_mode[measurement] = "-"
        self.point_label.setText("-")
        self.eta_label.setText("-")
        self._update_curves()

        self.measurement_thread = QtCore.QThread(self)
        wait_s = {
            "trkr": self.trkr_wait_spin,
            "srkr": self.srkr_wait_spin,
            "strkr": self.strkr_wait_spin,
            "srkr_2d": self.srkr_2d_wait_spin,
        }[measurement].value()
        self.replay_worker = ReplayWorker(dataset, wait_s)
        self.replay_worker.moveToThread(self.measurement_thread)
        self.measurement_thread.started.connect(self.replay_worker.run)
        self.replay_worker.cycle_started.connect(self._start_replay_cycle)
        self.replay_worker.point_ready.connect(self.handle_point)
        self.replay_worker.points_ready.connect(self.handle_points)
        self.replay_worker.status_changed.connect(self.handle_measurement_status)
        self.replay_worker.error_occurred.connect(self.handle_error)
        self.replay_worker.finished.connect(self.handle_finished)
        self.replay_worker.finished.connect(self.measurement_thread.quit)
        self.replay_worker.finished.connect(self.replay_worker.deleteLater)
        self.measurement_thread.finished.connect(self.measurement_thread.deleteLater)
        self.measurement_thread.finished.connect(self.cleanup_thread)
        self.running_measurement = measurement
        self.running_motion_axes = {
            axis for axis in (dataset.fast_axis, dataset.slow_axis) if axis is not None
        }
        if dataset.slow_axis is not None:
            self._scan2d_fast_point_count = len(
                {row[axis_target_key(dataset.fast_axis)] for row in dataset.rows}
            )
            self._scan2d_slow_point_count = len(
                {row[axis_target_key(dataset.slow_axis)] for row in dataset.rows}
            )
        self._apply_control_policy()
        self.append_log(
            f"Started repeating {measurement.upper()} replay: {dataset.path}"
        )
        self.measurement_thread.start()

    @QtCore.Slot(int)
    def _start_replay_cycle(self, cycle: int) -> None:
        measurement = self.running_measurement
        if measurement is None:
            return
        self.rows_by_mode[measurement].clear()
        self.point_text_by_mode[measurement] = "-"
        self.eta_text_by_mode[measurement] = "-"
        if measurement == self._measurement_name():
            self.point_label.setText(f"Loop {cycle}")
            self.eta_label.setText("-")
            self._update_curves()
        self.append_log(f"Replay loop {cycle}.")

    @QtCore.Slot()
    def stop_measurement(self) -> None:
        if self.replay_worker is not None:
            self.replay_worker.stop()
            self.append_log("Replay stop requested.")

    def cleanup_thread(self) -> None:
        super().cleanup_thread()
        self.replay_worker = None
        self._apply_control_policy()

    def closeEvent(self, event: Any) -> None:
        self.live_timer.stop()
        if self.replay_worker is not None:
            self.replay_worker.stop()
        if self.measurement_thread is not None:
            self.measurement_thread.quit()
            self.measurement_thread.wait(2000)
        self._restore_log_streams()
        QtWidgets.QMainWindow.closeEvent(self, event)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Replay TRKR/SRKR/STRKR/SRKR 2D CSV files without hardware."
    )
    parser.add_argument(
        "--data-dir",
        type=Path,
        default=Path("demo_csv"),
        help="Directory containing the default replay CSV files.",
    )
    parser.add_argument(
        "--interval",
        type=float,
        default=None,
        help="Optional initial value for each tab's Wait (s) field.",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.interval is not None and (
        not math.isfinite(args.interval) or args.interval < 0
    ):
        print("Error: --interval must be finite and non-negative.", file=sys.stderr)
        return 2

    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication(sys.argv)
    window = ReplayGui(interval_s=args.interval, initial_dir=args.data_dir)
    window.show()
    return app.exec()


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
