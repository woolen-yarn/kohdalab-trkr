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

## 日本語

独立したGPIB実行ファイルは、GPLv2互換のKAME/Linux-GPIBコードとGPL-2.0-onlyのAgilent対応部を組み合わせ、LGPL-2.1-or-laterのlibusbへ静的リンクします。受領者がlibusbを変更して再リンクできるよう、完全なlibusbソースアーカイブ、ソース、権利表記、ビルドスクリプトを提供します。

macOSではXcode Command Line Tools（clang・make）を導入し、冒頭の`bash scripts/build_native.sh`を実行します。WindowsではMSYS2 UCRT64にmake、GCC/G++、Python、windresを導入し、冒頭の`pacman`と`bash`のコマンドを実行します。

出力は`build/native/kohdalab-gpib-helper[.exe]`です。Windowsでは`kohdalab-visa-helper.exe`も生成します。再ビルドしたhelperを使うにはパッケージの`support/`内の対応ファイルを置き換えます。Rust本体は標準入出力で通信し、GPLコードにはリンクしません。ソース配布にはRust本体を別途再ビルド・置換するためのCargo manifestとlockfileも含みます。このスクリプトはドライバーをインストールしません。
