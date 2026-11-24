# KohdaLab Control Protocol Python client

This separate standard-library package discovers and controls the common `control.*`
GUI API of IV, TRKR, Camera, Shutter and PLE. It never opens an instrument connection.
Install the same package from any updated app's source or portable common SDK folder:

```sh
python -m pip install ./bindings/control
```

```python
from kohdalab_control import ControlClient, discover

for endpoint in discover():
    print(endpoint.application, endpoint.instance_id)  # token is not printed

with ControlClient("kohdalab-iv") as app:
    print(app.describe())
    print(app.status())
    # Only invoke operations advertised by this app's describe()['methods'].
    job = app.invoke("connect")
    app.wait(job["job_id"])
    job = app.invoke("start")
    finished = app.wait(job["job_id"])
```

The GUI must be started with its documented local API opt-in/launch mode and a
reviewed profile. Multiple instances require an explicit descriptor `endpoint=`;
selection by process recency is never used. Canonical discovery is
`~/.kohdalab/<app>/run` (`%USERPROFILE%\.kohdalab\<app>\run` on Windows).
Read-only inspection does not claim a lease. Context entry or `invoke` mutations
acquire GUI ownership. Do not use a second native client to open the same device.

`describe()` reports supported methods and the actual per-app safety policy.
Shutter holds its last state; IV verifies output OFF for release; motion/optical
cleanup and Camera acquisition have their own contracts. They are not made equal
by the shared API. A stop ACK is not proof of cleanup: use `stop_and_wait()` only
where stop is advertised. Apps whose stop targets a job (TRKR) require
`stop_and_wait(job["job_id"])`, which waits for that job's own terminal receipt
with verified cleanup; without an ID the original app-wide stop/status wait is kept. `release()` waits for the application to confirm release;
a failure is reported, not silently treated as success. Complete or explicitly
stop active work before closing when that application's policy requires it.

If the application reports `continuing: true` (a job intentionally keeps running
under its `on_release` policy), `release()` returns that result unchanged at once
after checking it carries the `job_id`, that job's non-terminal receipt and
`cleanup: "not_required"` (anything else is a validation error, never success);
follow it with `wait(job_id)`. A released client cannot stop it, so call
`stop_and_wait(job_id)` before releasing if it must stop. When release itself
requests a job stop, the SDK polls that job (following bounded receipt links, as
`stop_and_wait` does) until it is completed/stopped with confirmed cleanup; it
does not wait on global ownership, so a new owner cannot delay or fake it. The
original release acknowledgement is kept under `release`. Without a job, status
must show not owned, not busy and confirmed cleanup.

`release(reason=...)` sends a release reason only when given. When an exception
leaves a `with ControlClient(...)` block and the application advertises a
`stop_on_error` policy (TRKR), the reason `python_error` is sent so that policy
applies; other applications receive the original request. The experiment
exception is never suppressed.

Mutations use caller UUIDs and are never automatically retried on a transport
failure. `ControlError.request_id` and `outcome_unknown` preserve ambiguity for
manual status/result inspection. Job wait timeout does not secretly stop or replay
a measurement. `wait` follows an explicit command-ACK-to-job link and polls exactly
that job, never the latest job. Native product payload remains under `native`.
Read methods and the existing application-specific clients remain available.

共通窓口はGUIを介して操作し、機器を直接開きません。対応機能と停止・解放時の安全方針は
`describe()`で確認してください。各アプリ固有の停止動作や測定CSVは変更しません。
通信失敗時に操作を自動再送せず、Stop応答だけを停止完了とは見なしません。

If experiment code and release cleanup both fail, the cleanup error is raised
with the original experiment exception retained as its cause.
