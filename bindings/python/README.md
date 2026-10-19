# Portable TRKR notebook binding

Install the thin client with `pip install .` from the source checkout, or use
the `notebook-client/` folder next to the portable `notebook/` folder.  The
distributed notebooks find that bundled client automatically.  They also find
`kohdalab-trkr` on `PATH`, in a source build, beside a portable archive, or in
a macOS `.app` bundle.  Set `TRKR_BINARY` and `TRKR_PROFILE` when using an
unusual location.

Use `--profile-json` to have Rust validate the existing profile and prepare
the exact controller payload; this command never opens hardware.

```python
import json
import subprocess
from pathlib import Path

from kohdalab_trkr import Client

binary = Path("portable/kohdalab-trkr")
prepared = json.loads(subprocess.check_output(
    [binary, "--profile-json", "C:/Users/me/.kohdalab/config/default.json"],
    text=True,
))

config = prepared["hardware"]
plan = prepared["plans"]["signal_monitor"]

with Client(binary) as trkr:
    # ``demo`` is the safe default.  It never opens an instrument or moves.
    events = trkr.run(config, plan, {"type": "demo"}, "monitor.csv")

for event in events:
    if event["event"] == "sample":
        print(event["data"]["signal"])
```

`run` is synchronous because the JSON-lines protocol has no request IDs.
`stop()` may be called from another thread during a running acquisition.
For an explicit native motion command, call `move(...)` followed by
`wait_for_finished()`; do this only after reviewing the laboratory profile.
