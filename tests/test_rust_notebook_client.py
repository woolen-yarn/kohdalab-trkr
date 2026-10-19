"""Optional end-to-end checks against a locally built native controller."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]


def test_notebook_runs_native_workflows_recovers_errors_and_stops(
    tmp_path: Path,
) -> None:
    binary = Path(
        os.environ.get(
            "TRKR_NATIVE_BINARY",
            ROOT
            / "next"
            / "target"
            / "debug"
            / (
                "kohdalab-trkr.exe"
                if sys.platform == "win32"
                else "kohdalab-trkr"
            ),
        )
    )
    if not binary.is_file():
        pytest.skip("Build native TRKR or set TRKR_NATIVE_BINARY")
    code = r"""
import csv, json, subprocess, sys, threading
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]) / 'bindings/python'))
from kohdalab_trkr import Client
root, binary, output = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
prepared = json.loads(subprocess.check_output([binary, '--profile-json', root/'next/crates/trkr-desktop/assets/default.json'], text=True))
with Client(binary) as client:
    for key, plan in prepared['plans'].items():
        plan.update(interval_s=0., n_points=7)
        path=output/f'{key}.csv'
        events=client.run(prepared['hardware'], plan, {'type':'demo'}, path)
        count=len(list(csv.DictReader(path.open())))
        meta=json.loads(path.with_suffix('.run.json').read_text())
        assert meta['status']=='complete' and meta['rows']==count and meta['simulation']
        assert next(e for e in events if e['event']=='finished')['data']['Ok'][0]==count
    plan=prepared['plans']['signal_monitor']
    collision=output/'signal_monitor.csv'
    original=collision.read_bytes()
    try:
        client.run(prepared['hardware'], plan, {'type':'demo'}, collision)
    except RuntimeError:
        pass
    else:
        raise AssertionError('existing CSV was overwritten')
    assert collision.read_bytes()==original
    events=client.run(prepared['hardware'], plan, {'type':'demo'}, output/'recovered.csv')
    assert next(e for e in events if e['event']=='finished')['data']['Ok'][0]==7
    try:
        client.run(prepared['hardware'], {}, {'type':'demo'})
    except RuntimeError:
        pass
    else:
        raise AssertionError('invalid command was accepted')
    plan.update(interval_s=1., n_points=100)
    timer=threading.Timer(.1, client.stop)
    timer.start()
    client.run(prepared['hardware'], plan, {'type':'demo'}, output/'stopped.csv')
    timer.join()
    meta=json.loads((output/'stopped.run.json').read_text())
    assert meta['status']=='cancelled' and meta['rows']<100
"""
    result = subprocess.run(  # noqa: S603 - locally built controller via fixed validation script
        [sys.executable, "-c", code, str(ROOT), str(binary.resolve()), str(tmp_path)],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
