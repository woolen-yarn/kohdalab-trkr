from __future__ import annotations

import base64
import hashlib
import re
import runpy
import tomllib
from pathlib import Path

import pytest
import yaml


VERIFIER = runpy.run_path("scripts/verify_distributions.py")


def _record(payloads: dict[str, bytes]) -> bytes:
    lines = []
    for name, content in payloads.items():
        digest = base64.urlsafe_b64encode(hashlib.sha256(content).digest()).rstrip(b"=")
        lines.append(f"{name},sha256={digest.decode('ascii')},{len(content)}")
    lines.append("demo-1.0.dist-info/RECORD,,")
    return ("\n".join(lines) + "\n").encode()


def test_distribution_verifier_rejects_platform_and_cache_artifacts():
    is_forbidden = VERIFIER["_is_forbidden"]

    assert is_forbidden("kohdalab/.DS_Store")
    assert is_forbidden("kohdalab/__pycache__/module.pyc")
    assert is_forbidden("kohdalab/._module.py")
    assert not is_forbidden("kohdalab/api/models.py")


def test_distribution_verifier_checks_every_record_hash_and_size():
    verify_record = VERIFIER["_verify_record"]
    payloads = {"kohdalab/__init__.py": b"version = '1.0'\n"}
    payloads["demo-1.0.dist-info/RECORD"] = _record(payloads)

    verify_record(payloads)

    payloads["kohdalab/__init__.py"] = b"tampered\n"
    with pytest.raises(RuntimeError, match="hash mismatch"):
        verify_record(payloads)


def test_distribution_verifier_requires_one_artifact_of_each_kind(tmp_path):
    single = VERIFIER["_single"]

    with pytest.raises(RuntimeError, match="exactly one"):
        single(tmp_path, "*.whl")
    (tmp_path / "one.whl").touch()
    assert single(tmp_path, "*.whl").name == "one.whl"
    (tmp_path / "two.whl").touch()
    with pytest.raises(RuntimeError, match="found 2"):
        single(tmp_path, "*.whl")


def test_release_workflow_checks_lock_and_repeated_builds():
    workflow = yaml.safe_load(
        Path(".github/workflows/test.yml").read_text(encoding="utf-8")
    )
    jobs = workflow["jobs"]
    for job_name in ("test", "package", "audit"):
        commands = "\n".join(
            str(step.get("run", "")) for step in jobs[job_name]["steps"]
        )
        assert "uv lock --check" in commands

    package_commands = "\n".join(
        str(step.get("run", "")) for step in jobs["package"]["steps"]
    )
    assert "dist/first" in package_commands
    assert "dist/second" in package_commands
    assert "verify_distributions.py" in package_commands
    build_step = next(
        step
        for step in jobs["package"]["steps"]
        if step.get("name") == "Build wheel and source distribution twice"
    )
    assert build_step["env"]["SOURCE_DATE_EPOCH"].isdigit()


def test_workflow_has_bounded_jobs_and_immutable_actions():
    workflow = yaml.safe_load(
        Path(".github/workflows/test.yml").read_text(encoding="utf-8")
    )

    assert workflow["permissions"] == {"contents": "read"}
    assert "refs/tags/" in workflow["concurrency"]["cancel-in-progress"]
    for job in workflow["jobs"].values():
        assert 0 < job["timeout-minutes"] <= 30
        for step in job["steps"]:
            action = step.get("uses")
            if action is None:
                continue
            assert re.fullmatch(r"[^@]+@[0-9a-f]{40}", action)
            if action.startswith("actions/checkout@"):
                assert step["with"]["persist-credentials"] is False


def test_source_manifest_is_explicit_and_build_backend_is_pinned():
    manifest = Path("MANIFEST.in").read_text(encoding="utf-8")
    project = tomllib.loads(Path("pyproject.toml").read_text(encoding="utf-8"))

    for required in (
        "CHANGELOG.md",
        "CITATION.cff",
        "uv.lock",
        "docs",
        "notebook",
        "scripts",
        "tests",
    ):
        assert required in manifest
    assert "global-exclude .DS_Store" in manifest
    assert project["build-system"]["requires"] == ["setuptools==83.0.0"]
