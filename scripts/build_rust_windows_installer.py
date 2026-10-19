#!/usr/bin/env python3
"""Create a per-user NSIS installer from the Rust portable TRKR payload.

The installer deliberately uses ``$LOCALAPPDATA\\KohdaLab\\TRKR`` and
its own registry key.  It therefore cannot replace the legacy Python TRKR
installation or its configuration.  Pass ``--makensis`` (or have makensis on
PATH) to build Setup.exe; otherwise this writes inspectable NSIS source.
"""

from __future__ import annotations

import argparse
import hashlib
import shutil
import re
import stat
import subprocess
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VERSION = "0.3.0"
APP_EXE = "kohdalab-trkr.exe"
DEMO_EXE = "kohdalab-trkr-demo.exe"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def portable_directory(path: Path) -> Path:
    if not path.is_dir():
        raise ValueError(f"portable payload does not exist: {path}")
    executable = path / APP_EXE
    if not executable.is_file():
        raise ValueError(f"portable payload lacks {APP_EXE}")
    if not (path / "support" / "kohdalab-gpib-helper.exe").is_file():
        raise ValueError("portable payload lacks support/kohdalab-gpib-helper.exe")
    if not (path / "README.txt").is_file():
        raise ValueError("portable payload lacks README.txt")
    payload_files(path)
    return path.resolve()


def payload_files(payload: Path) -> list[Path]:
    """Return the exact payload files; payload symlinks are never installed."""
    files: list[Path] = []
    for item in sorted(payload.rglob("*")):
        if item.is_symlink():
            raise ValueError(f"portable payload contains a symlink: {item}")
        if item.is_file():
            files.append(item.relative_to(payload))
    return files


def nsis_path(path: Path) -> str:
    """Quote a source path for NSIS without allowing a compiler directive."""
    value = str(path.resolve())
    if any(char in value for char in ('"', "\r", "\n")):
        raise ValueError("NSIS paths cannot contain quotes or line breaks")
    return value.replace("$", "$$").replace("/", "\\")


def nsis_relative(path: Path) -> str:
    value = str(path).replace("/", "\\")
    if any(char in value for char in ('"', "\r", "\n")) or value.startswith("\\"):
        raise ValueError("unsafe relative payload path")
    return value.replace("$", "$$")


def validate_version(version: str) -> str:
    if not re.fullmatch(r"[0-9][0-9A-Za-z.-]{0,63}", version):
        raise ValueError("version must be 1-64 ASCII letters, digits, dots, or hyphens")
    return version


def safe_extract(archive: zipfile.ZipFile, destination: Path) -> None:
    for member in archive.infolist():
        name = member.filename
        parts = Path(name).parts
        mode = member.external_attr >> 16
        if (
            not name
            or "\\" in name
            or Path(name).is_absolute()
            or ".." in parts
            or stat.S_IFMT(mode) == stat.S_IFLNK
        ):
            raise ValueError(f"unsafe portable ZIP member: {name!r}")
        target = destination.joinpath(*parts)
        if not target.resolve().is_relative_to(destination.resolve()):
            raise ValueError(f"portable ZIP escapes destination: {name!r}")
        if member.is_dir():
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(member) as source, target.open("wb") as output:
                shutil.copyfileobj(source, output)


def nsis_source(payload: Path, output: Path, version: str) -> str:
    version = validate_version(version)
    payload_text = nsis_path(payload)
    output_text = nsis_path(output)
    files = payload_files(payload)
    deletes = "\n".join(f'  Delete "$INSTDIR\\{nsis_relative(path)}"' for path in files)
    directories = {path.parent for path in files if path.parent != Path(".")}
    remove_empty = "\n".join(
        f'  RMDir "$INSTDIR\\{nsis_relative(path)}"'
        for path in sorted(
            directories, key=lambda path: (len(path.parts), str(path)), reverse=True
        )
    )
    custom_icon = payload / "kohdalab-trkr.ico"
    icon_defines = (f'!define MUI_ICON "{nsis_path(custom_icon)}"\n!define MUI_UNICON "{nsis_path(custom_icon)}"\n' if custom_icon.is_file() else "")
    shortcut_icon = ' "" "$INSTDIR\\kohdalab-trkr.ico" 0' if custom_icon.is_file() else ""
    demo_shortcuts = ""
    demo_uninstall = ""
    if (payload / DEMO_EXE).is_file():
        demo_shortcuts = (
            f'  CreateShortcut "$SMPROGRAMS\\KohdaLab\\TRKR Demo.lnk" "$INSTDIR\\{DEMO_EXE}"{shortcut_icon}\n'
        )
        demo_uninstall = '  Delete "$SMPROGRAMS\\KohdaLab\\TRKR Demo.lnk"\n'
    return f'''Unicode true
SetCompressor /SOLID zlib
SetDatablockOptimize on
!include "MUI2.nsh"

Name "KohdaLab TRKR {version}"
OutFile "{output_text}"
InstallDir "$LOCALAPPDATA\\KohdaLab\\TRKR"
InstallDirRegKey HKCU "Software\\KohdaLab\\TRKR" "InstallLocation"
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show
BrandingText "KohdaLab TRKR"
VIProductVersion "{version.split('-')[0]}.0"
VIAddVersionKey "ProductName" "KohdaLab TRKR"
VIAddVersionKey "ProductVersion" "{version}"
VIAddVersionKey "CompanyName" "Kohdalab"
VIAddVersionKey "FileDescription" "KohdaLab TRKR Setup"
VIAddVersionKey "FileVersion" "{version}"
VIAddVersionKey "LegalCopyright" "Copyright KohdaLab contributors"

{icon_defines}!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "KohdaLab TRKR" SecMain
  SetOutPath "$INSTDIR"
  File /r "{payload_text}\\*.*"
  WriteUninstaller "$INSTDIR\\Uninstall.exe"
  WriteRegStr HKCU "Software\\KohdaLab\\TRKR" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KohdaLab TRKR" "DisplayName" "KohdaLab TRKR"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KohdaLab TRKR" "DisplayVersion" "{version}"
  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KohdaLab TRKR" "UninstallString" '"$INSTDIR\\Uninstall.exe"'
  CreateDirectory "$SMPROGRAMS\\KohdaLab"
  CreateShortcut "$SMPROGRAMS\\KohdaLab\\TRKR.lnk" "$INSTDIR\\{APP_EXE}"{shortcut_icon}
{demo_shortcuts}
  CreateShortcut "$DESKTOP\\KohdaLab TRKR.lnk" "$INSTDIR\\{APP_EXE}"{shortcut_icon}
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\\KohdaLab\\TRKR.lnk"
{demo_uninstall}
  Delete "$DESKTOP\\KohdaLab TRKR.lnk"
  DeleteRegKey HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\KohdaLab TRKR"
  DeleteRegKey HKCU "Software\\KohdaLab\\TRKR"
{deletes}
  Delete /REBOOTOK "$INSTDIR\\Uninstall.exe"
{remove_empty}
  RMDir "$INSTDIR"
SectionEnd
'''


def compiler(explicit: Path | None) -> Path | None:
    if explicit is not None:
        if not explicit.is_file():
            raise ValueError(f"makensis does not exist: {explicit}")
        return explicit.resolve()
    found = shutil.which("makensis") or shutil.which("makensis.exe")
    return Path(found).resolve() if found else None


def compile_installer(source: Path, output: Path, makensis: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(makensis), "/V3", str(source)], check=True)  # noqa: S603
    if not output.is_file() or not output.stat().st_size:
        raise RuntimeError(f"NSIS did not create {output}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    input_group = parser.add_mutually_exclusive_group(required=True)
    input_group.add_argument("--portable-dir", type=Path)
    input_group.add_argument("--portable-zip", type=Path)
    parser.add_argument(
        "--output", type=Path, required=True, help="Setup.exe destination"
    )
    parser.add_argument("--version", default=VERSION)
    parser.add_argument("--makensis", type=Path, help="existing NSIS compiler")
    parser.add_argument(
        "--nsi-output", type=Path, help="keep generated NSIS source here"
    )
    args = parser.parse_args()
    output = args.output.resolve()
    if output.exists():
        raise SystemExit(f"refusing to overwrite output: {output}")
    nsis_output = (args.nsi_output or output.with_suffix(".nsi")).resolve()
    if nsis_output.exists():
        raise SystemExit(f"refusing to overwrite NSIS source: {nsis_output}")
    tool = compiler(args.makensis)
    if args.portable_zip and tool is None:
        raise SystemExit(
            "ZIP input requires --makensis; extract it and use --portable-dir to retain inspectable source paths"
        )
    with tempfile.TemporaryDirectory(prefix="kohdalab-trkr-installer-") as temporary:
        if args.portable_zip:
            if not args.portable_zip.is_file():
                raise SystemExit(f"portable ZIP does not exist: {args.portable_zip}")
            with zipfile.ZipFile(args.portable_zip) as archive:
                safe_extract(archive, Path(temporary))
            roots = [p for p in Path(temporary).iterdir() if p.is_dir()]
            payload = portable_directory(
                roots[0] if len(roots) == 1 else Path(temporary)
            )
        else:
            payload = portable_directory(args.portable_dir)
        nsis_output.parent.mkdir(parents=True, exist_ok=True)
        nsis_output.write_text(
            nsis_source(payload, output, args.version), encoding="utf-8"
        )
        if tool is None:
            print(f"Wrote NSIS source: {nsis_output}")
            return
        compile_installer(nsis_output, output, tool)
        print(f"{output}  {sha256(output)}")


if __name__ == "__main__":
    main()
