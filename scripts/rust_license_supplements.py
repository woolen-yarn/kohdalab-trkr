"""Package pinned upstream notices omitted from registry crate archives."""
import hashlib
import json
import shutil
from pathlib import Path


def supplement(package: dict, destination: Path, *, supplements_root: Path | None = None) -> None:
    root = supplements_root or Path(__file__).resolve().parents[1] / "docs/licenses/rust-supplements"
    records = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    for record in records:
        if (record["name"], record["version"]) != (package["name"], package["version"]):
            continue
        registry = Path(package["manifest_path"]).parent
        vcs = json.loads((registry / ".cargo_vcs_info.json").read_text(encoding="utf-8"))
        if vcs["git"]["sha1"] != record["revision"]:
            raise ValueError(f"Upstream notice revision mismatch: {package['name']}")
        output = destination / f"{package['name']}-{package['version']}" / "upstream"
        output.mkdir(parents=True, exist_ok=True)
        for entry in record["files"]:
            source = root / entry["file"]
            if source.resolve().parent != root.resolve():
                raise ValueError("Notice path escapes supplement directory")
            if hashlib.sha256(source.read_bytes()).hexdigest() != entry["sha256"]:
                raise ValueError(f"Upstream notice hash mismatch: {source.name}")
            shutil.copy2(source, output / source.name)
        (output / "provenance.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
