# Native transport helpers

The Rust app uses separate C/C++ helpers for USB-GPIB and Windows system VISA.
All helper source is included and rebuilt by `scripts/build_native.sh`.
`PROVENANCE.json` records the imported KohdaLab IV revision and source hashes.

- `visa_helper.cpp` and `stdio_mode.hpp`: MIT; dynamically load the user's
  system VISA runtime, without distributing NI/Keysight proprietary binaries.
- `kohdalab_gpib_helper.cpp`, Agilent adaptation, KAME/Linux-GPIB and libusb:
  separate GPLv2 executable, with upstream notices and source retained.

See [REBUILDING.md](REBUILDING.md), [vendor/README.md](vendor/README.md) and the
root [redistribution notice](../REDISTRIBUTION.md). No instrument is opened by a build.
