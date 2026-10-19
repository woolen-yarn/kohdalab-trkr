from __future__ import annotations

import csv
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).parents[1]
SCRIPT = ROOT / "scripts" / "package_rust_portable.py"


def fixture_inputs(tmp_path: Path) -> dict[str, Path]:
    binary = tmp_path / "kohdalab-trkr"
    helper = tmp_path / "kohdalab-gpib-helper"
    binary.write_bytes(b"rust")
    helper.write_bytes(b"helper")
    source = tmp_path / "SOURCE.zip"
    with zipfile.ZipFile(source, "w") as archive:
        for name in (
            "native/kohdalab_gpib_helper.cpp",
            "native/agilent_82357b.cpp",
            "native/vendor/README.md",
            "scripts/build_native_gpib.sh",
        ):
            archive.writestr(name, "source")
    licenses = tmp_path / "licenses"
    licenses.mkdir()
    (licenses / "LICENSE").write_text("MIT", encoding="utf-8")
    (licenses / "GPL-2.0.txt").write_text("GPL", encoding="utf-8")
    (licenses / "LGPL-2.1.txt").write_text("LGPL", encoding="utf-8")
    (licenses / "DEPENDENCIES.json").write_text("[]", encoding="utf-8")
    return {"binary": binary, "helper": helper, "source": source, "licenses": licenses}


def run_package(
    inputs: dict[str, Path], output: Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603 — fixed interpreter and repository script
        [
            sys.executable,
            str(SCRIPT),
            "--binary",
            str(inputs["binary"]),
            "--helper",
            str(inputs["helper"]),
            "--source-zip",
            str(inputs["source"]),
            "--licenses",
            str(inputs["licenses"]),
            "--rust-licenses",
            str(inputs["licenses"]),
            *(["--visa-helper", str(inputs["visa"])] if "visa" in inputs else []),
            "--output",
            str(output),
        ],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )


def test_packager_refuses_to_clobber(tmp_path: Path) -> None:
    inputs = fixture_inputs(tmp_path)
    output = tmp_path / "out"
    output.mkdir()
    marker = output / "marker"
    marker.write_text("keep", encoding="utf-8")
    result = run_package(inputs, output)
    assert result.returncode
    assert marker.read_text(encoding="utf-8") == "keep"


def test_packager_rejects_missing_source(tmp_path: Path) -> None:
    inputs = fixture_inputs(tmp_path)
    inputs["source"].unlink()
    result = run_package(inputs, tmp_path / "out")
    assert result.returncode
    assert "SOURCE.zip" in result.stderr


def test_packager_contents_are_gpib_only(tmp_path: Path) -> None:
    inputs = fixture_inputs(tmp_path)
    result = run_package(inputs, tmp_path / "out")
    assert result.returncode == 0, result.stderr
    output = tmp_path / "out"
    assert (output / "kohdalab-trkr").is_file()
    assert (output / "support" / "kohdalab-gpib-helper").is_file()
    assert (output / "support" / "SOURCE.zip").is_file()
    assert not (output / "support" / "iv-support").exists()
    assert (output / "support" / "licenses" / "KohdaLab-TRKR-MIT.txt").is_file()
    assert (output / "support" / "licenses" / "trkr_portable_rust.md").is_file()
    assert "\\n" not in (output / "README.txt").read_text(encoding="utf-8")


def test_packager_includes_optional_visa_helper_and_source(tmp_path: Path) -> None:
    import hashlib
    import json

    inputs = fixture_inputs(tmp_path)
    visa = tmp_path / "kohdalab-visa-helper.exe"
    visa.write_bytes(b"visa-test")
    inputs["visa"] = visa
    output = tmp_path / "out"
    result = run_package(inputs, output)
    assert result.returncode == 0, result.stderr
    copied = output / "support/kohdalab-visa-helper.exe"
    assert copied.read_bytes() == b"visa-test"
    hashes = json.loads((output / "SHA256.json").read_text())
    assert (
        hashes["support/kohdalab-visa-helper.exe"]
        == hashlib.sha256(b"visa-test").hexdigest()
    )
    with zipfile.ZipFile(output / "support/TRKR-SOURCE.zip") as archive:
        assert "native/visa_helper.cpp" in archive.namelist()
        assert "native/stdio_mode.hpp" in archive.namelist()


def test_packager_includes_demo_launcher_and_synthetic_csvs(tmp_path: Path) -> None:
    inputs = fixture_inputs(tmp_path)
    demo = tmp_path / "kohdalab-trkr-demo.exe"
    demo.write_bytes(b"demo")
    inputs["demo"] = demo
    output = tmp_path / "out"
    result = subprocess.run(
        [
            sys.executable, str(SCRIPT), "--binary", str(inputs["binary"]),
            "--demo-binary", str(demo), "--helper", str(inputs["helper"]),
            "--source-zip", str(inputs["source"]), "--licenses", str(inputs["licenses"]),
            "--rust-licenses", str(inputs["licenses"]), "--output", str(output),
        ],
        cwd=ROOT, text=True, capture_output=True,
    )
    assert result.returncode == 0, result.stderr
    assert (output / "kohdalab-trkr-demo.exe").read_bytes() == b"demo"
    assert {path.name for path in (output / "demo_csv").glob("*.csv")} == {
        "signal_monitor.csv", "trkr.csv", "srkr.csv", "strkr.csv", "srkr_2d.csv"
    }
    assert "synthetic demo_csv" in (output / "README.txt").read_text(encoding="utf-8").lower()
    with (output / "demo_csv/srkr_2d.csv").open(encoding="utf-8", newline="") as stream:
        assert sum(1 for _ in csv.DictReader(stream)) == 1681


def test_zip_headers_ignore_checkout_file_timestamps(tmp_path: Path) -> None:
    import importlib.util
    import os
    spec=importlib.util.spec_from_file_location('portable_package',SCRIPT)
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    source=tmp_path/'source.rs';source.write_text('fn main() {}')
    archives=[]
    for i in range(2):
        os.utime(source,(1700000000+i*3600,1700000000+i*3600))
        path=tmp_path/f'{i}.zip'
        with zipfile.ZipFile(path,'w') as archive:module.zip_file(archive,source,'src/main.rs')
        archives.append(path.read_bytes())
    assert archives[0]==archives[1]


def test_packaged_notebooks_prefer_the_bundled_client_over_an_old_import(tmp_path: Path) -> None:
    """Exercise the actual installer/portable layout, including a warm kernel."""
    import json

    inputs = fixture_inputs(tmp_path)
    output = tmp_path / "out"
    result = run_package(inputs, output)
    assert result.returncode == 0, result.stderr
    assert (output / "notebook/README.md").is_file()
    code = r'''
import json, sys, types
from pathlib import Path
sys.modules["kohdalab_trkr"] = types.ModuleType("kohdalab_trkr")
for path in Path.cwd().glob("*.ipynb"):
    setup = next(c for c in json.loads(path.read_text(encoding="utf-8"))["cells"] if c.get("id") == "setup")
    bootstrap = "".join(setup["source"]).split("binary = find_binary()", 1)[0]
    namespace = {}
    exec(compile(bootstrap, str(path), "exec"), namespace)
    assert callable(namespace["prepare_profile"])
    assert Path(namespace["Client"].__init__.__globals__["__file__"]).resolve() == Path.cwd() / "kohdalab_trkr.py"
'''
    result = subprocess.run([sys.executable, "-c", code], cwd=output / "notebook", capture_output=True, text=True)
    assert result.returncode == 0, result.stdout + result.stderr
