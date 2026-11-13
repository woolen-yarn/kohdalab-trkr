"""Verify CI artifacts for release promotion, without changing GitHub or Git refs.

Requires Python 3.11+, gh authentication, and a local repository containing the
release commit and an up-to-date origin/main. Output must not already exist.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tempfile
import tomllib
from urllib.parse import quote
import zipfile

from package_rust_portable import is_application_source
from prepare_release_assets import prepare

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "woolen-yarn/kohdalab-trkr"
WORKFLOW = "Build release artifacts"
PLATFORMS = {"windows": "windows-x64", "macos": "macos-arm64"}


class GitHubError(RuntimeError):
    def __init__(self, message: str, status: int | None = None):
        super().__init__(message)
        self.status = status


def github(*args: str) -> bytes:
    """The only GitHub transport; tests replace this function, including downloads."""
    result = subprocess.run(["gh", *args], capture_output=True)
    if result.returncode:
        message = result.stderr.decode("utf-8", errors="replace").strip()
        match = re.search(r"HTTP (\d{3})", message)
        raise GitHubError(message, int(match[1]) if match else None)
    return result.stdout


def api(endpoint: str):
    return json.loads(github("api", f"repos/{REPOSITORY}/{endpoint}"))


def pages(endpoint: str) -> list:
    return json.loads(github("api", f"repos/{REPOSITORY}/{endpoint}",
                             "--paginate", "--slurp"))


def git(*args: str, data: bytes | None = None) -> bytes:
    result = subprocess.run(["git", "-C", str(ROOT), *args], input=data,
                            capture_output=True)
    if result.returncode:
        raise ValueError(result.stderr.decode("utf-8", errors="replace").strip()
                         or f"git {' '.join(args)} failed")
    return result.stdout


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tree_files(commit: str) -> dict[str, bytes]:
    """Read committed blobs in one batch, independent of checkout and line endings."""
    entries = []
    for record in git("ls-tree", "-rz", commit).split(b"\0"):
        if record:
            metadata, name = record.split(b"\t", 1)
            _, kind, sha = metadata.split()
            if kind == b"blob":
                entries.append((name.decode("utf-8"), sha))
    raw = git("cat-file", "--batch", data=b"".join(sha + b"\n" for _, sha in entries))
    result = {}
    offset = 0
    for name, _ in entries:
        end = raw.index(b"\n", offset)
        size = int(raw[offset:end].split()[2])
        result[name] = raw[end + 1:end + 1 + size]
        offset = end + size + 2
    return result


def check_versions(files: dict[str, bytes], tag: str) -> str:
    version = tomllib.loads(files["Cargo.toml"].decode())["workspace"]["package"]["version"]
    require(tag == "v" + version, "Cargo.toml version does not match --tag")
    python_version = tomllib.loads(files["pyproject.toml"].decode())["project"]["version"]
    require(python_version == version, "pyproject.toml version mismatch")
    citation = re.search(r'^version:\s*[\'\"]?([^\s\'\"#]+)[\'\"]?\s*$',
                         files["CITATION.cff"].decode(), re.MULTILINE)
    require(citation is not None and citation[1] == version, "CITATION.cff version mismatch")
    title = files["docs/release-notes.md"].decode().splitlines()[0]
    require(title == f"# KohdaLab TRKR {version}", "docs/release-notes.md version mismatch")
    require(re.search(r"^## " + re.escape(version) + r"(?=\s|$)",
                      files["CHANGELOG.md"].decode(), re.MULTILINE) is not None,
            "CHANGELOG.md version heading missing")
    return version


def check_absent(tag: str) -> None:
    for endpoint, label in ((f"git/ref/tags/{quote(tag, safe='')}", "tag"),
                            (f"releases/tags/{quote(tag, safe='')}", "Release")):
        try:
            api(endpoint)
        except GitHubError as error:
            if error.status != 404:
                raise
        else:
            raise ValueError(f"{label} {tag} already exists")


def successful(run: dict, name: str) -> bool:
    return (run.get("name") == name and run.get("status") == "completed"
            and run.get("conclusion") == "success")


def workflow_path(run: dict) -> str:
    # Actions may append @refs/pull/N/merge or @refs/heads/main to path.
    return run.get("path", "").split("@", 1)[0]


def check_ci(head: str, branch: str, event: str) -> tuple[dict, dict]:
    candidates = [candidate for page in pages(f"actions/runs?head_sha={head}&per_page=100")
                  for candidate in page["workflow_runs"]]
    checks = []
    for name, path in (("Test", ".github/workflows/test.yml"),
                       ("Documentation", ".github/workflows/docs.yml")):
        matches = [candidate for candidate in candidates
                   if candidate.get("name") == name and candidate.get("head_sha") == head
                   and workflow_path(candidate) == path and candidate.get("event") == event
                   and candidate.get("repository", {}).get("full_name") == REPOSITORY
                   and candidate.get("head_branch") == branch]
        require(bool(matches), f"No {name} run for {event} on {branch} at {head}")
        latest = max(matches, key=lambda candidate: (candidate["id"],
                                                     candidate.get("run_attempt", 1)))
        require(successful(latest, name),
                f"Latest {name} run {latest['id']} for {event} on {branch} is not successful")
        checks.append(latest)
    return checks[0], checks[1]


def check_run(run_id: str, commit: str) -> tuple[dict, dict | None, dict, dict]:
    run = api(f"actions/runs/{run_id}")
    require(str(run["id"]) == run_id and successful(run, WORKFLOW),
            f"run {run_id} must be a successful {WORKFLOW} run")
    require(run.get("repository", {}).get("full_name") == REPOSITORY,
            "Build run belongs to another repository")
    require(workflow_path(run) == ".github/workflows/release.yml",
            "Build run has a different workflow path")
    kind = run.get("event")
    require(kind in ("pull_request", "workflow_dispatch"), "Unsupported build event")
    if kind == "workflow_dispatch":
        require(run.get("head_branch") == "main" and run.get("head_sha") == commit,
                "Manual build must target the release commit on main")
        test_run, docs_run = check_ci(commit, "main", "push")
        return run, None, test_run, docs_run

    # A run's pull_requests list is empty once the PR is merged; resolve the PR
    # from the head commit instead.
    related = [item for page in pages(f"commits/{run['head_sha']}/pulls?per_page=100")
               for item in page
               if item.get("head", {}).get("sha") == run["head_sha"]
               and item.get("head", {}).get("ref") == run.get("head_branch")
               and item.get("base", {}).get("ref") == "main"
               and item.get("base", {}).get("repo", {}).get("full_name") == REPOSITORY]
    require(len(related) == 1, "Build run must identify exactly one pull request")
    pr = api(f"pulls/{related[0]['number']}")
    require(pr.get("merged") is True, "Pull request is not merged")
    require(pr.get("base", {}).get("ref") == "main"
            and pr["base"].get("repo", {}).get("full_name") == REPOSITORY,
            "Pull request must target this repository's main")
    head = pr["head"]["sha"]
    require(run.get("head_sha") == head and run.get("head_branch") == pr["head"]["ref"],
            "Build run does not match the merged PR head")
    test_run, docs_run = check_ci(head, pr["head"]["ref"], "pull_request")
    return run, pr, test_run, docs_run


def safe_name(name: str) -> bool:
    return (bool(name) and name not in (".", "..") and "/" not in name
            and "\\" not in name and "\n" not in name and "\r" not in name)


def check_artifact(root: Path, platform: str, version: str, run: dict) -> dict:
    dist = root / "dist"
    origin_name = f"BUILD-ORIGIN-{platform}.json"
    sums_name = f"SHA256SUMS-{platform}.txt"
    require((dist / origin_name).is_file(), f"Missing {origin_name}")
    origin = json.loads((dist / origin_name).read_text(encoding="utf-8"))
    require(origin.get("version") == version, f"{platform}: origin version mismatch")
    require(str(origin.get("GITHUB_RUN_ID")) == str(run["id"]),
            f"{platform}: origin run ID mismatch")
    require(origin.get("GITHUB_REPOSITORY") == REPOSITORY
            and origin.get("GITHUB_WORKFLOW") == WORKFLOW
            and origin.get("platform") == platform, f"{platform}: origin identity mismatch")
    require(str(origin.get("GITHUB_RUN_ATTEMPT")) == str(run.get("run_attempt", 1)),
            f"{platform}: origin run attempt mismatch. Use a fresh main build run for both platforms.")
    require(re.fullmatch(r"[0-9a-f]{40}", origin.get("GITHUB_SHA", "")) is not None,
            f"{platform}: invalid origin SHA")
    listed = origin.get("files")
    expected = {f"kohdalab-trkr-{version}-{platform}-source.zip",
                f"kohdalab-trkr-{version}-{platform}.zip",
                (f"KohdaLab-TRKR-{version}-Setup.exe" if platform == "windows-x64"
                 else f"KohdaLab-TRKR-{version}-{platform}.dmg")}
    require(isinstance(listed, dict) and set(listed) == expected,
            f"{platform}: origin files do not match expected packages")
    require((dist / sums_name).is_file(), f"Missing {sums_name}")
    sums = {}
    for line in (dist / sums_name).read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-fA-F]{64})  (.+)", line)
        require(match is not None, f"{platform}: invalid checksum line")
        checksum, name = match.groups()
        require(safe_name(name) and name not in sums, f"{platform}: invalid/duplicate checksum name")
        sums[name] = checksum.lower()
    require(set(sums) == expected | {origin_name}, f"{platform}: checksum manifest coverage mismatch")
    require({p.name for p in dist.iterdir()} == expected | {origin_name, sums_name},
            f"{platform}: unexpected or missing distribution files")
    for name, checksum in sums.items():
        path = dist / name
        require(path.is_file() and digest(path) == checksum, f"{platform}: hash mismatch: {name}")
    for name, checksum in listed.items():
        require(sums[name] == checksum, f"{platform}: origin hash mismatch: {name}")
    return origin


def download_artifacts(run: dict, version: str, downloads: Path) -> dict[str, dict]:
    artifacts = [artifact for page in pages(f"actions/runs/{run['id']}/artifacts?per_page=100")
                 for artifact in page["artifacts"]]
    origins = {}
    for directory, platform in PLATFORMS.items():
        prefix = f"release-{platform}-"
        matches = [artifact for artifact in artifacts if artifact["name"].startswith(prefix)]
        require(len(matches) == 1 and not matches[0].get("expired", True),
                f"Missing, expired or ambiguous artifact: {prefix}<sha>. "
                "Use a fresh main build run for both platforms.")
        name = matches[0]["name"]
        root = downloads / directory
        github("run", "download", str(run["id"]), "--repo", REPOSITORY,
               "--name", name, "--dir", str(root))
        require(not any(p.is_symlink() for p in root.rglob("*")),
                f"{platform}: artifact contains a symlink")
        origins[platform] = check_artifact(root, platform, version, run)
        origin = origins[platform]
        require(name == prefix + origin["GITHUB_SHA"], f"{platform}: artifact SHA mismatch")
    require(len({origin["GITHUB_SHA"] for origin in origins.values()}) == 1,
            "Platform origins have different build SHAs")
    return origins


def source_matches(archived: bytes, committed: bytes) -> bool:
    if archived == committed:
        return True
    if b"\0" in archived or b"\0" in committed:
        return False
    try:
        archived_text = archived.decode("utf-8")
        committed_text = committed.decode("utf-8")
    except UnicodeDecodeError:
        return False
    return archived_text.replace("\r\n", "\n") == committed_text.replace("\r\n", "\n")


def check_sources(downloads: Path, version: str, files: dict[str, bytes]) -> dict:
    expected = {name for name in files if is_application_source(PurePosixPath(name))}
    comparisons = {}
    for directory, platform in PLATFORMS.items():
        name = f"kohdalab-trkr-{version}-{platform}-source.zip"
        with zipfile.ZipFile(downloads / directory / "dist" / name) as archive:
            seen = set()
            for info in archive.infolist():
                path = PurePosixPath(info.filename)
                require(not path.is_absolute() and ".." not in path.parts
                        and "\\" not in info.filename, f"Unsafe source path: {info.filename}")
                if info.is_dir():
                    continue
                require(info.filename not in seen, f"Duplicate source file: {info.filename}")
                seen.add(info.filename)
                require(info.filename in expected, f"Extra source file: {info.filename}")
                require(source_matches(archive.read(info), files[info.filename]),
                        f"Source content mismatch: {info.filename}")
            require(bool(seen), f"Empty source archive: {name}")
            missing = expected - seen
            require(not missing, f"Missing source file in {name}: {min(missing, default='')}")
            comparisons[name] = {"files_compared": len(seen), "identical": True,
                                 "normalization": "CRLF to LF for UTF-8 text only"}
    return comparisons


def check_tree(origins: dict, release_tree: str, commit: str, run: dict, pr: dict | None) -> str:
    build_sha = next(iter(origins.values()))["GITHUB_SHA"]
    if run["event"] == "workflow_dispatch":
        require(build_sha == commit, "Manual build origin SHA is not the release commit")
        expected_ref = "main"
    else:
        expected_ref = f"{pr['number']}/merge"
    require(all(origin.get("GITHUB_REF_NAME") == expected_ref for origin in origins.values()),
            "Origin ref does not match build event/PR")
    try:
        build_tree = api(f"git/commits/{build_sha}")["tree"]["sha"]
    except GitHubError as error:
        if run["event"] != "pull_request" or error.status not in (404, 422):
            raise
        raise ValueError("PR build tree is unavailable. Use a fresh main build run.") from error
    target_tree = api(f"git/commits/{commit}")["tree"]["sha"]
    require(target_tree == release_tree, "API release tree differs from local release tree")
    require(build_tree == target_tree, "Build/release tree mismatch")
    return "api-identical"


def hardware_evidence(path: str | None, version: str,
                      files: dict[str, bytes]) -> dict[str, int] | None:
    if path is None:
        return None
    relative = PurePosixPath(path)
    require(not relative.is_absolute() and ".." not in relative.parts
            and "\\" not in path, "Hardware evidence must be a repository-relative path without '..'")
    require(path in files, f"Hardware evidence is not in the release commit tree: {path}")
    version_pattern = r"(?<![\w.+-])v?" + re.escape(version) + r"(?![\w.+-])"
    section = []
    level = None
    found = False
    fence = None
    # Include subsections, stopping at a heading of the same or higher level.
    # Aggregate repeated version/setup sections so a recorded fail is retained.
    for line in files[path].decode("utf-8").splitlines():
        if fence is not None:
            if re.fullmatch(r" {0,3}" + re.escape(fence[0]) + "{" + str(len(fence)) + r",}\s*", line):
                fence = None
            continue
        opening = re.match(r"^ {0,3}(`{3,}|~{3,})", line)
        if opening:
            fence = opening[1]
            section.append("")
            continue
        if line.startswith(("    ", "\t")):
            section.append("")
            continue
        heading = re.match(r"^\s{0,3}(#{1,6})\s+(.+)$", line)
        if heading:
            if level is not None and len(heading[1]) <= level:
                level = None
            if level is None and re.search(version_pattern, heading[2]):
                level = len(heading[1])
                found = True
            section.append("")
        elif level is not None:
            section.append(line)
    require(found,
            f"Hardware evidence has no section for version {version}: {path}")
    results = {"pass": 0, "fail": 0, "not_checked": 0}
    previous = []
    in_table = False
    for line in section:
        cells = re.split(r"(?<!\\)\|", line.strip()) if "|" in line else []
        if cells and not cells[0].strip():
            cells.pop(0)
        if cells and not cells[-1].strip():
            cells.pop()
        cells = [cell.strip() for cell in cells]
        if len(cells) < 2:
            previous, in_table = [], False
            continue
        if (len(cells) == len(previous)
                and all(re.fullmatch(r":?-{3,}:?", cell) for cell in cells)):
            in_table = True
        elif in_table:
            result = cells[1].lower()
            if result in ("pass", "fail", "not checked"):
                results[result.replace(" ", "_")] += 1
        previous = cells
    return results


def verify(args: argparse.Namespace) -> dict:
    require(re.fullmatch(r"[0-9a-fA-F]{40}", args.commit) is not None,
            "--commit must be a full 40-character commit SHA")
    require(args.run.isdecimal() and int(args.run) > 0, "--run must be a positive run ID")
    require(not args.output.exists(), "--output already exists; use a new directory")
    commit = git("rev-parse", "--verify", args.commit + "^{commit}").decode().strip()
    git("merge-base", "--is-ancestor", commit, "origin/main")
    files = tree_files(commit)
    version = check_versions(files, args.tag)
    hardware_results = hardware_evidence(args.hardware_evidence, version, files)
    if not args.allow_existing_release:
        check_absent(args.tag)
    run, pr, test_run, docs_run = check_run(args.run, commit)
    source_tree = git("rev-parse", commit + "^{tree}").decode().strip()
    # Download and verify in scratch space; failed checks never leave output assets.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="promote-release-", dir=args.output.parent) as temporary:
        scratch = Path(temporary)
        downloads = scratch / "downloads"
        origins = download_artifacts(run, version, downloads)
        tree_check = check_tree(origins, source_tree, commit, run, pr)
        comparison = check_sources(downloads, version, files)
        assets = scratch / "assets"
        prepare(downloads, assets)
        report = {
            "schema": 2, "application": "trkr", "version": version,
            "tag": args.tag, "release_commit": commit,
            "tested_pr_head": pr["head"]["sha"] if pr else None,
            "source_tree": source_tree, "ci_run": int(run["id"]),
            "ci_run_url": run["html_url"],
            "pr": int(pr["number"]) if pr else None,
            "pr_url": pr["html_url"] if pr else None,
            "hardware_tested": (hardware_results is not None
                                and hardware_results["pass"] > 0 and hardware_results["fail"] == 0),
            "build_kind": run["event"], "tree_check": tree_check,
            "test_run": int(test_run["id"]),
            "test_run_url": test_run["html_url"],
            "docs_run": int(docs_run["id"]),
            "docs_run_url": docs_run["html_url"],
            "source_comparison": comparison,
            "artifact_verification": origins,
        }
        if hardware_results is not None:
            report["hardware_evidence"] = args.hardware_evidence
            report["hardware_results"] = hardware_results
        (assets / "RELEASE-VERIFICATION.json").write_text(
            json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")
        files_by_name = sorted(assets.iterdir(), key=lambda path: path.name)
        (assets / "SHA256SUMS.txt").write_text(
            "".join(f"{digest(path)}  {path.name}\n" for path in files_by_name),
            encoding="utf-8", newline="\n")
        assets.rename(args.output)
    return report


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    command = commands.add_parser("verify", help="Read-only verification; never creates tags or Releases")
    command.add_argument("--run", required=True, help="Build release artifacts run ID")
    command.add_argument("--commit", required=True, help="Full release commit SHA in origin/main history")
    command.add_argument("--tag", required=True, help="v followed by the committed workspace version")
    command.add_argument("--output", required=True, type=Path, help="New directory for verified release assets")
    command.add_argument("--hardware-evidence",
                         help="Repository-relative document in the release commit with version results")
    command.add_argument("--allow-existing-release", action="store_true",
                         help="Skip only tag/Release existence checks for read-only reproduction")
    args = parser.parse_args(argv)
    try:
        report = verify(args)
    except (ValueError, RuntimeError, OSError, KeyError, TypeError, zipfile.BadZipFile) as error:
        print(f"Verification failed: {error}", file=sys.stderr)
        return 1
    print(f"Verified {report['tag']}: {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
