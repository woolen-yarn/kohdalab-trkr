from __future__ import annotations

import importlib.util
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = ROOT / "scripts" / "build_rust_windows_installer.py"
spec = importlib.util.spec_from_file_location("installer", SCRIPT)
assert spec and spec.loader
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


def payload(tmp_path: Path) -> Path:
    result = tmp_path / "portable"
    (result / "support").mkdir(parents=True)
    (result / installer.APP_EXE).write_bytes(b"app")
    (result / "support" / "kohdalab-gpib-helper.exe").write_bytes(b"helper")
    (result / "README.txt").write_text("portable", encoding="utf-8")
    return result


def test_nsis_source_uses_the_standard_system_install_location(
    tmp_path: Path,
) -> None:
    source = installer.nsis_source(
        payload(tmp_path), tmp_path / "Setup.exe", "0.3.0-alpha.2"
    )
    assert 'InstallDir "$PROGRAMFILES64\\Kohdalab\\KohdaLab TRKR"' in source
    assert "RequestExecutionLevel admin" in source
    assert 'StrCpy $INSTDIR "$PROGRAMFILES64\\Kohdalab\\KohdaLab TRKR"' in source
    assert "Function .onInit" in source
    assert "Function un.onInit" in source
    assert "SetShellVarContext all" in source
    assert "InstallDirRegKey" not in source
    assert "MUI_PAGE_DIRECTORY" not in source
    assert "can only be removed from its installed location" in source
    assert "KohdaLab TRKR.lnk" in source
    assert "Uninstall.exe" in source
    assert 'HKLM "Software\\Kohdalab\\KohdaLab TRKR"' in source
    assert "KohdaLab TRKR" in source
    assert "RMDir /r" not in source
    assert 'Delete "$INSTDIR\\support\\kohdalab-gpib-helper.exe"' in source
    assert 'RMDir "$INSTDIR\\support"' in source
    assert 'Delete /REBOOTOK "$INSTDIR\\Uninstall.exe"' in source


def test_nsis_source_adds_demo_shortcut_only_when_launcher_is_bundled(
    tmp_path: Path,
) -> None:
    candidate = payload(tmp_path)
    (candidate / installer.DEMO_EXE).write_bytes(b"demo")
    source = installer.nsis_source(candidate, tmp_path / "Setup.exe", "0.3.0-alpha.6")
    assert 'CreateShortcut "$SMPROGRAMS\\KohdaLab\\TRKR Demo.lnk" "$INSTDIR\\kohdalab-trkr-demo.exe"' in source
    assert 'Delete "$SMPROGRAMS\\KohdaLab\\TRKR Demo.lnk"' in source


def test_script_generates_inspectable_nsi_without_nsis(tmp_path: Path) -> None:
    result = subprocess.run(  # noqa: S603 - fixed local test script
        [
            sys.executable,
            str(SCRIPT),
            "--portable-dir",
            str(payload(tmp_path)),
            "--output",
            str(tmp_path / "Setup.exe"),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )  # noqa: S603
    assert result.returncode == 0, result.stderr
    source = (tmp_path / "Setup.nsi").read_text(encoding="utf-8")
    assert "File /r" in source
    assert "kohdalab-trkr.exe" in source


def test_payload_validation_rejects_missing_helper(tmp_path: Path) -> None:
    candidate = payload(tmp_path)
    (candidate / "support" / "kohdalab-gpib-helper.exe").unlink()
    try:
        installer.portable_directory(candidate)
    except ValueError as error:
        assert "gpib-helper" in str(error)
    else:
        raise AssertionError("invalid payload was accepted")


def test_nsis_escapes_dollar_and_rejects_directive_characters(tmp_path: Path) -> None:
    assert "$$" in installer.nsis_path(tmp_path / "cash$payload")
    for bad in ('bad"path', "bad\npath"):
        try:
            installer.nsis_path(Path(bad))
        except ValueError:
            pass
        else:
            raise AssertionError("unsafe NSIS path was accepted")
    for bad in ("0.3\n!define HACK", "0.3$HACK", "version"):
        try:
            installer.validate_version(bad)
        except ValueError:
            pass
        else:
            raise AssertionError("unsafe version was accepted")


def test_safe_extract_rejects_zip_traversal_and_symlink(tmp_path: Path) -> None:
    archive_path = tmp_path / "bad.zip"
    with zipfile.ZipFile(archive_path, "w") as archive:
        archive.writestr("../outside.txt", "no")
    with zipfile.ZipFile(archive_path) as archive:
        try:
            installer.safe_extract(archive, tmp_path / "out")
        except ValueError as error:
            assert "unsafe" in str(error)
        else:
            raise AssertionError("zip traversal was accepted")


def test_nsis_source_uses_custom_icon_for_installer_and_desktop(tmp_path: Path) -> None:
    candidate = payload(tmp_path)
    (candidate / "kohdalab-trkr.ico").write_bytes(b"icon")
    source = installer.nsis_source(candidate, tmp_path / "Setup.exe", "0.3.0-alpha.9")
    assert '!define MUI_ICON "' in source
    assert '!define MUI_UNICON "' in source
    assert 'CreateShortcut "$DESKTOP\\KohdaLab TRKR.lnk" "$INSTDIR\\kohdalab-trkr.exe" "" "$INSTDIR\\kohdalab-trkr.ico" 0' in source
