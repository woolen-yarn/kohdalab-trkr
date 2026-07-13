from __future__ import annotations

from contextlib import contextmanager
from copy import deepcopy
from pathlib import Path
from threading import RLock
from types import TracebackType
from typing import Any, Callable, Iterator, Literal, Self

from kohdalab.api.device_requirements import missing_devices, required_devices
from kohdalab.api.models import LiveStatus, Position
from kohdalab.api.scan_plan import (
    SignalMonitorPlan,
    Srkr2DPlan,
    SrkrPlan,
    StrkrPlan,
    TrkrPlan,
)
from kohdalab.api.session import DeviceSession
from kohdalab.api.config import load_config, normalize_config
from kohdalab.api.status import StatusCallback
from . import measurements


PointCallback = Callable[[Any], None]
ContinueCallback = Callable[[], bool]


class Experiment:
    """User-facing facade for notebooks, GUI, CLI, and future app workflows.

    The experiment owns one long-lived `DeviceSession`. Measurement methods
    always reuse that session and do not disconnect it after a run.
    """

    def __init__(self, config: dict[str, Any], *, auto_connect: bool = True):
        if not isinstance(auto_connect, bool):
            raise TypeError("auto_connect must be boolean.")
        self._config = normalize_config(config)
        self.auto_connect = auto_connect
        self.session = DeviceSession(self._config, auto_connect=self.auto_connect)
        self._state_lock = RLock()
        self._active_operations = 0
        self._config_blocking_operations = 0
        self._closed = False

    def _ensure_open(self) -> None:
        if self._closed:
            raise RuntimeError("Experiment is closed.")

    @contextmanager
    def _operation(self, *, config_update_safe: bool = False) -> Iterator[None]:
        with self._state_lock:
            self._ensure_open()
            self._active_operations += 1
            if not config_update_safe:
                self._config_blocking_operations += 1
        try:
            yield
        finally:
            with self._state_lock:
                self._active_operations -= 1
                if not config_update_safe:
                    self._config_blocking_operations -= 1

    def __enter__(self) -> Self:
        with self._state_lock:
            self._ensure_open()
            return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> Literal[False]:
        del exc_type, traceback
        try:
            self.close()
        except Exception as cleanup_exc:
            if exc is None:
                raise
            exc.add_note(f"Experiment cleanup also failed: {cleanup_exc}")
        return False

    def close(self) -> None:
        """Release all hardware leases owned by this experiment."""
        with self._state_lock:
            if self._closed:
                return
            if self._active_operations:
                raise RuntimeError(
                    "Cannot close Experiment while an operation is active."
                )
            self.session.close()
            self._closed = True

    @property
    def closed(self) -> bool:
        with self._state_lock:
            return self._closed

    @classmethod
    def from_config(
        cls, path: str | Path, *, auto_connect: bool = True
    ) -> "Experiment":
        return cls(load_config(path), auto_connect=auto_connect)

    @property
    def config(self) -> dict[str, Any]:
        with self._state_lock:
            return deepcopy(self._config)

    @config.setter
    def config(self, value: dict[str, Any]) -> None:
        normalized = normalize_config(value)
        with self._state_lock:
            self._ensure_open()
            if self._config_blocking_operations:
                raise RuntimeError(
                    "Cannot change Experiment config while an operation is active."
                )
            self.session.set_config(normalized)
            self._config = normalized

    @property
    def lockins(self) -> dict[str, Any]:
        with self._state_lock:
            return dict(self.session.lockins)

    @property
    def delay_stages(self) -> dict[str, Any]:
        with self._state_lock:
            return dict(self.session.delay_stages)

    @property
    def scanners(self) -> dict[str, Any]:
        with self._state_lock:
            return dict(self.session.scanners)

    def connect_all(self) -> None:
        with self._operation():
            self.session.connect_all()

    def connect(self) -> None:
        self.connect_all()

    def connect_device(self, ref: str) -> Any:
        with self._operation():
            return self.session.connect_device(ref)

    def disconnect_all(self) -> None:
        with self._state_lock:
            if self._closed:
                return
        with self._operation():
            self.session.disconnect_all()

    def disconnect(self) -> None:
        self.disconnect_all()

    def disconnect_device(self, ref: str) -> None:
        with self._state_lock:
            if self._closed:
                return
        with self._operation():
            self.session.disconnect_device(ref)

    def connected_devices(self) -> dict[str, bool]:
        return self.session.connected_devices()

    def required_devices(
        self,
        measurement_name: str,
        *,
        axis: str | None = None,
        fast_axis: str | None = None,
        slow_axis: str | None = None,
    ) -> list[str]:
        return required_devices(
            self.config,
            measurement_name,
            axis=axis,
            fast_axis=fast_axis,
            slow_axis=slow_axis,
        )

    def missing_devices(
        self,
        measurement_name: str,
        *,
        axis: str | None = None,
        fast_axis: str | None = None,
        slow_axis: str | None = None,
    ) -> list[str]:
        return missing_devices(
            self.config,
            self.connected_devices(),
            measurement_name,
            axis=axis,
            fast_axis=fast_axis,
            slow_axis=slow_axis,
        )

    def read_position(self, *, skip_busy: bool = False) -> Position:
        with self._operation(config_update_safe=True):
            if skip_busy:
                return self.session.read_position(skip_busy=True)
            return self.session.read_position()

    def read_lockin_signal(self, ref: str = "signal") -> dict[str, Any]:
        with self._operation(config_update_safe=True):
            return self.session.read_lockin_signal(ref)

    def read_lockin_settings(self, ref: str = "signal") -> dict[str, Any]:
        with self._operation(config_update_safe=True):
            return self.session.read_lockin_settings(ref)

    def read_lockin_overload(self, ref: str = "signal") -> dict[str, Any]:
        with self._operation(config_update_safe=True):
            return self.session.read_lockin_overload(ref)

    def lockin_wait_time(
        self, ref: str = "signal", *, multiplier: float = 4.0
    ) -> float:
        with self._operation():
            return self.session.lockin_wait_time(ref, multiplier=multiplier)

    def set_lockin_settings(
        self,
        ref: str = "signal",
        *,
        sensitivity: float | None = None,
        time_constant: float | None = None,
        ac_gain: float | None = None,
        coupling: str | None = None,
        slope: int | None = None,
    ) -> dict[str, Any]:
        with self._operation():
            return self.session.set_lockin_settings(
                ref,
                sensitivity=sensitivity,
                time_constant=time_constant,
                ac_gain=ac_gain,
                coupling=coupling,
                slope=slope,
            )

    def read_live_status(self, *, skip_busy_positions: bool = False) -> LiveStatus:
        with self._operation(config_update_safe=True):
            if skip_busy_positions:
                return self.session.read_live_status(skip_busy_positions=True)
            return self.session.read_live_status()

    def initialize_delay_stage(
        self,
        ref: str = "delay_stage",
        *,
        on_status: StatusCallback | None = None,
    ) -> dict[str, Any]:
        with self._operation():
            return self.session.initialize_delay_stage(ref, on_status=on_status)

    def initialize_scanner(
        self,
        axis: str,
        ref: str | None = None,
        *,
        on_status: StatusCallback | None = None,
    ) -> dict[str, Any]:
        with self._operation():
            return self.session.initialize_scanner(axis, ref, on_status=on_status)

    def initialize_xy(
        self, *, on_status: StatusCallback | None = None
    ) -> dict[str, Any]:
        with self._operation():
            return self.session.initialize_xy(on_status=on_status)

    def move_delay_stage(
        self,
        value: float,
        *,
        coordinate: str = "measurement",
        ref: str = "delay_stage",
        on_status: StatusCallback | None = None,
        on_position: Callable[[dict[str, Any]], None] | None = None,
    ) -> Position:
        with self._operation():
            return self.session.move_delay_stage(
                value,
                coordinate=coordinate,
                ref=ref,
                on_status=on_status,
                on_position=on_position,
            )

    def move_scanner(
        self,
        axis: str,
        value: float,
        *,
        coordinate: str = "measurement",
        ref: str | None = None,
        apply_software_hysteresis: bool = True,
        on_status: StatusCallback | None = None,
        on_position: Callable[[dict[str, Any]], None] | None = None,
    ) -> Position:
        if not isinstance(apply_software_hysteresis, bool):
            raise TypeError("apply_software_hysteresis must be boolean.")
        with self._operation():
            return self.session.move_scanner(
                axis,
                value,
                coordinate=coordinate,
                ref=ref,
                apply_software_hysteresis=apply_software_hysteresis,
                on_status=on_status,
                on_position=on_position,
            )

    def run_signal_monitor(
        self,
        *,
        plan: SignalMonitorPlan | None = None,
        interval_s: float | None = None,
        n_points: int | None = None,
        output: str | Path | None = None,
        on_status: StatusCallback | None = None,
        on_point: PointCallback | None = None,
        should_continue: ContinueCallback | None = None,
    ) -> list[dict[str, Any]]:
        with self._operation():
            return measurements.run_signal_monitor(
                self.config,
                plan=plan,
                interval_s=interval_s,
                n_points=n_points,
                output=output,
                on_point=on_point,
                should_continue=should_continue,
                on_status=on_status,
                session=self.session,
            )

    def run_trkr(
        self,
        *,
        plan: TrkrPlan | None = None,
        scan_points: list[float | int] | None = None,
        target_points: list[float | int] | None = None,
        coordinate: str | None = None,
        wait_s: float | None = None,
        output: str | Path | None = None,
        return_to_zero: bool | None = None,
        on_status: StatusCallback | None = None,
        on_point: PointCallback | None = None,
        should_continue: ContinueCallback | None = None,
    ) -> list[dict[str, Any]]:
        with self._operation():
            return measurements.run_trkr(
                self.config,
                plan=plan,
                scan_points=scan_points,
                target_points=target_points,
                coordinate=coordinate,
                wait_s=wait_s,
                output=output,
                return_to_zero=return_to_zero,
                on_point=on_point,
                should_continue=should_continue,
                on_status=on_status,
                session=self.session,
            )

    def run_srkr(
        self,
        *,
        plan: SrkrPlan | None = None,
        axis: str | None = None,
        scan_points: list[float | int] | None = None,
        target_points: list[float | int] | None = None,
        coordinate: str | None = None,
        wait_s: float | None = None,
        output: str | Path | None = None,
        return_to_zero: bool | None = None,
        on_status: StatusCallback | None = None,
        on_point: PointCallback | None = None,
        should_continue: ContinueCallback | None = None,
    ) -> list[dict[str, Any]]:
        with self._operation():
            return measurements.run_srkr(
                self.config,
                plan=plan,
                axis=axis,
                scan_points=scan_points,
                target_points=target_points,
                coordinate=coordinate,
                wait_s=wait_s,
                output=output,
                return_to_zero=return_to_zero,
                on_point=on_point,
                should_continue=should_continue,
                on_status=on_status,
                session=self.session,
            )

    def run_strkr(
        self,
        *,
        plan: StrkrPlan | None = None,
        wait_s: float | None = None,
        output: str | Path | None = None,
        on_status: StatusCallback | None = None,
        on_point: PointCallback | None = None,
        should_continue: ContinueCallback | None = None,
    ) -> list[dict[str, Any]]:
        with self._operation():
            return measurements.run_strkr(
                self.config,
                plan=plan,
                wait_s=wait_s,
                output=output,
                on_point=on_point,
                should_continue=should_continue,
                on_status=on_status,
                session=self.session,
            )

    def run_srkr_2d(
        self,
        *,
        plan: Srkr2DPlan | None = None,
        wait_s: float | None = None,
        output: str | Path | None = None,
        on_status: StatusCallback | None = None,
        on_point: PointCallback | None = None,
        should_continue: ContinueCallback | None = None,
    ) -> list[dict[str, Any]]:
        with self._operation():
            return measurements.run_srkr_2d(
                self.config,
                plan=plan,
                wait_s=wait_s,
                output=output,
                on_point=on_point,
                should_continue=should_continue,
                on_status=on_status,
                session=self.session,
            )
