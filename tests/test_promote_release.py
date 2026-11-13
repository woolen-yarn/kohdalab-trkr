"""Exercise the real verifier against local Git and a replaceable gh transport."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import zipfile

import pytest


@pytest.fixture
def promotion(tmp_path, monkeypatch):
    scripts = Path(__file__).resolve().parents[1] / "scripts"
    monkeypatch.syspath_prepend(str(scripts))
    spec = importlib.util.spec_from_file_location("promote_release", scripts / "promote_release.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    repo = tmp_path / "repo"
    repo.mkdir()

    def git(*args):
        return subprocess.check_output(["git", "-C", str(repo), *args]).decode().strip()

    git("init", "-q", "-b", "main")
    git("config", "user.name", "Release tests")
    git("config", "user.email", "release@example.invalid")
    files = {
        "Cargo.toml": b'[workspace.package]\nversion = "0.5.7"\n',
        "pyproject.toml": b'[project]\nversion = "0.5.7"\n',
        "CITATION.cff": b'version: "0.5.7"\n',
        "docs/release-notes.md": b'# KohdaLab TRKR 0.5.7\n',
        "CHANGELOG.md": b'# Changelog\n\n## 0.5.7 -- today\n',
        "native/sample.bin": b'\x00\xffbinary\n',
    }
    for name, content in files.items():
        path = repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    git("add", ".")
    git("commit", "-qm", "Release fixture")
    commit = git("rev-parse", "HEAD")
    git("update-ref", "refs/remotes/origin/main", commit)
    tree = git("rev-parse", "HEAD^{tree}")
    monkeypatch.setattr(module, "ROOT", repo)
    build_sha = "a" * 40
    head = "b" * 40
    identity = {"full_name": module.REPOSITORY}
    pr = {"number": 75, "merged": True, "head": {"sha": head, "ref": "release"},
          "base": {"ref": "main", "repo": identity},
          "html_url": f"https://github.com/{module.REPOSITORY}/pull/75"}
    run = {"id": 123, "name": module.WORKFLOW, "status": "completed", "conclusion": "success",
           "repository": identity, "event": "pull_request", "head_sha": head,
           "head_branch": "release", "pull_requests": [], "run_attempt": 1,
           "path": ".github/workflows/release.yml@refs/pull/75/merge",
           "html_url": f"https://github.com/{module.REPOSITORY}/actions/runs/123"}
    checks = []
    for index, (name, path) in enumerate((("Test", "test.yml"), ("Documentation", "docs.yml")), 124):
        check = copy.deepcopy(run)
        check.update(id=index, name=name, path=f".github/workflows/{path}@refs/pull/75/merge",
                     html_url=f"https://github.com/{module.REPOSITORY}/actions/runs/{index}")
        checks.append(check)
    artifacts = []
    sources = {}
    for directory, platform in module.PLATFORMS.items():
        root = tmp_path / "fixtures" / directory
        dist = root / "dist"
        dist.mkdir(parents=True)
        source = dist / f"kohdalab-trkr-0.5.7-{platform}-source.zip"
        with zipfile.ZipFile(source, "w") as archive:
            for name, content in files.items():
                if name != "CITATION.cff":
                    archive.writestr(name, content)
        (dist / f"kohdalab-trkr-0.5.7-{platform}.zip").write_bytes(b"packaged application")
        package = ("KohdaLab-TRKR-0.5.7-Setup.exe" if directory == "windows"
                   else "KohdaLab-TRKR-0.5.7-macos-arm64.dmg")
        (dist / package).write_bytes(b"installer")
        origin = {"version": "0.5.7", "platform": platform, "GITHUB_SHA": build_sha,
                  "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "1",
                  "GITHUB_REPOSITORY": module.REPOSITORY, "GITHUB_WORKFLOW": module.WORKFLOW,
                  "GITHUB_REF_NAME": "75/merge",
                  "files": {p.name: module.digest(p) for p in dist.iterdir()}}
        (dist / f"BUILD-ORIGIN-{platform}.json").write_text(json.dumps(origin))
        for folder, filename in (("measurement-validation", "validation.json"),
                                 ("common-control-validation", "evidence.json"),
                                 ("installed-validation" if directory == "windows" else "dmg-validation",
                                  "validation.json")):
            path = root / "build" / "release-fixture" / folder / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('{"simulation": true}\n')
        artifact_name = f"release-{platform}-{build_sha}"
        artifacts.append({"id": len(artifacts) + 1, "name": artifact_name, "expired": False})
        sources[artifact_name] = root

    class Fixture:
        pass

    fixture = Fixture()
    fixture.module, fixture.repo, fixture.git = module, repo, git
    fixture.commit, fixture.tree = commit, tree
    fixture.files, fixture.run, fixture.pr = files, run, pr
    fixture.related_prs = [copy.deepcopy(pr)]
    fixture.checks, fixture.artifacts, fixture.sources = checks, artifacts, sources
    fixture.faults, fixture.calls = {}, []
    fixture.output = tmp_path / "output"
    fixture.arguments = ["verify", "--run", "123", "--commit", commit, "--tag", "v0.5.7",
                         "--output", str(fixture.output)]

    def reseal(directory="windows", update_origin=True):
        platform = module.PLATFORMS[directory]
        root = next(root for root in sources.values() if root.name == directory)
        dist = root / "dist"
        origin_file = dist / f"BUILD-ORIGIN-{platform}.json"
        origin = json.loads(origin_file.read_text())
        if update_origin:
            origin["files"] = {name: module.digest(dist / name) for name in origin["files"]}
            origin_file.write_text(json.dumps(origin))
        (dist / f"SHA256SUMS-{platform}.txt").write_text("".join(
            f"{module.digest(p)}  {p.name}\n" for p in sorted(dist.iterdir())
            if p.name != f"SHA256SUMS-{platform}.txt"))

    def transport(*args):
        fixture.calls.append(args)
        if args[:2] == ("run", "download"):
            assert args[2:5] == ("123", "--repo", module.REPOSITORY)
            root = sources[args[args.index("--name") + 1]]
            shutil.copytree(root, args[args.index("--dir") + 1])
            return b""
        assert args[0] == "api", "verify must only read API data or download artifacts"
        endpoint = args[1].removeprefix(f"repos/{module.REPOSITORY}/")
        if endpoint in fixture.faults:
            error = fixture.faults[endpoint]
            if isinstance(error, Exception):
                raise error
            return json.dumps(error).encode()
        if endpoint.startswith(("git/ref/tags/", "releases/tags/")):
            raise module.GitHubError("not found (HTTP 404)", 404)
        if endpoint == "actions/runs/123":
            value = run
        elif endpoint == "pulls/75":
            value = pr
        elif endpoint.startswith("commits/") and endpoint.endswith("/pulls?per_page=100"):
            value = [fixture.related_prs]
        elif endpoint.startswith("actions/runs?head_sha="):
            value = [{"workflow_runs": checks}]
        elif endpoint == "actions/runs/123/artifacts?per_page=100":
            value = [{"artifacts": artifacts}]
        elif endpoint.startswith("git/commits/"):
            value = {"tree": {"sha": fixture.tree}}
        else:
            pytest.fail(f"Unexpected GitHub access: {args}")
        return json.dumps(value).encode()

    fixture.reseal = reseal
    fixture.dist = lambda directory="windows": next(root for root in sources.values()
                                                    if root.name == directory) / "dist"
    fixture.run_cli = lambda *extra: module.main(fixture.arguments + list(extra))
    monkeypatch.setattr(module, "github", transport)
    for directory in module.PLATFORMS:
        reseal(directory)
    return fixture


def assert_failed(fixture, capsys, message):
    assert fixture.run_cli() == 1
    assert message in capsys.readouterr().err
    assert not fixture.output.exists()


def test_pr_success_and_sorted_complete_checksums(promotion):
    f = promotion
    assert f.run_cli() == 0
    assets = list(f.output.iterdir())
    assert len(assets) == 18
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["schema"] == 2
    assert report["application"] == "trkr"
    assert report["release_commit"] == f.commit
    assert report["tested_pr_head"] == f.pr["head"]["sha"]
    assert report["source_tree"] == f.tree
    for field, record, id_field in (("ci_run", f.run, "id"), ("pr", f.pr, "number"),
                                    ("test_run", f.checks[0], "id"),
                                    ("docs_run", f.checks[1], "id")):
        assert type(report[field]) is int
        assert report[field] == record[id_field]
        assert report[field + "_url"] == record["html_url"]
    assert report["build_kind"] == "pull_request"
    assert report["tree_check"] == "api-identical"
    assert report["hardware_tested"] is False
    assert "hardware_evidence" not in report
    assert "hardware_results" not in report
    assert f.run["pull_requests"] == []
    assert all(check["pull_requests"] == [] for check in f.checks)
    lines = (f.output / "SHA256SUMS.txt").read_text().splitlines()
    names = [line.split("  ")[1] for line in lines]
    assert names == sorted(p.name for p in assets if p.name != "SHA256SUMS.txt")
    for line in lines:
        checksum, name = line.split("  ")
        assert checksum == hashlib.sha256((f.output / name).read_bytes()).hexdigest()
    # Flattening must preserve every byte of the sixteen original assets.
    for root in f.sources.values():
        for path in (root / "dist").iterdir():
            assert path.read_bytes() == (f.output / path.name).read_bytes()
    assert all(call[0] == "api" or call[:2] == ("run", "download") for call in f.calls)


def test_bare_workflow_paths_are_also_supported(promotion):
    f = promotion
    for run in [f.run, *f.checks]:
        run["path"] = run["path"].split("@", 1)[0]
    assert f.run_cli() == 0


@pytest.mark.parametrize("filename", ["Cargo.toml", "pyproject.toml", "CITATION.cff",
                                     "docs/release-notes.md", "CHANGELOG.md"])
def test_each_version_mismatch(promotion, capsys, filename):
    f = promotion
    path = f.repo / filename
    path.write_bytes(path.read_bytes().replace(b"0.5.7", b"0.5.8"))
    f.git("add", filename)
    f.git("commit", "-qm", "Mismatched release metadata")
    commit = f.git("rev-parse", "HEAD")
    f.git("update-ref", "refs/remotes/origin/main", commit)
    f.arguments[f.arguments.index("--commit") + 1] = commit
    assert_failed(f, capsys, filename)


@pytest.mark.parametrize("field,value,message", [
    ("conclusion", "failure", "successful"), ("status", "in_progress", "successful"),
    ("name", "Other workflow", "successful"), ("path", "other.yml", "workflow path"),
    ("event", "push", "Unsupported"), ("head_sha", "c" * 40, "exactly one"),
    ("head_branch", "another-branch", "exactly one"),
    ("repository", {"full_name": "another/repository"}, "another repository"),
])
def test_invalid_build_run(promotion, capsys, field, value, message):
    promotion.run[field] = value
    assert_failed(promotion, capsys, message)


def test_unmerged_pr(promotion, capsys):
    promotion.pr["merged"] = False
    assert_failed(promotion, capsys, "not merged")


def test_pr_targets_other_branch(promotion, capsys):
    promotion.pr["base"]["ref"] = "other"
    assert_failed(promotion, capsys, "must target")


@pytest.mark.parametrize("count", [0, 2])
def test_pr_resolution_requires_exactly_one_match(promotion, capsys, count):
    f = promotion
    f.related_prs = [copy.deepcopy(f.pr) for _ in range(count)]
    if count == 2:
        f.related_prs[1]["number"] = 76
    assert_failed(f, capsys, "exactly one")


@pytest.mark.parametrize("part,field,value", [
    ("head", "sha", "c" * 40), ("head", "ref", "another-branch"),
    ("base", "ref", "other"), ("base", "repo", {"full_name": "another/repository"}),
])
def test_pr_resolution_filters_unrelated_candidates(promotion, capsys, part, field, value):
    f = promotion
    f.related_prs[0][part][field] = value
    assert_failed(f, capsys, "exactly one")


def test_pr_resolution_ignores_other_branch_with_same_sha(promotion):
    f = promotion
    other = copy.deepcopy(f.pr)
    other["number"] = 76
    other["head"]["ref"] = "another-branch"
    f.related_prs.insert(0, other)
    assert f.run_cli() == 0


def test_pr_resolution_checks_all_pages(promotion, capsys):
    f = promotion
    second = copy.deepcopy(f.pr)
    second["number"] = 76
    f.faults[f"commits/{f.run['head_sha']}/pulls?per_page=100"] = [[f.pr], [second]]
    assert_failed(f, capsys, "exactly one")


@pytest.mark.parametrize("field,value", [("sha", "c" * 40), ("ref", "another-branch")])
def test_resolved_pr_head_is_rechecked(promotion, capsys, field, value):
    promotion.pr["head"][field] = value
    assert_failed(promotion, capsys, "PR head")


@pytest.mark.parametrize("index", [0, 1])
@pytest.mark.parametrize("field,value", [("conclusion", "failure"), ("head_sha", "c" * 40),
                                        ("event", "workflow_dispatch"),
                                        ("head_branch", "another-branch"),
                                        ("path", ".github/workflows/other.yml"),
                                        ("repository", {"full_name": "another/repository"})])
def test_required_ci_checks(promotion, capsys, index, field, value):
    name = promotion.checks[index]["name"]
    promotion.checks[index][field] = value
    assert_failed(promotion, capsys, name)


def test_tree_mismatch(promotion, capsys):
    promotion.faults["git/commits/" + "a" * 40] = {"tree": {"sha": "d" * 40}}
    assert_failed(promotion, capsys, "tree mismatch")


@pytest.mark.parametrize("status", [404, 422])
def test_unavailable_merge_commit_requires_fresh_main_build(promotion, capsys, status):
    f = promotion
    f.faults["git/commits/" + "a" * 40] = f.module.GitHubError("unavailable", status)
    assert_failed(f, capsys, "PR build tree is unavailable. Use a fresh main build run.")


@pytest.mark.parametrize("status", [401, 403, 429, 500])
def test_api_errors_are_not_treated_as_missing(promotion, capsys, status):
    f = promotion
    f.faults["git/commits/" + "a" * 40] = f.module.GitHubError("API error", status)
    assert_failed(f, capsys, "API error")


@pytest.mark.parametrize("kind,message", [("extra", "Extra source"), ("content", "Source content"),
                                         ("traversal", "Unsafe source"), ("empty", "Empty source"),
                                         ("missing", "Missing source"), ("binary_crlf", "Source content")])
def test_bad_source_archive_with_valid_hashes(promotion, capsys, kind, message):
    f = promotion
    source = next(f.dist().glob("*-source.zip"))
    files = {name: data for name, data in f.files.items() if name != "CITATION.cff"}
    if kind == "extra":
        files["untracked.txt"] = b"extra"
    elif kind == "content":
        files["native/sample.bin"] = b"changed binary"
    elif kind == "traversal":
        files["../escape"] = b"bad"
    elif kind == "empty":
        files = {}
    elif kind == "missing":
        del files["native/sample.bin"]
    elif kind == "binary_crlf":
        files["native/sample.bin"] = files["native/sample.bin"].replace(b"\n", b"\r\n")
    with zipfile.ZipFile(source, "w") as archive:
        for name, data in files.items():
            archive.writestr(name, data)
    f.reseal()
    assert_failed(f, capsys, message)


def test_crlf_only_is_normalized(promotion):
    f = promotion
    with zipfile.ZipFile(next(f.dist().glob("*-source.zip")), "w") as archive:
        for name, data in f.files.items():
            if name != "CITATION.cff":
                archive.writestr(name, data.replace(b"\n", b"\r\n") if name.endswith(".md") else data)
    f.reseal()
    assert f.run_cli() == 0


@pytest.mark.parametrize("manifest", ["origin", "sums", "file"])
def test_hash_mismatch(promotion, capsys, manifest):
    f = promotion
    if manifest == "file":
        next(f.dist().glob("*Setup.exe")).write_bytes(b"tampered")
    elif manifest == "sums":
        path = next(f.dist().glob("SHA256SUMS-*.txt"))
        lines = path.read_text().splitlines()
        lines[0] = "0" * 64 + lines[0][64:]
        path.write_text("\n".join(lines) + "\n")
    else:
        path = next(f.dist().glob("BUILD-ORIGIN-*.json"))
        origin = json.loads(path.read_text())
        name = next(iter(origin["files"]))
        origin["files"][name] = "0" * 64
        path.write_text(json.dumps(origin))
        f.reseal(update_origin=False)
    assert_failed(f, capsys, "hash mismatch")


@pytest.mark.parametrize("index", [0, 1])
@pytest.mark.parametrize("expired", [False, True])
def test_artifact_missing_or_expired(promotion, capsys, index, expired):
    if expired:
        promotion.artifacts[index]["expired"] = True
    else:
        promotion.artifacts.pop(index)
    assert_failed(promotion, capsys, "artifact:")


@pytest.mark.parametrize("endpoint,label", [("git/ref/tags/v0.5.7", "tag"),
                                            ("releases/tags/v0.5.7", "Release")])
def test_existing_tag_or_release(promotion, capsys, endpoint, label):
    promotion.faults[endpoint] = {"id": 10}
    assert_failed(promotion, capsys, label + " v0.5.7 already exists")


def test_allow_existing_skips_only_existence_checks(promotion, capsys):
    f = promotion
    f.faults["git/ref/tags/v0.5.7"] = {"object": {"sha": f.commit}}
    f.faults["releases/tags/v0.5.7"] = {"draft": False}
    assert f.run_cli("--allow-existing-release") == 0
    assert not any("/tags/" in call[1] for call in f.calls if call[0] == "api")
    shutil.rmtree(f.output)
    f.run["conclusion"] = "failure"
    assert f.run_cli("--allow-existing-release") == 1
    assert "successful" in capsys.readouterr().err


def test_existence_network_error_is_not_absence(promotion, capsys):
    promotion.faults["git/ref/tags/v0.5.7"] = promotion.module.GitHubError("auth failed", 401)
    assert_failed(promotion, capsys, "auth failed")


def test_commit_not_in_origin_main(promotion, capsys):
    f = promotion
    f.git("checkout", "-qb", "unmerged")
    (f.repo / "unmerged.txt").write_text("unmerged")
    f.git("add", ".")
    f.git("commit", "-qm", "Unmerged change")
    f.arguments[f.arguments.index("--commit") + 1] = f.git("rev-parse", "HEAD")
    assert_failed(f, capsys, "origin/main")
    assert not f.calls


def commit_hardware_evidence(f, text):
    path = "docs/validation/hardware.md"
    (f.repo / path).parent.mkdir(parents=True, exist_ok=True)
    (f.repo / path).write_text(text, encoding="utf-8")
    f.git("add", path)
    f.git("commit", "-qm", "Hardware evidence fixture")
    f.commit = f.git("rev-parse", "HEAD")
    f.tree = f.git("rev-parse", "HEAD^{tree}")
    f.git("update-ref", "refs/remotes/origin/main", f.commit)
    f.arguments[f.arguments.index("--commit") + 1] = f.commit
    f.files[path] = text.encode()
    for directory in f.module.PLATFORMS:
        with zipfile.ZipFile(next(f.dist(directory).glob("*-source.zip")), "w") as archive:
            for name, content in f.files.items():
                if name != "CITATION.cff":
                    archive.writestr(name, content)
        f.reseal(directory)
    return path


@pytest.mark.parametrize("heading", ["## v0.5.6\n", "## v0.5.70\n", "Mention v0.5.7 only\n",
                                     "## 10.5.7\n", "## v0.5.7-rc1\n", "## v0.5.7+other\n",
                                     "### 2026-11-10 — test build `test-pr75`\n"])
def test_hardware_evidence_requires_matching_section(promotion, capsys, heading):
    f = promotion
    path = commit_hardware_evidence(f, heading + "| # | Result |\n|---|---|\n| C1 | pass |\n")
    assert f.run_cli("--hardware-evidence", path) == 1
    assert "no section for version" in capsys.readouterr().err
    assert not f.output.exists()


@pytest.mark.parametrize("rows,expected,tested", [
    ("| C1 |  PaSs  | |\n| C2 | NOT CHECKED | |\n",
     {"pass": 1, "fail": 0, "not_checked": 1}, True),
    ("| C1 | | |\n| C2 | | |\n", {"pass": 0, "fail": 0, "not_checked": 0}, False),
    ("| C1 | pass | |\n| C2 | FaIl | |\n", {"pass": 1, "fail": 1, "not_checked": 0}, False),
    ("| C1 | not checked | |\n", {"pass": 0, "fail": 0, "not_checked": 1}, False),
    ("| C1 | failed | pass |\n| C2 | `pass` | |\n",
     {"pass": 0, "fail": 0, "not_checked": 0}, False),
])
def test_hardware_evidence_results_are_recorded(promotion, rows, expected, tested):
    f = promotion
    path = commit_hardware_evidence(f, "# Commissioning\n\n## v0.5.7\n\n"
                                    "| # | Result | Notes |\n|---|---|---|\n" + rows)
    assert f.run_cli("--hardware-evidence", path) == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["hardware_tested"] is tested
    assert report["hardware_evidence"] == path
    assert report["hardware_results"] == expected


@pytest.mark.parametrize("next_heading", ["## v0.5.8", "# Other releases"])
def test_hardware_section_ends_at_same_or_higher_heading(promotion, next_heading):
    f = promotion
    path = commit_hardware_evidence(f, "## v0.5.7\n\n### Setup\n\n"
                                    "| # | Result |\n|---|---|\n| C1 | not checked |\n\n"
                                    + next_heading + "\n\n"
                                    "| # | Result |\n|---|---|\n| C1 | pass |\n")
    assert f.run_cli("--hardware-evidence", path) == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["hardware_tested"] is False
    assert report["hardware_results"] == {"pass": 0, "fail": 0, "not_checked": 1}


def test_hardware_evidence_reads_commit_not_working_tree(promotion):
    f = promotion
    path = commit_hardware_evidence(f, "## 0.5.7\n\n| # | Result |\n|---|---|\n| C1 | |\n")
    (f.repo / path).write_text("## 0.5.7\n\n| # | Result |\n|---|---|\n| C1 | pass |\n")
    assert f.run_cli("--hardware-evidence", path) == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["hardware_tested"] is False
    assert report["hardware_results"] == {"pass": 0, "fail": 0, "not_checked": 0}


def test_hardware_evidence_aggregates_version_setups_and_subsections(promotion):
    f = promotion
    path = commit_hardware_evidence(f, "## v0.5.7 — setup A\n\n### Connection\n\n"
                                    "| # | Result |\n|---|---|\n| C1 | pass |\n\n"
                                    "## v0.5.8\n\n| # | Result |\n|---|---|\n| C2 | pass |\n\n"
                                    "## v0.5.7 — setup B\n\n"
                                    "| # | Result |\n|---|---|\n| C1 | fail |\n")
    assert f.run_cli("--hardware-evidence", path) == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["hardware_tested"] is False
    assert report["hardware_results"] == {"pass": 1, "fail": 1, "not_checked": 0}


@pytest.mark.parametrize("fence", ["```", "~~~~"])
def test_hardware_evidence_ignores_markdown_examples(promotion, fence):
    f = promotion
    path = commit_hardware_evidence(f, fence + "markdown\n## v0.5.7\n" + fence
                                    + "\n## v0.5.7\n\n"
                                    + fence + "\n| # | Result |\n|---|---|\n| C1 | pass |\n"
                                    + fence + "\n\n"
                                    "    | # | Result |\n    |---|---|\n    | C2 | pass |\n\n"
                                    "| C3 | pass | (no table header)\n")
    assert f.run_cli("--hardware-evidence", path) == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["hardware_tested"] is False
    assert report["hardware_results"] == {"pass": 0, "fail": 0, "not_checked": 0}


@pytest.mark.parametrize("local_file", [False, True])
def test_hardware_evidence_must_exist_in_release_tree(promotion, capsys, local_file):
    f = promotion
    if local_file:
        (f.repo / "hardware.md").write_text("## v0.5.7\n| C1 | pass |\n")
    assert f.run_cli("--hardware-evidence", "hardware.md") == 1
    assert "not in the release commit tree" in capsys.readouterr().err
    assert not f.output.exists()


@pytest.mark.parametrize("path", ["/tmp/hardware.md", "../hardware.md", "docs/../hardware.md",
                                  "docs/../../hardware.md", "docs\\hardware.md"])
def test_hardware_evidence_rejects_unsafe_paths(promotion, capsys, path):
    assert promotion.run_cli("--hardware-evidence", path) == 1
    assert "repository-relative path" in capsys.readouterr().err
    assert not promotion.output.exists()


def dispatch(f):
    f.run.update(event="workflow_dispatch", head_branch="main", head_sha=f.commit, pull_requests=[],
                 path=".github/workflows/release.yml@refs/heads/main")
    for check in f.checks:
        check.update(event="push", head_branch="main", head_sha=f.commit)
    for artifact in f.artifacts:
        old_name = artifact["name"]
        platform = old_name.removeprefix("release-").rsplit("-", 1)[0]
        new_name = f"release-{platform}-{f.commit}"
        root = f.sources.pop(old_name)
        f.sources[new_name] = root
        artifact["name"] = new_name
        path = next((root / "dist").glob("BUILD-ORIGIN-*.json"))
        origin = json.loads(path.read_text())
        origin.update(GITHUB_SHA=f.commit, GITHUB_REF_NAME="main")
        path.write_text(json.dumps(origin))
        f.reseal(root.name)


def test_manual_main_build(promotion):
    f = promotion
    dispatch(f)
    assert f.run_cli() == 0
    report = json.loads((f.output / "RELEASE-VERIFICATION.json").read_text())
    assert report["build_kind"] == "workflow_dispatch"
    assert report["tested_pr_head"] is report["pr"] is report["pr_url"] is None
    for field, check in zip(("test_run", "docs_run"), f.checks, strict=True):
        assert report[field] == check["id"]
        assert report[field + "_url"] == check["html_url"]
    assert type(report["ci_run"]) is int
    assert report["ci_run"] == 123
    assert report["ci_run_url"] == f.run["html_url"]
    assert not any("/pulls/" in call[1] for call in f.calls)


@pytest.mark.parametrize("field,value", [("event", "pull_request"), ("head_sha", "c" * 40),
                                         ("conclusion", "failure")])
def test_manual_build_requires_main_push_ci(promotion, capsys, field, value):
    dispatch(promotion)
    promotion.checks[0][field] = value
    assert_failed(promotion, capsys, "Test run")


@pytest.mark.parametrize("manual", [False, True])
def test_latest_ci_failure_blocks_older_success(promotion, capsys, manual):
    if manual:
        dispatch(promotion)
    promotion.checks.append(dict(promotion.checks[0], id=12500, conclusion="failure"))
    assert_failed(promotion, capsys, "Latest Test run 12500")


@pytest.mark.parametrize("macos_attempt", ["1", "2"])
def test_platforms_must_match_latest_build_attempt(promotion, capsys, macos_attempt):
    f = promotion
    f.run["run_attempt"] = 2
    for directory, attempt in (("windows", "2"), ("macos", macos_attempt)):
        path = next(f.dist(directory).glob("BUILD-ORIGIN-*.json"))
        origin = json.loads(path.read_text())
        origin["GITHUB_RUN_ATTEMPT"] = attempt
        path.write_text(json.dumps(origin))
        f.reseal(directory)
    if macos_attempt == "1":
        assert_failed(f, capsys, "origin run attempt mismatch. Use a fresh main build run")
    else:
        assert f.run_cli() == 0


@pytest.mark.parametrize("field,value", [("head_branch", "release"), ("head_sha", "c" * 40)])
def test_manual_build_must_use_main_release_commit(promotion, capsys, field, value):
    dispatch(promotion)
    promotion.run[field] = value
    assert_failed(promotion, capsys, "release commit on main")


def test_manual_build_cannot_use_unavailable_tree_fallback(promotion, capsys):
    f = promotion
    dispatch(f)
    f.faults["git/commits/" + f.commit] = f.module.GitHubError("not found", 404)
    assert_failed(f, capsys, "not found")


@pytest.mark.parametrize("field,value,message", [
    ("GITHUB_RUN_ID", "999", "run ID"), ("version", "0.5.8", "origin version"),
    ("GITHUB_SHA", "c" * 40, "artifact SHA"),
    ("GITHUB_RUN_ATTEMPT", "2", "attempt"), ("GITHUB_REF_NAME", "999/merge", "Origin ref"),
])
def test_origin_mismatches(promotion, capsys, field, value, message):
    f = promotion
    path = next(f.dist().glob("BUILD-ORIGIN-*.json"))
    origin = json.loads(path.read_text())
    origin[field] = value
    path.write_text(json.dumps(origin))
    f.reseal()
    assert_failed(f, capsys, message)


def test_missing_validation_report(promotion, capsys):
    root = next(iter(promotion.sources.values()))
    next(root.glob("build/**/evidence.json")).unlink()
    assert_failed(promotion, capsys, "Expected one")


def test_existing_output_is_preserved(promotion, capsys):
    f = promotion
    f.output.mkdir()
    (f.output / "keep.txt").write_text("keep")
    assert f.run_cli() == 1
    assert "already exists" in capsys.readouterr().err
    assert (f.output / "keep.txt").read_text() == "keep"
    assert not f.calls


def test_github_transport_invokes_only_gh(promotion, monkeypatch):
    calls = []
    def run(command, **kwargs):
        calls.append((command, kwargs))
        return subprocess.CompletedProcess(command, 0, b'{"ok": true}', b'')
    monkeypatch.setattr(promotion.module.subprocess, "run", run)
    # Load original transport rather than the fixture replacement.
    original = importlib.util.spec_from_file_location(
        "promote_transport", Path(__file__).resolve().parents[1] / "scripts/promote_release.py")
    module = importlib.util.module_from_spec(original)
    original.loader.exec_module(module)
    assert module.github("api", "repos/example/repo") == b'{"ok": true}'
    assert calls == [(["gh", "api", "repos/example/repo"], {"capture_output": True})]


def test_github_transport_preserves_http_errors(promotion, monkeypatch):
    spec = importlib.util.spec_from_file_location(
        "promote_transport", Path(__file__).resolve().parents[1] / "scripts/promote_release.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    monkeypatch.setattr(module.subprocess, "run", lambda *a, **k:
                        subprocess.CompletedProcess(a[0], 1, b'', b'gh: Not Found (HTTP 404)'))
    with pytest.raises(module.GitHubError) as error:
        module.github("api", "repos/example/repo")
    assert error.value.status == 404
