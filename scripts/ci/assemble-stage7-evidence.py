#!/usr/bin/env python3
"""Assemble hashed runner fragments into one deterministic Stage 7 manifest."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import sys
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, NoReturn


SCRIPT_DIR = Path(__file__).resolve().parent
CHECKER_PATH = SCRIPT_DIR / "check-stage7-split-gate.py"
_SPEC = importlib.util.spec_from_file_location("rssh_stage7_gate_core", CHECKER_PATH)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"cannot import Stage 7 validator core: {CHECKER_PATH}")
gate = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(gate)


class EvidenceError(RuntimeError):
    """A fragment or assembly input violated the frozen fail-closed contract."""


def fail(message: str) -> NoReturn:
    raise EvidenceError(message)


def relative_input(root: Path, value: Path | str, label: str) -> Path:
    value = Path(value)
    if value.is_absolute():
        candidate = value.resolve()
    else:
        text = value.as_posix()
        if "\\" in text or PureWindowsPath(text).drive:
            fail(f"{label} must be a normalized relative path beneath evidence-root")
        pure = PurePosixPath(text)
        if (
            pure.is_absolute()
            or any(part in {"", ".", ".."} for part in pure.parts)
            or not gate.windows_path_components_are_safe(pure.parts)
        ):
            fail(f"{label} escapes evidence-root")
        candidate = (root / Path(*pure.parts)).resolve()
    try:
        relative = candidate.relative_to(root.resolve())
    except ValueError:
        fail(f"{label} escapes evidence-root")
    if not gate.windows_path_components_are_safe(relative.parts):
        fail(f"{label} is not a safe Windows-compatible evidence path")
    return candidate


def relative_to_manifest(path: Path, manifest_directory: Path, label: str) -> str:
    try:
        relative = path.resolve().relative_to(manifest_directory.resolve())
    except ValueError:
        fail(f"{label} is not beneath the output manifest directory")
    value = relative.as_posix()
    if (
        not value
        or any(part in {"", ".", ".."} for part in PurePosixPath(value).parts)
        or not gate.windows_path_components_are_safe(PurePosixPath(value).parts)
    ):
        fail(f"{label} is not a safe manifest-relative path")
    return value


def load_json(path: Path, label: str) -> dict[str, Any]:
    try:
        value = gate.strict_json_loads(gate.read_bounded_json_text(path))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        fail(f"{label}: cannot parse JSON: {error}")
    if not isinstance(value, dict):
        fail(f"{label}: JSON root must be an object")
    return value


def atomic_write_json(path: Path, value: dict[str, Any]) -> None:
    encoded = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode("utf-8")
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    try:
        with temporary.open("xb") as output:
            output.write(encoded)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def validate_fragment_entry(
    entry_value: Any,
    fragment_path: Path,
    output_directory: Path,
    evidence_root: Path,
    contract: dict[str, Any],
) -> dict[str, Any]:
    violations: list[str] = []
    entry = gate.validate_entry_shape(
        entry_value,
        contract["artifact_policies"],
        f"fragment {fragment_path.name}",
        violations,
    )
    if entry is None or violations:
        fail("; ".join(violations or [f"fragment {fragment_path.name}: invalid entry"]))
    artifact_path = gate.contained_file(
        fragment_path.parent,
        entry.get("path"),
        f"fragment {fragment_path.name} artifact {entry.get('artifact_id')}",
        violations,
    )
    if artifact_path is None or violations:
        fail("; ".join(violations))
    try:
        artifact_path.relative_to(evidence_root)
    except ValueError:
        fail(f"fragment {fragment_path.name}: artifact escapes evidence-root")
    if not gate.verify_hash(
        artifact_path,
        entry.get("sha256"),
        f"fragment {fragment_path.name} artifact {entry.get('artifact_id')}",
        violations,
    ):
        fail("; ".join(violations))
    if entry.get("size_bytes") != artifact_path.stat().st_size:
        fail(f"fragment {fragment_path.name}: size_bytes mismatch for {entry.get('artifact_id')}")
    payload = load_json(artifact_path, f"artifact {entry.get('artifact_id')}")
    if payload.get("schema") != entry.get("payload_schema"):
        fail(f"fragment {fragment_path.name}: payload_schema mismatch for {entry.get('artifact_id')}")
    if entry.get("role") == "aggregate" and entry.get("children") != payload.get("raw_children"):
        fail(f"fragment {fragment_path.name}: aggregate children mismatch for {entry.get('artifact_id')}")
    rebased = dict(entry)
    rebased["path"] = relative_to_manifest(
        artifact_path,
        output_directory,
        f"artifact {entry.get('artifact_id')}",
    )
    return rebased


def validate_prior_without_root_scan(
    contract_path: Path,
    contract: dict[str, Any],
    prior_path: Path,
    prior_state: str,
) -> tuple[dict[str, Any], dict[str, dict[str, Any]], set[Path]]:
    violations: list[str] = []
    referenced: set[Path] = set()
    manifest, entries = gate.validate_manifest_recursive(
        contract_path,
        contract,
        gate.file_sha256(contract_path),
        prior_path,
        prior_state,
        prior_path.parent,
        referenced,
        set(),
        violations,
    )
    if manifest is None or violations:
        fail("prior manifest is invalid: " + "; ".join(violations))
    return manifest, entries, referenced


def assemble(
    contract_path: Path | str,
    requested_state: str,
    evidence_root: Path | str,
    fragments: list[Path | str],
    output: Path | str,
    *,
    prior_manifest: Path | str | None = None,
) -> dict[str, Any]:
    contract_path = Path(contract_path).resolve()
    evidence_root = Path(evidence_root).resolve()
    if not evidence_root.is_dir():
        fail(f"evidence-root must be an existing bounded directory: {evidence_root}")
    contract, contract_violations = gate.validate_contract(contract_path)
    if contract is None or contract_violations:
        fail("invalid Stage 7 contract: " + "; ".join(contract_violations))
    if requested_state not in gate.STATES[1:]:
        fail("assembler requested-state must be a certifiable non-blocked state")
    if not fragments:
        fail("at least one --fragment is required")

    output_path = relative_input(evidence_root, output, "output")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    if output_path.exists():
        fail(f"output already exists: {output_path}")
    fragment_paths = [relative_input(evidence_root, value, "fragment") for value in fragments]
    if len(set(fragment_paths)) != len(fragment_paths):
        fail("duplicate fragment inputs are forbidden")
    if output_path in fragment_paths:
        fail("output cannot also be a fragment")

    fragment_refs: list[dict[str, str]] = []
    current_entries: list[dict[str, Any]] = []
    artifact_ids: set[str] = set()
    artifact_path_keys: dict[str, str] = {}
    type_counts: dict[str, int] = {}
    certified_commit: str | None = None
    rssh_epoch: Any = None
    rterm_epoch: Any = None
    referenced_current: set[Path] = set(fragment_paths)
    for fragment_path in sorted(fragment_paths, key=lambda item: item.as_posix()):
        if not fragment_path.is_file():
            fail(f"fragment does not exist: {fragment_path}")
        fragment = load_json(fragment_path, f"fragment {fragment_path.name}")
        expected_fields = {
            "schema",
            "requested_state",
            "certified_commit",
            "epoch_id",
            "rssh",
            "rterm",
            "entries",
        }
        if set(fragment) != expected_fields:
            fail(f"fragment {fragment_path.name}: fields do not match the frozen fragment schema")
        if fragment.get("schema") != gate.FRAGMENT_SCHEMA:
            fail(f"fragment {fragment_path.name}: schema mismatch")
        if fragment.get("requested_state") != requested_state:
            fail(f"fragment {fragment_path.name}: requested state mismatch")
        commit = fragment.get("certified_commit")
        if not gate.is_full_sha(commit):
            fail(f"fragment {fragment_path.name}: certified commit must be a full immutable SHA")
        if not gate.git_commit_available(gate.repository_root(contract_path), commit):
            fail(f"fragment {fragment_path.name}: certified commit is unavailable in Git")
        expected_epoch_id = gate.certification_epoch_id(
            requested_state, commit, fragment.get("rssh"), fragment.get("rterm")
        )
        if fragment.get("epoch_id") != expected_epoch_id:
            fail(f"fragment {fragment_path.name}: epoch_id does not bind its certification inputs")
        if certified_commit is None:
            certified_commit = commit
            rssh_epoch = fragment.get("rssh")
            rterm_epoch = fragment.get("rterm")
        elif (commit, fragment.get("rssh"), fragment.get("rterm")) != (
            certified_commit,
            rssh_epoch,
            rterm_epoch,
        ):
            fail(f"fragment {fragment_path.name}: certification epoch identity drift")
        entries = fragment.get("entries")
        if not isinstance(entries, list) or not entries:
            fail(f"fragment {fragment_path.name}: entries must be non-empty")
        for entry_value in entries:
            entry = validate_fragment_entry(
                entry_value,
                fragment_path,
                output_path.parent,
                evidence_root,
                contract,
            )
            artifact_id = entry["artifact_id"]
            artifact_type = entry["artifact_type"]
            if artifact_id in artifact_ids:
                fail(f"duplicate artifact_id across fragments: {artifact_id}")
            artifact_ids.add(artifact_id)
            artifact_path = (output_path.parent / entry["path"]).resolve()
            path_key = str(artifact_path).casefold()
            if path_key in artifact_path_keys:
                fail(
                    f"artifact path collision between {artifact_path_keys[path_key]} and {artifact_id}"
                )
            artifact_path_keys[path_key] = artifact_id
            type_counts[artifact_type] = type_counts.get(artifact_type, 0) + 1
            current_entries.append(entry)
            referenced_current.add((output_path.parent / entry["path"]).resolve())
        fragment_refs.append(
            {
                "path": relative_to_manifest(fragment_path, output_path.parent, "fragment"),
                "sha256": gate.file_sha256(fragment_path),
            }
        )

    assert certified_commit is not None
    epoch_probe = {"rssh": rssh_epoch, "rterm": rterm_epoch}
    epoch_violations: list[str] = []
    gate.validate_epoch_shape(
        epoch_probe,
        requested_state,
        contract,
        "fragment epoch",
        epoch_violations,
    )
    if epoch_violations:
        fail("; ".join(epoch_violations))
    expected_types = set(contract["new_artifacts_by_state"][requested_state])
    if set(type_counts) != expected_types:
        missing = sorted(expected_types - set(type_counts))
        extra = sorted(set(type_counts) - expected_types)
        fail(f"fragment artifact set mismatch: missing={missing} extra={extra}")
    singleton = set(contract["artifact_multiplicity"]["singleton"])
    multiple = contract["artifact_multiplicity"]["multiple"]
    for artifact_type, count in type_counts.items():
        if artifact_type in singleton and count != 1:
            fail(f"singleton artifact_type is duplicated: {artifact_type}")
        if artifact_type in multiple:
            rule = multiple[artifact_type]
            if count < rule["minimum"]:
                fail(f"artifact_type is below minimum multiplicity: {artifact_type}")
            platforms = {
                entry["platform"]
                for entry in current_entries
                if entry["artifact_type"] == artifact_type
            }
            missing_platforms = set(rule.get("required_platforms", [])) - platforms
            if missing_platforms:
                fail(f"artifact_type {artifact_type} lacks platform cohorts: {sorted(missing_platforms)}")

    state_index = gate.STATES.index(requested_state)
    prior_state = gate.STATES[state_index - 1] if state_index > 1 else None
    prior_ref: dict[str, Any] | None = None
    inherited_entries: list[dict[str, Any]] = []
    referenced_prior: set[Path] = set()
    if prior_state is None:
        if prior_manifest is not None:
            fail("attribution-ready must not provide --prior-manifest")
    else:
        if prior_manifest is None:
            fail(f"{requested_state} requires exactly the {prior_state} predecessor manifest")
        prior_path = relative_input(evidence_root, prior_manifest, "prior-manifest")
        if not prior_path.is_file():
            fail(f"prior manifest does not exist: {prior_path}")
        relative_prior = relative_to_manifest(prior_path, output_path.parent, "prior manifest")
        prior_data, prior_entries, referenced_prior = validate_prior_without_root_scan(
            contract_path, contract, prior_path, prior_state
        )
        if state_index <= gate.STATES.index("cross-platform-go"):
            if prior_data["certified_commit"] != certified_commit:
                fail("attribution, Windows, and cross-platform states must certify the exact same candidate commit")
        elif not gate.git_is_ancestor(
            gate.repository_root(contract_path),
            prior_data["certified_commit"],
            certified_commit,
        ):
            fail("current certified commit is not descended from prior certified commit")
        progression_violations: list[str] = []
        gate.validate_epoch_progression(
            epoch_probe,
            prior_data,
            "assembly epoch",
            progression_violations,
        )
        if progression_violations:
            fail("; ".join(progression_violations))
        for artifact_id, entry in prior_entries.items():
            if artifact_id in artifact_ids:
                fail(f"current fragments reuse predecessor artifact_id: {artifact_id}")
            inherited = dict(entry)
            prior_artifact = (prior_path.parent / entry["path"]).resolve()
            inherited["path"] = relative_to_manifest(
                prior_artifact, output_path.parent, f"prior artifact {artifact_id}"
            )
            path_key = str(prior_artifact).casefold()
            if path_key in artifact_path_keys:
                fail(
                    f"artifact path collision between {artifact_path_keys[path_key]} and prior {artifact_id}"
                )
            artifact_path_keys[path_key] = artifact_id
            inherited_entries.append(inherited)
        prior_ref = {
            "path": relative_prior,
            "sha256": gate.file_sha256(prior_path),
            "certified_state": prior_state,
            "certified_commit": prior_data["certified_commit"],
        }

    all_entries = inherited_entries + current_entries
    all_entries.sort(
        key=lambda entry: (
            entry["artifact_type"],
            entry["platform"],
            entry["run_id"],
            entry["artifact_id"],
            entry["path"],
        )
    )
    manifest = {
        "schema": gate.MANIFEST_SCHEMA,
        "contract_sha256": gate.file_sha256(contract_path),
        "requested_state": requested_state,
        "certified_state": requested_state,
        "certified_commit": certified_commit,
        "epoch_id": gate.certification_epoch_id(
            requested_state, certified_commit, rssh_epoch, rterm_epoch
        ),
        "rssh": rssh_epoch,
        "rterm": rterm_epoch,
        "created_by": "assemble-stage7-evidence.py",
        "prior_manifest": prior_ref,
        "fragments": sorted(fragment_refs, key=lambda item: item["path"]),
        "entries": all_entries,
    }

    expected_referenced = referenced_current | referenced_prior
    if prior_ref is not None:
        expected_referenced.add((output_path.parent / prior_ref["path"]).resolve())
    existing_files = {
        path.resolve()
        for path in output_path.parent.rglob("*")
        if path.is_file() and path.resolve() != output_path
    }
    unreferenced = sorted(existing_files - expected_referenced, key=lambda item: item.as_posix())
    if unreferenced:
        fail(
            "unreferenced files beneath output manifest directory: "
            + ", ".join(path.relative_to(output_path.parent).as_posix() for path in unreferenced)
        )

    temporary = output_path.with_name(f".{output_path.name}.validate-{os.getpid()}.json")
    if temporary.exists():
        fail(f"temporary validation path already exists: {temporary}")
    atomic_write_json(temporary, manifest)
    try:
        decision = gate.validate_gate(contract_path, requested_state, temporary)
        if not decision["ok"]:
            fail("assembled manifest failed validation: " + "; ".join(decision["violations"]))
        os.replace(temporary, output_path)
    finally:
        temporary.unlink(missing_ok=True)
    return manifest


def cli_relative(value: str, label: str) -> Path:
    path = Path(value)
    text = path.as_posix()
    if path.is_absolute() or PureWindowsPath(value).drive or "\\" in value:
        fail(f"{label} must be relative to --evidence-root")
    pure = PurePosixPath(text)
    if (
        pure.is_absolute()
        or any(part in {"", ".", ".."} for part in pure.parts)
        or not gate.windows_path_components_are_safe(pure.parts)
    ):
        fail(f"{label} escapes --evidence-root")
    return Path(*pure.parts)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--contract", required=True, type=Path)
    parser.add_argument("--requested-state", required=True)
    parser.add_argument("--evidence-root", required=True, type=Path)
    parser.add_argument("--prior-manifest")
    parser.add_argument("--fragment", action="append", required=True)
    parser.add_argument("--output", required=True)
    arguments = parser.parse_args()
    try:
        prior = (
            cli_relative(arguments.prior_manifest, "prior-manifest")
            if arguments.prior_manifest is not None
            else None
        )
        manifest = assemble(
            arguments.contract,
            arguments.requested_state,
            arguments.evidence_root,
            [cli_relative(value, "fragment") for value in arguments.fragment],
            cli_relative(arguments.output, "output"),
            prior_manifest=prior,
        )
        output_path = relative_input(
            Path(arguments.evidence_root).resolve(),
            cli_relative(arguments.output, "output"),
            "output",
        )
        report = {
            "ok": True,
            "requested_state": arguments.requested_state,
            "output": output_path.relative_to(Path(arguments.evidence_root).resolve()).as_posix(),
            "sha256": gate.file_sha256(output_path),
            "artifact_count": len(manifest["entries"]),
        }
        print(json.dumps(report, sort_keys=True, separators=(",", ":")))
        return 0
    except EvidenceError as error:
        print(
            json.dumps(
                {
                    "ok": False,
                    "go": False,
                    "decision": "NO-GO",
                    "state": "blocked",
                    "requested_state": arguments.requested_state,
                    "violations": [str(error)],
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 1
    except Exception as error:  # pragma: no cover - defensive CLI boundary
        print(
            json.dumps(
                {
                    "ok": False,
                    "go": False,
                    "decision": "NO-GO",
                    "state": "blocked",
                    "requested_state": arguments.requested_state,
                    "violations": [
                        f"assembler failed closed: {type(error).__name__}"
                    ],
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
        return 1


if __name__ == "__main__":
    sys.exit(main())
