#!/usr/bin/env bash
# Self-contained GPIB helper build; no instrument or USB device is opened.
set -euo pipefail
cd "$(dirname "$0")/.."
TASK_ROOT="$PWD"
TASK_BUILD="${KOHDALAB_NATIVE_BUILD_DIR:-$TASK_ROOT/build/native}"
TASK_DRIVER="$TASK_ROOT/native/vendor/ni-gpib"
TASK_PYTHON="${KOHDALAB_PYTHON:-python3}"
TASK_WINDOWS=false
case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) TASK_WINDOWS=true;; esac
if [ -n "${KOHDALAB_WINDOWS_CC:-}" ]; then TASK_WINDOWS=true; fi
TASK_CC="${KOHDALAB_WINDOWS_CC:-cc}"
TASK_CXX="${KOHDALAB_WINDOWS_CXX:-c++}"
TASK_ARCHIVE="$TASK_ROOT/native/vendor/libusb-1.0.30.tar.bz2"
"$TASK_PYTHON" - "$TASK_ARCHIVE" <<'PY'
import hashlib,sys
assert hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest()=='fea36f34f9156400209595e300840767ab1a385ede1dc7ee893015aea9c6dbaf','libusb source checksum mismatch'
PY
mkdir -p "$TASK_BUILD/obj" "$TASK_BUILD/include/libusb-1.0"
cd "$TASK_BUILD"
if [ ! -d libusb-1.0.30 ]; then tar -xjf "$TASK_ARCHIVE"; fi
TASK_PLATFORM=(-O2)
if $TASK_WINDOWS; then
 TASK_CC="${KOHDALAB_WINDOWS_CC:-gcc}";TASK_CXX="${KOHDALAB_WINDOWS_CXX:-g++}"
else
 TASK_PLATFORM=(-mmacosx-version-min=13.0)
fi
if [ ! -f libusb-1.0.30/libusb/.libs/libusb-1.0.a ]; then
 (cd libusb-1.0.30
  if $TASK_WINDOWS; then
   CC="$TASK_CC" CXX="$TASK_CXX" CFLAGS=-O2 ./configure --host=x86_64-w64-mingw32 --disable-shared --enable-static
  else
   CC="$TASK_CC" CXX="$TASK_CXX" CFLAGS='-O2 -mmacosx-version-min=13.0' ./configure --disable-shared --enable-static
  fi
  make -j4)
fi
cp libusb-1.0.30/libusb/libusb.h include/libusb-1.0/
TASK_COMMON=(-O2 "${TASK_PLATFORM[@]}" -I"$TASK_DRIVER" -I"$TASK_DRIVER/linux-gpib" -Iinclude)
"$TASK_CC" "${TASK_COMMON[@]}" -std=gnu11 -c "$TASK_DRIVER/linux-gpib/ni_usb_gpib.c" -o obj/ni_usb_gpib.o
"$TASK_CC" "${TASK_COMMON[@]}" -std=gnu11 -c "$TASK_DRIVER/gpib_stubs.c" -o obj/gpib_stubs.o
"$TASK_CXX" "${TASK_COMMON[@]}" -std=gnu++17 -c "$TASK_DRIVER/NiGpibDriver.cpp" -o obj/NiGpibDriver.o
"$TASK_CXX" "${TASK_COMMON[@]}" -std=gnu++17 -c "$TASK_ROOT/native/agilent_82357b.cpp" -o obj/agilent_82357b.o
"$TASK_CXX" "${TASK_COMMON[@]}" -std=gnu++17 -c "$TASK_ROOT/native/kohdalab_gpib_helper.cpp" -o obj/main.o
TASK_OBJS=(obj/main.o obj/ni_usb_gpib.o obj/gpib_stubs.o obj/NiGpibDriver.o obj/agilent_82357b.o)
if $TASK_WINDOWS; then
 TASK_LIBS=(libusb-1.0.30/libusb/.libs/libusb-1.0.a -lsetupapi -lcfgmgr32 -ladvapi32 -lole32)
 "$TASK_CXX" -static -s "${TASK_OBJS[@]}" "${TASK_LIBS[@]}" -o kohdalab-gpib-helper.exe
 "$TASK_PYTHON" "$TASK_ROOT/scripts/windows_resource.py" --output obj/visa.rc --filename kohdalab-visa-helper.exe --description 'KohdaLab TRKR VISA Helper'
 "${KOHDALAB_WINDOWS_RC:-windres}" -O coff -i obj/visa.rc -o obj/visa.res.o
 "$TASK_CXX" -std=c++17 -O2 -Wall -Wextra -Werror -static -s "$TASK_ROOT/native/visa_helper.cpp" obj/visa.res.o -o kohdalab-visa-helper.exe
else
 "$TASK_CXX" "${TASK_PLATFORM[@]}" "${TASK_OBJS[@]}" libusb-1.0.30/libusb/.libs/libusb-1.0.a -framework IOKit -framework CoreFoundation -framework Security -lpthread -o kohdalab-gpib-helper
 codesign --force --sign - kohdalab-gpib-helper
fi
