"""Flatten downloaded CI artifacts without colliding validation report names."""
from pathlib import Path
import argparse
import shutil


def prepare(downloads: Path, destination: Path) -> list[Path]:
    sources: dict[str, Path] = {}

    def add(source: Path, name: str) -> None:
        if name in sources:
            raise ValueError(f"Duplicate release asset: {name}")
        sources[name] = source

    for platform in ("windows", "macos"):
        root = downloads / platform
        packages = list((root / "dist").glob("*"))
        if not packages:
            raise ValueError(f"Missing {platform} distribution files")
        for source in packages:
            if source.is_file():
                add(source, source.name)
        reports = {
            "measurement-validation/validation.json": "zip-measurements",
            "common-control-validation/evidence.json": "gui-api",
            ("installed-validation/validation.json" if platform == "windows"
             else "dmg-validation/validation.json"): "installed-measurements" if platform == "windows" else "dmg-measurements",
        }
        for suffix, label in reports.items():
            matches = [p for p in (root / "build").rglob("*.json") if p.as_posix().endswith(suffix)]
            if len(matches) != 1:
                raise ValueError(f"Expected one {platform} {label} report, got {len(matches)}")
            add(matches[0], f"validation-{platform}-{label}.json")
    destination.mkdir(parents=True, exist_ok=False)
    for name, source in sources.items():
        shutil.copy2(source, destination / name)
    return [destination / name for name in sorted(sources)]


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("downloads", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    prepare(args.downloads, args.destination)
