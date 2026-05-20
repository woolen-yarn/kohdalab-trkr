from __future__ import annotations

import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from PySide6 import QtCore, QtWidgets

from kohdalab.api.config import delay_stage_config_for, load_config, lockin_config_for, scanner_config_for, zero_for
from kohdalab.api.devices import (
    connect_delay_stage,
    connect_lockin,
    connect_scanner,
    disconnect_delay_stage,
    disconnect_lockin,
    disconnect_scanner,
    get_lockin_wait_time,
    read_delay_stage,
    read_lockin_overload,
    read_lockin_settings,
    read_lockin_signal,
    read_scanner,
)
from kohdalab.interfaces.lockin import list_visa_resources


PACKAGE_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CONFIG_PATH = PACKAGE_ROOT / "config" / "trkr_config_kikuchi.json"
LOCKIN_MODELS = ["SR7265", "SR830", "LI5640", "SR5210"]


def _format_float(value: Any, digits: int = 6) -> str:
    if value is None:
        return "-"
    try:
        return f"{float(value):.{digits}g}"
    except (TypeError, ValueError):
        return str(value)


def _format_bool(value: Any) -> str:
    if value is None:
        return "-"
    return "YES" if bool(value) else "no"


class TrkrGuiTestWindow(QtWidgets.QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("TRKR GUI Test - Session / Devices")
        self.resize(920, 760)
        self._config: dict[str, Any] = {}
        self._lockin = None
        self._delay_stage = None
        self._scanners: dict[str, Any] = {"x": None, "y": None}

        self.config_path_edit = QtWidgets.QLineEdit(str(DEFAULT_CONFIG_PATH))
        self.config_browse_button = QtWidgets.QPushButton("Browse")
        self.config_load_button = QtWidgets.QPushButton("Load")

        self.lockin_model_combo = QtWidgets.QComboBox()
        self.lockin_model_combo.addItems(LOCKIN_MODELS)
        self.lockin_resource_combo = QtWidgets.QComboBox()
        self.lockin_resource_combo.setEditable(True)
        self.lockin_refresh_button = QtWidgets.QPushButton("Refresh")
        self.lockin_connect_button = QtWidgets.QPushButton("Connect")
        self.lockin_disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.lockin_read_button = QtWidgets.QPushButton("Read")
        self.lockin_wait_button = QtWidgets.QPushButton("TC x 4")

        self.delay_controller_edit = QtWidgets.QLineEdit()
        self.delay_stage_edit = QtWidgets.QLineEdit()
        self.delay_port_edit = QtWidgets.QLineEdit()
        self.delay_direction_spin = QtWidgets.QSpinBox()
        self.delay_direction_spin.setRange(0, 1)
        self.delay_connect_button = QtWidgets.QPushButton("Connect")
        self.delay_disconnect_button = QtWidgets.QPushButton("Disconnect")
        self.delay_read_button = QtWidgets.QPushButton("Read")
        self.delay_initialize_button = QtWidgets.QPushButton("Initialize")

        self.scanner_widgets: dict[str, dict[str, Any]] = {}
        for axis in ("x", "y"):
            self.scanner_widgets[axis] = {
                "controller": QtWidgets.QLineEdit(),
                "actuator": QtWidgets.QLineEdit(),
                "port": QtWidgets.QLineEdit(),
                "axis": QtWidgets.QLineEdit(),
                "scale": QtWidgets.QDoubleSpinBox(),
                "connect": QtWidgets.QPushButton("Connect"),
                "disconnect": QtWidgets.QPushButton("Disconnect"),
                "read": QtWidgets.QPushButton("Read"),
                "initialize": QtWidgets.QPushButton("Initialize"),
            }
            self.scanner_widgets[axis]["scale"].setRange(-1_000_000.0, 1_000_000.0)
            self.scanner_widgets[axis]["scale"].setDecimals(6)

        self.lockin_connection_label = QtWidgets.QLabel("disconnected")
        self.x_label = QtWidgets.QLabel("-")
        self.y_label = QtWidgets.QLabel("-")
        self.r_label = QtWidgets.QLabel("-")
        self.theta_label = QtWidgets.QLabel("-")
        self.overload_label = QtWidgets.QLabel("-")
        self.sensitivity_label = QtWidgets.QLabel("-")
        self.tc_label = QtWidgets.QLabel("-")
        self.ref_freq_label = QtWidgets.QLabel("-")
        self.wait_label = QtWidgets.QLabel("-")

        self.delay_connection_label = QtWidgets.QLabel("disconnected")
        self.delay_t_label = QtWidgets.QLabel("-")
        self.delay_mm_label = QtWidgets.QLabel("-")
        self.delay_pulse_label = QtWidgets.QLabel("-")

        self.scanner_status_labels: dict[str, dict[str, QtWidgets.QLabel]] = {}
        for axis in ("x", "y"):
            self.scanner_status_labels[axis] = {
                "connection": QtWidgets.QLabel("disconnected"),
                "um": QtWidgets.QLabel("-"),
                "control": QtWidgets.QLabel("-"),
                "state": QtWidgets.QLabel("-"),
                "moving": QtWidgets.QLabel("-"),
            }
        for label in self.findChildren(QtWidgets.QLabel):
            label.setTextInteractionFlags(QtCore.Qt.TextSelectableByMouse)

        self.log = QtWidgets.QPlainTextEdit()
        self.log.setReadOnly(True)
        self.log.setMaximumBlockCount(500)

        self._build_layout()
        self._connect_signals()
        self.refresh_resources()
        self.load_config()

    def _build_layout(self):
        root = QtWidgets.QWidget()
        layout = QtWidgets.QVBoxLayout(root)

        session_group = QtWidgets.QGroupBox("Session")
        session_layout = QtWidgets.QGridLayout(session_group)
        session_layout.addWidget(QtWidgets.QLabel("Config"), 0, 0)
        session_layout.addWidget(self.config_path_edit, 0, 1)
        session_layout.addWidget(self.config_browse_button, 0, 2)
        session_layout.addWidget(self.config_load_button, 0, 3)

        lockin_group = QtWidgets.QGroupBox("Lock-in")
        lockin_layout = QtWidgets.QGridLayout(lockin_group)
        lockin_layout.addWidget(QtWidgets.QLabel("Model"), 0, 0)
        lockin_layout.addWidget(self.lockin_model_combo, 0, 1)
        lockin_layout.addWidget(QtWidgets.QLabel("Resource"), 1, 0)
        lockin_layout.addWidget(self.lockin_resource_combo, 1, 1)
        lockin_layout.addWidget(self.lockin_refresh_button, 1, 2)
        buttons = QtWidgets.QHBoxLayout()
        buttons.addWidget(self.lockin_connect_button)
        buttons.addWidget(self.lockin_disconnect_button)
        buttons.addWidget(self.lockin_read_button)
        buttons.addWidget(self.lockin_wait_button)
        lockin_layout.addLayout(buttons, 2, 1, 1, 2)

        lockin_status_group = QtWidgets.QGroupBox("Lock-in Status")
        status_layout = QtWidgets.QGridLayout(lockin_status_group)
        self._add_status_rows(
            status_layout,
            [
                ("Connection", self.lockin_connection_label),
                ("X (V)", self.x_label),
                ("Y (V)", self.y_label),
                ("R (V)", self.r_label),
                ("Theta (deg)", self.theta_label),
                ("Overload", self.overload_label),
                ("Sensitivity (V)", self.sensitivity_label),
                ("Time Constant (s)", self.tc_label),
                ("Ref. Freq (Hz)", self.ref_freq_label),
                ("Wait TC x 4 (s)", self.wait_label),
            ],
        )

        delay_group = QtWidgets.QGroupBox("Delay Line")
        delay_layout = QtWidgets.QGridLayout(delay_group)
        delay_layout.addWidget(QtWidgets.QLabel("Controller"), 0, 0)
        delay_layout.addWidget(self.delay_controller_edit, 0, 1)
        delay_layout.addWidget(QtWidgets.QLabel("Stage"), 1, 0)
        delay_layout.addWidget(self.delay_stage_edit, 1, 1)
        delay_layout.addWidget(QtWidgets.QLabel("Port"), 2, 0)
        delay_layout.addWidget(self.delay_port_edit, 2, 1)
        delay_layout.addWidget(QtWidgets.QLabel("Direction"), 3, 0)
        delay_layout.addWidget(self.delay_direction_spin, 3, 1)
        delay_buttons = QtWidgets.QHBoxLayout()
        delay_buttons.addWidget(self.delay_connect_button)
        delay_buttons.addWidget(self.delay_disconnect_button)
        delay_buttons.addWidget(self.delay_read_button)
        delay_buttons.addWidget(self.delay_initialize_button)
        delay_layout.addLayout(delay_buttons, 4, 1)
        delay_status = QtWidgets.QGridLayout()
        self._add_status_rows(
            delay_status,
            [
                ("Connection", self.delay_connection_label),
                ("t (ps)", self.delay_t_label),
                ("Stage (mm)", self.delay_mm_label),
                ("Pulse", self.delay_pulse_label),
            ],
        )
        delay_layout.addLayout(delay_status, 5, 0, 1, 2)

        scanner_row = QtWidgets.QHBoxLayout()
        scanner_row.addWidget(self._build_scanner_group("x"))
        scanner_row.addWidget(self._build_scanner_group("y"))

        left_column = QtWidgets.QVBoxLayout()
        left_column.addWidget(session_group)
        left_column.addWidget(lockin_group)
        left_column.addWidget(lockin_status_group)
        left_column.addWidget(delay_group)
        left_column.addLayout(scanner_row)

        layout.addLayout(left_column)
        layout.addWidget(QtWidgets.QLabel("Event Log"))
        layout.addWidget(self.log, 1)
        self.setCentralWidget(root)

    def _add_status_rows(self, layout: QtWidgets.QGridLayout, rows: list[tuple[str, QtWidgets.QLabel]]):
        for row, (name, label) in enumerate(rows):
            layout.addWidget(QtWidgets.QLabel(name), row, 0)
            layout.addWidget(label, row, 1)

    def _build_scanner_group(self, axis: str) -> QtWidgets.QGroupBox:
        widgets = self.scanner_widgets[axis]
        labels = self.scanner_status_labels[axis]
        group = QtWidgets.QGroupBox(f"Scanner {axis.upper()}")
        layout = QtWidgets.QGridLayout(group)
        layout.addWidget(QtWidgets.QLabel("Controller"), 0, 0)
        layout.addWidget(widgets["controller"], 0, 1)
        layout.addWidget(QtWidgets.QLabel("Actuator"), 1, 0)
        layout.addWidget(widgets["actuator"], 1, 1)
        layout.addWidget(QtWidgets.QLabel("Port"), 2, 0)
        layout.addWidget(widgets["port"], 2, 1)
        layout.addWidget(QtWidgets.QLabel("Axis"), 3, 0)
        layout.addWidget(widgets["axis"], 3, 1)
        layout.addWidget(QtWidgets.QLabel("sample um / unit"), 4, 0)
        layout.addWidget(widgets["scale"], 4, 1)
        buttons = QtWidgets.QHBoxLayout()
        buttons.addWidget(widgets["connect"])
        buttons.addWidget(widgets["disconnect"])
        buttons.addWidget(widgets["read"])
        buttons.addWidget(widgets["initialize"])
        layout.addLayout(buttons, 5, 0, 1, 2)
        status_layout = QtWidgets.QGridLayout()
        self._add_status_rows(
            status_layout,
            [
                ("Connection", labels["connection"]),
                (f"{axis} (um)", labels["um"]),
                ("Control", labels["control"]),
                ("State", labels["state"]),
                ("Moving", labels["moving"]),
            ],
        )
        layout.addLayout(status_layout, 6, 0, 1, 2)
        return group

    def _connect_signals(self):
        self.config_browse_button.clicked.connect(self.choose_config)
        self.config_load_button.clicked.connect(self.load_config)
        self.lockin_refresh_button.clicked.connect(self.refresh_resources)
        self.lockin_connect_button.clicked.connect(self.handle_connect_lockin)
        self.lockin_disconnect_button.clicked.connect(self.handle_disconnect_lockin)
        self.lockin_read_button.clicked.connect(self.handle_read_lockin)
        self.lockin_wait_button.clicked.connect(self.handle_read_wait_time)
        self.delay_connect_button.clicked.connect(self.handle_connect_delay_stage)
        self.delay_disconnect_button.clicked.connect(self.handle_disconnect_delay_stage)
        self.delay_read_button.clicked.connect(self.handle_read_delay_stage)
        self.delay_initialize_button.clicked.connect(self.handle_initialize_delay_stage)
        for axis in ("x", "y"):
            widgets = self.scanner_widgets[axis]
            widgets["connect"].clicked.connect(lambda checked=False, axis=axis: self.handle_connect_scanner(axis))
            widgets["disconnect"].clicked.connect(lambda checked=False, axis=axis: self.handle_disconnect_scanner(axis))
            widgets["read"].clicked.connect(lambda checked=False, axis=axis: self.handle_read_scanner(axis))
            widgets["initialize"].clicked.connect(lambda checked=False, axis=axis: self.handle_initialize_scanner(axis))

    def append_log(self, message: str):
        timestamp = datetime.now().strftime("%H:%M:%S")
        self.log.appendPlainText(f"[{timestamp}] {message}")

    def choose_config(self):
        path, _ = QtWidgets.QFileDialog.getOpenFileName(
            self,
            "Open config",
            str(Path(self.config_path_edit.text()).parent),
            "JSON Files (*.json);;All Files (*)",
        )
        if path:
            self.config_path_edit.setText(path)
            self.load_config()

    def load_config(self):
        path = Path(self.config_path_edit.text().strip())
        try:
            self._config = load_config(path)
            lockin_config = lockin_config_for(self._config, "signal_monitor")
            self.lockin_model_combo.setCurrentText(str(lockin_config.get("model", "SR7265")))
            resource = str(lockin_config.get("resource", ""))
            if resource:
                self._set_combo_text(self.lockin_resource_combo, resource)
            self._apply_delay_stage_config(delay_stage_config_for(self._config, "trkr"))
            self._apply_scanner_config("x", scanner_config_for(self._config, "x", "srkr"))
            self._apply_scanner_config("y", scanner_config_for(self._config, "y", "srkr"))
            self.append_log(f"Loaded config: {path}")
        except Exception as e:
            self.append_log(f"Config load error: {e}")
            QtWidgets.QMessageBox.warning(self, "Config Load Error", str(e))

    def _set_combo_text(self, combo: QtWidgets.QComboBox, text: str):
        index = combo.findText(text)
        if index < 0:
            combo.insertItem(0, text)
            index = 0
        combo.setCurrentIndex(index)

    def _lockin_config(self) -> dict[str, Any]:
        config = {
            "model": self.lockin_model_combo.currentText().strip(),
            "resource": self.lockin_resource_combo.currentText().strip(),
        }
        if self._config:
            try:
                loaded = lockin_config_for(self._config, "signal_monitor")
            except ValueError:
                loaded = {}
            if loaded:
                merged = dict(loaded)
                merged.update({k: v for k, v in config.items() if v})
                return merged
        return config

    def _delay_stage_config(self) -> dict[str, Any]:
        config = {
            "controller": self.delay_controller_edit.text().strip(),
            "stage": self.delay_stage_edit.text().strip(),
            "port": self.delay_port_edit.text().strip(),
            "direction": self.delay_direction_spin.value(),
        }
        if self._config:
            try:
                loaded = delay_stage_config_for(self._config, "trkr")
            except ValueError:
                loaded = {}
            if loaded:
                merged = dict(loaded)
                merged.update({k: v for k, v in config.items() if v is not None and v != ""})
                return merged
        return config

    def _scanner_config(self, axis: str) -> dict[str, Any]:
        widgets = self.scanner_widgets[axis]
        config = {
            "controller": widgets["controller"].text().strip(),
            "actuator": widgets["actuator"].text().strip(),
            "port": widgets["port"].text().strip(),
            "axis": widgets["axis"].text().strip(),
            "sample_um_per_unit": widgets["scale"].value(),
        }
        if self._config:
            try:
                loaded = scanner_config_for(self._config, axis, "srkr")
            except ValueError:
                loaded = {}
            if loaded:
                merged = dict(loaded)
                merged.update({k: v for k, v in config.items() if v is not None and v != ""})
                return merged
        return config

    def _apply_delay_stage_config(self, config: dict[str, Any]):
        self.delay_controller_edit.setText(str(config.get("controller", "SHOT302GS")))
        self.delay_stage_edit.setText(str(config.get("stage", "")))
        self.delay_port_edit.setText(str(config.get("port", "")))
        self.delay_direction_spin.setValue(int(config.get("direction", 0)))

    def _apply_scanner_config(self, axis: str, config: dict[str, Any]):
        widgets = self.scanner_widgets[axis]
        widgets["controller"].setText(str(config.get("controller", "CONEXCC")))
        widgets["actuator"].setText(str(config.get("actuator", "")))
        widgets["port"].setText(str(config.get("port", "")))
        widgets["axis"].setText(str(config.get("axis", 1)))
        widgets["scale"].setValue(float(config.get("sample_um_per_unit", config.get("sample_um_per_actuator_mm", 1.0))))

    def refresh_resources(self):
        current = self.lockin_resource_combo.currentText().strip()
        try:
            resources = list_visa_resources()
            self.lockin_resource_combo.clear()
            self.lockin_resource_combo.addItems(resources)
            if current:
                self._set_combo_text(self.lockin_resource_combo, current)
            self.append_log(f"VISA resources: {', '.join(resources) if resources else '(none)'}")
        except Exception as e:
            self.append_log(f"VISA refresh error: {e}")

    def handle_connect_lockin(self):
        try:
            self._lockin = connect_lockin(self._lockin_config())
            self.lockin_connection_label.setText("connected")
            self.append_log("Lock-in connected.")
            self.handle_read_lockin()
        except Exception as e:
            self.lockin_connection_label.setText("error")
            self.append_log(f"Lock-in connection error: {e}")
            QtWidgets.QMessageBox.warning(self, "Lock-in Error", str(e))

    def handle_disconnect_lockin(self):
        try:
            disconnect_lockin(self._lockin_config())
        finally:
            self._lockin = None
            self.lockin_connection_label.setText("disconnected")
            self.append_log("Lock-in disconnected.")

    def handle_read_lockin(self):
        if self._lockin is None:
            self._lockin = connect_lockin(self._lockin_config())
            self.lockin_connection_label.setText("connected")
        signal = read_lockin_signal(lockin=self._lockin)
        settings = read_lockin_settings(lockin=self._lockin)
        overload = read_lockin_overload(lockin=self._lockin)
        overloaded = bool(overload.get("overload"))
        self.x_label.setText(_format_float(signal.get("X")))
        self.y_label.setText(_format_float(signal.get("Y")))
        self.r_label.setText(_format_float(signal.get("R")))
        self.theta_label.setText(_format_float(signal.get("Theta")))
        self.overload_label.setText("YES" if overloaded else "no")
        self.sensitivity_label.setText(_format_float(settings.get("Sensitivity")))
        self.tc_label.setText(_format_float(settings.get("Time Constant")))
        self.ref_freq_label.setText(_format_float(settings.get("Ref. Freq")))
        self.append_log("Lock-in status read.")

    def handle_read_wait_time(self):
        if self._lockin is None:
            self._lockin = connect_lockin(self._lockin_config())
            self.lockin_connection_label.setText("connected")
        wait_s = get_lockin_wait_time(lockin=self._lockin, multiplier=4.0)
        self.wait_label.setText(_format_float(wait_s))
        self.append_log(f"Wait time read: {wait_s:.6g} s")

    def handle_connect_delay_stage(self):
        try:
            self._delay_stage = connect_delay_stage(self._delay_stage_config())
            self.delay_connection_label.setText("connected")
            self.append_log("Delay line connected.")
            self.handle_read_delay_stage()
        except Exception as e:
            self.delay_connection_label.setText("error")
            self.append_log(f"Delay line connection error: {e}")
            QtWidgets.QMessageBox.warning(self, "Delay Line Error", str(e))

    def handle_disconnect_delay_stage(self):
        try:
            disconnect_delay_stage(self._delay_stage_config())
        finally:
            self._delay_stage = None
            self.delay_connection_label.setText("disconnected")
            self.append_log("Delay line disconnected.")

    def handle_read_delay_stage(self):
        try:
            if self._delay_stage is None:
                self._delay_stage = connect_delay_stage(self._delay_stage_config())
                self.delay_connection_label.setText("connected")
            row = read_delay_stage(self._delay_stage_config(), delay_stage=self._delay_stage)
            self.delay_t_label.setText(_format_float(row.get("t_ps")))
            self.delay_mm_label.setText(_format_float(row.get("stage_mm")))
            self.delay_pulse_label.setText(str(row.get("stage_pulse", "-")))
            self.append_log("Delay line status read.")
        except Exception as e:
            self.delay_connection_label.setText("error")
            self.append_log(f"Delay line read error: {e}")
            QtWidgets.QMessageBox.warning(self, "Delay Line Error", str(e))

    def handle_initialize_delay_stage(self):
        try:
            if self._delay_stage is None:
                self._delay_stage = connect_delay_stage(self._delay_stage_config())
                self.delay_connection_label.setText("connected")
            info = self._delay_stage.initialize(home=True)
            self.append_log(f"Delay line initialized: {info}")
            self.handle_read_delay_stage()
        except Exception as e:
            self.delay_connection_label.setText("error")
            self.append_log(f"Delay line initialize error: {e}")
            QtWidgets.QMessageBox.warning(self, "Delay Line Error", str(e))

    def handle_connect_scanner(self, axis: str):
        try:
            scanner = connect_scanner(self._scanner_config(axis))
            self._scanners[axis] = scanner
            self.scanner_status_labels[axis]["connection"].setText("connected")
            self.append_log(f"Scanner {axis.upper()} connected.")
            self.handle_read_scanner(axis)
        except Exception as e:
            self.scanner_status_labels[axis]["connection"].setText("error")
            self.append_log(f"Scanner {axis.upper()} connection error: {e}")
            QtWidgets.QMessageBox.warning(self, f"Scanner {axis.upper()} Error", str(e))

    def handle_disconnect_scanner(self, axis: str):
        try:
            disconnect_scanner(self._scanner_config(axis))
        finally:
            self._scanners[axis] = None
            self.scanner_status_labels[axis]["connection"].setText("disconnected")
            self.append_log(f"Scanner {axis.upper()} disconnected.")

    def handle_read_scanner(self, axis: str):
        try:
            if self._scanners[axis] is None:
                self._scanners[axis] = connect_scanner(self._scanner_config(axis))
                self.scanner_status_labels[axis]["connection"].setText("connected")
            zero_um = zero_for(self._config, axis) if self._config else None
            row = read_scanner(axis, self._scanner_config(axis), scanner=self._scanners[axis], zero_um=zero_um)
            labels = self.scanner_status_labels[axis]
            labels["um"].setText(_format_float(row.get(f"{axis}_um")))
            control_unit = "mm" if f"{axis}_mm" in row else "deg"
            labels["control"].setText(f"{_format_float(row.get(f'{axis}_{control_unit}'))} {control_unit}")
            labels["state"].setText(str(self._scanners[axis].get_state()))
            labels["moving"].setText(_format_bool(self._scanners[axis].is_moving()))
            self.append_log(f"Scanner {axis.upper()} status read.")
        except Exception as e:
            self.scanner_status_labels[axis]["connection"].setText("error")
            self.append_log(f"Scanner {axis.upper()} read error: {e}")
            QtWidgets.QMessageBox.warning(self, f"Scanner {axis.upper()} Error", str(e))

    def handle_initialize_scanner(self, axis: str):
        try:
            if self._scanners[axis] is None:
                self._scanners[axis] = connect_scanner(self._scanner_config(axis))
                self.scanner_status_labels[axis]["connection"].setText("connected")
            info = self._scanners[axis].initialize(home=True)
            self.append_log(f"Scanner {axis.upper()} initialized: {info}")
            self.handle_read_scanner(axis)
        except Exception as e:
            self.scanner_status_labels[axis]["connection"].setText("error")
            self.append_log(f"Scanner {axis.upper()} initialize error: {e}")
            QtWidgets.QMessageBox.warning(self, f"Scanner {axis.upper()} Error", str(e))

    def closeEvent(self, event):
        if self._lockin is not None:
            try:
                disconnect_lockin(self._lockin_config())
            except Exception:
                pass
        if self._delay_stage is not None:
            try:
                disconnect_delay_stage(self._delay_stage_config())
            except Exception:
                pass
        for axis, scanner in self._scanners.items():
            if scanner is not None:
                try:
                    disconnect_scanner(self._scanner_config(axis))
                except Exception:
                    pass
        super().closeEvent(event)


def main():
    app = QtWidgets.QApplication(sys.argv)
    window = TrkrGuiTestWindow()
    window.show()
    return app.exec()


if __name__ == "__main__":
    raise SystemExit(main())
