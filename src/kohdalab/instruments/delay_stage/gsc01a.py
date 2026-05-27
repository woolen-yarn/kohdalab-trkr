from __future__ import annotations

import re
import time
from typing import Optional

from .gsc01 import GSC01


class GSC01A(GSC01):
    """Raw controller API for Sigma Koki GSC-01A.

    GSC-01A keeps the SHOT/GSC-style command set used by `GSC01`, but its
    manual documents OK/NG command responses and does not expose the old
    `?:MS` microstep query. Physical-unit conversion is handled by
    `kohdalab.interfaces.delay_stage.DelayStage`.
    """

    def set_logical_zero(self, axis: Optional[int] = None):
        axis = axis or self.default_axis
        self._check_response(self.ask(f"R:{axis}"), "R")

    def stop(self, axis: Optional[int] = None):
        axis = axis or self.default_axis
        self._check_response(self.ask(f"L:{axis}"), "L")

    def immediate_stop(self):
        self._check_response(self.ask("L:E"), "L:E")

    def query_sensor_status(self, sensor: int = 0) -> str:
        return self.query_internal(f"L{sensor}")

    def get_microstep_division(self, axis: Optional[int] = None) -> int:
        axis = axis or self.default_axis
        responses = []
        for _attempt in range(3):
            resp = self.query_internal(f"S{axis}")
            responses.append(resp)
            values = re.findall(r"\d+", resp)
            if values:
                return int(values[-1])
            time.sleep(0.05)
        raise RuntimeError(f"Unexpected GSC-01A ?S response: {responses[-1]}")
