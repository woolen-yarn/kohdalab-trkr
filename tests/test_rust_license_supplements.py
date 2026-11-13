import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "rust_license_supplements", ROOT / "scripts" / "rust_license_supplements.py"
)
assert SPEC and SPEC.loader
SUPPLEMENTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SUPPLEMENTS)


class RustLicenseSupplementsTests(unittest.TestCase):
    def fixture(self, directory: Path, *, revision: str = "expected") -> tuple[dict, Path]:
        root = directory / "supplements"
        root.mkdir()
        notice = root / "NOTICE.txt"
        notice.write_bytes(b"pinned upstream notice\n")
        record = {
            "name": "missing-license", "version": "1.2.3", "revision": revision,
            "files": [{"file": notice.name, "sha256": hashlib.sha256(notice.read_bytes()).hexdigest()}],
        }
        (root / "manifest.json").write_text(json.dumps([record]), encoding="utf-8")
        registry = directory / "registry"
        registry.mkdir()
        (registry / ".cargo_vcs_info.json").write_text(
            json.dumps({"git": {"sha1": "expected"}}), encoding="utf-8"
        )
        return {"name": record["name"], "version": record["version"], "manifest_path": str(registry / "Cargo.toml")}, root

    def test_copies_verified_notice_and_provenance(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            package, root = self.fixture(directory)
            destination = directory / "output"
            SUPPLEMENTS.supplement(package, destination, supplements_root=root)
            upstream = destination / "missing-license-1.2.3" / "upstream"
            self.assertEqual((upstream / "NOTICE.txt").read_bytes(), b"pinned upstream notice\n")
            self.assertEqual(json.loads((upstream / "provenance.json").read_text())["revision"], "expected")

    def test_rejects_tampered_notice(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            package, root = self.fixture(directory)
            (root / "NOTICE.txt").write_bytes(b"tampered\n")
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                SUPPLEMENTS.supplement(package, directory / "output", supplements_root=root)

    def test_rejects_registry_revision_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            package, root = self.fixture(directory, revision="different")
            with self.assertRaisesRegex(ValueError, "revision mismatch"):
                SUPPLEMENTS.supplement(package, directory / "output", supplements_root=root)


if __name__ == "__main__":
    unittest.main()
