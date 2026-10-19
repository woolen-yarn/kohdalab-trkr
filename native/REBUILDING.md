# Rebuilding and relinking native helpers

The separate GPIB executable combines GPLv2-compatible KAME/Linux-GPIB code and
the GPL-2.0-only Agilent adaptation. It statically links LGPL-2.1-or-later libusb.
The complete libusb source archive, source files, notices and build script are
provided, so recipients can change libusb and relink the helper.

On macOS install Xcode Command Line Tools (clang, make), then run:

```sh
bash scripts/build_native.sh
```

On Windows use MSYS2 UCRT64 with make, GCC/G++, Python and windres:

```sh
pacman -S --needed make mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-python
bash scripts/build_native.sh
```

Output is `build/native/kohdalab-gpib-helper[.exe]`; Windows also builds
`kohdalab-visa-helper.exe`. Replace the corresponding package `support/` file to
use a rebuilt helper. The main Rust process communicates over stdio and is not
linked to GPL code. Source packages include Cargo manifests/lockfile for rebuilding
and replacing the Rust application separately. No driver is installed by this script.
