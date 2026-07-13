from __future__ import annotations

from typing import Any, TextIO, cast

from PySide6 import QtCore
from serial.tools import list_ports

from kohdalab.api import Experiment
from kohdalab.api.scan_plan import Scan2DPlan, Srkr2DPlan, SrkrPlan, StrkrPlan, TrkrPlan
from kohdalab.interfaces.lockin import list_visa_resources


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
    ) -> None:
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

    def stop(self) -> None:
        self._running = False

    def _should_continue(self) -> bool:
        return self._running

    def run(self) -> None:
        try:
            if self.measurement == "signal_monitor":
                rows = self.experiment.run_signal_monitor(
                    interval_s=self.interval_s,
                    n_points=self.n_points,
                    output=self.output_path,
                    on_point=self.point_ready.emit,
                    on_status=self.status_changed.emit,
                    should_continue=self._should_continue,
                )
            elif self.measurement == "trkr":
                trkr_scan_plan = cast(TrkrPlan | None, self.scan_plan)
                rows = self.experiment.run_trkr(
                    plan=trkr_scan_plan,
                    wait_s=self.wait_s,
                    return_to_zero=self.return_to_zero,
                    output=self.output_path,
                    on_point=self.point_ready.emit,
                    on_status=self.status_changed.emit,
                    should_continue=self._should_continue,
                )
            elif self.measurement == "srkr":
                srkr_scan_plan = cast(SrkrPlan | None, self.scan_plan)
                rows = self.experiment.run_srkr(
                    axis=self.axis,
                    plan=srkr_scan_plan,
                    wait_s=self.wait_s,
                    return_to_zero=self.return_to_zero,
                    output=self.output_path,
                    on_point=self.point_ready.emit,
                    on_status=self.status_changed.emit,
                    should_continue=self._should_continue,
                )
            elif self.measurement == "strkr":
                strkr_scan_plan = cast(StrkrPlan | None, self.scan_plan)
                rows = self.experiment.run_strkr(
                    plan=strkr_scan_plan,
                    wait_s=self.wait_s,
                    output=self.output_path,
                    on_point=self.point_ready.emit,
                    on_status=self.status_changed.emit,
                    should_continue=self._should_continue,
                )
            elif self.measurement == "srkr_2d":
                srkr_2d_scan_plan = cast(Srkr2DPlan | None, self.scan_plan)
                rows = self.experiment.run_srkr_2d(
                    plan=srkr_2d_scan_plan,
                    wait_s=self.wait_s,
                    output=self.output_path,
                    on_point=self.point_ready.emit,
                    on_status=self.status_changed.emit,
                    should_continue=self._should_continue,
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
    ) -> None:
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
            info = self.experiment.initialize_delay_stage(
                "delay_stage.t", on_status=self.status_changed.emit
            )
            return {
                "command": command,
                "kind": "delay_stage",
                "axis": "t",
                "info": info,
            }
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
    def run_command(self, request: object) -> None:
        try:
            data = request if isinstance(request, dict) else {}
            self.finished.emit(self._execute(data))
        except Exception as e:
            self.error_occurred.emit(str(e))

    def run(self) -> None:
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
    ) -> None:
        super().__init__()
        self.experiment = experiment
        self.axis = axis
        self.value = value
        self.coordinate = coordinate

    def run(self) -> None:
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
        key = next(iter(self.experiment.lockins), None)
        return None if key is None else f"lockin.{key}"

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
    def read_full(self) -> None:
        if self._busy:
            return
        self._busy = True
        try:
            status = self.experiment.read_live_status(skip_busy_positions=True)
            self.live_status_ready.emit(status, status.lockin_overload)
        except Exception as e:
            self.error_occurred.emit(str(e))
        finally:
            self._busy = False

    @QtCore.Slot()
    def read_lockin(self) -> None:
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
    def run(self) -> None:
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

    def __init__(self, stream: TextIO) -> None:
        super().__init__()
        self._stream = stream
        self._buffer = ""

    def write(self, text: str) -> None:
        self._stream.write(text)
        self._stream.flush()
        self._buffer += text
        while "\n" in self._buffer:
            line, self._buffer = self._buffer.split("\n", 1)
            if line:
                self.text_ready.emit(line)

    def flush(self) -> None:
        self._stream.flush()
        if self._buffer:
            self.text_ready.emit(self._buffer)
            self._buffer = ""
