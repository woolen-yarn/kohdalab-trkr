from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import io
import tarfile
import tomllib
import zipfile
from email.parser import BytesParser
from pathlib import Path, PurePosixPath


ALLOWED_PACKAGE_SUFFIXES = {".py", ".json", ".toml"}
FORBIDDEN_PARTS = {".DS_Store", "__pycache__", ".git", ".pytest_cache"}
REQUIRED_SDIST_FILES = {
    "CHANGELOG.md",
    "CITATION.cff",
    "CONTRIBUTING.md",
    "LICENSE",
    "MANIFEST.in",
    "README.md",
    "ROADMAP.md",
    "SAFETY.md",
    "SECURITY.md",
    "pyproject.toml",
    "uv.lock",
}
REQUIRED_RESOURCES = {
    "kohdalab/resources/default.json",
    "kohdalab/instruments/delay_stage/stages.toml",
    "kohdalab/instruments/scanner/actuator.toml",
}


def _single(directory: Path, pattern: str) -> Path:
    matches = sorted(directory.glob(pattern))
    if len(matches) != 1:
        raise RuntimeError(
            f"Expected exactly one {pattern!r} in {directory}, found {len(matches)}."
        )
    return matches[0]


def _is_forbidden(path: str) -> bool:
    parts = PurePosixPath(path).parts
    return (
        any(part in FORBIDDEN_PARTS for part in parts)
        or path.endswith((".pyc", ".pyo"))
        or any(part.startswith("._") for part in parts)
    )


def _workspace_package_files(root: Path) -> set[str]:
    source = root / "src" / "kohdalab"
    return {
        path.relative_to(root / "src").as_posix()
        for path in source.rglob("*")
        if path.is_file() and path.suffix in ALLOWED_PACKAGE_SUFFIXES
    }


def _wheel_payloads(path: Path) -> dict[str, bytes]:
    with zipfile.ZipFile(path) as archive:
        bad = archive.testzip()
        if bad is not None:
            raise RuntimeError(f"Wheel CRC verification failed for {bad}.")
        return {
            info.filename: archive.read(info)
            for info in archive.infolist()
            if not info.is_dir()
        }


def _sdist_payloads(path: Path) -> dict[str, bytes]:
    payloads: dict[str, bytes] = {}
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            if not member.isfile():
                continue
            parts = PurePosixPath(member.name).parts
            if len(parts) < 2:
                raise RuntimeError(
                    f"sdist member lacks a root directory: {member.name}"
                )
            relative = PurePosixPath(*parts[1:]).as_posix()
            stream = archive.extractfile(member)
            if stream is None:
                raise RuntimeError(f"Unable to read sdist member: {member.name}")
            if relative in payloads:
                raise RuntimeError(f"Duplicate sdist member: {relative}")
            payloads[relative] = stream.read()
    return payloads


def _verify_record(payloads: dict[str, bytes]) -> None:
    record_names = [name for name in payloads if name.endswith(".dist-info/RECORD")]
    if len(record_names) != 1:
        raise RuntimeError("Wheel must contain exactly one dist-info/RECORD file.")
    record_name = record_names[0]
    rows = list(csv.reader(io.StringIO(payloads[record_name].decode("utf-8"))))
    recorded = {row[0] for row in rows}
    if recorded != set(payloads):
        raise RuntimeError("Wheel RECORD paths do not exactly match archive members.")
    for name, digest, size in rows:
        if name == record_name:
            if digest or size:
                raise RuntimeError(
                    "Wheel RECORD must leave its own hash and size empty."
                )
            continue
        expected_digest = (
            base64.urlsafe_b64encode(hashlib.sha256(payloads[name]).digest())
            .rstrip(b"=")
            .decode("ascii")
        )
        if digest != f"sha256={expected_digest}":
            raise RuntimeError(f"Wheel RECORD hash mismatch for {name}.")
        if size != str(len(payloads[name])):
            raise RuntimeError(f"Wheel RECORD size mismatch for {name}.")


def _verify_metadata(payloads: dict[str, bytes], root: Path) -> None:
    metadata_names = [name for name in payloads if name.endswith(".dist-info/METADATA")]
    entry_names = [
        name for name in payloads if name.endswith(".dist-info/entry_points.txt")
    ]
    if len(metadata_names) != 1 or len(entry_names) != 1:
        raise RuntimeError("Wheel must contain one METADATA and one entry_points.txt.")
    project = tomllib.loads((root / "pyproject.toml").read_text(encoding="utf-8"))[
        "project"
    ]
    metadata = BytesParser().parsebytes(payloads[metadata_names[0]])
    if metadata["Name"] != project["name"]:
        raise RuntimeError("Wheel project name does not match pyproject.toml.")
    if metadata["Version"] != project["version"]:
        raise RuntimeError("Wheel version does not match pyproject.toml.")
    metadata_python = {
        specifier.strip() for specifier in metadata["Requires-Python"].split(",")
    }
    project_python = {
        specifier.strip() for specifier in project["requires-python"].split(",")
    }
    if metadata_python != project_python:
        raise RuntimeError("Wheel Requires-Python does not match pyproject.toml.")
    requirements = set(metadata.get_all("Requires-Dist", []))
    for requirement in project["dependencies"]:
        if requirement not in requirements:
            raise RuntimeError(f"Wheel is missing runtime requirement {requirement!r}.")
    expected_entries = {
        f"{name} = {target}" for name, target in project["scripts"].items()
    }
    entries = {
        line.strip()
        for line in payloads[entry_names[0]].decode("utf-8").splitlines()
        if line.strip() and not line.startswith("[")
    }
    if entries != expected_entries:
        raise RuntimeError("Wheel console entry points do not match pyproject.toml.")


def verify_build(directory: Path, root: Path) -> tuple[Path, Path, dict[str, bytes]]:
    wheel = _single(directory, "*.whl")
    sdist = _single(directory, "*.tar.gz")
    wheel_payloads = _wheel_payloads(wheel)
    sdist_payloads = _sdist_payloads(sdist)
    forbidden = [
        name for name in (*wheel_payloads, *sdist_payloads) if _is_forbidden(name)
    ]
    if forbidden:
        raise RuntimeError(f"Forbidden files found in distributions: {forbidden}")

    expected_package = _workspace_package_files(root)
    wheel_package = {name for name in wheel_payloads if name.startswith("kohdalab/")}
    if wheel_package != expected_package:
        missing = sorted(expected_package - wheel_package)
        extra = sorted(wheel_package - expected_package)
        raise RuntimeError(f"Wheel package mismatch; missing={missing}, extra={extra}")
    sdist_package = {
        name.removeprefix("src/")
        for name in sdist_payloads
        if name.startswith("src/kohdalab/")
    }
    if sdist_package != expected_package:
        raise RuntimeError("sdist and workspace package file sets do not match.")
    if not REQUIRED_RESOURCES <= wheel_package:
        raise RuntimeError("Wheel is missing one or more required runtime resources.")
    if not REQUIRED_SDIST_FILES <= set(sdist_payloads):
        missing = sorted(REQUIRED_SDIST_FILES - set(sdist_payloads))
        raise RuntimeError(f"sdist is missing required project files: {missing}")
    if not any(name.startswith("tests/") for name in sdist_payloads):
        raise RuntimeError("sdist does not contain the test suite.")
    if not any(name.startswith("scripts/") for name in sdist_payloads):
        raise RuntimeError("sdist does not contain release verification scripts.")
    if not any(name.startswith("docs/") for name in sdist_payloads):
        raise RuntimeError("sdist does not contain project documentation.")
    if not any(name.startswith("notebook/") for name in sdist_payloads):
        raise RuntimeError("sdist does not contain maintained notebooks.")

    _verify_record(wheel_payloads)
    _verify_metadata(wheel_payloads, root)
    return wheel, sdist, sdist_payloads


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify Python distribution artifacts."
    )
    parser.add_argument("first", type=Path)
    parser.add_argument("second", type=Path)
    parser.add_argument("--project-root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    root = args.project_root.resolve()
    first_wheel, first_sdist, first_payloads = verify_build(args.first, root)
    second_wheel, second_sdist, second_payloads = verify_build(args.second, root)
    if first_wheel.name != second_wheel.name or first_sdist.name != second_sdist.name:
        raise RuntimeError("Repeated builds produced different artifact filenames.")
    if first_wheel.read_bytes() != second_wheel.read_bytes():
        raise RuntimeError("Repeated wheels are not byte-for-byte reproducible.")
    if first_payloads != second_payloads:
        raise RuntimeError("Repeated sdists do not contain identical file payloads.")
    print(
        f"Verified reproducible wheel and sdist payloads: {first_wheel.name}, "
        f"{first_sdist.name}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
