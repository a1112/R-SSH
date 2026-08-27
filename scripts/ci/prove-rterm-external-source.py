#!/usr/bin/env python3
"""Prove that the R-Term packages can be consumed from one immutable Git SHA.

The proof intentionally uses real disposable Git repositories and Cargo commands.  It
has two modes: ``--synthesize`` builds a small pre-R1 candidate from the contract-owned
paths, while ``--candidate-repo`` verifies an already extracted candidate without
changing its worktree.  All successful evidence is written atomically and disposable
checkouts are removed before the process exits.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
import zlib
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Iterable


FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
FRAGMENT_SCHEMA = "rssh.stage7-artifact-manifest-fragment/v1"
RESULT_SCHEMA = "rssh.stage7.result/v1"
GIT_STORE_SCHEMA = "rssh.stage7.git-object-store-proof/v1"
REPLAYABLE_BARE_SCHEMA = "rssh.stage7.replayable-bare-repository/v1"
PACKAGE_COUNT = 7
COMMAND_OUTPUT_LIMIT = 8 * 1024 * 1024
MAX_OBJECT_BYTES = 16 * 1024 * 1024
MAX_OBJECT_COUNT = 65_536


class ProofError(RuntimeError):
    """A fail-closed proof input or command error."""


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_relative(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value or "\x00" in value:
        raise ProofError(f"{label} must be a contained POSIX relative path")
    posix = PurePosixPath(value)
    windows = PureWindowsPath(value)
    if (
        posix.is_absolute()
        or windows.is_absolute()
        or windows.drive
        or any(part in {"", ".", ".."} for part in posix.parts)
    ):
        raise ProofError(f"{label} must be contained beneath the repository")
    return posix.as_posix()


def ensure_contained(root: Path, relative: str, label: str) -> Path:
    normalized = safe_relative(relative, label)
    path = (root / PurePosixPath(normalized)).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError as error:
        raise ProofError(f"{label} escapes the repository") from error
    return path


def run_command(
    arguments: list[str],
    cwd: Path,
    *,
    environment: dict[str, str] | None = None,
    phase: str,
    allow_failure: bool = False,
) -> dict[str, Any]:
    started = time.perf_counter()
    try:
        result = subprocess.run(
            arguments,
            cwd=cwd,
            env=environment,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as error:
        if not allow_failure:
            raise ProofError(f"command could not start: {' '.join(arguments)}: {error}") from error
        return {
            "phase": phase,
            "argv": arguments,
            "cwd": str(cwd),
            "returncode": -1,
            "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
            "stdout": "",
            "stderr": str(error),
        }
    record = {
        "phase": phase,
        "argv": arguments,
        "cwd": str(cwd),
        "returncode": result.returncode,
        "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
        "stdout": result.stdout[-COMMAND_OUTPUT_LIMIT:],
        "stderr": result.stderr[-COMMAND_OUTPUT_LIMIT:],
    }
    if result.returncode != 0 and not allow_failure:
        detail = (result.stderr or result.stdout).strip()
        raise ProofError(f"command failed ({result.returncode}): {' '.join(arguments)}: {detail[-2000:]}")
    return record


def cargo_environment(work: Path) -> dict[str, str]:
    environment = os.environ.copy()
    environment["CARGO_TERM_COLOR"] = "never"
    environment["CARGO_TARGET_DIR"] = str(work / "cargo-target")
    return environment


def git_command(
    repository: Path,
    *arguments: str,
    allow_failure: bool = False,
) -> subprocess.CompletedProcess[str]:
    environment = os.environ.copy()
    environment.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_NO_REPLACE_OBJECTS": "1",
        }
    )
    result = subprocess.run(
        ["git", "-C", str(repository), *arguments],
        text=True,
        capture_output=True,
        check=False,
        env=environment,
    )
    if result.returncode != 0 and not allow_failure:
        detail = (result.stderr or result.stdout).strip()
        raise ProofError(f"git command failed: git -C {repository} {' '.join(arguments)}: {detail[-2000:]}")
    return result


def git_value(repository: Path, *arguments: str) -> str:
    result = git_command(repository, *arguments)
    value = result.stdout.strip()
    if not value:
        raise ProofError(f"git command returned no value: {' '.join(arguments)}")
    return value


def git_status(repository: Path) -> list[str]:
    result = git_command(repository, "status", "--porcelain", "--untracked-files=all")
    return [line for line in result.stdout.splitlines() if line]


def require_clean(repository: Path, label: str) -> None:
    status = git_status(repository)
    if status:
        raise ProofError(f"{label} is dirty: {status[:8]}")


def clone_at(source: Path, destination: Path, commit: str) -> None:
    if destination.exists():
        raise ProofError(f"refusing to reuse disposable checkout: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
        }
    )
    result = subprocess.run(
        ["git", "clone", "--no-local", "--no-checkout", "--quiet", str(source), str(destination)],
        text=True,
        capture_output=True,
        check=False,
        env=environment,
    )
    if result.returncode != 0:
        raise ProofError(f"clean clone failed: {result.stderr.strip()[-2000:]}")
    git_command(destination, "checkout", "--detach", "--quiet", commit)
    git_command(destination, "config", "user.name", "Stage 8 Proof")
    git_command(destination, "config", "user.email", "stage8-proof@example.invalid")
    require_clean(destination, f"clone {destination}")


def init_bare(path: Path) -> None:
    if path.exists():
        raise ProofError(f"refusing existing bare repository: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    environment = os.environ.copy()
    environment.update(
        {
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
        }
    )
    result = subprocess.run(
        ["git", "init", "--bare", "--quiet", str(path)],
        text=True,
        capture_output=True,
        check=False,
        env=environment,
    )
    if result.returncode != 0:
        raise ProofError(f"bare repository initialization failed: {result.stderr.strip()}")


def push(repository: Path, remote: Path, source_ref: str, destination_ref: str) -> None:
    result = git_command(
        repository,
        "push",
        "--quiet",
        str(remote),
        f"{source_ref}:refs/heads/{destination_ref}",
    )
    if result.returncode != 0:
        raise ProofError(f"push failed for {destination_ref}")


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    os.replace(temporary, path)


def remove_tree(path: Path) -> None:
    if not path.exists():
        return

    def retry(function: Any, failed_path: str, _error: Any) -> None:
        os.chmod(failed_path, stat.S_IWRITE)
        function(failed_path)

    shutil.rmtree(path, onerror=retry)


def load_contract(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ProofError(f"cannot read contract: {path}: {error}") from error
    if not isinstance(value, dict) or value.get("schema") != "rssh.stage7/rterm-external-source-proof/v1":
        raise ProofError("contract schema must be rssh.stage7/rterm-external-source-proof/v1")
    return value


def command_list(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        raise ProofError(f"{label} must be a non-empty command array")
    return list(value)


def contract_config(contract: dict[str, Any]) -> dict[str, Any]:
    candidate = contract.get("candidate")
    consumer = contract.get("consumer")
    if not isinstance(candidate, dict) or not isinstance(consumer, dict):
        raise ProofError("contract must contain candidate and consumer objects")
    package_paths = candidate.get("package_paths")
    vendor_paths = candidate.get("vendor_paths")
    if not isinstance(package_paths, list) or len(package_paths) != PACKAGE_COUNT:
        raise ProofError("candidate.package_paths must contain exactly seven package paths")
    if not isinstance(vendor_paths, list) or len(vendor_paths) != 2:
        raise ProofError("candidate.vendor_paths must contain glyphon and gpu-allocator")
    normalized_packages = [safe_relative(item, "candidate package path") for item in package_paths]
    normalized_vendors = [safe_relative(item, "candidate vendor path") for item in vendor_paths]
    if len(set(normalized_packages)) != PACKAGE_COUNT or len(set(normalized_vendors)) != 2:
        raise ProofError("candidate paths must be unique")
    all_paths = normalized_packages + normalized_vendors
    for index, left in enumerate(all_paths):
        for right in all_paths[index + 1 :]:
            if left.startswith(f"{right}/") or right.startswith(f"{left}/"):
                raise ProofError(f"candidate paths overlap: {left}, {right}")

    consumer_path = safe_relative(consumer.get("path"), "consumer path")
    manifest = safe_relative(consumer.get("manifest"), "consumer manifest")
    lockfile = safe_relative(consumer.get("lockfile"), "consumer lockfile")
    vendor_root = safe_relative(consumer.get("vendor_root"), "consumer vendor root")
    dependencies = consumer.get("dependencies")
    if not isinstance(dependencies, list) or len(dependencies) != PACKAGE_COUNT:
        raise ProofError("consumer.dependencies must contain exactly seven package names")
    if not all(isinstance(item, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*", item) for item in dependencies):
        raise ProofError("consumer.dependencies must contain Cargo package names")
    if len(set(dependencies)) != PACKAGE_COUNT:
        raise ProofError("consumer.dependencies must be unique")
    metadata = command_list(contract.get("metadata_command"), "metadata_command")
    if "--locked" not in metadata or "--format-version" not in metadata:
        raise ProofError("metadata_command must be a --locked Cargo metadata command")
    if "generate-lockfile" in metadata:
        raise ProofError("metadata_command cannot generate a lockfile")
    locked = contract.get("locked_commands")
    if not isinstance(locked, list) or not locked:
        raise ProofError("locked_commands must be a non-empty list")
    locked_commands = []
    for index, command in enumerate(locked):
        argv = command_list(command, f"locked_commands[{index}]")
        if "--locked" not in argv:
            raise ProofError(f"locked_commands[{index}] must contain --locked")
        if "generate-lockfile" in argv:
            raise ProofError("cargo generate-lockfile is forbidden after the proof commit")
        locked_commands.append(argv)
    lkg = contract.get("lkg_rssh_ref")
    if not isinstance(lkg, str) or FULL_SHA.fullmatch(lkg) is None:
        raise ProofError("lkg_rssh_ref must be a full lowercase Git SHA")
    return {
        "package_paths": normalized_packages,
        "vendor_paths": normalized_vendors,
        "workspace_files": [
            safe_relative(item, "candidate workspace file")
            for item in candidate.get("workspace_files", ["Cargo.toml", "Cargo.lock"])
        ],
        "consumer_path": consumer_path,
        "consumer_manifest": manifest,
        "consumer_lockfile": lockfile,
        "consumer_vendor_root": vendor_root,
        "dependencies": list(dependencies),
        "metadata_command": metadata,
        "locked_commands": locked_commands,
        "lkg": lkg,
    }


def resolve_source_repository(contract_path: Path, contract: dict[str, Any]) -> Path:
    value = contract.get("source_repository")
    if not isinstance(value, str) or not value:
        raise ProofError("source_repository must be a repository path")
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = contract_path.parent / candidate
    repository = candidate.resolve()
    if not repository.is_dir():
        raise ProofError(f"source repository is missing: {repository}")
    git_value(repository, "rev-parse", "--git-dir")
    return repository


def resolve_commit(repository: Path, reference: str) -> str:
    commit = git_value(repository, "rev-parse", "--verify", f"{reference}^{{commit}}")
    if FULL_SHA.fullmatch(commit) is None:
        raise ProofError(f"reference is not a full commit: {reference}")
    return commit


def ensure_ancestor(repository: Path, ancestor: str, descendant: str, label: str) -> None:
    result = git_command(repository, "merge-base", "--is-ancestor", ancestor, descendant, allow_failure=True)
    if result.returncode != 0:
        raise ProofError(f"{label} is not an ancestor of {descendant}")


def copy_path(source: Path, destination: Path) -> None:
    if source.is_symlink():
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.symlink_to(os.readlink(source), target_is_directory=source.is_dir())
    elif source.is_dir():
        shutil.copytree(source, destination, symlinks=True)
    elif source.is_file():
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    else:
        raise ProofError(f"cannot copy unsupported candidate path: {source}")


def synthetic_workspace_manifest(package_paths: list[str], vendor_paths: list[str]) -> str:
    members = "\n".join(f'    "{path}",' for path in package_paths)
    glyphon = vendor_paths[0]
    allocator = vendor_paths[1]
    return (
        "[workspace]\n"
        "resolver = \"3\"\n"
        "members = [\n"
        f"{members}\n"
        "]\n\n"
        "[workspace.package]\n"
        "edition = \"2024\"\n"
        "license = \"MIT\"\n"
        "repository = \"https://example.invalid/rterm\"\n"
        "rust-version = \"1.89\"\n"
        "version = \"0.1.0\"\n\n"
        "[workspace.lints.rust]\n"
        "unsafe_code = \"forbid\"\n\n"
        "[workspace.lints.clippy]\n"
        "all = \"warn\"\n"
        "pedantic = \"warn\"\n\n"
        "[patch.crates-io]\n"
        f"glyphon = {{ path = \"{glyphon}\" }}\n"
        f"gpu-allocator = {{ path = \"{allocator}\" }}\n"
    )


def synthesize_candidate(
    source_repository: Path,
    source_head: str,
    lkg: str,
    config: dict[str, Any],
    work: Path,
    commands: list[dict[str, Any]],
) -> tuple[Path, str]:
    source_snapshot = work / "source-snapshot"
    clone_at(source_repository, source_snapshot, source_head)
    candidate = work / "candidate"
    clone_at(source_repository, candidate, lkg)
    for child in list(candidate.iterdir()):
        if child.name == ".git":
            continue
        if child.is_dir() and not child.is_symlink():
            remove_tree(child)
        else:
            child.unlink()
    for relative in config["package_paths"] + config["vendor_paths"]:
        source = ensure_contained(source_snapshot, relative, "synthesized source path")
        if not source.exists():
            raise ProofError(f"synthesized source path is missing: {relative}")
        copy_path(source, candidate / PurePosixPath(relative))
    root_manifest = candidate / "Cargo.toml"
    root_manifest.write_text(
        synthetic_workspace_manifest(config["package_paths"], config["vendor_paths"]),
        encoding="utf-8",
    )
    source_toolchain = source_snapshot / "rust-toolchain.toml"
    if source_toolchain.is_file():
        shutil.copy2(source_toolchain, candidate / "rust-toolchain.toml")
    else:
        (candidate / "rust-toolchain.toml").write_text(
            "[toolchain]\nchannel = \"stable\"\n", encoding="utf-8"
        )
    commands.append(
        run_command(
            ["cargo", "generate-lockfile"],
            candidate,
            environment=cargo_environment(work),
            phase="candidate-pre-commit",
        )
    )
    candidate_sha = git_value(candidate, "rev-parse", "HEAD")
    # The synthetic tree is a new child of the frozen R-SSH LKG.  This keeps the
    # local topology proof independently replayable and makes the boundary explicit.
    git_command(candidate, "add", "--all")
    git_command(candidate, "commit", "--quiet", "-m", "stage8 synthesized R-Term candidate")
    candidate_sha = git_value(candidate, "rev-parse", "HEAD")
    if FULL_SHA.fullmatch(candidate_sha) is None:
        raise ProofError("synthesized candidate commit is not a full SHA")
    return candidate, candidate_sha


def validate_candidate_paths(candidate: Path, config: dict[str, Any]) -> None:
    for relative in config["package_paths"] + config["vendor_paths"]:
        path = ensure_contained(candidate, relative, "candidate path")
        if not path.is_dir() or not (path / "Cargo.toml").is_file():
            raise ProofError(f"candidate path is not a Cargo package/vendor tree: {relative}")
    lock = candidate / "Cargo.lock"
    if not lock.is_file() or git_command(candidate, "ls-files", "--error-unmatch", "Cargo.lock", allow_failure=True).returncode != 0:
        raise ProofError("candidate Cargo.lock must be committed")


def candidate_from_mode(
    *,
    synthesize: bool,
    candidate_repository: Path | None,
    candidate_ref: str | None,
    source_repository: Path,
    source_head: str,
    config: dict[str, Any],
    work: Path,
    commands: list[dict[str, Any]],
) -> tuple[Path, str, str]:
    if synthesize:
        candidate, candidate_sha = synthesize_candidate(
            source_repository, source_head, config["lkg"], config, work, commands
        )
        validate_candidate_paths(candidate, config)
        return candidate, candidate_sha, "synthesize"
    if candidate_repository is None or candidate_ref is None:
        raise ProofError("canonical mode requires candidate repository and candidate ref")
    require_clean(candidate_repository, "candidate repository")
    head = resolve_commit(candidate_repository, "HEAD")
    if head != candidate_ref:
        raise ProofError(f"candidate HEAD {head} does not equal requested SHA {candidate_ref}")
    candidate = work / "candidate"
    clone_at(candidate_repository, candidate, candidate_ref)
    validate_candidate_paths(candidate, config)
    return candidate, candidate_ref, "canonical"


DEPENDENCY_BLOCK = re.compile(
    r"(?ms)^(?P<indent>[ \t]*)(?P<name>[A-Za-z0-9][A-Za-z0-9_-]*)[ \t]*=[ \t]*\{(?P<body>[^{}]*)\}"
)


def toml_value(body: str, key: str) -> str | None:
    match = re.search(rf"(?m)\b{re.escape(key)}[ \t]*=[ \t]*\"([^\"]*)\"", body)
    return match.group(1) if match else None


def switch_consumer_sources(
    manifest: Path,
    package_names: list[str],
    candidate_url: str,
    candidate_sha: str,
) -> None:
    original = manifest.read_text(encoding="utf-8")
    found: dict[str, int] = {}

    def replace(match: re.Match[str]) -> str:
        name = match.group("name")
        body = match.group("body")
        package = toml_value(body, "package") or name
        if package not in package_names:
            return match.group(0)
        if re.search(r"(?m)\bfile\s*=|\bsource\s*=\s*\"file:", body):
            raise ProofError(f"consumer dependency {package} contains a path+file source")
        if "path" not in body:
            raise ProofError(f"consumer dependency {package} is not a path source")
        if re.search(r"(?m)\bgit\s*=|\brev\s*=", body):
            raise ProofError(f"consumer dependency {package} has multiple source kinds")
        replaced = re.sub(r"(?m)\bpath\s*=\s*\"[^\"]*\"\s*,?", "", body)
        replaced = f' git = "{candidate_url}", rev = "{candidate_sha}",' + replaced
        found[package] = found.get(package, 0) + 1
        return f"{match.group('indent')}{name} = {{{replaced}}}"

    switched = DEPENDENCY_BLOCK.sub(replace, original)
    missing = [name for name in package_names if found.get(name) != 1]
    if missing:
        raise ProofError(f"consumer manifest did not contain exactly one path dependency for: {missing}")
    if switched == original:
        raise ProofError("consumer manifest source switch made no changes")
    manifest.write_text(switched, encoding="utf-8")
    # A second closed-source scan catches multiline and unusual formatting that the
    # replacement expression could not recognize.
    changed = manifest.read_text(encoding="utf-8")
    for match in DEPENDENCY_BLOCK.finditer(changed):
        package = toml_value(match.group("body"), "package") or match.group("name")
        if package not in package_names:
            continue
        body = match.group("body")
        if "path" in body or re.search(r"(?m)\bfile\s*=|\bsource\s*=\s*\"file:", body):
            raise ProofError(f"consumer dependency {package} retained a path+file source")
        if toml_value(body, "git") != candidate_url or toml_value(body, "rev") != candidate_sha:
            raise ProofError(f"consumer dependency {package} did not bind the candidate SHA")


def status_paths(repository: Path) -> list[str]:
    return [line[3:] if len(line) >= 3 else line for line in git_status(repository)]


def metadata_summary(
    metadata: dict[str, Any],
    package_names: list[str],
    candidate_url: str,
    candidate_sha: str,
    vendor_root: Path,
) -> dict[str, Any]:
    packages = metadata.get("packages")
    if not isinstance(packages, list):
        raise ProofError("cargo metadata did not return packages")
    by_name: dict[str, list[dict[str, Any]]] = {}
    for package in packages:
        if isinstance(package, dict) and isinstance(package.get("name"), str):
            by_name.setdefault(package["name"], []).append(package)
    missing = [name for name in package_names if name not in by_name]
    if missing:
        raise ProofError(f"cargo metadata omitted R-Term packages: {missing}")
    rterm_sources: dict[str, str] = {}
    package_summaries: list[dict[str, Any]] = []
    expected_fragment = f"#{candidate_sha}"
    for name in package_names:
        records = by_name[name]
        if len(records) != 1:
            raise ProofError(f"cargo metadata returned duplicate package {name}")
        package = records[0]
        source = package.get("source")
        if not isinstance(source, str) or not source.startswith("git+") or expected_fragment not in source:
            raise ProofError(f"package {name} does not use the immutable candidate Git SHA")
        if candidate_url not in source:
            raise ProofError(f"package {name} does not use the candidate bare repository")
        rterm_sources[name] = source
        package_summaries.append(
            {
                "name": name,
                "version": package.get("version"),
                "source": source,
                "manifest_path": package.get("manifest_path"),
                "id": package.get("id"),
            }
        )
    vendor_resolutions: dict[str, dict[str, Any]] = {}
    for vendor_name in ("glyphon", "gpu-allocator"):
        records = by_name.get(vendor_name, [])
        if len(records) != 1:
            raise ProofError(f"cargo metadata did not resolve exactly one vendor package {vendor_name}")
        manifest_path = records[0].get("manifest_path")
        if not isinstance(manifest_path, str):
            raise ProofError(f"vendor package {vendor_name} has no manifest path")
        resolved = Path(manifest_path).resolve()
        try:
            relative = resolved.relative_to(vendor_root.resolve()).as_posix()
        except ValueError as error:
            raise ProofError(f"vendor package {vendor_name} resolved outside the consumer vendor root") from error
        vendor_resolutions[vendor_name] = {
            "manifest_path": str(resolved),
            "relative_path": relative,
            "source": records[0].get("source"),
        }
    nodes = metadata.get("resolve", {}).get("nodes", []) if isinstance(metadata.get("resolve"), dict) else []
    target_ids = {item.get("id") for item in package_summaries if isinstance(item.get("id"), str)}
    for node in nodes:
        if not isinstance(node, dict) or node.get("id") not in target_ids:
            continue
        if candidate_sha not in node["id"]:
            raise ProofError(f"cargo resolve nodes contain a mutable or mismatched R-Term source: {node['id']}")
    return {
        "packages": package_summaries,
        "rterm_sources": rterm_sources,
        "vendor_resolutions": vendor_resolutions,
        "metadata_schema": metadata.get("version"),
        "metadata_package_count": len(packages),
        "metadata_sha256": canonical_sha256(metadata),
    }


def parse_metadata_output(record: dict[str, Any]) -> dict[str, Any]:
    output = record.get("stdout", "")
    try:
        return json.loads(output)
    except json.JSONDecodeError as error:
        raise ProofError(f"cargo metadata output is not JSON: {error}") from error


def loose_object(raw: bytes, object_type: str) -> bytes:
    return zlib.compress(f"{object_type} {len(raw)}\0".encode("ascii") + raw)


def parse_tree(raw: bytes) -> list[tuple[str, str, str]]:
    records: list[tuple[str, str, str]] = []
    cursor = 0
    while cursor < len(raw):
        space = raw.find(b" ", cursor)
        nul = raw.find(b"\0", space + 1)
        if space <= cursor or nul < 0 or nul + 21 > len(raw):
            raise ProofError("malformed Git tree object while building proof")
        mode = raw[cursor:space].decode("ascii")
        name = raw[space + 1 : nul].decode("utf-8")
        oid = raw[nul + 1 : nul + 21].hex()
        object_type = "tree" if mode == "40000" else "blob"
        records.append((mode, name, oid if FULL_SHA.fullmatch(oid) else ""))
        cursor = nul + 21
        if not oid:
            raise ProofError("malformed Git tree object OID")
        # The type is derived from mode at traversal time.
        records[-1] = (mode, name, oid)
    return records


class GitClosure:
    def __init__(self, repository: Path) -> None:
        self.repository = repository
        self.objects: dict[str, tuple[str, bytes]] = {}

    def read_object(self, object_type: str, oid: str) -> bytes:
        if not FULL_SHA.fullmatch(oid):
            raise ProofError(f"invalid Git object ID {oid}")
        cached = self.objects.get(oid)
        if cached is not None:
            if cached[0] != object_type:
                raise ProofError(f"Git object type changed for {oid}")
            return cached[1]
        environment = os.environ.copy()
        environment.update(
            {
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": os.devnull,
                "GIT_NO_REPLACE_OBJECTS": "1",
            }
        )
        result = subprocess.run(
            ["git", "-C", str(self.repository), "cat-file", object_type, oid],
            capture_output=True,
            check=False,
            env=environment,
        )
        if result.returncode != 0:
            raise ProofError(f"cannot read Git object {oid}: {result.stderr.decode(errors='replace').strip()}")
        raw = result.stdout
        if len(raw) > MAX_OBJECT_BYTES:
            raise ProofError(f"Git object exceeds bounded proof size: {oid}")
        self.objects[oid] = (object_type, raw)
        return raw

    def walk(self, refs: dict[str, str], boundaries: list[str]) -> None:
        pending: list[tuple[str, str]] = [(oid, "commit") for oid in refs.values()]
        boundary_set = set(boundaries)
        seen: set[tuple[str, str]] = set()
        while pending:
            oid, object_type = pending.pop()
            if (oid, object_type) in seen:
                continue
            seen.add((oid, object_type))
            raw = self.read_object(object_type, oid)
            if object_type == "commit":
                header = raw.partition(b"\n\n")[0].decode("ascii", errors="strict")
                lines = header.splitlines()
                tree_line = next((line for line in lines if line.startswith("tree ")), "")
                tree_oid = tree_line[5:]
                if not FULL_SHA.fullmatch(tree_oid):
                    raise ProofError(f"commit {oid} has no readable tree")
                pending.append((tree_oid, "tree"))
                if oid not in boundary_set:
                    pending.extend((line[7:], "commit") for line in lines if line.startswith("parent "))
            elif object_type == "tree":
                for mode, _name, child in parse_tree(raw):
                    pending.append((child, "tree" if mode == "40000" else "blob"))
        if len(self.objects) > MAX_OBJECT_COUNT:
            raise ProofError("Git object closure exceeds the bounded object count")

    def proof_repository(
        self,
        role: str,
        refs: dict[str, str],
        boundaries: list[str],
    ) -> dict[str, Any]:
        self.walk(refs, boundaries)
        objects = [
            {
                "oid": oid,
                "object_type": object_type,
                "body_base64": base64.b64encode(raw).decode("ascii"),
            }
            for oid, (object_type, raw) in sorted(self.objects.items())
        ]
        files: list[dict[str, str]] = [
            {
                "path": "HEAD",
                "body_base64": base64.b64encode(
                    f"ref: refs/heads/stage7-proof/{sorted(refs)[0]}\n".encode("ascii")
                ).decode("ascii"),
            },
            {
                "path": "config",
                "body_base64": base64.b64encode(
                    b"[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\tbare = true\n"
                ).decode("ascii"),
            },
        ]
        for name, oid in sorted(refs.items()):
            files.append(
                {
                    "path": f"refs/heads/stage7-proof/{name}",
                    "body_base64": base64.b64encode(f"{oid}\n".encode("ascii")).decode("ascii"),
                }
            )
        for oid, (object_type, raw) in sorted(self.objects.items()):
            files.append(
                {
                    "path": f"objects/{oid[:2]}/{oid[2:]}",
                    "body_base64": base64.b64encode(loose_object(raw, object_type)).decode("ascii"),
                }
            )
        if boundaries:
            files.append(
                {
                    "path": "shallow",
                    "body_base64": base64.b64encode(
                        "".join(f"{item}\n" for item in boundaries).encode("ascii")
                    ).decode("ascii"),
                }
            )
        files.sort(key=lambda item: item["path"])
        replay = {
            "schema": REPLAYABLE_BARE_SCHEMA,
            "files": files,
        }
        replay["snapshot_sha256"] = canonical_sha256(replay)
        repository = {
            "role": role,
            "bare": True,
            "alternates": [],
            "refs": refs,
            "history_boundaries": boundaries,
            "git_objects": objects,
            "bare_repository_snapshot": replay,
        }
        repository["snapshot_sha256"] = canonical_sha256(
            {
                key: repository[key]
                for key in (
                    "bare",
                    "alternates",
                    "history_boundaries",
                    "git_objects",
                    "bare_repository_snapshot",
                )
            }
        )
        return repository


def build_local_object_store_proof(repository: Path, source_ref: str, lkg: str) -> dict[str, Any]:
    first = GitClosure(repository).proof_repository(
        "candidate",
        {"candidate": source_ref, "lkg_boundary": lkg},
        [lkg],
    )
    second = GitClosure(repository).proof_repository("lkg", {"lkg": lkg}, [lkg])
    return {
        "schema": GIT_STORE_SCHEMA,
        "object_format": "sha1",
        "repositories": [first, second],
    }


def bare_identity(path: Path, role: str) -> dict[str, Any]:
    refs_result = subprocess.run(
        ["git", f"--git-dir={path}", "for-each-ref", "--format=%(refname)=%(objectname)"],
        text=True,
        capture_output=True,
        check=False,
    )
    if refs_result.returncode != 0:
        raise ProofError(f"cannot inspect bare repository {path}: {refs_result.stderr.strip()}")
    refs = {
        line.split("=", 1)[0]: line.split("=", 1)[1]
        for line in refs_result.stdout.splitlines()
        if "=" in line
    }
    object_format = subprocess.run(
        ["git", f"--git-dir={path}", "rev-parse", "--show-object-format"],
        text=True,
        capture_output=True,
        check=False,
    ).stdout.strip()
    identity_material = {"role": role, "refs": refs, "object_format": object_format}
    return {
        "role": role,
        "url": path.as_uri(),
        "path": str(path),
        "object_format": object_format,
        "refs": refs,
        "identity": canonical_sha256(identity_material),
    }


def dependency_subject_refs(rssh: Any, rterm: Any) -> dict[str, str]:
    result: dict[str, str] = {}
    for namespace, epoch in (("rssh", rssh), ("rterm", rterm)):
        if not isinstance(epoch, dict):
            continue
        for key, value in epoch.items():
            if re.fullmatch(r"[0-9a-f]{40}", value or ""):
                result[f"{namespace}.{key}"] = value
    return result


def cohort_id(entry: dict[str, Any]) -> str:
    return canonical_sha256(
        {
            "scope": entry.get("scope"),
            "source_sha": entry.get("source_sha"),
            "subject_refs": entry.get("subject_refs"),
            "platform": entry.get("platform"),
            "binary_hashes": entry.get("binary_hashes"),
            "runner_fingerprint_sha256": entry.get("runner_fingerprint_sha256"),
        }
    )


def produce_proof(
    *,
    mode: str,
    source_repository: Path,
    source_ref: str,
    candidate: Path,
    candidate_ref: str,
    config: dict[str, Any],
    contract: dict[str, Any],
    work: Path,
    output: Path,
    commands: list[dict[str, Any]],
    candidate_remote: Path,
    consumer_remote: Path,
    source_switch_ref: str,
    rollback_ref: str,
    baseline_manifest_sha: str,
    baseline_lock_sha: str,
    consumer: Path,
    consumer_root: Path,
    metadata: dict[str, Any],
    metadata_raw: dict[str, Any],
    candidate_tree_sha: str,
    source_switch_tree_sha: str,
    rollback_tree_sha: str,
    source_switch_manifest_sha: str,
    source_switch_lock_sha: str,
    source_switch_command_index: int,
) -> dict[str, Any]:
    state = "attribution-ready" if mode == "synthesize" else contract.get("canonical_fragment_state", "extraction-ready")
    rssh_epoch = contract.get("canonical_rssh_epoch") if mode == "canonical" else None
    rterm_epoch = contract.get("canonical_rterm_epoch") if mode == "canonical" else None
    if state == "attribution-ready":
        rssh_epoch = None
        rterm_epoch = None
    subject_refs = dependency_subject_refs(rssh_epoch, rterm_epoch)
    artifact_type = (
        "local-two-bare-git-source-proof" if mode == "synthesize" else "rterm-external-source-proof"
    )
    run_id = f"stage8-{mode}"
    identity = {"source_sha": source_ref, "platform": "repository", "run_id": run_id}
    candidate_tree_digest = hashlib.sha256(
        GitClosure(candidate).read_object("tree", candidate_tree_sha)
    ).hexdigest()
    payload: dict[str, Any] = {
        "schema": RESULT_SCHEMA,
        "identity": identity,
        "ok": True,
        "proof": artifact_type,
        "claims": (
            {"object_databases_independent": True, "immutable_refs": True}
            if mode == "synthesize"
            else {
                "r1_ref": candidate_ref,
                "two_bare_git_repositories": True,
                "immutable_metadata_sources": True,
            }
        ),
        "immutable": True,
        "mode": mode,
        "candidate_ref": candidate_ref,
        "candidate_tree_sha256": candidate_tree_digest,
        "source_switch_ref": source_switch_ref,
        "rollback_ref": rollback_ref,
        "source_refs": [source_ref, config["lkg"]],
        "candidate_repository": str(candidate),
        "consumer_repository": str(consumer),
        "consumer_root": str(consumer),
        "consumer_workspace": str(consumer_root),
        "consumer_manifest": str(consumer_root / config["consumer_manifest"]),
        "consumer_lockfile": str(consumer_root / config["consumer_lockfile"]),
        "bare_repositories": {
            "candidate": bare_identity(candidate_remote, "rterm-candidate"),
            "consumer": bare_identity(consumer_remote, "rssh-consumer"),
        },
        "baseline": {
            "manifest_sha256": baseline_manifest_sha,
            "lockfile_sha256": baseline_lock_sha,
        },
        "source_switch": {
            "tree_sha256": source_switch_tree_sha,
            "manifest_sha256": source_switch_manifest_sha,
            "lockfile_sha256": source_switch_lock_sha,
        },
        "rollback": {
            "tree_sha256": rollback_tree_sha,
            "manifest_sha256": file_sha256(consumer_root / config["consumer_manifest"]),
            "lockfile_sha256": file_sha256(consumer_root / config["consumer_lockfile"]),
        },
        "source_switch_command_count": source_switch_command_index,
        "commands": commands,
        "metadata": metadata,
        "metadata_sha256": metadata["metadata_sha256"],
        "metadata_raw_sha256": canonical_sha256(metadata_raw),
        "vendor_resolutions": metadata["vendor_resolutions"],
        "worktree_hashes": {
            "candidate_tree": candidate_tree_sha,
            "source_switch_tree": source_switch_tree_sha,
            "rollback_tree": rollback_tree_sha,
        },
        "lockfile_sha256": source_switch_lock_sha,
        "post_commit_cargo_generate_lockfile": False,
        "bare_repository_count": 2,
        "bare_remote_count": 2,
    }
    if mode == "synthesize":
        payload["git_object_store_proof"] = build_local_object_store_proof(
            consumer, source_ref, config["lkg"]
        )
    else:
        payload["r1_ref"] = candidate_ref
        payload["source_to_filtered_map_sha256"] = canonical_sha256(
            {"candidate_ref": candidate_ref, "source_ref": source_ref}
        )
        payload["tree_projection_sha256"] = canonical_sha256(
            {"candidate_tree_sha256": candidate_tree_sha}
        )
        payload["bootstrap_projection_sha256"] = canonical_sha256(
            {"metadata_sha256": metadata["metadata_sha256"], "lockfile_sha256": payload["lockfile_sha256"]}
        )

    artifact_name = f"{artifact_type}.json"
    artifact_path = output / artifact_name
    atomic_json(artifact_path, payload)
    entry: dict[str, Any] = {
        "artifact_type": artifact_type,
        "artifact_id": artifact_type,
        "role": "proof",
        "scope": state,
        "payload_schema": RESULT_SCHEMA,
        "path": artifact_name,
        "sha256": file_sha256(artifact_path),
        "size_bytes": artifact_path.stat().st_size,
        "producing_command": "python scripts/ci/prove-rterm-external-source.py",
        "producing_argv": ["python", "scripts/ci/prove-rterm-external-source.py", "--mode", mode],
        "subject_refs": subject_refs,
        "children": [],
        "source_sha": source_ref,
        "platform": "repository",
        "run_id": run_id,
    }
    entry["cohort_id"] = cohort_id(entry)
    fragment = {
        "schema": FRAGMENT_SCHEMA,
        "requested_state": state,
        "certified_commit": source_ref,
        "epoch_id": canonical_sha256(
            {
                "state": state,
                "certified_commit": source_ref,
                "rssh": rssh_epoch,
                "rterm": rterm_epoch,
            }
        ),
        "rssh": rssh_epoch,
        "rterm": rterm_epoch,
        "entries": [entry],
    }
    atomic_json(output / "artifact-manifest-fragment.json", fragment)
    return {
        "ok": True,
        "mode": mode,
        "artifact_type": artifact_type,
        "candidate_ref": candidate_ref,
        "source_ref": source_ref,
        "source_switch_ref": source_switch_ref,
        "rollback_ref": rollback_ref,
        "fragment": str(output / "artifact-manifest-fragment.json"),
        "artifact": str(artifact_path),
    }


def execute(
    contract_path: Path,
    contract: dict[str, Any],
    *,
    synthesize: bool,
    candidate_repository: Path | None,
    candidate_ref_argument: str | None,
    output: Path,
    keep_on_failure: bool,
) -> dict[str, Any]:
    config = contract_config(contract)
    source_repository = resolve_source_repository(contract_path, contract)
    source_ref = resolve_commit(source_repository, "HEAD")
    if not FULL_SHA.fullmatch(source_ref):
        raise ProofError("source HEAD is not a full SHA")
    resolve_commit(source_repository, config["lkg"])
    ensure_ancestor(source_repository, config["lkg"], source_ref, "frozen R-SSH LKG")
    if candidate_ref_argument is not None:
        if FULL_SHA.fullmatch(candidate_ref_argument) is None:
            raise ProofError("candidate-ref must be a full 40-character lowercase SHA")
    if not synthesize and candidate_repository is None:
        raise ProofError("canonical mode requires --candidate-repo")
    if not synthesize and candidate_ref_argument is None:
        raise ProofError("canonical mode requires --candidate-ref")
    if synthesize and (candidate_repository is not None or candidate_ref_argument is not None):
        raise ProofError("--synthesize and canonical candidate arguments are mutually exclusive")
    if candidate_repository is not None:
        candidate_repository = candidate_repository.resolve()
        if not candidate_repository.is_dir():
            raise ProofError(f"candidate repository is missing: {candidate_repository}")

    output = output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output / "work"
    if work.exists():
        raise ProofError(f"output already contains a disposable work directory: {work}")
    work.mkdir()
    commands: list[dict[str, Any]] = []
    candidate_remote = work / "candidate-remote.git"
    consumer_remote = work / "consumer-remote.git"
    init_bare(candidate_remote)
    init_bare(consumer_remote)
    try:
        candidate, candidate_ref, mode = candidate_from_mode(
            synthesize=synthesize,
            candidate_repository=candidate_repository,
            candidate_ref=candidate_ref_argument,
            source_repository=source_repository,
            source_head=source_ref,
            config=config,
            work=work,
            commands=commands,
        )
        push(candidate, candidate_remote, candidate_ref, "candidate")
        if synthesize:
            push(candidate, candidate_remote, config["lkg"], "lkg")

        consumer = work / "consumer"
        clone_at(source_repository, consumer, source_ref)
        consumer_root = ensure_contained(consumer, config["consumer_path"], "consumer path")
        manifest = ensure_contained(consumer_root, config["consumer_manifest"], "consumer manifest")
        lockfile = ensure_contained(consumer_root, config["consumer_lockfile"], "consumer lockfile")
        if not manifest.is_file() or not lockfile.is_file():
            raise ProofError("consumer manifest and lockfile must exist")
        if git_command(consumer, "ls-files", "--error-unmatch", str(manifest.relative_to(consumer)).replace("\\", "/"), allow_failure=True).returncode != 0:
            raise ProofError("consumer manifest must be committed")
        if git_command(consumer, "ls-files", "--error-unmatch", str(lockfile.relative_to(consumer)).replace("\\", "/"), allow_failure=True).returncode != 0:
            raise ProofError("consumer lockfile must be committed")
        baseline_manifest_sha = file_sha256(manifest)
        baseline_lock_sha = file_sha256(lockfile)
        baseline_ref = git_value(consumer, "rev-parse", "HEAD")
        push(consumer, consumer_remote, baseline_ref, "baseline")
        push(consumer, consumer_remote, config["lkg"], "lkg")

        candidate_url = candidate_remote.as_uri()
        switch_consumer_sources(manifest, config["dependencies"], candidate_url, candidate_ref)
        prepare = run_command(
            ["cargo", "generate-lockfile"],
            consumer_root,
            environment=cargo_environment(work),
            phase="source-switch-pre-commit",
        )
        commands.append(prepare)
        changed = status_paths(consumer)
        expected = sorted(
            [
                str(manifest.relative_to(consumer)).replace("\\", "/"),
                str(lockfile.relative_to(consumer)).replace("\\", "/"),
            ]
        )
        if sorted(changed) != expected:
            raise ProofError(f"source switch changed unexpected consumer files: {changed}")
        git_command(consumer, "add", str(manifest.relative_to(consumer)), str(lockfile.relative_to(consumer)))
        git_command(consumer, "commit", "--quiet", "-m", "stage8 temporary immutable R-Term source switch")
        source_switch_ref = git_value(consumer, "rev-parse", "HEAD")
        push(consumer, consumer_remote, source_switch_ref, "source-switch")
        source_switch_tree_sha = git_value(consumer, "rev-parse", f"{source_switch_ref}^{{tree}}")
        source_switch_manifest_sha = file_sha256(manifest)
        source_switch_lock_sha = file_sha256(lockfile)
        source_switch_command_index = len(commands)

        metadata_record = run_command(
            config["metadata_command"],
            consumer_root,
            environment=cargo_environment(work),
            phase="post-source-switch",
        )
        commands.append(metadata_record)
        metadata_raw = parse_metadata_output(metadata_record)
        metadata = metadata_summary(
            metadata_raw,
            config["dependencies"],
            candidate_url,
            candidate_ref,
            ensure_contained(consumer, config["consumer_vendor_root"], "consumer vendor root"),
        )
        for command in config["locked_commands"]:
            commands.append(
                run_command(
                    command,
                    consumer_root,
                    environment=cargo_environment(work),
                    phase="post-source-switch",
                )
            )

        git_command(consumer, "revert", "--no-edit", source_switch_ref)
        rollback_ref = git_value(consumer, "rev-parse", "HEAD")
        push(consumer, consumer_remote, rollback_ref, "rollback")
        rollback_tree_sha = git_value(consumer, "rev-parse", f"{rollback_ref}^{{tree}}")
        if file_sha256(manifest) != baseline_manifest_sha or file_sha256(lockfile) != baseline_lock_sha:
            raise ProofError("source-switch rollback did not restore committed path sources")
        require_clean(consumer, "consumer rollback worktree")
        diff = git_command(
            consumer,
            "diff",
            "--exit-code",
            baseline_ref,
            "--",
            str(manifest.relative_to(consumer)),
            str(lockfile.relative_to(consumer)),
            allow_failure=True,
        )
        if diff.returncode != 0:
            raise ProofError("source-switch rollback differs from the baseline manifest or lockfile")

        result = produce_proof(
            mode=mode,
            source_repository=source_repository,
            source_ref=source_ref,
            candidate=candidate,
            candidate_ref=candidate_ref,
            config=config,
            contract=contract,
            work=work,
            output=output,
            commands=commands,
            candidate_remote=candidate_remote,
            consumer_remote=consumer_remote,
            source_switch_ref=source_switch_ref,
            rollback_ref=rollback_ref,
            baseline_manifest_sha=baseline_manifest_sha,
            baseline_lock_sha=baseline_lock_sha,
            consumer=consumer,
            consumer_root=consumer_root,
            metadata=metadata,
            metadata_raw=metadata_raw,
            candidate_tree_sha=git_value(candidate, "rev-parse", f"{candidate_ref}^{{tree}}"),
            source_switch_tree_sha=source_switch_tree_sha,
            rollback_tree_sha=rollback_tree_sha,
            source_switch_manifest_sha=source_switch_manifest_sha,
            source_switch_lock_sha=source_switch_lock_sha,
            source_switch_command_index=source_switch_command_index,
        )
        remove_tree(work)
        return result
    except Exception:
        if not keep_on_failure:
            remove_tree(work)
        raise


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--contract", required=True, type=Path)
    value.add_argument("--synthesize", action="store_true")
    value.add_argument("--candidate-repo", type=Path)
    value.add_argument("--candidate-ref")
    value.add_argument("--output", required=True, type=Path)
    value.add_argument("--keep-on-failure", action="store_true")
    return value


def main(argv: list[str] | None = None) -> int:
    arguments = parser().parse_args(argv)
    try:
        if not arguments.synthesize and arguments.candidate_repo is None and arguments.candidate_ref is None:
            raise ProofError("exactly one proof mode is required: --synthesize or canonical candidate arguments")
        if arguments.synthesize and (arguments.candidate_repo is not None or arguments.candidate_ref is not None):
            raise ProofError("proof modes are mutually exclusive")
        if (arguments.candidate_repo is None) != (arguments.candidate_ref is None):
            raise ProofError("canonical mode requires both --candidate-repo and --candidate-ref")
        contract_path = arguments.contract.resolve()
        contract = load_contract(contract_path)
        result = execute(
            contract_path,
            contract,
            synthesize=arguments.synthesize,
            candidate_repository=arguments.candidate_repo,
            candidate_ref_argument=arguments.candidate_ref,
            output=arguments.output,
            keep_on_failure=arguments.keep_on_failure,
        )
        print(json.dumps(result, sort_keys=True))
        return 0
    except (ProofError, OSError, subprocess.SubprocessError) as error:
        failure = {"ok": False, "error": str(error)}
        print(json.dumps(failure, sort_keys=True), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
