import importlib.util
from pathlib import Path

import pytest


def module():
    path = Path(__file__).parents[1] / "scripts/prepare_release_assets.py"
    spec = importlib.util.spec_from_file_location("release_assets", path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def artifacts(root):
    for platform, kind in (("windows", "installed"), ("macos", "dmg")):
        dist = root / platform / "dist"
        dist.mkdir(parents=True)
        (dist / f"{platform}.zip").write_bytes(platform.encode())
        for folder, name in (("measurement-validation", "validation.json"),
                             ("common-control-validation", "evidence.json"),
                             (f"{kind}-validation", "validation.json")):
            report = root / platform / "build" / folder / name
            report.parent.mkdir(parents=True)
            report.write_text(platform + folder)


def test_same_named_reports_are_distinct_and_packages_unchanged(tmp_path):
    artifacts(tmp_path / "input")
    files = module().prepare(tmp_path / "input", tmp_path / "out")
    assert len(files) == 8
    assert len({p.name for p in files}) == 8
    assert (tmp_path / "out/windows.zip").read_bytes() == b"windows"
    assert (tmp_path / "out/validation-windows-gui-api.json").read_text() == "windowscommon-control-validation"
    assert (tmp_path / "out/validation-macos-gui-api.json").read_text() == "macoscommon-control-validation"


def test_missing_evidence_does_not_create_partial_upload(tmp_path):
    artifacts(tmp_path / "input")
    (tmp_path / "input/windows/build/common-control-validation/evidence.json").unlink()
    with pytest.raises(ValueError, match="Expected one"):
        module().prepare(tmp_path / "input", tmp_path / "out")
    assert not (tmp_path / "out").exists()


def test_package_name_collision_is_rejected(tmp_path):
    artifacts(tmp_path / "input")
    (tmp_path / "input/macos/dist/windows.zip").write_bytes(b"different")
    with pytest.raises(ValueError, match="Duplicate release asset"):
        module().prepare(tmp_path / "input", tmp_path / "out")
