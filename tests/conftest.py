from __future__ import annotations

import pytest

from kohdalab.api.session import DeviceSession


@pytest.fixture(autouse=True)
def isolate_device_session_ownership():
    with DeviceSession._ownership_lock:
        DeviceSession._shared_owners.clear()
        DeviceSession._shared_targets.clear()
    yield
    with DeviceSession._ownership_lock:
        DeviceSession._shared_owners.clear()
        DeviceSession._shared_targets.clear()
