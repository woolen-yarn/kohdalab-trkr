#!/usr/bin/env bash
# Build private review artifacts on Ubuntu x86_64.
set -euo pipefail
cd "$(dirname "$0")/.."
test "$(dpkg --print-architecture)" = amd64
cargo build --release --locked
bash scripts/build_native.sh

out="${1:-$PWD/dist/linux}"
mkdir -p "$out"
out=$(cd "$out" && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')
name="kohdalab-trkr-$version-linux-x86_64"
payload="$scratch/$name"
python3 - "$scratch/SOURCE.zip" <<'PY'
import sys
from pathlib import Path
sys.path.insert(0, 'scripts')
from package_rust_portable import application_source
application_source(Path(sys.argv[1]))
PY
python3 scripts/package_rust_portable.py \
  --binary target/release/kohdalab-trkr --demo-binary target/release/kohdalab-trkr-demo \
  --helper build/native/kohdalab-gpib-helper --source-zip "$scratch/SOURCE.zip" \
  --licenses native/licenses --target x86_64-unknown-linux-gnu --output "$payload"
tar -C "$scratch" -czf "$out/$name.tar.gz" "$name"

deb="$scratch/deb"
install -d "$deb/DEBIAN" "$deb/opt/kohdalab" "$deb/usr/bin" "$deb/usr/share/applications" \
  "$deb/usr/share/icons/hicolor/256x256/apps" "$deb/usr/share/doc/kohdalab-trkr"
cp -a "$payload" "$deb/opt/kohdalab/kohdalab-trkr"
for binary in kohdalab-trkr kohdalab-trkr-demo; do
  ln -s "/opt/kohdalab/kohdalab-trkr/$binary" "$deb/usr/bin/$binary"
done
install -m644 LICENSE "$deb/usr/share/doc/kohdalab-trkr/copyright"
install -m644 crates/trkr-desktop/assets/icon.png "$deb/usr/share/icons/hicolor/256x256/apps/kohdalab-trkr.png"
cat > "$deb/usr/share/applications/kohdalab-trkr.desktop" <<'DESKTOP'
[Desktop Entry]
Type=Application
Name=KohdaLab TRKR
Exec=kohdalab-trkr
Icon=kohdalab-trkr
Terminal=false
Categories=Science;
DESKTOP
cat > "$deb/DEBIAN/control" <<CONTROL
Package: kohdalab-trkr
Version: $version
Architecture: amd64
Section: science
Priority: optional
Maintainer: KohdaLab
Depends: libc6 (>= 2.39), libgcc-s1, libstdc++6, libgl1, libegl1, libx11-6, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libwayland-client0
Description: Lock-in signal acquisition and delay/spatial scan control
CONTROL
dpkg-deb --root-owner-group --build "$deb" "$out/kohdalab-trkr_${version}_amd64.deb"

mkdir "$scratch/extracted"
tar -C "$scratch/extracted" -xzf "$out/$name.tar.gz"
dpkg-deb -x "$out/kohdalab-trkr_${version}_amd64.deb" "$scratch/installed"
for root in "$scratch/extracted/$name" "$scratch/installed/opt/kohdalab/kohdalab-trkr"; do
  label=$(basename "$(dirname "$root")")
  cmp target/release/kohdalab-trkr "$root/kohdalab-trkr"
  python3 scripts/verify_packaged_measurements.py "$root/kohdalab-trkr" --output "$out/measurement-$label"
  # xvfb-run defaults to 640x480x8, which offers no GLX config for the glow
  # renderer; use a 24-bit screen large enough for the minimum window size.
  mkdir -p "$out/control-$label"
  xvfb-run -a -e "$out/control-$label/xvfb.log" -s "-screen 0 1920x1080x24 -nolisten tcp" \
    python3 scripts/verify_common_control.py "$root/kohdalab-trkr" --output "$out/control-$label"
done
(cd "$out" && sha256sum "$name.tar.gz" "kohdalab-trkr_${version}_amd64.deb" > SHA256SUMS-linux.txt)
