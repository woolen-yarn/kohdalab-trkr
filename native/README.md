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

## 日本語

RustアプリはUSB-GPIBとWindowsシステムVISA用の独立したC/C++ helperを使います。全ソースを同梱し、`scripts/build_native.sh`で再ビルドします。`PROVENANCE.json`は取り込んだKohdaLab IVのrevisionとソースハッシュを記録します。

- `visa_helper.cpp`と`stdio_mode.hpp`：MIT。利用者のシステムVISAを動的に読み込み、NI・Keysightの独占的バイナリは配布しません。
- `kohdalab_gpib_helper.cpp`、Agilent対応部、KAME/Linux-GPIB、libusb：独立したGPLv2実行ファイル。上流の権利表記とソースを保持します。

[再ビルド](REBUILDING.md)、[上流部品](vendor/README.md)、[再配布条件](../REDISTRIBUTION.md)を参照してください。ビルドによって実機を開くことはありません。
