# Shared GPIB development mode

The updated TRKR transport can connect through the shared instrument service
developed in the KohdaLab IV source tree. This is explicit opt-in development
support, not a published portable feature or hardware-qualified backend.

The service alone opens the native GPIB adapter. Different addresses can be
owned by IV and TRKR simultaneously; the same address has only one owner.
TRKR retains its existing measurement, cancellation and Stop behavior.
A client disconnect never sends global REN-off/reset/EXIT to a peer's bus.

Build the updated TRKR and the IV workspace's `kohdalab-instrument-hub` binary.
Start the service with a dedicated private state directory and an operator
device map that registers the TRKR lock-in address as `instrument` and every
output source with its exact `source` model. Unregistered addresses are refused.
See the IV repository's `docs/guides/shared-instruments-development.md` for the
service command, device-map format, safety policies and recovery procedure.

Launch this rebuilt TRKR from a shell with these variables:

```sh
export KOHDALAB_INSTRUMENT_HUB_CONFIG=/private/path/to/endpoint.json
export KOHDALAB_INSTRUMENT_HUB_CLIENT=/absolute/path/to/kohdalab-instrument-hub
```

On Windows, set the same variables in PowerShell (`$env:NAME = 'path'`) before
launching the rebuilt executable. An unavailable/incompatible service or bad
configuration is an error; TRKR never opens a local GPIB helper as fallback.
The default backend is unchanged when the configuration variable is absent.

The shared service currently supports one native NI/82357B controller and
primary GPIB addresses 1-30. Official VISA support, automatic startup/packaging,
same-instrument observation, and physical macOS/Windows validation are pending.
Existing published TRKR/IV versions keep whole-adapter exclusion.
Do not remove their lock files to try to share a controller.
