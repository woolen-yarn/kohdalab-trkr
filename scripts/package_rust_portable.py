"""Stage a GPIB-only Rust portable package with corresponding source and notices."""

from __future__ import annotations

import argparse
import csv
from datetime import datetime, timezone
import hashlib
import json
import math
import os
import shutil
import subprocess
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def collect_rust_notices(destination: Path, target: str | None) -> None:
    command = [
        os.environ.get("CARGO", "cargo"),
        "metadata",
        "--manifest-path",
        str(ROOT / "next/Cargo.toml"),
        "--format-version",
        "1",
        "--locked",
    ]
    if target:
        command.extend(["--filter-platform", target])
    metadata = json.loads(subprocess.check_output(command))  # noqa: S603 — explicit developer-selected Cargo executable
    destination.mkdir(parents=True)
    records = []
    for package in metadata["packages"]:
        records.append(
            {key: package.get(key) for key in ("name", "version", "license", "source")}
        )
        directory = Path(package["manifest_path"]).parent
        for item in directory.rglob("*"):
            if item.is_file() and any(
                item.name.lower().startswith(prefix)
                for prefix in ("license", "copying", "copyright", "notice", "ofl")
            ):
                # Workspace build/output trees are never distribution sources.
                relative = item.relative_to(directory)
                if any(part in {"target", ".git", "dist"} for part in relative.parts):
                    continue
                output = (
                    destination / f"{package['name']}-{package['version']}" / relative
                )
                output.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(item, output)
    (destination / "DEPENDENCIES.json").write_text(
        json.dumps(records, indent=2) + "\n", encoding="utf-8"
    )


def zip_file(archive: zipfile.ZipFile, path: Path, name: str | Path) -> None:
    """Keep source/portable archive headers independent of checkout times."""
    epoch=max(315532800, int(os.environ.get("SOURCE_DATE_EPOCH", "315532800")))
    info=zipfile.ZipInfo(str(name).replace(os.sep, "/"), datetime.fromtimestamp(epoch,timezone.utc).timetuple()[:6])
    info.create_system=3
    info.external_attr=(0o100755 if path.stat().st_mode & 0o111 else 0o100644)<<16
    info.compress_type=zipfile.ZIP_DEFLATED
    archive.writestr(info,path.read_bytes())


def application_source(destination: Path) -> None:
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
        for name in (
            "next", "native", "scripts", "bindings", "notebook", "tests", ".github", ".signpath",
            "rust-toolchain.toml", "pyproject.toml", "README.md", "LICENSE",
            "REDISTRIBUTION.md", "CODE_SIGNING_POLICY.md", "PRIVACY.md", "CHANGELOG.md", "docs",
        ):
            base = ROOT / name
            for item in [base] if base.is_file() else sorted(base.rglob("*")):
                relative = item.relative_to(ROOT)
                if item.is_file() and not any(
                    part in {"target", "dist", "runs", "__pycache__"}
                    for part in relative.parts
                ):
                    zip_file(archive,item,relative)


def required(path: Path, label: str) -> Path:
    if not path.exists():
        raise SystemExit(f"missing {label}: {path}")
    return path


def validate_source(path: Path) -> None:
    if path.name != "SOURCE.zip" or not path.is_file():
        raise SystemExit("--source-zip must be an existing file named SOURCE.zip")
    try:
        with zipfile.ZipFile(path) as archive:
            names = {name.rstrip("/") for name in archive.namelist()}
            required = {
                "native/kohdalab_gpib_helper.cpp",
                "native/agilent_82357b.cpp",
                "native/vendor/README.md",
                "scripts/build_native_gpib.sh",
            }
            if archive.testzip() is not None or not required.issubset(names):
                raise SystemExit(
                    "--source-zip is missing required IV GPIB source files"
                )
    except zipfile.BadZipFile as error:
        raise SystemExit(f"--source-zip is not a valid archive: {error}") from error


def validate_license_tree(path: Path) -> None:
    if not path.is_dir() or not any(path.rglob("*")):
        raise SystemExit("--licenses must be a non-empty directory")
    names = {item.name.lower() for item in path.rglob("*") if item.is_file()}
    if not any(
        name
        in {
            "license",
            "license.txt",
            "license.md",
            "mit-license.txt",
            "kohdalab-iv-mit.txt",
        }
        for name in names
    ):
        raise SystemExit("--licenses must contain the root MIT license")
    if not any("gpl" in name and "lgpl" not in name for name in names):
        raise SystemExit("--licenses must include GPL terms for the GPIB helper")
    if not any("lgpl" in name for name in names):
        raise SystemExit("--licenses must include LGPL terms for libusb")


DEMO_FIELDS = (
    "timestamp", "measurement", "fast_axis", "slow_axis", "target_elapsed_s",
    "target_t_cor_ps", "target_x_cor_um", "target_y_cor_um", "target_u_cor_um",
    "target_v_cor_um", "elapsed_s", "t_cor_ps", "t_ps", "x_cor_um", "x_um",
    "y_cor_um", "y_um", "u_cor_um", "v_cor_um", "X_V", "Y_V", "R_V",
    "Theta_deg", "spatial_theta_deg", "coordinate", "delay_stage_mm",
    "delay_stage_pulse", "x_scanner_mm", "x_scanner_deg", "y_scanner_mm",
    "y_scanner_deg",
)


def write_demo_csvs(destination: Path) -> None:
    """Write small synthetic preview data; never package a measurement record."""
    destination.mkdir()
    workflows = {
        "signal_monitor": {"target_elapsed_s": "0"},
        "trkr": {"fast_axis": "t", "target_t_cor_ps": "0"},
        "srkr": {"fast_axis": "x", "target_x_cor_um": "0"},
        "strkr": {
            "fast_axis": "t", "slow_axis": "x", "target_t_cor_ps": "0",
            "target_x_cor_um": "0",
        },
        "srkr_2d": {
            "fast_axis": "x", "slow_axis": "y", "target_x_cor_um": "0",
            "target_y_cor_um": "0",
        },
    }
    for workflow, axes in workflows.items():
        samples = (
            [(-2.0 + point / 30.0, None) for point in range(121)]
            if workflow in {"signal_monitor", "trkr", "srkr"}
            else [
                (-2.0 + fast / 10.0, -2.0 + slow / (5.0 if workflow == "strkr" else 10.0))
                for slow in range(21 if workflow == "strkr" else 41)
                for fast in range(41)
            ]
        )
        with (destination / f"{workflow}.csv").open(
            "w", newline="", encoding="utf-8"
        ) as stream:
            writer = csv.DictWriter(stream, fieldnames=DEMO_FIELDS)
            writer.writeheader()
            for point, (fast, slow) in enumerate(samples):
                coordinate = fast
                secondary = slow or 0.0
                envelope = math.exp(-coordinate * coordinate - secondary * secondary)
                phase = coordinate * (5.0 if workflow in {"trkr", "strkr"} else 1.5)
                x_signal = 8e-6 * envelope * math.cos(phase)
                y_signal = 5e-6 * envelope * math.sin(phase)
                delay = coordinate * 75.0 + 100.0
                spatial_x = (secondary if workflow == "strkr" else coordinate) * 15.0
                spatial_y = secondary * 15.0
                row = {
                    "timestamp": f"2026-01-01T00:{point // 60:02d}:{point % 60:02d}Z",
                    "measurement": workflow,
                    "elapsed_s": str(point),
                    "t_cor_ps": str(delay),
                    "t_ps": str(delay),
                    "x_cor_um": str(spatial_x),
                    "x_um": str(spatial_x),
                    "y_cor_um": str(spatial_y),
                    "y_um": str(spatial_y),
                    "u_cor_um": str(spatial_x),
                    "v_cor_um": str(spatial_y),
                    "X_V": f"{x_signal:.6e}",
                    "Y_V": f"{y_signal:.6e}",
                    "R_V": f"{math.hypot(x_signal, y_signal):.6e}",
                    "Theta_deg": f"{math.degrees(math.atan2(y_signal, x_signal)):.6f}",
                    "spatial_theta_deg": "0",
                    "coordinate": "measurement",
                    **axes,
                }
                if workflow == "signal_monitor":
                    row["target_elapsed_s"] = str(point)
                elif workflow == "trkr":
                    row["target_t_cor_ps"] = str(delay)
                elif workflow == "srkr":
                    row["target_x_cor_um"] = str(spatial_x)
                elif workflow == "strkr":
                    row["target_t_cor_ps"] = str(delay)
                    row["target_x_cor_um"] = str(spatial_x)
                else:
                    row["target_x_cor_um"] = str(spatial_x)
                    row["target_y_cor_um"] = str(spatial_y)
                writer.writerow(row)
    (destination / "README.txt").write_text(
        "Synthetic demo CSV files generated for the KohdaLab TRKR Demo launcher.\n"
        "They are illustrative only and are not measurement data.\n",
        encoding="utf-8",
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument(
        "--demo-binary", type=Path, help="Optional kohdalab-trkr-demo launcher"
    )
    parser.add_argument("--helper", type=Path, required=True)
    parser.add_argument(
        "--visa-helper",
        type=Path,
        help="Optional official VISA helper; uses an existing system runtime",
    )
    parser.add_argument(
        "--source-zip", type=Path, required=True, help="IV support/SOURCE.zip"
    )
    parser.add_argument("--licenses", type=Path, required=True, help="IV license tree")
    parser.add_argument("--target", help="Cargo target triple for dependency notices")
    parser.add_argument(
        "--rust-licenses",
        type=Path,
        help="Pre-collected Cargo dependency notices (otherwise collect with Cargo)",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    binary = required(args.binary, "Rust binary")
    demo_binary = required(args.demo_binary, "TRKR demo launcher") if args.demo_binary else None
    helper = required(args.helper, "GPIB helper")
    if not binary.is_file() or binary.name not in {
        "kohdalab-trkr",
        "kohdalab-trkr.exe",
    }:
        raise SystemExit("--binary must be the canonical kohdalab-trkr executable")
    if demo_binary and (
        not demo_binary.is_file()
        or demo_binary.name not in {"kohdalab-trkr-demo", "kohdalab-trkr-demo.exe"}
    ):
        raise SystemExit("--demo-binary must be the canonical kohdalab-trkr-demo executable")
    if not helper.is_file() or helper.name not in {
        "kohdalab-gpib-helper",
        "kohdalab-gpib-helper.exe",
    }:
        raise SystemExit(
            "--helper must be the canonical kohdalab-gpib-helper executable"
        )
    if args.visa_helper and (
        not args.visa_helper.is_file()
        or args.visa_helper.name != "kohdalab-visa-helper.exe"
    ):
        raise SystemExit("--visa-helper must be the canonical kohdalab-visa-helper.exe")
    source_zip = required(args.source_zip, "IV SOURCE.zip")
    licenses = required(args.licenses, "IV license tree")
    validate_source(source_zip)
    validate_license_tree(licenses)
    if args.rust_licenses and not (args.rust_licenses / "DEPENDENCIES.json").is_file():
        raise SystemExit(
            "--rust-licenses must include DEPENDENCIES.json and dependency license texts"
        )
    if args.output.exists():
        raise SystemExit(f"refusing to overwrite existing output: {args.output}")
    stage = args.output
    (stage / "support" / "licenses").mkdir(parents=True)
    shutil.copy2(binary, stage / binary.name)
    shutil.copy2(ROOT / "next/crates/trkr-desktop/assets/icon.ico", stage / "kohdalab-trkr.ico")
    if demo_binary:
        shutil.copy2(demo_binary, stage / demo_binary.name)
    shutil.copy2(helper, stage / "support" / helper.name)
    if args.visa_helper:
        shutil.copy2(args.visa_helper, stage / "support" / args.visa_helper.name)
    shutil.copy2(source_zip, stage / "support" / "SOURCE.zip")
    shutil.copytree(licenses, stage / "support" / "licenses" / "native")
    rust_notices = stage / "support" / "licenses" / "trkr-rust"
    if args.rust_licenses:
        shutil.copytree(args.rust_licenses, rust_notices)
    else:
        collect_rust_notices(rust_notices, args.target)
    application_source(stage / "support" / "TRKR-SOURCE.zip")
    shutil.copy2(
        Path(__file__).parents[1] / "LICENSE",
        stage / "support" / "licenses" / "KohdaLab-TRKR-MIT.txt",
    )
    shutil.copy2(
        Path(__file__).parents[1] / "docs" / "trkr_portable_rust.md",
        stage / "support" / "licenses" / "trkr_portable_rust.md",
    )
    shutil.copytree(
        ROOT / "bindings" / "python",
        stage / "notebook",
        ignore=shutil.ignore_patterns("__pycache__"),
    )
    for notebook in (ROOT / "notebook").glob("*.ipynb"):
        shutil.copy2(notebook, stage / "notebook" / notebook.name)
    (stage / "config").mkdir()
    write_demo_csvs(stage / "demo_csv")
    shutil.copy2(
        ROOT / "next" / "crates" / "trkr-desktop" / "assets" / "default.json",
        stage / "config" / "default.json",
    )
    shutil.copy2(ROOT / "next/crates/trkr-desktop/assets/default_demo.json", stage / "config/default_demo.json")
    (stage / "README.txt").write_text(
        "KohdaLab TRKR Rust portable\nUnzip the entire folder and run kohdalab-trkr. No Python installation is needed.\nStarts disconnected. Native hardware workflows: Signal Monitor, TRKR, SRKR, STRKR, SRKR 2D.\nIf included, run kohdalab-trkr-demo.exe to preview the synthetic demo_csv files without opening hardware. Drag one existing CSV onto it to load that CSV. Select a tab, Browse/Load the CSV, Connect All (virtual), then Start/Stop. Wait (s) controls timing; zero gives fast playback. Playback loops without recording or modifying source files.\nSerial motion: SHOT302GS/GSC01/GSC01A and CONEXCC/CONEXAGAP. Review your ports, calibration, origins and limits before hardware use.\nPython notebook access is in notebook/. Settings can import existing Python JSON profiles. Physical scan validation remains required.\nIf system VISA enumerates the GPIB resource, official VISA is selected. Otherwise the adapter must have IV native WinUSB setup. Failed VISA I/O is never replayed on native GPIB.\nSee support/licenses/trkr_portable_rust.md for setup and limitations.\nGPIB helper source: support/SOURCE.zip. Application source: support/TRKR-SOURCE.zip.\n",
        encoding="utf-8",
    )
    hashes = {
        str(item.relative_to(stage)).replace(os.sep, "/"): hashlib.sha256(
            item.read_bytes()
        ).hexdigest()
        for item in sorted(stage.rglob("*"))
        if item.is_file()
    }
    (stage / "SHA256.json").write_text(
        json.dumps(hashes, indent=2) + "\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
