#!/usr/bin/env python3
"""Rehearse candidate and rollback R-Term sources in clean consumer clones."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path, PurePosixPath
from typing import Any, Iterable


SHA1 = re.compile(r"^[0-9a-f]{40}$")
FORBIDDEN_PRODUCT_ROOTS = (
    "crates/rssh-app",
    "crates/rssh-config",
    "crates/rssh-core",
    "crates/rssh-diagnostics",
    "crates/rssh-domain",
    "crates/rssh-functional-tests",
    "crates/rssh-native",
    "crates/rssh-pty",
    "crates/rssh-renderer",
    "crates/rssh-ssh",
    "crates/rssh-test-support",
    "crates/rssh-web",
    "tauri",
)


class RehearsalError(RuntimeError):
    pass


def git(repo: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo), *arguments],
        text=True,
        capture_output=True,
        check=False,
    )


def resolve_commit(repo: Path, reference: str) -> str:
    result = git(repo, "rev-parse", "--verify", f"{reference}^{{commit}}")
    commit = result.stdout.strip()
    if result.returncode != 0 or SHA1.fullmatch(commit) is None:
        raise RehearsalError(f"cannot resolve immutable commit for {reference}: {result.stderr.strip()}")
    return commit


def validate_overlay_path(value: str) -> str:
    path = PurePosixPath(value)
    if (
        not value
        or path.is_absolute()
        or value in (".", "..")
        or ".." in path.parts
        or "\\" in value
    ):
        raise RehearsalError(f"refusing overlay path outside repository: {value}")
    normalized = path.as_posix()
    if any(
        normalized == root or normalized.startswith(f"{root}/")
        for root in FORBIDDEN_PRODUCT_ROOTS
    ):
        raise RehearsalError(f"refusing overlay path owned by R-SSH product code: {value}")
    return normalized


def contract_overlay_paths(contract: dict[str, Any]) -> list[str]:
    paths: list[str] = []
    for section in ("packages", "vendor_trees"):
        entries = contract.get(section)
        if not isinstance(entries, list):
            raise RehearsalError(f"{section} must be a list")
        for entry in entries:
            if not isinstance(entry, dict) or not isinstance(entry.get("path"), str):
                raise RehearsalError(f"{section} entries must declare a path")
            normalized = validate_overlay_path(entry["path"])
            if normalized not in paths:
                paths.append(normalized)
    for index, left in enumerate(paths):
        for right in paths[index + 1 :]:
            if left.startswith(f"{right}/") or right.startswith(f"{left}/"):
                raise RehearsalError(f"refusing overlapping overlay paths: {left}, {right}")
    return paths


def command_list(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or not value or not all(isinstance(item, str) and item for item in value):
        raise RehearsalError(f"{label} must be a nonempty argument list")
    return list(value)


def clone_at(source: Path, destination: Path, commit: str) -> None:
    if destination.exists():
        raise RehearsalError(f"refusing existing rehearsal checkout: {destination}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    cloned = subprocess.run(
        ["git", "clone", "--no-local", "--no-checkout", "--quiet", str(source), str(destination)],
        text=True,
        capture_output=True,
        check=False,
    )
    if cloned.returncode != 0:
        raise RehearsalError(f"clean clone failed: {cloned.stderr.strip()}")
    checked_out = git(destination, "checkout", "--detach", "--quiet", commit)
    if checked_out.returncode != 0:
        raise RehearsalError(f"checkout failed for {commit}: {checked_out.stderr.strip()}")


def overlay_paths(source: Path, consumer: Path, paths: Iterable[str]) -> None:
    for relative in paths:
        source_path = source / relative
        destination = consumer / relative
        if not source_path.exists():
            raise RehearsalError(f"overlay source path is missing: {relative}")
        if destination.is_dir():
            shutil.rmtree(destination)
        elif destination.exists():
            destination.unlink()
        destination.parent.mkdir(parents=True, exist_ok=True)
        if source_path.is_dir():
            shutil.copytree(source_path, destination, symlinks=True)
        else:
            shutil.copy2(source_path, destination)


def run_command(
    arguments: list[str], cwd: Path, environment: dict[str, str], kind: str
) -> dict[str, Any]:
    started = time.perf_counter()
    result = subprocess.run(
        arguments,
        cwd=cwd,
        env=environment,
        text=True,
        capture_output=True,
        check=False,
    )
    return {
        "kind": kind,
        "argv": arguments,
        "cwd": str(cwd),
        "returncode": result.returncode,
        "elapsed_ms": round((time.perf_counter() - started) * 1000, 3),
        "stdout": result.stdout[-16_384:],
        "stderr": result.stderr[-16_384:],
    }


def write_json_atomic(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(f"{path.suffix}.tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)


def remove_readonly_tree(path: Path) -> None:
    def retry(function: Any, failed_path: str, _error: Any) -> None:
        os.chmod(failed_path, stat.S_IWRITE)
        function(failed_path)

    shutil.rmtree(path, onerror=retry)


def run_mode(
    *,
    mode: str,
    repo: Path,
    work: Path,
    output: Path,
    contract: dict[str, Any],
    source_commit: str,
    consumer_commit: str,
    candidate_probe: Path | None,
    overlay: list[str],
) -> tuple[dict[str, Any], Path]:
    source = work / f"{mode}-rterm"
    consumer = work / f"{mode}-consumer"
    clone_at(repo, source, source_commit)
    clone_at(repo, consumer, consumer_commit)
    overlay_paths(source, consumer, overlay)

    probe = contract.get("standalone_probe")
    if not isinstance(probe, dict) or not isinstance(probe.get("path"), str):
        raise RehearsalError("standalone_probe must declare a path and command")
    probe_relative = PurePosixPath(probe["path"])
    if probe_relative.is_absolute() or ".." in probe_relative.parts:
        raise RehearsalError(f"invalid standalone probe path: {probe_relative}")
    probe_root = source / probe_relative.as_posix()
    copied_probe = False
    if not probe_root.exists():
        if candidate_probe is None or not candidate_probe.exists():
            raise RehearsalError(f"standalone probe is missing: {probe_relative}")
        probe_root.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(candidate_probe, probe_root)
        copied_probe = True

    environment = os.environ.copy()
    environment.update(
        {
            "RTERM_REHEARSAL_MODE": mode,
            "RTERM_REHEARSAL_SOURCE_REF": source_commit,
            "RTERM_REHEARSAL_CONSUMER_REF": consumer_commit,
        }
    )
    commands: list[dict[str, Any]] = []
    if copied_probe and probe.get("rollback_prepare_command") is not None:
        command = command_list(
            probe["rollback_prepare_command"], "rollback_prepare_command"
        )
        commands.append(run_command(command, probe_root, environment, "probe-prepare"))
    if not commands or commands[-1]["returncode"] == 0:
        commands.append(
            run_command(
                command_list(probe.get("command"), "standalone_probe command"),
                probe_root,
                environment,
                "standalone-probe",
            )
        )

    consumer_prepare = contract.get("consumer_prepare_command")
    if (
        all(command["returncode"] == 0 for command in commands)
        and consumer_prepare is not None
    ):
        commands.append(
            run_command(
                command_list(consumer_prepare, "consumer_prepare_command"),
                consumer,
                environment,
                "consumer-prepare",
            )
        )
    consumer_commands = contract.get("consumer_commands")
    if not isinstance(consumer_commands, list) or not consumer_commands:
        raise RehearsalError("consumer_commands must be a nonempty list")
    for index, value in enumerate(consumer_commands):
        if any(command["returncode"] != 0 for command in commands):
            break
        commands.append(
            run_command(
                command_list(value, f"consumer command {index}"),
                consumer,
                environment,
                "consumer",
            )
        )

    evidence = {
        "schema_version": 1,
        "mode": mode,
        "ok": bool(commands) and all(command["returncode"] == 0 for command in commands),
        "source_commit": source_commit,
        "consumer_commit": consumer_commit,
        "overlay_paths": overlay,
        "probe_copied_from_candidate": copied_probe,
        "commands": commands,
    }
    write_json_atomic(output / f"{mode}.json", evidence)
    return evidence, probe_root


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--candidate-ref", required=True)
    parser.add_argument("--consumer-ref", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    arguments = parser.parse_args()

    try:
        repo = arguments.repo.resolve()
        output = arguments.output_dir.resolve()
        contract = json.loads(arguments.contract.read_text(encoding="utf-8"))
        overlay = contract_overlay_paths(contract)
        candidate_commit = resolve_commit(repo, arguments.candidate_ref)
        consumer_commit = resolve_commit(repo, arguments.consumer_ref)
        rollback_ref = contract.get("last_known_good_rterm_ref")
        if not isinstance(rollback_ref, str) or SHA1.fullmatch(rollback_ref) is None:
            raise RehearsalError("last_known_good_rterm_ref must be an immutable commit")
        rollback_commit = resolve_commit(repo, rollback_ref)
        work = output / "work"
        if work.exists():
            raise RehearsalError(f"refusing existing rehearsal work directory: {work}")

        candidate, candidate_probe = run_mode(
            mode="candidate",
            repo=repo,
            work=work,
            output=output,
            contract=contract,
            source_commit=candidate_commit,
            consumer_commit=consumer_commit,
            candidate_probe=None,
            overlay=overlay,
        )
        if not candidate["ok"]:
            return 1
        rollback, _ = run_mode(
            mode="rollback",
            repo=repo,
            work=work,
            output=output,
            contract=contract,
            source_commit=rollback_commit,
            consumer_commit=consumer_commit,
            candidate_probe=candidate_probe,
            overlay=overlay,
        )
        if not rollback["ok"]:
            return 1
        remove_readonly_tree(work)
        print(
            json.dumps(
                {
                    "ok": True,
                    "candidate_commit": candidate_commit,
                    "consumer_commit": consumer_commit,
                    "rollback_commit": rollback_commit,
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 0
    except (OSError, json.JSONDecodeError, RehearsalError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())

