"""Behavioral tests for the fail-closed Stage 7 evidence gate."""

from __future__ import annotations

import copy
import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
import weakref
import zlib
from pathlib import Path
from unittest import mock


REPO = Path(__file__).resolve().parents[3]
CONTRACT_PATH = REPO / "scripts/ci/stage7-split-contract.json"
SCHEMA_PATH = REPO / "scripts/ci/stage7-evidence-manifest.schema.json"
CHECKER_PATH = REPO / "scripts/ci/check-stage7-split-gate.py"
ASSEMBLER_PATH = REPO / "scripts/ci/assemble-stage7-evidence.py"
PYTHON = Path(sys.executable)
SOURCE_SHA = subprocess.run(
    ["git", "rev-parse", "HEAD"],
    cwd=REPO,
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
PARENT_SHA = subprocess.run(
    ["git", "rev-parse", "HEAD^"],
    cwd=REPO,
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
BLOB_SHA = subprocess.run(
    ["git", "rev-parse", f"{PARENT_SHA}:Cargo.toml"],
    cwd=REPO,
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
TREE_SHA = subprocess.run(
    ["git", "rev-parse", "HEAD^{tree}"],
    cwd=REPO,
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
PARENT_TREE_SHA = subprocess.run(
    ["git", "rev-parse", f"{PARENT_SHA}^{{tree}}"],
    cwd=REPO,
    check=True,
    capture_output=True,
    text=True,
).stdout.strip()
LKG_SHA = "21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6"
BINARY_SHA = "b" * 64
RUNNER_SHA = "c" * 64
_BARE_PROOF_ROOT = tempfile.TemporaryDirectory(prefix="rssh-stage7-bare-proof-")
_BARE_PROOF_CACHE: dict[tuple[str, tuple[tuple[str, str], ...], tuple[str, ...]], dict] = {}
_COMPLETE_BARE_PROOF_CACHE: dict[str, dict] = {}
_GIT_OBJECT_BODY_CACHE: dict[str, tuple[str, bytes]] = {}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def canonical_sha256(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def git_commit_object(ref: str, raw: bytes | None = None) -> dict:
    if raw is None:
        raw = subprocess.run(
            ["git", "cat-file", "commit", ref],
            cwd=REPO,
            check=True,
            capture_output=True,
        ).stdout
    return {
        "oid": ref,
        "object_type": "commit",
        "body_base64": base64.b64encode(raw).decode("ascii"),
    }


def git_repository_snapshot(role: str, refs: dict[str, str], objects: list[dict]) -> dict:
    cache_key = (
        role,
        tuple(sorted(refs.items())),
        tuple(sorted(item["oid"] for item in objects)),
    )
    observation = _BARE_PROOF_CACHE.get(cache_key)
    if observation is None:
        repository_path = Path(_BARE_PROOF_ROOT.name) / canonical_sha256(cache_key)
        subprocess.run(
            ["git", "init", "--bare", "--quiet", str(repository_path)],
            check=True,
            capture_output=True,
        )
        for item in objects:
            raw = base64.b64decode(item["body_base64"], validate=True)
            observed_oid = subprocess.run(
                [
                    "git",
                    f"--git-dir={repository_path}",
                    "hash-object",
                    "-w",
                    "-t",
                    "commit",
                    "--stdin",
                ],
                input=raw,
                check=True,
                capture_output=True,
            ).stdout.decode("ascii").strip()
            if observed_oid != item["oid"]:
                raise AssertionError("bare proof fixture commit OID drift")
        ref_observations = []
        for logical_name, oid in sorted(refs.items()):
            refname = f"refs/stage7-proof/{logical_name}"
            subprocess.run(
                ["git", f"--git-dir={repository_path}", "update-ref", refname, oid],
                check=True,
                capture_output=True,
            )
            object_type = subprocess.run(
                ["git", f"--git-dir={repository_path}", "cat-file", "-t", refname],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            ref_observations.append(
                {
                    "logical_name": logical_name,
                    "refname": refname,
                    "oid": oid,
                    "object_type": object_type,
                }
            )
        is_bare = subprocess.run(
            ["git", f"--git-dir={repository_path}", "rev-parse", "--is-bare-repository"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        object_format = subprocess.run(
            ["git", f"--git-dir={repository_path}", "rev-parse", "--show-object-format"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        objects_stat = (repository_path / "objects").stat()
        if is_bare != "true" or object_format != "sha1" or objects_stat.st_ino == 0:
            raise AssertionError("bare proof fixture observation drift")
        canonical_config = (
            b"[core]\n"
            b"\trepositoryformatversion = 0\n"
            b"\tfilemode = false\n"
            b"\tbare = true\n"
        )
        head_ref = ref_observations[0]["refname"]
        snapshot_files = [
            {
                "path": "HEAD",
                "body_base64": base64.b64encode(
                    f"ref: {head_ref}\n".encode("ascii")
                ).decode("ascii"),
            },
            {
                "path": "config",
                "body_base64": base64.b64encode(canonical_config).decode("ascii"),
            },
        ]
        for ref in ref_observations:
            snapshot_files.append(
                {
                    "path": ref["refname"],
                    "body_base64": base64.b64encode(
                        f"{ref['oid']}\n".encode("ascii")
                    ).decode("ascii"),
                }
            )
        for item in objects:
            oid = item["oid"]
            raw = base64.b64decode(item["body_base64"], validate=True)
            loose = zlib.compress(
                f"commit {len(raw)}\0".encode("ascii") + raw,
                level=9,
            )
            snapshot_files.append(
                {
                    "path": f"objects/{oid[:2]}/{oid[2:]}",
                    "body_base64": base64.b64encode(loose).decode("ascii"),
                }
            )
        observation = {
            "schema": "rssh.stage7.replayable-bare-repository/v1",
            "files": sorted(snapshot_files, key=lambda record: record["path"]),
        }
        observation["snapshot_sha256"] = canonical_sha256(observation)
        _BARE_PROOF_CACHE[cache_key] = copy.deepcopy(observation)
    repository = {
        "role": role,
        "bare": True,
        "alternates": [],
        "refs": refs,
        "commit_objects": objects,
        "bare_repository_snapshot": copy.deepcopy(observation),
    }
    repository["snapshot_sha256"] = canonical_sha256(
        {
            "bare": repository["bare"],
            "alternates": repository["alternates"],
            "commit_objects": repository["commit_objects"],
            "bare_repository_snapshot": repository["bare_repository_snapshot"],
        }
    )
    return repository


def complete_git_repository_snapshot(
    role: str,
    refs: dict[str, str],
    *,
    history_boundaries: list[str],
    overrides: dict[str, tuple[str, bytes]] | None = None,
) -> dict:
    overrides = overrides or {}
    cache_key = canonical_sha256(
        {
            "role": role,
            "refs": refs,
            "history_boundaries": history_boundaries,
            "overrides": {
                oid: {"object_type": value[0], "body_sha256": hashlib.sha256(value[1]).hexdigest()}
                for oid, value in sorted(overrides.items())
            },
        }
    )
    cached = _COMPLETE_BARE_PROOF_CACHE.get(cache_key)
    if cached is not None:
        return copy.deepcopy(cached)

    batch = subprocess.Popen(
        ["git", "cat-file", "--batch"],
        cwd=REPO,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )

    def read_object(oid: str) -> tuple[str, bytes]:
        if oid in overrides:
            return overrides[oid]
        cached_body = _GIT_OBJECT_BODY_CACHE.get(oid)
        if cached_body is not None:
            return cached_body
        if batch.stdin is None or batch.stdout is None:
            raise AssertionError("Git batch pipes are unavailable")
        batch.stdin.write(f"{oid}\n".encode("ascii"))
        batch.stdin.flush()
        header = batch.stdout.readline().decode("ascii", errors="strict").strip().split()
        if len(header) != 3 or header[0] != oid:
            raise AssertionError(f"Git object {oid} is unavailable: {header}")
        object_type = header[1]
        size = int(header[2])
        body = batch.stdout.read(size)
        if len(body) != size or batch.stdout.read(1) != b"\n":
            raise AssertionError(f"Git batch body for {oid} is truncated")
        _GIT_OBJECT_BODY_CACHE[oid] = (object_type, body)
        return object_type, body

    records: dict[str, dict] = {}
    pending = [(oid, "commit") for oid in refs.values()]
    try:
        while pending:
            oid, expected_type = pending.pop()
            if oid in records:
                if records[oid]["object_type"] != expected_type:
                    raise AssertionError(f"fixture Git object type drift for {oid}")
                continue
            object_type, raw = read_object(oid)
            if object_type != expected_type:
                raise AssertionError(
                    f"fixture Git object {oid} is {object_type}, expected {expected_type}"
                )
            records[oid] = {
                "oid": oid,
                "object_type": object_type,
                "body_base64": base64.b64encode(raw).decode("ascii"),
            }
            if object_type == "commit":
                header = raw.partition(b"\n\n")[0].decode("ascii", errors="strict").splitlines()
                tree_oid = header[0].removeprefix("tree ")
                pending.append((tree_oid, "tree"))
                if oid not in history_boundaries:
                    pending.extend(
                        (line.removeprefix("parent "), "commit")
                        for line in header
                        if line.startswith("parent ")
                    )
            elif object_type == "tree":
                for mode, _name, child_oid in parse_raw_tree_for_fixture(raw):
                    if mode == "160000":
                        raise AssertionError("self-contained fixture cannot contain gitlinks")
                    pending.append((child_oid, "tree" if mode == "40000" else "blob"))
    finally:
        if batch.stdin is not None:
            batch.stdin.close()
        batch.wait(timeout=10)
        if batch.stdout is not None:
            batch.stdout.close()

    canonical_config = (
        b"[core]\n"
        b"\trepositoryformatversion = 0\n"
        b"\tfilemode = false\n"
        b"\tbare = true\n"
    )
    first_ref = f"refs/heads/stage7-proof/{sorted(refs)[0]}"
    snapshot_files = [
        {
            "path": "HEAD",
            "body_base64": base64.b64encode(f"ref: {first_ref}\n".encode("ascii")).decode(
                "ascii"
            ),
        },
        {
            "path": "config",
            "body_base64": base64.b64encode(canonical_config).decode("ascii"),
        },
    ]
    snapshot_files.extend(
        {
            "path": f"refs/heads/stage7-proof/{logical_name}",
            "body_base64": base64.b64encode(f"{oid}\n".encode("ascii")).decode("ascii"),
        }
        for logical_name, oid in sorted(refs.items())
    )
    if history_boundaries:
        snapshot_files.append(
            {
                "path": "shallow",
                "body_base64": base64.b64encode(
                    "".join(f"{oid}\n" for oid in history_boundaries).encode("ascii")
                ).decode("ascii"),
            }
        )
    for oid, record in records.items():
        raw = base64.b64decode(record["body_base64"], validate=True)
        loose = zlib.compress(
            f"{record['object_type']} {len(raw)}\0".encode("ascii") + raw,
            level=9,
        )
        snapshot_files.append(
            {
                "path": f"objects/{oid[:2]}/{oid[2:]}",
                "body_base64": base64.b64encode(loose).decode("ascii"),
            }
        )
    observation = {
        "schema": "rssh.stage7.replayable-bare-repository/v1",
        "files": sorted(snapshot_files, key=lambda record: record["path"]),
    }
    observation["snapshot_sha256"] = canonical_sha256(observation)
    repository = {
        "role": role,
        "bare": True,
        "alternates": [],
        "refs": refs,
        "history_boundaries": history_boundaries,
        "git_objects": sorted(records.values(), key=lambda record: record["oid"]),
        "bare_repository_snapshot": observation,
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
    _COMPLETE_BARE_PROOF_CACHE[cache_key] = copy.deepcopy(repository)
    return repository


def git_object_store_proof(repositories: list[dict]) -> dict:
    return {
        "schema": "rssh.stage7.git-object-store-proof/v1",
        "object_format": "sha1",
        "repositories": repositories,
    }


def fixture_owned_path_mappings() -> list[dict]:
    contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))
    rules = contract["owned_projection_inventory"]
    raw = subprocess.run(
        ["git", "ls-tree", "-r", "--full-tree", "-z", PARENT_SHA],
        cwd=REPO,
        check=True,
        capture_output=True,
    ).stdout
    source_leaves: dict[str, dict[str, str]] = {}
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        metadata, encoded_path = encoded.split(b"\t", 1)
        mode, object_type, oid = metadata.decode("ascii").split(" ")
        source_leaves[encoded_path.decode("utf-8")] = {
            "mode": mode,
            "object_type": object_type,
            "object_oid": oid,
        }
    bootstrap_targets = {
        item["source_path"]: item["filtered_path"]
        for item in contract["bootstrap_template_mappings"]
    }
    for source_path in bootstrap_targets:
        source_leaves.setdefault(
            source_path,
            {
                "mode": "100644",
                "object_type": "blob",
                "object_oid": BLOB_SHA,
            },
        )
    mappings: list[dict] = []
    for rule in rules["required"] + rules["future_required"]:
        source_prefix = rule["source_prefix"]
        filtered_prefix = rule["filtered_prefix"]
        matched = sorted(
            path
            for path in source_leaves
            if path == source_prefix or path.startswith(source_prefix + "/")
        )
        if not matched:
            raise AssertionError(f"fixture R0 lacks frozen owned root {source_prefix}")
        for source_path in matched:
            filtered_path = (
                bootstrap_targets[source_path]
                if source_prefix == "release/rterm-bootstrap"
                else filtered_prefix + source_path[len(source_prefix) :]
            )
            identity = source_leaves[source_path]
            mappings.append(
                {
                    "source_path": source_path,
                    "filtered_path": filtered_path,
                    **identity,
                }
            )
    return sorted(
        mappings, key=lambda record: (record["filtered_path"], record["source_path"])
    )


def build_filtered_tree_objects(mappings: list[dict]) -> tuple[str, dict[str, bytes]]:
    root: dict[str, object] = {}
    for mapping in mappings:
        components = mapping["filtered_path"].split("/")
        node = root
        for component in components[:-1]:
            child = node.setdefault(component, {})
            if not isinstance(child, dict):
                raise AssertionError("fixture filtered path collision")
            node = child
        leaf = components[-1]
        if leaf in node:
            raise AssertionError("fixture filtered leaf collision")
        node[leaf] = (
            mapping["mode"],
            mapping["object_type"],
            mapping["object_oid"],
        )

    objects: dict[str, bytes] = {}

    def encode_tree(node: dict[str, object]) -> str:
        encoded_entries: list[tuple[bytes, bytes]] = []
        for name, value in node.items():
            encoded_name = name.encode("utf-8")
            if isinstance(value, dict):
                oid = encode_tree(value)
                mode = "40000"
                sort_key = encoded_name + b"/"
            else:
                mode, _object_type, oid = value
                sort_key = encoded_name
            encoded_entries.append(
                (
                    sort_key,
                    mode.encode("ascii")
                    + b" "
                    + encoded_name
                    + b"\0"
                    + bytes.fromhex(oid),
                )
            )
        raw_tree = b"".join(value for _key, value in sorted(encoded_entries))
        oid = hashlib.sha1(
            f"tree {len(raw_tree)}\0".encode("ascii") + raw_tree,
            usedforsecurity=False,
        ).hexdigest()
        objects[oid] = raw_tree
        return oid

    return encode_tree(root), objects


FIXTURE_OWNED_MAPPINGS = fixture_owned_path_mappings()
FIXTURE_FILTERED_HISTORY_MAPPINGS = [
    mapping
    for mapping in FIXTURE_OWNED_MAPPINGS
    if not mapping["source_path"].startswith("release/rterm-bootstrap/")
]
FIXTURE_BOOTSTRAP_TEMPLATE_MAPPINGS = [
    mapping
    for mapping in FIXTURE_OWNED_MAPPINGS
    if mapping["source_path"].startswith("release/rterm-bootstrap/")
]
FILTERED_TREE_SHA, FILTERED_TREE_OBJECTS = build_filtered_tree_objects(
    FIXTURE_FILTERED_HISTORY_MAPPINGS
)
ROOT_LOCK_RAW = b"# deterministic Stage 7 root lock fixture\n"
CONSUMER_LOCK_RAW = b"# deterministic Stage 7 consumer lock fixture\n"
ROOT_LOCK_SHA = hashlib.sha1(
    f"blob {len(ROOT_LOCK_RAW)}\0".encode("ascii") + ROOT_LOCK_RAW,
    usedforsecurity=False,
).hexdigest()
CONSUMER_LOCK_SHA = hashlib.sha1(
    f"blob {len(CONSUMER_LOCK_RAW)}\0".encode("ascii") + CONSUMER_LOCK_RAW,
    usedforsecurity=False,
).hexdigest()
FIXTURE_GENERATED_BOOTSTRAP_FILES = [
    {
        "filtered_path": "Cargo.lock",
        "mode": "100644",
        "object_type": "blob",
        "object_oid": ROOT_LOCK_SHA,
    },
    {
        "filtered_path": "contracts/rterm-consumer/Cargo.lock",
        "mode": "100644",
        "object_type": "blob",
        "object_oid": CONSUMER_LOCK_SHA,
    },
]
_r1_leaves_by_path = {
    mapping["filtered_path"]: mapping
    for mapping in FIXTURE_FILTERED_HISTORY_MAPPINGS
}
for _mapping in FIXTURE_BOOTSTRAP_TEMPLATE_MAPPINGS:
    _r1_leaves_by_path[_mapping["filtered_path"]] = _mapping
for _mapping in FIXTURE_GENERATED_BOOTSTRAP_FILES:
    _r1_leaves_by_path[_mapping["filtered_path"]] = _mapping
R1_TREE_SHA, R1_TREE_OBJECTS = build_filtered_tree_objects(
    list(_r1_leaves_by_path.values())
)
FIXTURE_TREE_OBJECTS = {**FILTERED_TREE_OBJECTS, **R1_TREE_OBJECTS}


def synthetic_commit(
    message: str,
    parent: str | None = None,
    tree_oid: str | None = None,
) -> tuple[str, bytes]:
    tree_oid = tree_oid or FILTERED_TREE_SHA
    parent_line = b"" if parent is None else f"parent {parent}\n".encode("ascii")
    raw = (
        f"tree {tree_oid}\n".encode("ascii")
        + parent_line
        + b"author Stage 7 <stage7@example.invalid> 1700000000 +0000\n"
        + b"committer Stage 7 <stage7@example.invalid> 1700000000 +0000\n\n"
        + message.encode("ascii")
        + b"\n"
    )
    oid = hashlib.sha1(
        f"commit {len(raw)}\0".encode("ascii") + raw,
        usedforsecurity=False,
    ).hexdigest()
    return oid, raw


FILTERED_SHA, FILTERED_RAW = synthetic_commit(
    "filtered boundary", tree_oid=FILTERED_TREE_SHA
)
R1_SHA, R1_RAW = synthetic_commit(
    "deterministic R1 bootstrap", FILTERED_SHA, tree_oid=R1_TREE_SHA
)
_TREE_SNAPSHOT_CACHE: dict[str, dict] = {}


def parse_raw_tree_for_fixture(raw: bytes) -> list[tuple[str, str, str]]:
    entries: list[tuple[str, str, str]] = []
    cursor = 0
    while cursor < len(raw):
        space = raw.find(b" ", cursor)
        nul = raw.find(b"\0", space + 1)
        if space < 0 or nul < 0 or nul + 21 > len(raw):
            raise AssertionError("malformed Git tree fixture")
        mode = raw[cursor:space].decode("ascii")
        name = raw[space + 1 : nul].decode("utf-8")
        oid = raw[nul + 1 : nul + 21].hex()
        entries.append((mode, name, oid))
        cursor = nul + 21
    return entries


def git_tree_snapshot(root_tree_oid: str) -> dict:
    cached = _TREE_SNAPSHOT_CACHE.get(root_tree_oid)
    if cached is not None:
        return copy.deepcopy(cached)
    records: list[dict] = []
    pending = [root_tree_oid]
    seen: set[str] = set()
    while pending:
        oid = pending.pop()
        if oid in seen:
            continue
        seen.add(oid)
        raw = (
            FIXTURE_TREE_OBJECTS[oid]
            if oid in FIXTURE_TREE_OBJECTS
            else subprocess.run(
                ["git", "cat-file", "tree", oid],
                cwd=REPO,
                check=True,
                capture_output=True,
            ).stdout
        )
        records.append(
            {
                "oid": oid,
                "object_type": "tree",
                "body_base64": base64.b64encode(raw).decode("ascii"),
            }
        )
        pending.extend(
            child_oid
            for mode, _name, child_oid in parse_raw_tree_for_fixture(raw)
            if mode == "40000"
        )
    snapshot = {
        "schema": "rssh.stage7.filtered-tree-snapshot/v1",
        "root_tree_oid": root_tree_oid,
        "tree_objects": sorted(records, key=lambda record: record["oid"]),
    }
    snapshot["snapshot_sha256"] = canonical_sha256(snapshot)
    _TREE_SNAPSHOT_CACHE[root_tree_oid] = copy.deepcopy(snapshot)
    return snapshot


def git_leaf_records(ref: str, selected_paths: set[str] | None = None) -> list[dict]:
    raw = subprocess.run(
        ["git", "ls-tree", "-r", "--full-tree", "-z", ref],
        cwd=REPO,
        check=True,
        capture_output=True,
    ).stdout
    records: list[dict] = []
    for encoded in raw.split(b"\0"):
        if not encoded:
            continue
        metadata, path = encoded.split(b"\t", 1)
        mode, object_type, oid = metadata.decode("ascii").split(" ")
        decoded_path = path.decode("utf-8")
        if selected_paths is not None and decoded_path not in selected_paths:
            continue
        records.append(
            {
                "source_path": decoded_path,
                "filtered_path": decoded_path,
                "mode": mode,
                "object_type": object_type,
                "object_oid": oid,
            }
        )
    return sorted(records, key=lambda record: record["filtered_path"])


def tree_projection_proof(
    filtered_ref: str = FILTERED_SHA,
    filtered_tree_oid: str = FILTERED_TREE_SHA,
) -> dict:
    path_mappings = copy.deepcopy(FIXTURE_FILTERED_HISTORY_MAPPINGS)
    inventory = owned_projection_inventory()
    proof = {
        "schema": "rssh.stage7.tree-projection-proof/v1",
        "r0_ref": PARENT_SHA,
        "filtered_boundary_ref": filtered_ref,
        "source_root_tree_oid": PARENT_TREE_SHA,
        "extraction_manifest_sha256": inventory["inventory_sha256"],
        "filtered_tree_snapshot": git_tree_snapshot(filtered_tree_oid),
        "path_mappings": path_mappings,
    }
    proof["projection_sha256"] = canonical_sha256(proof)
    return proof


def owned_projection_inventory() -> dict:
    contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))
    inventory = {
        "schema": "rssh.stage7.owned-projection-inventory/v1",
        "r0_ref": PARENT_SHA,
        "root_rules": copy.deepcopy(contract["owned_projection_inventory"]),
        "bootstrap_template_mappings": copy.deepcopy(
            contract["bootstrap_template_mappings"]
        ),
        "path_mappings": copy.deepcopy(FIXTURE_OWNED_MAPPINGS),
        "bootstrap_inventory_complete": True,
    }
    inventory["inventory_sha256"] = canonical_sha256(inventory)
    return inventory


def local_two_bare_proof() -> dict:
    return git_object_store_proof(
        [
            complete_git_repository_snapshot(
                "candidate",
                {"candidate": SOURCE_SHA, "lkg_boundary": LKG_SHA},
                history_boundaries=[LKG_SHA],
            ),
            complete_git_repository_snapshot(
                "lkg",
                {"lkg": LKG_SHA},
                history_boundaries=[LKG_SHA],
            ),
        ]
    )


def rterm_object_store_proof() -> dict:
    overrides = {
        FILTERED_SHA: ("commit", FILTERED_RAW),
        R1_SHA: ("commit", R1_RAW),
        ROOT_LOCK_SHA: ("blob", ROOT_LOCK_RAW),
        CONSUMER_LOCK_SHA: ("blob", CONSUMER_LOCK_RAW),
        **{oid: ("tree", raw) for oid, raw in FIXTURE_TREE_OBJECTS.items()},
    }
    return git_object_store_proof(
        [
            complete_git_repository_snapshot(
                "rssh-source",
                {"r0": PARENT_SHA},
                history_boundaries=[PARENT_SHA],
            ),
            complete_git_repository_snapshot(
                "rterm-filtered",
                {"filtered_boundary": FILTERED_SHA, "r1": R1_SHA},
                history_boundaries=[],
                overrides=overrides,
            ),
        ]
    )


def source_to_filtered_map_proof() -> dict:
    records = [{"source_oid": PARENT_SHA, "filtered_oid": FILTERED_SHA}]
    return {
        "schema": "rssh.stage7.source-to-filtered-map-proof/v1",
        "records": records,
        "map_sha256": canonical_sha256(records),
        "source_refs_before": {"r0": PARENT_SHA},
        "source_refs_after": {"r0": PARENT_SHA},
        "tree_projection_proof": tree_projection_proof(),
    }


def bootstrap_projection_proof() -> dict:
    proof = {
        "schema": "rssh.stage7.bootstrap-projection-proof/v1",
        "filtered_boundary_ref": FILTERED_SHA,
        "r1_ref": R1_SHA,
        "filtered_tree_oid": FILTERED_TREE_SHA,
        "r1_tree_oid": R1_TREE_SHA,
        "template_mappings": copy.deepcopy(FIXTURE_BOOTSTRAP_TEMPLATE_MAPPINGS),
        "generated_files": copy.deepcopy(FIXTURE_GENERATED_BOOTSTRAP_FILES),
    }
    proof["projection_sha256"] = canonical_sha256(proof)
    return proof


def load_script(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot import {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


class Stage7SplitGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))
        cls.schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        cls.checker = load_script(CHECKER_PATH, "stage7_split_gate")
        cls.assembler = load_script(ASSEMBLER_PATH, "stage7_evidence_assembler")
        cls.real_checker_git_tree_identity_and_leaves = staticmethod(
            cls.checker.git_tree_identity_and_leaves
        )

        def with_future_bootstrap(original):
            def wrapped(root, commit, label, violations):
                tree_oid, leaves = original(root, commit, label, violations)
                if root.resolve() == REPO.resolve() and commit == PARENT_SHA and leaves:
                    leaves = {path: dict(identity) for path, identity in leaves.items()}
                    for mapping in cls.contract["bootstrap_template_mappings"]:
                        leaves[mapping["source_path"]] = {
                            "mode": "100644",
                            "object_type": "blob",
                            "object_oid": BLOB_SHA,
                        }
                return tree_oid, leaves

            return wrapped

        cls.checker.git_tree_identity_and_leaves = with_future_bootstrap(
            cls.checker.git_tree_identity_and_leaves
        )
        cls.assembler.gate.git_tree_identity_and_leaves = with_future_bootstrap(
            cls.assembler.gate.git_tree_identity_and_leaves
        )

    def make_two_commit_repository(self, root: Path) -> tuple[Path, str, str]:
        repository = root / "history"
        subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
        subprocess.run(
            ["git", "config", "user.name", "Stage 7 Test"],
            cwd=repository,
            check=True,
        )
        subprocess.run(
            ["git", "config", "user.email", "stage7@example.invalid"],
            cwd=repository,
            check=True,
        )
        (repository / "proof.txt").write_text("base\n", encoding="utf-8")
        subprocess.run(["git", "add", "proof.txt"], cwd=repository, check=True)
        subprocess.run(
            ["git", "commit", "--quiet", "-m", "base"],
            cwd=repository,
            check=True,
        )
        base = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repository,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        (repository / "proof.txt").write_text("head\n", encoding="utf-8")
        subprocess.run(
            ["git", "commit", "--quiet", "-am", "head"],
            cwd=repository,
            check=True,
        )
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repository,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        return repository, base, head

    def test_contract_freezes_states_refs_protocol_backends_and_thresholds(self) -> None:
        self.assertEqual(self.contract["schema"], "rssh.stage7-split-contract/v1")
        self.assertEqual(self.contract["initial_state"], "blocked")
        self.assertEqual(
            self.contract["states"],
            [
                "blocked",
                "attribution-ready",
                "windows-memory-go",
                "cross-platform-go",
                "extraction-ready",
                "dual-source-verified",
                "split-complete",
            ],
        )
        self.assertEqual(self.contract["lkg_rssh_ref"], LKG_SHA)
        self.assertNotIn("lkg_rterm_ref", self.contract)
        self.assertEqual(
            self.contract["windows_product_gates"],
            {
                "first_present_p50_ms_max": 400,
                "first_present_p95_ms_max": 500,
                "first_frame_private_bytes_p95_max": 57_671_680,
                "first_frame_private_bytes_max_exclusive": 62_914_560,
                "empty_window_private_working_set_p95_max": 47_185_920,
                "ssh1_private_working_set_p95_max": 62_914_560,
                "gpu_steady_bytes_max": 268_435_456,
                "relative_regression_ratio_max": 1.05,
            },
        )
        self.assertEqual(self.contract["windows_backends"]["required_product"], ["auto"])
        self.assertEqual(
            self.contract["windows_backends"]["diagnostic_only"],
            ["dx12", "vulkan", "gl"],
        )
        self.assertEqual(
            self.contract["diagnostic_probe_outcomes"],
            {
                "statuses": ["supported", "unsupported"],
                "required_product_unsupported": "forbidden",
                "reason_pattern": "^[a-z0-9][a-z0-9-]*$",
                "stage_semantics": "supported-prefix-then-unsupported-suffix",
            },
        )
        protocol = self.contract["protocol"]
        self.assertEqual(protocol["warmups"], 5)
        self.assertEqual(protocol["measured_cold_processes"], 30)
        self.assertEqual(protocol["timeout_seconds"], 60)
        self.assertEqual(protocol["cross_process_percentiles"], "nearest-rank")
        self.assertEqual(protocol["process_representative"], "nearest-rank-p50")
        self.assertEqual(protocol["maximum"], "raw-maximum")
        startup = self.contract["sampling"]["startup"]
        self.assertEqual(startup["marker"], "first_frame_memory")
        self.assertEqual(startup["samples_per_process"], 1)
        self.assertTrue(startup["exit_immediately_after_cpu_bootstrap_present"])
        self.assertEqual(startup["stabilization_ms"], 0)
        residence = self.contract["sampling"]["residence"]
        self.assertEqual(residence["stabilization_ms"], 5_000)
        self.assertEqual(residence["sample_interval_ms"], 100)
        self.assertEqual(residence["samples_per_process"], 10)
        self.assertEqual(residence["owner_ready_marker_required"], True)

    def test_font_ownership_raw_inventory_is_exactly_the_900_ascii_sample_contract(self) -> None:
        groups = self.contract["artifact_policies"]["font-ownership-raw"][
            "required_groups"
        ]
        self.assertEqual(
            groups,
            [
                "current-copied/ascii",
                "shared-all/ascii",
                "lazy/ascii",
            ],
        )
        self.assertEqual(
            len(groups)
            * self.contract["protocol"]["measured_cold_processes"]
            * self.contract["sampling"]["residence"]["samples_per_process"],
            900,
        )
        self.assertFalse(any(group.endswith(("/cjk", "/emoji")) for group in groups))

    def test_font_proof_contract_freezes_reductions_and_functional_claims(self) -> None:
        policy = self.contract["artifact_policies"]["font-ownership-aggregate"]
        self.assertEqual(
            policy["p50_reductions"],
            [
                {
                    "minuend_group": "current-copied/ascii",
                    "subtrahend_group": "shared-all/ascii",
                    "minimum_bytes": 67_108_864,
                },
                {
                    "minuend_group": "shared-all/ascii",
                    "subtrahend_group": "lazy/ascii",
                    "minimum_bytes": 33_554_432,
                },
            ],
        )
        self.assertEqual(
            self.contract["result_claims"]["font-catalog-fingerprint"],
            {
                "catalog_policy_version": {"kind": "non-empty-string"},
                "ordered_sources_hashed": {"kind": "exact", "value": True},
                "functional_specimen_count": {"kind": "exact", "value": 6},
                "zero_tofu": {"kind": "exact", "value": True},
                "single_frame_generation": {"kind": "exact", "value": True},
                "recovery_retained_bytes_stable": {"kind": "exact", "value": True},
                "same_actual_backend": {"kind": "exact", "value": True},
                "activation_latency_report_only": {"kind": "exact", "value": True},
            },
        )

    def test_font_catalog_functional_specimens_are_derived_not_self_attested(self) -> None:
        specimens = self.font_functional_specimens()
        payload = {
            "schema": "rssh.stage7.result/v1",
            "identity": {
                "source_sha": SOURCE_SHA,
                "binary_hashes": {"rssh.exe": BINARY_SHA},
                "runner_fingerprint_sha256": RUNNER_SHA,
                "platform": "windows-x86_64",
                "run_id": "font-functional-run",
            },
            "ok": True,
            "proof": "font-catalog-fingerprint",
            "claims": {
                "catalog_policy_version": "stage7-private-v1",
                "ordered_sources_hashed": True,
                "functional_specimen_count": 6,
                "zero_tofu": True,
                "single_frame_generation": True,
                "recovery_retained_bytes_stable": True,
                "same_actual_backend": True,
                "activation_latency_report_only": True,
            },
            "catalog_fingerprint_sha256": canonical_sha256(specimens),
            "functional_specimens": specimens,
        }
        violations: list[str] = []
        self.checker.validate_result_artifact(
            "font-catalog-fingerprint",
            payload,
            self.contract,
            {},
            REPO,
            "font catalog",
            violations,
        )
        self.assertEqual(violations, [])

        cases = [
            (
                "mode fallback",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "actual_font_mode", "shared"
                ),
                "mode/specimen fallback",
            ),
            (
                "backend fallback",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "requested_backend", "dx12"
                ),
                "requested backend",
            ),
            (
                "mixed backend",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "actual_backend", "vulkan"
                ),
                "actual backend",
            ),
            (
                "negative activation latency",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "activation_latency_ms", -0.001
                ),
                "activation latency",
            ),
            (
                "tofu",
                lambda value: value["functional_specimens"][0].__setitem__("tofu_count", 1),
                "tofu",
            ),
            (
                "mixed frame generation",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "frame_catalog_generation", 3
                ),
                "frame generation",
            ),
            (
                "recovery duplication",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "recovery_retained_source_bytes", 1_000_001
                ),
                "recovery retained",
            ),
            (
                "missing active source",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "active_source_count", 0
                ),
                "indexed/active",
            ),
            (
                "raw path key",
                lambda value: value["functional_specimens"][0].__setitem__(
                    "font_path", "C:/Windows/Fonts/fixture.ttf"
                ),
                "path key",
            ),
            (
                "digest drift",
                lambda value: value.__setitem__("catalog_fingerprint_sha256", "f" * 64),
                "canonical functional specimen digest",
            ),
        ]
        for name, mutate, expected in cases:
            with self.subTest(name=name):
                invalid = copy.deepcopy(payload)
                mutate(invalid)
                if name != "digest drift":
                    invalid["catalog_fingerprint_sha256"] = canonical_sha256(
                        invalid["functional_specimens"]
                    )
                violations = []
                self.checker.validate_result_artifact(
                    "font-catalog-fingerprint",
                    invalid,
                    self.contract,
                    {},
                    REPO,
                    "font catalog",
                    violations,
                )
                self.assertTrue(
                    any(expected in violation for violation in violations), violations
                )

    def test_font_ownership_reductions_use_recomputed_process_medians(self) -> None:
        policy = self.contract["artifact_policies"]["font-ownership-raw"]
        raw = self.make_metric_payload(
            "font-ownership-raw",
            policy,
            {
                "source_sha": SOURCE_SHA,
                "binary_hashes": {"rssh.exe": BINARY_SHA},
                "runner_fingerprint_sha256": RUNNER_SHA,
                "platform": "windows-x86_64",
                "run_id": "font-reduction-run",
            },
        )
        values = [300_000_000, 300_000_000 - 67_108_864, 300_000_000 - 67_108_864 - 33_554_432]
        for group, value in zip(raw["groups"], values):
            for process in group["processes"]:
                process["samples"] = [value] * 10
                process["representative"] = value
            group["statistics"] = {"p50": value, "p95": value, "max": value}
        violations: list[str] = []
        statistics = self.checker.combine_metric_shards(
            "font-ownership-raw",
            [("font-ownership-raw", self.checker.validate_metric_artifact(
                raw,
                policy,
                self.contract,
                "font raw",
                violations,
            ))],
            policy,
            self.contract,
            violations,
        )
        self.assertEqual(len(statistics), 3)
        self.checker.validate_font_ownership_reductions(
            statistics,
            self.contract["artifact_policies"]["font-ownership-aggregate"],
            violations,
        )
        self.assertEqual(violations, [])

        below = copy.deepcopy(statistics)
        below[1]["p50"] += 1
        violations = []
        self.checker.validate_font_ownership_reductions(
            below,
            self.contract["artifact_policies"]["font-ownership-aggregate"],
            violations,
        )
        self.assertTrue(any("minimum p50 reduction" in item for item in violations))

        flattened = copy.deepcopy(raw)
        flattened["groups"][0]["flattened_samples"] = [1] * 300
        violations = []
        self.checker.validate_metric_artifact(
            flattened, policy, self.contract, "font raw", violations
        )
        self.assertTrue(any("flattened" in item for item in violations))

        wrong_name = copy.deepcopy(raw)
        wrong_name["groups"][0]["name"] = "current/ascii"
        violations = []
        groups = self.checker.validate_metric_artifact(
            wrong_name, policy, self.contract, "font raw", violations
        )
        self.checker.combine_metric_shards(
            "font-ownership-raw",
            [("font-ownership-raw", groups)],
            policy,
            self.contract,
            violations,
        )
        self.assertTrue(any("group inventory" in item for item in violations))

    def test_contract_freezes_every_bootstrap_template_source_and_target(self) -> None:
        expected = [
            {"source_path": "release/rterm-bootstrap/Cargo.toml", "filtered_path": "Cargo.toml"},
            {
                "source_path": "release/rterm-bootstrap/rust-toolchain.toml",
                "filtered_path": "rust-toolchain.toml",
            },
            {"source_path": "release/rterm-bootstrap/.gitignore", "filtered_path": ".gitignore"},
            {
                "source_path": "release/rterm-bootstrap/.gitattributes",
                "filtered_path": ".gitattributes",
            },
            {"source_path": "release/rterm-bootstrap/README.md", "filtered_path": "README.md"},
            {
                "source_path": "release/rterm-bootstrap/CONTRIBUTING.md",
                "filtered_path": "CONTRIBUTING.md",
            },
            {"source_path": "release/rterm-bootstrap/SECURITY.md", "filtered_path": "SECURITY.md"},
            {"source_path": "release/rterm-bootstrap/LICENSE", "filtered_path": "LICENSE"},
            {"source_path": "release/rterm-bootstrap/NOTICE", "filtered_path": "NOTICE"},
            {"source_path": "release/rterm-bootstrap/deny.toml", "filtered_path": "deny.toml"},
            {
                "source_path": "release/rterm-bootstrap/.github/workflows/ci.yml",
                "filtered_path": ".github/workflows/ci.yml",
            },
            {
                "source_path": "release/rterm-bootstrap/contracts/rterm-consumer/Cargo.toml",
                "filtered_path": "contracts/rterm-consumer/Cargo.toml",
            },
            {
                "source_path": "release/rterm-bootstrap/docs/release-policy.md",
                "filtered_path": "docs/release-policy.md",
            },
        ]
        self.assertEqual(self.contract.get("bootstrap_template_mappings"), expected)

    def test_windows_deterministic_suite_is_the_exact_approved_ordered_suite(self) -> None:
        expected = [
            {
                "id": "format",
                "argv": ["cargo", "fmt", "--all", "--", "--check"],
                "exit_code": 0,
            },
            {
                "id": "clippy",
                "argv": [
                    "cargo",
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
                "exit_code": 0,
            },
            {
                "id": "workspace-tests",
                "argv": [
                    "cargo",
                    "test",
                    "--workspace",
                    "--all-targets",
                    "--locked",
                    "-j1",
                ],
                "exit_code": 0,
            },
            {
                "id": "python-ci-tests",
                "argv": [
                    "python",
                    "-m",
                    "unittest",
                    "discover",
                    "-s",
                    "scripts/ci/tests",
                    "-p",
                    "test_*.py",
                    "-v",
                ],
                "exit_code": 0,
            },
        ]
        self.assertEqual(self.contract["windows_deterministic_suite"], expected)
        self.assertEqual(
            self.contract["result_claims"]["windows-deterministic-suite"][
                "exact_suite"
            ],
            {"kind": "exact", "value": expected},
        )

        mutations = {
            "missing": lambda suite: suite.pop(),
            "replaced": lambda suite: suite[0].__setitem__("id", "replacement"),
            "duplicated": lambda suite: suite.__setitem__(1, copy.deepcopy(suite[0])),
            "reordered": lambda suite: suite.__setitem__(
                slice(None), [suite[1], suite[0], *suite[2:]]
            ),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "windows-memory-go")[
                    "windows-memory-go"
                ]
                self.mutate_artifact(
                    root,
                    manifest,
                    "windows-deterministic-suite",
                    lambda artifact, mutate=mutate: mutate(
                        artifact["claims"]["exact_suite"]
                    ),
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "windows-memory-go", manifest
                )
                self.assertFalse(decision["ok"], decision)

    def test_manifest_schema_closes_artifact_types_and_identity_fields(self) -> None:
        self.assertEqual(
            self.contract["evidence_manifest_schema_sha256"], sha256(SCHEMA_PATH)
        )
        self.assertEqual(
            self.schema["$id"],
            "https://r-ssh.dev/schemas/stage7-evidence-manifest-v1.json",
        )
        self.assertEqual(
            self.schema["properties"]["schema"]["const"],
            "rssh.stage7-evidence-manifest/v1",
        )
        entry = self.schema["$defs"]["entry"]
        self.assertFalse(entry["additionalProperties"])
        for field in (
            "artifact_type",
            "artifact_id",
            "role",
            "path",
            "sha256",
            "producing_command",
            "source_sha",
            "platform",
            "run_id",
        ):
            self.assertIn(field, entry["required"])
        self.assertEqual(
            set(entry["properties"]["artifact_type"]["enum"]),
            set(self.contract["artifact_types"]),
        )

    def test_contract_digest_freezes_every_nested_policy_and_claim(self) -> None:
        for mutation in (
            lambda contract: contract["artifact_policies"][
                "windows-first-present-raw"
            ].pop("thresholds"),
            lambda contract: contract["artifact_policies"][
                "windows-first-frame-raw"
            ].pop("same_machine_lkg"),
        ):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                contract = copy.deepcopy(self.contract)
                mutation(contract)
                contract_path = root / CONTRACT_PATH.name
                write_json(contract_path, contract)
                (root / SCHEMA_PATH.name).write_bytes(SCHEMA_PATH.read_bytes())
                _loaded, violations = self.checker.validate_contract(contract_path)
                self.assertTrue(
                    any("frozen contract digest" in item for item in violations),
                    violations,
                )

    def test_canonical_sha256_streams_without_materializing_one_large_json_string(self) -> None:
        value = {"large": ["x" * 1_024 for _ in range(128)]}
        expected = canonical_sha256(value)
        with mock.patch.object(
            self.checker.json,
            "dumps",
            side_effect=AssertionError("canonical hashing must stream JSON"),
        ):
            observed = self.checker.canonical_sha256(value)
        self.assertEqual(observed, expected)

    def test_json_inputs_are_size_bounded_before_parsing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "oversized.json"
            path.write_text("{}", encoding="utf-8")
            with mock.patch.object(self.checker, "MAX_JSON_BYTES", 1):
                violations: list[str] = []
                self.assertIsNone(self.checker.read_json(path, "oversized", violations))
                self.assertTrue(any("size limit" in item for item in violations), violations)
            with mock.patch.object(self.assembler.gate, "MAX_JSON_BYTES", 1):
                with self.assertRaises(self.assembler.EvidenceError) as error:
                    self.assembler.load_json(path, "oversized")
                self.assertIn("size limit", str(error.exception))

    def test_bounded_json_reader_uses_small_fixed_chunks(self) -> None:
        class ReadGuard(io.BytesIO):
            def read(self, size=-1):
                if size < 0 or size > 1024 * 1024:
                    raise AssertionError("bounded JSON reads must use chunks of at most 1 MiB")
                return super().read(size)

        with mock.patch.object(Path, "open", return_value=ReadGuard(b"{}")):
            self.assertEqual(
                self.checker.read_bounded_json_text(Path("unused.json")),
                "{}",
            )

    def test_windows_ads_and_reserved_device_paths_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for value in (
                "fragment.json:artifact",
                "NUL",
                "nested/aux.txt",
                "COM¹",
                "nested/COM².txt",
                "nested/COM³",
                "LPT¹.txt",
                "nested/LPT²",
                "nested/LPT³.log",
                "nested/trailing.",
                "nested/trailing ",
            ):
                with self.subTest(path=value):
                    violations: list[str] = []
                    self.assertIsNone(
                        self.checker.contained_file(
                            root,
                            value,
                            "evidence",
                            violations,
                            must_exist=False,
                        )
                    )
                    self.assertTrue(violations)
                    self.assertFalse(self.checker.safe_git_path(value))
                    with self.assertRaises(self.assembler.EvidenceError):
                        self.assembler.cli_relative(value, "fragment")

    def test_git_environment_ignores_system_and_global_configuration(self) -> None:
        environment = self.checker.clean_git_environment()
        self.assertEqual(environment["GIT_CONFIG_NOSYSTEM"], "1")
        self.assertEqual(environment["GIT_CONFIG_GLOBAL"], os.devnull)

    def test_assembler_cli_fails_closed_as_one_json_for_unexpected_errors(self) -> None:
        argv = [
            str(ASSEMBLER_PATH),
            "--contract",
            str(CONTRACT_PATH),
            "--requested-state",
            "attribution-ready",
            "--evidence-root",
            str(REPO),
            "--fragment",
            "fragment.json",
            "--output",
            "manifest.json",
        ]
        stdout = io.StringIO()
        with (
            mock.patch.object(sys, "argv", argv),
            mock.patch.object(
                self.assembler,
                "assemble",
                side_effect=TypeError("sensitive malformed value"),
            ),
            contextlib.redirect_stdout(stdout),
        ):
            exit_code = self.assembler.main()
        self.assertEqual(exit_code, 1)
        self.assertEqual(len(stdout.getvalue().splitlines()), 1)
        decision = json.loads(stdout.getvalue())
        self.assertFalse(decision["ok"])
        self.assertFalse(decision["go"])
        self.assertEqual(decision["decision"], "NO-GO")
        self.assertNotIn("sensitive malformed value", stdout.getvalue())

    def test_assembler_executes_the_frozen_manifest_schema_patterns(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            entry = next(
                item
                for item in data["entries"]
                if item["artifact_type"] == "runner-fingerprint"
            )
            artifact = fragment.parent / entry["path"]
            payload = json.loads(artifact.read_text(encoding="utf-8"))
            entry["run_id"] = "bad/run"
            payload["identity"]["run_id"] = entry["run_id"]
            entry["cohort_id"] = self.checker.cohort_id(entry)
            write_json(artifact, payload)
            entry["sha256"] = sha256(artifact)
            entry["size_bytes"] = artifact.stat().st_size
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertIn("run_id", str(error.exception))

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            for entry in data["entries"]:
                if "binary_hashes" not in entry:
                    continue
                entry["binary_hashes"] = {"bad/key": BINARY_SHA}
                entry["cohort_id"] = self.checker.cohort_id(entry)
                artifact = fragment.parent / entry["path"]
                payload = json.loads(artifact.read_text(encoding="utf-8"))
                payload["identity"]["binary_hashes"] = entry["binary_hashes"]
                write_json(artifact, payload)
                entry["sha256"] = sha256(artifact)
                entry["size_bytes"] = artifact.stat().st_size
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertIn("binary hashes", str(error.exception))

    def test_nearest_rank_and_process_representative_do_not_interpolate(self) -> None:
        self.assertEqual(self.checker.nearest_rank([4, 1, 3, 2], 0.50), 2)
        self.assertEqual(self.checker.nearest_rank([4, 1, 3, 2], 0.95), 4)
        self.assertEqual(self.checker.process_representative(range(1, 11)), 5)
        with self.assertRaises(ValueError):
            self.checker.nearest_rank([], 0.95)

    def test_blocked_is_a_successful_no_go_and_rejects_any_manifest(self) -> None:
        command = [
            str(PYTHON),
            str(CHECKER_PATH),
            "--contract",
            str(CONTRACT_PATH),
            "--requested-state",
            "blocked",
        ]
        result = subprocess.run(command, cwd=REPO, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        decision = json.loads(result.stdout)
        self.assertEqual(decision["state"], "blocked")
        self.assertEqual(decision["decision"], "NO-GO")
        self.assertFalse(decision["go"])
        self.assertIn("physical repository split", decision["reason"])

        result = subprocess.run(
            command + ["--evidence-manifest", str(CONTRACT_PATH)],
            cwd=REPO,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        self.assertIn("does not accept", " ".join(json.loads(result.stdout)["violations"]))

    def test_real_fragments_assemble_and_recursive_chain_validates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifests = self.build_chain(root, "split-complete")
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "split-complete", manifests["split-complete"]
            )
            self.assertTrue(decision["ok"], decision["violations"])
            self.assertTrue(decision["go"])
            self.assertEqual(decision["state"], "split-complete")
            self.assertEqual(
                decision["artifact_count"],
                len(json.loads(manifests["split-complete"].read_text(encoding="utf-8"))["entries"]),
            )

    def test_every_state_fails_when_each_required_artifact_is_omitted(self) -> None:
        for state in self.contract["states"][1:]:
            with self.subTest(state=state), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, state)[state]
                original = json.loads(manifest.read_text(encoding="utf-8"))
                for artifact_type in self.contract["required_artifacts_by_state"][state]:
                    with self.subTest(state=state, artifact_type=artifact_type):
                        mutated = copy.deepcopy(original)
                        mutated["entries"] = [
                            entry
                            for entry in mutated["entries"]
                            if entry["artifact_type"] != artifact_type
                        ]
                        write_json(manifest, mutated)
                        decision = self.checker.validate_gate(CONTRACT_PATH, state, manifest)
                        self.assertFalse(decision["ok"])
                        self.assertTrue(
                            any(artifact_type in item for item in decision["violations"]),
                            decision["violations"],
                        )
                write_json(manifest, original)

    def test_assembler_rejects_escape_duplicate_and_unreferenced_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = "attribution-ready"
            fragment = self.make_fragment(root, state)
            fragment_data = json.loads(fragment.read_text(encoding="utf-8"))

            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH, state, root, [fragment, fragment], root / "out.json"
                )

            escaped = root.parent / f"{root.name}-escape.json"
            write_json(escaped, {"ok": True})
            try:
                bad = copy.deepcopy(fragment_data)
                bad["entries"][0]["path"] = "../../" + escaped.name
                bad["entries"][0]["sha256"] = sha256(escaped)
                write_json(fragment, bad)
                with self.assertRaises(self.assembler.EvidenceError):
                    self.assembler.assemble(
                        CONTRACT_PATH, state, root, [fragment], root / "out.json"
                    )
            finally:
                escaped.unlink(missing_ok=True)

            write_json(fragment, fragment_data)
            duplicate = root / "fragments/duplicate/artifact-manifest-fragment.json"
            write_json(duplicate, fragment_data)
            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH, state, root, [fragment, duplicate], root / "out.json"
                )

            duplicate.unlink()
            (root / "unreferenced.txt").write_text("not evidence", encoding="utf-8")
            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH, state, root, [fragment], root / "out.json"
                )

    def test_validator_rejects_hash_containment_and_unreferenced_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]
            data = json.loads(manifest.read_text(encoding="utf-8"))
            first = data["entries"][0]
            artifact = root / first["path"]
            artifact.write_text("tampered", encoding="utf-8")
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("SHA-256" in item for item in decision["violations"]))

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]
            data = json.loads(manifest.read_text(encoding="utf-8"))
            data["entries"][0]["path"] = "../escape.json"
            write_json(manifest, data)
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("contained" in item for item in decision["violations"]))

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]
            (root / "unreferenced.txt").write_text("unexpected", encoding="utf-8")
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("unreferenced" in item for item in decision["violations"]))

    def test_validator_rejects_missing_or_drifting_embedded_identity(self) -> None:
        mutations = {
            "missing source identity": lambda entry: entry.pop("source_sha"),
            "missing binary identity": lambda entry: entry.pop("binary_hashes"),
            "missing runner identity": lambda entry: entry.pop("runner_fingerprint_sha256"),
            "mutable source ref": lambda entry: entry.__setitem__("source_sha", "main"),
            "stale binary hash": lambda entry: entry.__setitem__(
                "binary_hashes", {"rssh.exe": "d" * 64}
            ),
            "runner fingerprint mismatch": lambda entry: entry.__setitem__(
                "runner_fingerprint_sha256", "e" * 64
            ),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "windows-memory-go")[
                    "windows-memory-go"
                ]
                data = json.loads(manifest.read_text(encoding="utf-8"))
                entry = next(
                    item
                    for item in data["entries"]
                    if item["artifact_type"] == "windows-first-present-raw"
                )
                mutate(entry)
                write_json(manifest, data)
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "windows-memory-go", manifest
                )
                self.assertFalse(decision["ok"])
                self.assertTrue(decision["violations"])
    def test_validator_rejects_backend_renderer_connection_and_threshold_drift(self) -> None:
        cases = [
            ("windows-empty-window-raw", "requested_backend", "dx12"),
            ("windows-empty-window-raw", "final_renderer", "cpu"),
            ("windows-ssh1-raw", "connection_state", "failed"),
            ("windows-gpu-steady-raw", "final_renderer", "cpu"),
            ("windows-first-frame-raw", "final_renderer", "gpu"),
            ("windows-first-present-raw", "actual_backend", "dx12"),
            ("windows-first-frame-raw", "adapter_identity", "unexpected-adapter"),
        ]
        for artifact_type, field, value in cases:
            with self.subTest(artifact_type=artifact_type, field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "windows-memory-go")[
                    "windows-memory-go"
                ]
                self.mutate_artifact(
                    root,
                    manifest,
                    artifact_type,
                    lambda artifact, field=field, value=value: artifact["groups"][0].__setitem__(
                        field, value
                    ),
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "windows-memory-go", manifest
                )
                self.assertFalse(decision["ok"])
                self.assertTrue(decision["violations"])
                if field == "adapter_identity":
                    self.assertTrue(
                        any(
                            "startup CPU bootstrap" in item
                            for item in decision["violations"]
                        )
                    )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")[
                "windows-memory-go"
            ]

            def exceed_threshold(artifact: dict) -> None:
                group = artifact["groups"][0]
                for process in group["processes"]:
                    process["value"] = 501
                group["statistics"] = {"p50": 501, "p95": 501, "max": 501}

            self.mutate_artifact(
                root, manifest, "windows-first-present-raw", exceed_threshold
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("threshold" in item for item in decision["violations"]))

    def test_validator_recomputes_raw_statistics_and_rejects_wrong_sampling_shapes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")[
                "windows-memory-go"
            ]

            def forge_startup(artifact: dict) -> None:
                group = artifact["groups"][0]
                group["processes"][0]["residence_samples"] = [1] * 10

            self.mutate_artifact(
                root, manifest, "windows-first-frame-raw", forge_startup
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("startup" in item for item in decision["violations"]))

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")[
                "windows-memory-go"
            ]

            def flatten_residence(artifact: dict) -> None:
                group = artifact["groups"][0]
                group["flattened_samples"] = [
                    value
                    for process in group.pop("processes")
                    for value in process["samples"]
                ]

            self.mutate_artifact(
                root, manifest, "windows-empty-window-raw", flatten_residence
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("flatten" in item for item in decision["violations"]))

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")[
                "windows-memory-go"
            ]

            def forge_summary(artifact: dict) -> None:
                artifact["groups"][0]["statistics"]["p95"] -= 1

            self.mutate_artifact(
                root, manifest, "windows-empty-window-raw", forge_summary
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("recomputed" in item for item in decision["violations"]))

    def test_summary_requires_raw_children_and_exact_recomputed_statistics(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]

            def remove_children(artifact: dict) -> None:
                artifact["raw_children"] = []

            self.mutate_artifact(
                root, manifest, "font-ownership-aggregate", remove_children
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("raw child" in item for item in decision["violations"]))

    def test_result_and_aggregate_payloads_reject_unknown_fields(self) -> None:
        for artifact_type in ("runner-fingerprint", "font-ownership-aggregate"):
            with self.subTest(artifact_type=artifact_type), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "attribution-ready")[
                    "attribution-ready"
                ]
                self.mutate_artifact(
                    root,
                    manifest,
                    artifact_type,
                    lambda artifact: artifact.__setitem__("unexpected_field", True),
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "attribution-ready", manifest
                )
                self.assertFalse(decision["ok"], decision)
                self.assertIn("unexpected_field", " ".join(decision["violations"]))

    def test_raw_metric_payload_and_every_nested_record_are_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")[
                "windows-memory-go"
            ]

            def add_unknown_fields(artifact: dict) -> None:
                group = artifact["groups"][0]
                artifact["raw_root_extra"] = True
                group["group_extra"] = True
                group["processes"][0]["process_extra"] = True
                group["lkg"]["lkg_extra"] = True
                group["lkg"]["processes"][0]["lkg_process_extra"] = True

            self.mutate_artifact(
                root,
                manifest,
                "windows-empty-window-raw",
                add_unknown_fields,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"], decision)
            joined = " ".join(decision["violations"])
            for field in (
                "raw_root_extra",
                "group_extra",
                "process_extra",
                "lkg_extra",
                "lkg_process_extra",
            ):
                self.assertIn(field, joined)

    def test_requested_state_and_predecessor_chain_cannot_skip_or_reorder(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifests = self.build_chain(root, "cross-platform-go")
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifests["attribution-ready"]
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("requested state" in item for item in decision["violations"]))

            current = manifests["cross-platform-go"]
            data = json.loads(current.read_text(encoding="utf-8"))
            prior = manifests["attribution-ready"]
            data["prior_manifest"] = {
                "path": prior.relative_to(root).as_posix(),
                "sha256": sha256(prior),
                "certified_state": "attribution-ready",
                "certified_commit": SOURCE_SHA,
            }
            write_json(current, data)
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "cross-platform-go", current
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("immediate predecessor" in item for item in decision["violations"]))

    def test_assembler_cli_is_deterministic_and_validator_stdout_is_one_json_decision(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            command = [
                str(PYTHON),
                str(ASSEMBLER_PATH),
                "--contract",
                str(CONTRACT_PATH),
                "--requested-state",
                "attribution-ready",
                "--evidence-root",
                str(root),
                "--fragment",
                fragment.relative_to(root).as_posix(),
                "--output",
                "manifest.json",
            ]
            result = subprocess.run(command, cwd=REPO, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(len(result.stdout.splitlines()), 1)
            assembled = json.loads(result.stdout)
            self.assertEqual(assembled["requested_state"], "attribution-ready")

            check = subprocess.run(
                [
                    str(PYTHON),
                    str(CHECKER_PATH),
                    "--contract",
                    str(CONTRACT_PATH),
                    "--requested-state",
                    "attribution-ready",
                    "--evidence-manifest",
                    str(root / "manifest.json"),
                ],
                cwd=REPO,
                capture_output=True,
                text=True,
            )
            self.assertEqual(check.returncode, 0, check.stderr)
            self.assertEqual(len(check.stdout.splitlines()), 1)
            self.assertTrue(json.loads(check.stdout)["go"])

    def test_pre_extraction_states_require_the_exact_same_candidate_commit(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            attribution = self.make_fragment(root, "attribution-ready")
            self.rewrite_fragment_source(attribution, PARENT_SHA)
            prior = root / "attribution-ready.json"
            self.assembler.assemble(
                CONTRACT_PATH,
                "attribution-ready",
                root,
                [attribution],
                prior,
            )
            windows = self.make_fragment(root, "windows-memory-go")
            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "windows-memory-go",
                    root,
                    [windows],
                    root / "windows-memory-go.json",
                    prior_manifest=prior,
                )

    def test_current_commit_must_be_a_real_descendant_of_later_predecessor(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "extraction-ready")["extraction-ready"]
            dual = self.make_fragment(root, "dual-source-verified")
            self.rewrite_fragment_source(dual, LKG_SHA)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "dual-source-verified",
                    root,
                    [dual],
                    root / "dual-source-verified.json",
                    prior_manifest=prior,
                )
            self.assertIn("not descended", str(error.exception))

    def test_local_two_bare_git_proof_binds_the_candidate_and_lkg_commits(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]
            self.mutate_artifact(
                root,
                manifest,
                "local-two-bare-git-source-proof",
                lambda artifact: artifact.__setitem__(
                    "source_refs", ["d" * 40, "e" * 40]
                ),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("candidate" in item or "LKG" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_local_two_bare_git_proof_recomputes_commit_objects_and_store_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]

            def forge_object_proof(artifact: dict) -> None:
                artifact["git_object_store_proof"] = {
                    "schema": "rssh.stage7.git-object-store-proof/v1",
                    "object_format": "sha1",
                    "repositories": [],
                }

            self.mutate_artifact(
                root,
                manifest,
                "local-two-bare-git-source-proof",
                forge_object_proof,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any(
                    "commit object" in item
                    or "object database" in item
                    or "repository inventory" in item
                    for item in decision["violations"]
                ),
                decision["violations"],
            )

            manifest = self.build_chain(root / "duplicate-identity", "attribution-ready")[
                "attribution-ready"
            ]

            def duplicate_replayable_store(artifact: dict) -> None:
                repositories = artifact["git_object_store_proof"]["repositories"]
                repositories[1]["bare_repository_snapshot"] = copy.deepcopy(
                    repositories[0]["bare_repository_snapshot"]
                )
                for repository in repositories:
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

            self.mutate_artifact(
                root / "duplicate-identity",
                manifest,
                "local-two-bare-git-source-proof",
                duplicate_replayable_store,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("replay" in item or "bare repository" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_git_object_store_proof_requires_complete_commit_tree_blob_closure(self) -> None:
        proof = git_object_store_proof(
            [
                git_repository_snapshot(
                    "candidate",
                    {"candidate": SOURCE_SHA},
                    [git_commit_object(SOURCE_SHA)],
                ),
                git_repository_snapshot(
                    "lkg",
                    {"lkg": LKG_SHA},
                    [git_commit_object(LKG_SHA)],
                ),
            ]
        )
        violations: list[str] = []
        self.checker.validate_git_object_store_proof(
            proof,
            {
                "candidate": {"candidate": SOURCE_SHA},
                "lkg": {"lkg": LKG_SHA},
            },
            "ref-only attack",
            violations,
        )
        self.assertTrue(
            any("tree/blob" in item or "reachable object closure" in item for item in violations),
            violations,
        )

    def test_git_object_base64_is_rejected_before_oversized_decode(self) -> None:
        encoded = "A" * (self.checker.MAX_GIT_OBJECT_BASE64_CHARS + 1)
        violations: list[str] = []
        with mock.patch.object(
            self.checker.base64,
            "b64decode",
            side_effect=AssertionError("oversized base64 reached decoder"),
        ):
            _oid, _object_type, raw = self.checker.decode_git_object_record(
                {
                    "oid": "a" * 40,
                    "object_type": "blob",
                    "body_base64": encoded,
                },
                "oversized object",
                violations,
            )
        self.assertIsNone(raw)
        self.assertTrue(any("encoded size limit" in item for item in violations), violations)

    def test_loose_git_object_decompression_has_an_output_limit(self) -> None:
        compressed = zlib.compress(b"A" * 2_048, level=9)
        violations: list[str] = []
        with mock.patch.object(self.checker, "MAX_LOOSE_OBJECT_BYTES", 1_024):
            output = self.checker.bounded_zlib_decompress(
                compressed,
                "decompression bomb",
                violations,
            )
        self.assertIsNone(output)
        self.assertTrue(any("decompressed size limit" in item for item in violations), violations)

    def test_bounded_decompression_does_not_allocate_a_limit_sized_flush_buffer(self) -> None:
        real_factory = self.checker.zlib.decompressobj

        class FlushGuard:
            def __init__(self) -> None:
                self.inner = real_factory()

            def decompress(self, *args, **kwargs):
                return self.inner.decompress(*args, **kwargs)

            def flush(self, *_args, **_kwargs):
                raise AssertionError("bounded decompression must not allocate through flush")

            def __getattr__(self, name):
                return getattr(self.inner, name)

        violations: list[str] = []
        with mock.patch.object(
            self.checker.zlib,
            "decompressobj",
            side_effect=FlushGuard,
        ):
            output = self.checker.bounded_zlib_decompress(
                zlib.compress(b"small valid object"),
                "small object",
                violations,
            )
        self.assertEqual(output, b"small valid object")
        self.assertEqual(violations, [])

    def test_cumulative_payload_budgets_stop_before_decoding_or_retaining(self) -> None:
        class Base64PreflightGuard(str):
            def rstrip(self, *_args, **_kwargs):
                raise AssertionError("base64 preflight must not copy the encoded payload")

        self.assertEqual(
            self.checker.declared_canonical_base64_length(Base64PreflightGuard("eHg=")),
            2,
        )
        oversized_body = base64.b64encode(b"xx").decode("ascii")

        replay = {
            "schema": "rssh.stage7.replayable-bare-repository/v1",
            "files": [
                {"path": path, "body_base64": oversized_body}
                for path in ("HEAD", "config", "refs/heads/proof", "shallow")
            ],
            "snapshot_sha256": "0" * 64,
        }
        replay_violations: list[str] = []
        with (
            mock.patch.object(self.checker, "MAX_REPLAY_TOTAL_BYTES", 2),
            mock.patch.object(
                self.checker,
                "decode_canonical_base64",
                wraps=self.checker.decode_canonical_base64,
            ) as replay_decoder,
        ):
            replay_digest = self.checker.validate_replayable_bare_repository(
                replay,
                {},
                [],
                {},
                "oversized replay",
                replay_violations,
            )
        self.assertIsNone(replay_digest)
        self.assertEqual(replay_decoder.call_count, 1)
        self.assertTrue(any("192 MiB" in item for item in replay_violations), replay_violations)

        git_proof = {
            "schema": "rssh.stage7.git-object-store-proof/v1",
            "object_format": "sha1",
            "repositories": [
                {
                    "role": "candidate",
                    "bare": True,
                    "alternates": [],
                    "refs": {"candidate": "a" * 40},
                    "history_boundaries": [],
                    "git_objects": [
                        {
                            "oid": "b" * 40,
                            "object_type": "blob",
                            "body_base64": oversized_body,
                        },
                        {
                            "oid": "c" * 40,
                            "object_type": "blob",
                            "body_base64": oversized_body,
                        },
                    ],
                    "bare_repository_snapshot": {},
                    "snapshot_sha256": "0" * 64,
                }
            ],
        }
        closure_violations: list[str] = []
        with (
            mock.patch.object(self.checker, "MAX_GIT_CLOSURE_TOTAL_BYTES", 2),
            mock.patch.object(
                self.checker,
                "decode_git_object_record",
                wraps=self.checker.decode_git_object_record,
            ) as git_decoder,
        ):
            repositories = self.checker.validate_git_object_store_proof(
                git_proof,
                {"candidate": {"candidate": "a" * 40}},
                "oversized Git closure",
                closure_violations,
            )
        self.assertEqual(repositories, {})
        self.assertEqual(git_decoder.call_count, 1)
        self.assertTrue(any("192 MiB" in item for item in closure_violations), closure_violations)

        filtered_snapshot = {
            "schema": "rssh.stage7.filtered-tree-snapshot/v1",
            "root_tree_oid": "c" * 40,
            "tree_objects": [
                {
                    "oid": "c" * 40,
                    "object_type": "tree",
                    "body_base64": oversized_body,
                },
                {
                    "oid": "d" * 40,
                    "object_type": "tree",
                    "body_base64": oversized_body,
                },
            ],
            "snapshot_sha256": "0" * 64,
        }
        filtered_violations: list[str] = []
        with (
            mock.patch.object(self.checker, "MAX_FILTERED_TREE_TOTAL_BYTES", 2),
            mock.patch.object(
                self.checker,
                "decode_canonical_base64",
                wraps=self.checker.decode_canonical_base64,
            ) as tree_decoder,
        ):
            leaves = self.checker.validate_filtered_tree_snapshot(
                filtered_snapshot,
                "c" * 40,
                "oversized filtered tree",
                filtered_violations,
            )
        self.assertEqual(leaves, {})
        self.assertEqual(tree_decoder.call_count, 1)
        self.assertTrue(any("16 MiB" in item for item in filtered_violations), filtered_violations)

    def test_malformed_git_metadata_cannot_bypass_the_cumulative_decode_budget(self) -> None:
        encoded = base64.b64encode(b"xx").decode("ascii")
        proof = {
            "schema": "rssh.stage7.git-object-store-proof/v1",
            "object_format": "sha1",
            "repositories": [
                {
                    "role": "candidate",
                    "bare": True,
                    "alternates": [],
                    "refs": {"candidate": "a" * 40},
                    "history_boundaries": [],
                    "git_objects": [
                        {
                            "oid": invalid_oid,
                            "object_type": "blob",
                            "body_base64": encoded,
                        }
                        for invalid_oid in ("not-an-oid", "still-not-an-oid")
                    ],
                    "bare_repository_snapshot": {},
                    "snapshot_sha256": "0" * 64,
                }
            ],
        }
        violations: list[str] = []
        with (
            mock.patch.object(self.checker, "MAX_GIT_CLOSURE_TOTAL_BYTES", 2),
            mock.patch.object(
                self.checker,
                "decode_canonical_base64",
                wraps=self.checker.decode_canonical_base64,
            ) as base64_decoder,
            mock.patch.object(
                self.checker,
                "decode_git_object_record",
                wraps=self.checker.decode_git_object_record,
            ) as object_decoder,
        ):
            repositories = self.checker.validate_git_object_store_proof(
                proof,
                {"candidate": {"candidate": "a" * 40}},
                "malformed closure",
                violations,
            )
        self.assertEqual(repositories, {})
        self.assertEqual(object_decoder.call_count, 1)
        self.assertEqual(base64_decoder.call_count, 0)
        self.assertTrue(any("192 MiB" in item for item in violations), violations)

    def test_large_cross_repository_proofs_are_released_before_predecessor_recursion(self) -> None:
        class TrackedArtifact(dict):
            pass

        proof = TrackedArtifact({"large-proof": "x"})
        aggregate = {"group_statistics": []}
        fingerprint = {"fingerprint_sha256": "a" * 64}
        entries_by_type = {
            "source-to-filtered-history-map": [{"artifact_id": "proof"}],
            "font-ownership-aggregate": [{"artifact_id": "aggregate"}],
            "runner-fingerprint": [{"artifact_id": "fingerprint"}],
        }
        artifacts = {
            "proof": proof,
            "aggregate": aggregate,
            "fingerprint": fingerprint,
        }
        proof_reference = weakref.ref(proof)

        policies = self.contract["artifact_policies"]
        self.assertFalse(
            self.checker.artifact_payload_needed_after_individual_validation(
                "local-two-bare-git-source-proof",
                policies["local-two-bare-git-source-proof"],
            )
        )
        for artifact_type in (
            "source-to-filtered-history-map",
            "rterm-external-source-proof",
            "rterm-extraction-manifest",
            "font-ownership-aggregate",
            "runner-fingerprint",
        ):
            with self.subTest(retained_type=artifact_type):
                self.assertTrue(
                    self.checker.artifact_payload_needed_after_individual_validation(
                        artifact_type,
                        policies[artifact_type],
                    )
                )

        retained = self.checker.retain_post_cross_repository_artifacts(
            entries_by_type,
            artifacts,
            self.contract,
        )
        self.assertEqual(set(retained), {"aggregate", "fingerprint"})
        del proof
        del artifacts
        self.assertIsNone(
            proof_reference(),
            "large Git proof payload remained live across predecessor recursion",
        )

    def test_tree_entry_and_expanded_path_byte_budgets_are_hard_limits(self) -> None:
        first_blob = "1" * 40
        second_blob = "2" * 40
        raw = (
            b"100644 a\0"
            + bytes.fromhex(first_blob)
            + b"100644 b\0"
            + bytes.fromhex(second_blob)
        )
        entry_budget = {"entries": 0, "exceeded": False}
        entry_violations: list[str] = []
        with mock.patch.object(self.checker, "MAX_PARSED_TREE_ENTRIES", 1):
            entries = self.checker.parse_raw_git_tree(
                raw,
                "too many tree entries",
                entry_violations,
                entry_budget,
            )
        self.assertEqual(entries, [])
        self.assertTrue(entry_budget["exceeded"])
        self.assertTrue(any("entry budget" in item for item in entry_violations), entry_violations)

        root_oid = "3" * 40
        trees = {
            root_oid: [
                {
                    "mode": "100644",
                    "name": name,
                    "object_type": "blob",
                    "oid": first_blob,
                }
                for name in ("aa", "bb")
            ]
        }
        path_violations: list[str] = []
        with mock.patch.object(self.checker, "MAX_EXPANDED_TREE_PATH_BYTES", 2):
            leaves = self.checker.flatten_git_tree(
                root_oid,
                trees,
                "expanded paths",
                path_violations,
            )
        self.assertEqual(leaves, {})
        self.assertTrue(any("path byte budget" in item for item in path_violations), path_violations)

    def test_shared_git_tree_expansion_has_a_global_budget(self) -> None:
        root_oid = "1" * 40
        child_oid = "2" * 40
        blob_oid = "3" * 40
        trees = {
            root_oid: [
                {
                    "mode": "40000",
                    "name": name,
                    "object_type": "tree",
                    "oid": child_oid,
                }
                for name in ("a", "b", "c")
            ],
            child_oid: [
                {
                    "mode": "100644",
                    "name": "leaf",
                    "object_type": "blob",
                    "oid": blob_oid,
                }
            ],
        }
        violations: list[str] = []
        with mock.patch.object(self.checker, "MAX_FLATTENED_TREE_NODES", 2):
            self.checker.flatten_git_tree(root_oid, trees, "shared DAG", violations)
        self.assertTrue(any("expansion budget" in item for item in violations), violations)

    def test_git_object_closure_rejects_missing_or_corrupt_tree_and_blob_objects(self) -> None:
        def rehash(repository: dict) -> None:
            snapshot = repository["bare_repository_snapshot"]
            snapshot["snapshot_sha256"] = canonical_sha256(
                {key: value for key, value in snapshot.items() if key != "snapshot_sha256"}
            )
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

        for object_type in ("tree", "blob"):
            with self.subTest(missing=object_type):
                proof = local_two_bare_proof()
                repository = proof["repositories"][0]
                removed = next(
                    item for item in repository["git_objects"] if item["object_type"] == object_type
                )
                repository["git_objects"].remove(removed)
                loose_path = f"objects/{removed['oid'][:2]}/{removed['oid'][2:]}"
                repository["bare_repository_snapshot"]["files"] = [
                    item
                    for item in repository["bare_repository_snapshot"]["files"]
                    if item["path"] != loose_path
                ]
                rehash(repository)
                violations: list[str] = []
                self.checker.validate_git_object_store_proof(
                    proof,
                    {
                        "candidate": {
                            "candidate": SOURCE_SHA,
                            "lkg_boundary": LKG_SHA,
                        },
                        "lkg": {"lkg": LKG_SHA},
                    },
                    f"missing {object_type}",
                    violations,
                )
                self.assertTrue(
                    any("closure" in item or f"missing {object_type}" in item for item in violations),
                    violations,
                )

        proof = local_two_bare_proof()
        repository = proof["repositories"][0]
        tree = next(item for item in repository["git_objects"] if item["object_type"] == "tree")
        raw_tree = bytearray(base64.b64decode(tree["body_base64"], validate=True))
        raw_tree[0] = ord("1") if raw_tree[0] != ord("1") else ord("4")
        tree["body_base64"] = base64.b64encode(raw_tree).decode("ascii")
        loose_path = f"objects/{tree['oid'][:2]}/{tree['oid'][2:]}"
        loose = zlib.compress(f"tree {len(raw_tree)}\0".encode("ascii") + raw_tree, level=9)
        replay_file = next(
            item
            for item in repository["bare_repository_snapshot"]["files"]
            if item["path"] == loose_path
        )
        replay_file["body_base64"] = base64.b64encode(loose).decode("ascii")
        rehash(repository)
        violations = []
        self.checker.validate_git_object_store_proof(
            proof,
            {
                "candidate": {"candidate": SOURCE_SHA, "lkg_boundary": LKG_SHA},
                "lkg": {"lkg": LKG_SHA},
            },
            "corrupt tree",
            violations,
        )
        self.assertTrue(
            any("recomputed Git object SHA" in item for item in violations),
            violations,
        )

    def test_complete_git_snapshot_replays_for_fsck_and_archive(self) -> None:
        repository = local_two_bare_proof()["repositories"][0]
        with tempfile.TemporaryDirectory() as temporary:
            bare = Path(temporary) / "proof.git"
            for record in repository["bare_repository_snapshot"]["files"]:
                target = bare / record["path"]
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(base64.b64decode(record["body_base64"], validate=True))
            fsck = subprocess.run(
                ["git", f"--git-dir={bare}", "fsck", "--strict", "--no-reflogs"],
                capture_output=True,
                text=True,
            )
            self.assertEqual(fsck.returncode, 0, fsck.stdout + fsck.stderr)
            archive = subprocess.run(
                [
                    "git",
                    f"--git-dir={bare}",
                    "archive",
                    "refs/heads/stage7-proof/candidate",
                ],
                capture_output=True,
            )
            self.assertEqual(archive.returncode, 0, archive.stderr.decode(errors="replace"))
            self.assertTrue(archive.stdout)

    def test_replay_validation_rejects_git_invalid_commit_identities(self) -> None:
        raw = (
            f"tree {TREE_SHA}\n".encode("ascii")
            + b"author malformed\n"
            + b"committer malformed\n\n"
            + b"invalid identity fixture\n"
        )
        oid = hashlib.sha1(
            f"commit {len(raw)}\0".encode("ascii") + raw,
            usedforsecurity=False,
        ).hexdigest()
        proof = git_object_store_proof(
            [
                complete_git_repository_snapshot(
                    "malformed",
                    {"tip": oid},
                    history_boundaries=[],
                    overrides={oid: ("commit", raw)},
                )
            ]
        )
        violations: list[str] = []
        self.checker.validate_git_object_store_proof(
            proof,
            {"malformed": {"tip": oid}},
            "malformed identity",
            violations,
        )
        self.assertTrue(
            any("fsck" in item or "strict Git replay" in item for item in violations),
            violations,
        )

    def test_candidate_proof_requires_the_frozen_lkg_as_an_ancestor(self) -> None:
        root_commit = subprocess.run(
            ["git", "rev-list", "--max-parents=0", "HEAD"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()[0]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.rewrite_fragment_source(fragment, root_commit)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertIn("ancestor", str(error.exception).lower())

    def test_self_attested_object_database_identities_cannot_certify_two_bare_repositories(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]

            def forge_distinct_observations(artifact: dict) -> None:
                repositories = artifact["git_object_store_proof"]["repositories"]
                for index, repository in enumerate(repositories, start=1):
                    repository["bare_repository_snapshot"] = {
                        "schema": "rssh.stage7.git-object-database-observation/v1",
                        "scheme": "posix-device-inode",
                        "volume_or_device_id": f"dead{index}",
                        "file_id": f"beef{index}",
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

            self.mutate_artifact(
                root,
                manifest,
                "local-two-bare-git-source-proof",
                forge_distinct_observations,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("replay" in item or "bare repository" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_rterm_history_map_and_external_proof_bind_raw_cross_repository_commits(self) -> None:
        self.assertFalse(self.checker.git_commit_available(REPO, FILTERED_SHA))
        self.assertFalse(self.checker.git_commit_available(REPO, R1_SHA))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]
            forged_map = {
                "schema": "rssh.stage7.source-to-filtered-map-proof/v1",
                "records": [
                    {"source_oid": PARENT_SHA, "filtered_oid": "d" * 40}
                ],
                "map_sha256": "e" * 64,
                "source_refs_before": {"r0": PARENT_SHA},
                "source_refs_after": {"r0": "f" * 40},
            }
            forged_store = {
                "schema": "rssh.stage7.git-object-store-proof/v1",
                "object_format": "sha1",
                "repositories": [],
            }
            self.mutate_artifact(
                root,
                manifest,
                "source-to-filtered-history-map",
                lambda artifact: artifact.update(
                    {
                        "commit_map_proof": forged_map,
                        "git_object_store_proof": forged_store,
                    }
                ),
            )
            self.mutate_artifact(
                root,
                manifest,
                "rterm-external-source-proof",
                lambda artifact: artifact.update(
                    {
                        "source_to_filtered_map_sha256": "e" * 64,
                        "git_object_store_proof": forged_store,
                    }
                ),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any(
                    "source-to-filtered" in item
                    or "commit map" in item
                    or "repository inventory" in item
                    for item in decision["violations"]
                ),
                decision["violations"],
            )

    def test_r1_requires_an_exact_bootstrap_tree_projection_proof(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]
            self.mutate_artifact(
                root,
                manifest,
                "source-to-filtered-history-map",
                lambda artifact: artifact.pop("bootstrap_projection_proof", None),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("bootstrap" in item and "projection" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_r1_bootstrap_projection_rejects_one_missing_template_after_rehash(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]

            def remove_template(artifact: dict) -> None:
                proof = artifact["bootstrap_projection_proof"]
                proof["template_mappings"].pop()
                proof["projection_sha256"] = canonical_sha256(
                    {key: value for key, value in proof.items() if key != "projection_sha256"}
                )

            self.mutate_artifact(
                root,
                manifest,
                "source-to-filtered-history-map",
                remove_template,
            )
            manifest_data = json.loads(manifest.read_text(encoding="utf-8"))
            history_entry = next(
                item
                for item in manifest_data["entries"]
                if item["artifact_type"] == "source-to-filtered-history-map"
            )
            history = json.loads((root / history_entry["path"]).read_text(encoding="utf-8"))
            mutated_digest = history["bootstrap_projection_proof"]["projection_sha256"]
            self.mutate_artifact(
                root,
                manifest,
                "rterm-external-source-proof",
                lambda artifact: artifact.__setitem__(
                    "bootstrap_projection_sha256", mutated_digest
                ),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("exact thirteen" in item or "bootstrap product" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_arbitrary_synthetic_filtered_tree_is_not_a_valid_r0_projection(self) -> None:
        arbitrary_filtered_ref, _raw = synthetic_commit(
            "arbitrary filtered tree",
            tree_oid=TREE_SHA,
        )
        proof = tree_projection_proof(
            filtered_ref=arbitrary_filtered_ref,
            filtered_tree_oid=TREE_SHA,
        )
        violations: list[str] = []
        self.checker.validate_tree_projection_proof(
            proof,
            REPO,
            PARENT_SHA,
            arbitrary_filtered_ref,
            TREE_SHA,
            "synthetic filtered projection",
            violations,
        )
        self.assertTrue(violations, "an arbitrary synthetic tree must not certify")
        self.assertTrue(
            any("projection" in item or "R0" in item for item in violations),
            violations,
        )

    def test_r0_subset_must_match_the_closed_owned_path_inventory(self) -> None:
        metadata = subprocess.run(
            ["git", "ls-tree", PARENT_SHA, "--", "README.md"],
            cwd=REPO,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.split("\t", 1)[0]
        mode, object_type, oid = metadata.split(" ")
        raw_tree = f"{mode} README.md\0".encode("ascii") + bytes.fromhex(oid)
        tree_oid = hashlib.sha1(
            f"tree {len(raw_tree)}\0".encode("ascii") + raw_tree,
            usedforsecurity=False,
        ).hexdigest()
        filtered_ref, _raw = synthetic_commit("unauthorized README-only extraction", tree_oid=tree_oid)
        tree_snapshot = {
            "schema": "rssh.stage7.filtered-tree-snapshot/v1",
            "root_tree_oid": tree_oid,
            "tree_objects": [
                {
                    "oid": tree_oid,
                    "object_type": "tree",
                    "body_base64": base64.b64encode(raw_tree).decode("ascii"),
                }
            ],
        }
        tree_snapshot["snapshot_sha256"] = canonical_sha256(tree_snapshot)
        proof = {
            "schema": "rssh.stage7.tree-projection-proof/v1",
            "r0_ref": PARENT_SHA,
            "filtered_boundary_ref": filtered_ref,
            "source_root_tree_oid": PARENT_TREE_SHA,
            "extraction_manifest_sha256": "a" * 64,
            "filtered_tree_snapshot": tree_snapshot,
            "path_mappings": [
                {
                    "source_path": "README.md",
                    "filtered_path": "README.md",
                    "mode": mode,
                    "object_type": object_type,
                    "object_oid": oid,
                }
            ],
        }
        proof["projection_sha256"] = canonical_sha256(proof)
        violations: list[str] = []
        self.checker.validate_tree_projection_proof(
            proof,
            REPO,
            PARENT_SHA,
            filtered_ref,
            tree_oid,
            "README-only projection",
            violations,
            copy.deepcopy(FIXTURE_OWNED_MAPPINGS),
        )
        self.assertTrue(violations, "an undeclared R0 subset must not certify")
        self.assertTrue(
            any("inventory" in item or "owned" in item for item in violations),
            violations,
        )

    def test_extraction_ready_rejects_r0_without_future_bootstrap_inventory(self) -> None:
        inventory = owned_projection_inventory()
        wrapped = self.checker.git_tree_identity_and_leaves
        self.checker.git_tree_identity_and_leaves = (
            self.real_checker_git_tree_identity_and_leaves
        )
        try:
            violations: list[str] = []
            self.checker.validate_owned_projection_inventory(
                inventory,
                REPO,
                PARENT_SHA,
                self.contract,
                "actual R0 inventory",
                violations,
            )
        finally:
            self.checker.git_tree_identity_and_leaves = wrapped
            self.assertTrue(
                any(
                    "release/rterm-bootstrap" in item
                    or "bootstrap/template" in item
                    for item in violations
                ),
                violations,
            )

    def test_bootstrap_readme_alone_cannot_certify_the_template_inventory(self) -> None:
        inventory = owned_projection_inventory()
        inventory["path_mappings"] = [
            mapping
            for mapping in inventory["path_mappings"]
            if not mapping["source_path"].startswith("release/rterm-bootstrap/")
            or mapping["source_path"] == "release/rterm-bootstrap/README.md"
        ]
        inventory["inventory_sha256"] = canonical_sha256(
            {key: value for key, value in inventory.items() if key != "inventory_sha256"}
        )
        violations: list[str] = []
        self.checker.validate_owned_projection_inventory(
            inventory,
            REPO,
            PARENT_SHA,
            self.contract,
            "README-only bootstrap attack",
            violations,
        )
        self.assertTrue(
            any("bootstrap" in item and "incomplete" in item for item in violations),
            violations,
        )

    def test_exact_claim_rules_use_strict_recursive_json_types(self) -> None:
        violations: list[str] = []
        self.checker.validate_claim_rule(
            "nested-result",
            {"result": [{"exit_code": False}]},
            {"kind": "exact", "value": {"result": [{"exit_code": 0}]}},
            {},
            "strict exact claim",
            violations,
        )
        self.assertTrue(violations)

    def test_malformed_children_cli_is_one_json_no_go_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]
            data = json.loads(manifest.read_text(encoding="utf-8"))
            data["entries"][0]["children"] = [[]]
            write_json(manifest, data)
            result = subprocess.run(
                [
                    str(PYTHON),
                    str(CHECKER_PATH),
                    "--contract",
                    str(CONTRACT_PATH),
                    "--requested-state",
                    "attribution-ready",
                    "--evidence-manifest",
                    str(manifest),
                ],
                cwd=REPO,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertEqual(result.stderr, "")
            self.assertEqual(len(result.stdout.splitlines()), 1, result.stdout)
            decision = json.loads(result.stdout)
            self.assertEqual(decision["decision"], "NO-GO")
            self.assertFalse(decision["go"])

    def test_owned_inventory_count_digest_and_projection_must_recompute_together(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]
            self.mutate_artifact(
                root,
                manifest,
                "rterm-extraction-manifest",
                lambda artifact: artifact["claims"].__setitem__(
                    "owned_path_count", 999_999
                ),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"], decision)
            self.assertTrue(
                any("owned_path_count" in item or "owned inventory" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_rterm_external_domain_cannot_reuse_rssh_commits(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root / "prior", "cross-platform-go")[
                "cross-platform-go"
            ]
            fragment = self.make_fragment(root, "extraction-ready")
            self.rewrite_fragment_epoch_ref(
                fragment, "rterm", "filtered_boundary_ref", PARENT_SHA
            )
            self.rewrite_fragment_epoch_ref(
                fragment, "rterm", "r1_ref", SOURCE_SHA
            )
            data = json.loads(fragment.read_text(encoding="utf-8"))
            local_store = git_object_store_proof(
                [
                    git_repository_snapshot(
                        "rssh-source",
                        {"r0": PARENT_SHA},
                        [git_commit_object(PARENT_SHA)],
                    ),
                    git_repository_snapshot(
                        "rterm-filtered",
                        {"filtered_boundary": PARENT_SHA, "r1": SOURCE_SHA},
                        [
                            git_commit_object(PARENT_SHA),
                            git_commit_object(SOURCE_SHA),
                        ],
                    ),
                ]
            )
            records = [{"source_oid": PARENT_SHA, "filtered_oid": PARENT_SHA}]
            local_map = {
                "schema": "rssh.stage7.source-to-filtered-map-proof/v1",
                "records": records,
                "map_sha256": canonical_sha256(records),
                "source_refs_before": {"r0": PARENT_SHA},
                "source_refs_after": {"r0": PARENT_SHA},
            }
            for entry in data["entries"]:
                if entry["artifact_type"] not in {
                    "source-to-filtered-history-map",
                    "rterm-external-source-proof",
                }:
                    continue
                artifact_path = fragment.parent / entry["path"]
                artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
                artifact["git_object_store_proof"] = copy.deepcopy(local_store)
                if entry["artifact_type"] == "source-to-filtered-history-map":
                    artifact["commit_map_proof"] = local_map
                else:
                    artifact["source_to_filtered_map_sha256"] = local_map[
                        "map_sha256"
                    ]
                write_json(artifact_path, artifact)
                entry["sha256"] = sha256(artifact_path)
                entry["size_bytes"] = artifact_path.stat().st_size
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "extraction-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                    prior_manifest=prior,
                )
            self.assertTrue(
                "external" in str(error.exception).lower()
                or "filtered" in str(error.exception).lower()
            )

    def test_local_git_proof_fails_closed_for_replace_graft_and_shallow_history(self) -> None:
        for control in ("shallow", "graft", "replace"):
            with self.subTest(control=control), tempfile.TemporaryDirectory() as temporary:
                repository, base, head = self.make_two_commit_repository(Path(temporary))
                git_dir = repository / ".git"
                self.assertFalse(
                    self.checker.git_repository_has_history_overrides(repository)
                )
                self.assertTrue(self.checker.git_commit_available(repository, base))
                self.assertTrue(self.checker.git_is_ancestor(repository, base, head))
                if control == "shallow":
                    (git_dir / "shallow").write_text(base + "\n", encoding="ascii")
                elif control == "graft":
                    grafts = git_dir / "info" / "grafts"
                    grafts.parent.mkdir(parents=True, exist_ok=True)
                    grafts.write_text(f"{head} {base}\n", encoding="ascii")
                else:
                    subprocess.run(
                        ["git", "update-ref", f"refs/replace/{base}", head],
                        cwd=repository,
                        check=True,
                    )
                self.assertTrue(
                    self.checker.git_repository_has_history_overrides(repository)
                )
                self.assertFalse(self.checker.git_commit_available(repository, base))
                self.assertFalse(self.checker.git_is_ancestor(repository, base, head))

    def test_cached_r0_tree_cannot_survive_missing_object_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            repository, base, _head = self.make_two_commit_repository(Path(temporary))
            violations: list[str] = []
            tree_oid, leaves = self.checker.git_tree_identity_and_leaves(
                repository, base, "cached immutable tree", violations
            )
            self.assertRegex(tree_oid or "", r"^[0-9a-f]{40}$")
            self.assertTrue(leaves)
            self.assertEqual(violations, [])
            object_path = repository / ".git" / "objects" / tree_oid[:2] / tree_oid[2:]
            object_path.chmod(0o600)
            object_path.unlink()
            real_ls_tree = subprocess.run(
                ["git", "ls-tree", "-r", base],
                cwd=repository,
                check=False,
                capture_output=True,
            )
            self.assertNotEqual(real_ls_tree.returncode, 0)
            violations = []
            observed_tree, observed_leaves = self.checker.git_tree_identity_and_leaves(
                repository, base, "cached immutable tree", violations
            )
            self.assertIsNone(observed_tree)
            self.assertEqual(observed_leaves, {})
            self.assertTrue(violations)

    def test_deep_filtered_tree_is_a_json_no_go_not_recursion_crash(self) -> None:
        leaf_raw = b"100644 leaf\0" + bytes.fromhex(BLOB_SHA)
        child_oid = hashlib.sha1(
            f"tree {len(leaf_raw)}\0".encode("ascii") + leaf_raw,
            usedforsecurity=False,
        ).hexdigest()
        records = [
            {
                "oid": child_oid,
                "object_type": "tree",
                "body_base64": base64.b64encode(leaf_raw).decode("ascii"),
            }
        ]
        for depth in range(1_100):
            raw = f"40000 d{depth:04d}\0".encode("ascii") + bytes.fromhex(child_oid)
            child_oid = hashlib.sha1(
                f"tree {len(raw)}\0".encode("ascii") + raw,
                usedforsecurity=False,
            ).hexdigest()
            records.append(
                {
                    "oid": child_oid,
                    "object_type": "tree",
                    "body_base64": base64.b64encode(raw).decode("ascii"),
                }
            )
        snapshot = {
            "schema": "rssh.stage7.filtered-tree-snapshot/v1",
            "root_tree_oid": child_oid,
            "tree_objects": sorted(records, key=lambda record: record["oid"]),
        }
        snapshot["snapshot_sha256"] = canonical_sha256(snapshot)
        violations: list[str] = []
        self.checker.validate_filtered_tree_snapshot(
            snapshot,
            child_oid,
            "deep filtered tree",
            violations,
        )
        self.assertTrue(any("depth" in item for item in violations), violations)

    def test_generic_later_result_cannot_certify_and_matrix_needs_every_stage_backend(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]

            def erase_specific_proof(artifact: dict) -> None:
                identity = artifact["identity"]
                artifact.clear()
                artifact.update(
                    {"schema": "rssh.stage7.result/v1", "identity": identity, "ok": True}
                )

            self.mutate_artifact(
                root, manifest, "release-contract-v2", erase_specific_proof
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(
                any("proof" in item or "claim" in item for item in decision["violations"]),
                decision["violations"],
            )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "attribution-ready")["attribution-ready"]

            def omit_matrix_cell(artifact: dict) -> None:
                artifact["groups"] = [
                    group
                    for group in artifact["groups"]
                    if group["name"] != "gl/full-frame"
                ]

            self.mutate_artifact(
                root, manifest, "attribution-matrix-raw", omit_matrix_cell
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("group inventory" in item for item in decision["violations"]))

    def test_diagnostic_backend_may_record_a_closed_unsupported_suffix(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.set_diagnostic_unsupported_suffix(
                fragment,
                backend="gl",
                first_stage="adapter-device",
                reason="backend-unavailable",
            )
            manifest = root / "manifest.json"
            self.assembler.assemble(
                CONTRACT_PATH,
                "attribution-ready",
                root,
                [fragment],
                manifest,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertTrue(decision["ok"], decision["violations"])

    def test_diagnostic_unsupported_outcomes_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.set_diagnostic_unsupported_suffix(
                fragment,
                backend="auto",
                first_stage="adapter-device",
                reason="backend-unavailable",
            )
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertIn("auto", str(error.exception))

        def first_unsupported(artifact: dict) -> dict:
            return next(
                group
                for group in artifact["groups"]
                if group.get("support_status") == "unsupported"
            )

        cases = {
            "missing reason": lambda artifact: first_unsupported(artifact).pop(
                "unsupported_reason"
            ),
            "forged metric": lambda artifact: first_unsupported(artifact).update(
                {"processes": [], "statistics": {"p50": 1, "p95": 1, "max": 1}}
            ),
            "supported carries reason": lambda artifact: next(
                group for group in artifact["groups"] if group["name"] == "gl/cpu-window"
            ).__setitem__("unsupported_reason", "not-applicable"),
            "unsupported resumes": lambda artifact: next(
                group for group in artifact["groups"] if group["name"] == "gl/full-frame"
            ).update(
                {
                    "support_status": "supported",
                    "unsupported_reason": None,
                    "unsupported_at_stage": None,
                }
            ),
        }
        for name, mutate in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                fragment = self.make_fragment(root, "attribution-ready")
                self.set_diagnostic_unsupported_suffix(
                    fragment,
                    backend="gl",
                    first_stage="adapter-device",
                    reason="backend-unavailable",
                )
                manifest = root / "manifest.json"
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    manifest,
                )
                self.mutate_artifact(
                    root,
                    manifest,
                    "attribution-matrix-raw",
                    mutate,
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "attribution-ready", manifest
                )
                self.assertFalse(decision["ok"], decision)

    def test_lkg_recomputes_all_ratios_and_binds_binary_runner_backend_adapter(self) -> None:
        mutations = {
            "p95 ratio": lambda lkg: lkg["relative_regression_ratios"].__setitem__("p95", 0.5),
            "binary identity": lambda lkg: lkg.pop("binary_hashes"),
            "runner cohort": lambda lkg: lkg.__setitem__("runner_fingerprint_sha256", "d" * 64),
            "backend cohort": lambda lkg: lkg.__setitem__("actual_backend", "vulkan"),
            "adapter cohort": lambda lkg: lkg.__setitem__("adapter_identity", "other-adapter"),
            "raw residence": lambda lkg: lkg.__setitem__(
                "processes",
                [
                    {"process_id": f"flat-{index}", "representative": 1_000_000}
                    for index in range(30)
                ],
            ),
        }
        for label, mutate in mutations.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "windows-memory-go")["windows-memory-go"]

                def mutate_lkg(artifact: dict) -> None:
                    mutate(artifact["groups"][0]["lkg"])

                self.mutate_artifact(
                    root, manifest, "windows-empty-window-raw", mutate_lkg
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "windows-memory-go", manifest
                )
                self.assertFalse(decision["ok"])
                self.assertTrue(decision["violations"])

    def test_run_id_change_cannot_hide_binary_or_runner_identity_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            entry = next(
                item
                for item in data["entries"]
                if item["artifact_type"] == "font-catalog-fingerprint"
            )
            artifact = fragment.parent / entry["path"]
            payload = json.loads(artifact.read_text(encoding="utf-8"))
            entry["run_id"] = "isolated-run-id"
            entry["binary_hashes"] = {"rssh.exe": "d" * 64}
            entry["cohort_id"] = self.checker.cohort_id(entry)
            payload["identity"]["run_id"] = entry["run_id"]
            payload["identity"]["binary_hashes"] = entry["binary_hashes"]
            write_json(artifact, payload)
            entry["sha256"] = sha256(artifact)
            entry["size_bytes"] = artifact.stat().st_size
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertTrue(
                "identity" in str(error.exception)
                or "binary_hashes mismatch" in str(error.exception)
            )

    def test_exact_private_bytes_ceiling_and_strict_json_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "windows-memory-go")["windows-memory-go"]

            def hit_exclusive_ceiling(artifact: dict) -> None:
                group = artifact["groups"][0]
                value = 62_914_560
                for process in group["processes"]:
                    process["value"] = value
                group["statistics"] = {"p50": value, "p95": value, "max": value}
                lkg = group["lkg"]
                for process in lkg["processes"]:
                    process["value"] = value
                lkg["statistics"] = {"p50": value, "p95": value, "max": value}

            self.mutate_artifact(
                root, manifest, "windows-first-frame-raw", hit_exclusive_ceiling
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "windows-memory-go", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(any("exclusive" in item for item in decision["violations"]))

        for invalid_json in (
            '{"schema":"rssh.stage7.result/v1","schema":"duplicate"}',
            '{"value":NaN}',
            '{"value":Infinity}',
        ):
            with self.subTest(invalid_json=invalid_json), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                fragment = self.make_fragment(root, "attribution-ready")
                fragment_data = json.loads(fragment.read_text(encoding="utf-8"))
                entry = next(
                    item
                    for item in fragment_data["entries"]
                    if item["artifact_type"] == "runner-fingerprint"
                )
                artifact = fragment.parent / entry["path"]
                artifact.write_text(invalid_json, encoding="utf-8")
                entry["sha256"] = sha256(artifact)
                entry["size_bytes"] = artifact.stat().st_size
                write_json(fragment, fragment_data)
                with self.assertRaises(self.assembler.EvidenceError):
                    self.assembler.assemble(
                        CONTRACT_PATH,
                        "attribution-ready",
                        root,
                        [fragment],
                        root / "manifest.json",
                    )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest = self.build_chain(root, "extraction-ready")["extraction-ready"]
            self.mutate_artifact(
                root,
                manifest,
                "full-history-security-scan",
                lambda artifact: artifact["claims"].__setitem__(
                    "unresolved_findings", False
                ),
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "extraction-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(
                any("unresolved_findings" in item for item in decision["violations"])
            )

    def test_multiple_process_raw_artifacts_are_allowed_but_second_aggregate_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.split_raw_entry_by_process(
                fragment,
                "attribution-matrix-raw",
                "attribution-matrix-aggregate",
            )
            manifest = root / "manifest.json"
            self.assembler.assemble(
                CONTRACT_PATH,
                "attribution-ready",
                root,
                [fragment],
                manifest,
            )
            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertTrue(decision["ok"], decision["violations"])

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            aggregate = next(
                entry
                for entry in data["entries"]
                if entry["artifact_type"] == "attribution-matrix-aggregate"
            )
            duplicate = copy.deepcopy(aggregate)
            duplicate["artifact_id"] = "attribution-matrix-aggregate/second"
            duplicate["cohort_id"] = self.checker.cohort_id(duplicate)
            source = fragment.parent / aggregate["path"]
            target = fragment.parent / "attribution-matrix-aggregate--second.json"
            target.write_bytes(source.read_bytes())
            duplicate["path"] = target.name
            duplicate["sha256"] = sha256(target)
            duplicate["size_bytes"] = target.stat().st_size
            data["entries"].append(duplicate)
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )

    def test_metric_shards_must_share_one_exact_five_warmup_cohort(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "windows-memory-go")
            self.split_metric_entry_with_different_warmups(
                fragment, "windows-first-present-raw"
            )
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "windows-memory-go",
                    root,
                    [fragment],
                    root / "manifest.json",
                    prior_manifest=self.build_chain(root / "prior", "attribution-ready")[
                        "attribution-ready"
                    ],
                )
            self.assertIn("warmup", str(error.exception).lower())

    def test_all_groups_of_one_raw_artifact_type_share_the_same_warmups(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.split_raw_entry_by_group_with_different_warmups(
                fragment,
                "attribution-matrix-raw",
                "attribution-matrix-aggregate",
            )
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "attribution-ready",
                    root,
                    [fragment],
                    root / "manifest.json",
                )
            self.assertIn("warmup", str(error.exception).lower())

    def test_lkg_requires_its_own_complete_five_plus_thirty_protocol(self) -> None:
        cases = {
            "missing protocol": (
                "windows-empty-window-raw",
                lambda lkg: lkg.pop("protocol"),
            ),
            "duplicate warmup": (
                "windows-empty-window-raw",
                lambda lkg: lkg.__setitem__("warmup_process_ids", ["same"] * 5),
            ),
            "timeout drift": (
                "windows-empty-window-raw",
                lambda lkg: lkg.__setitem__("timeout_seconds", 59),
            ),
            "warmup mixed into measured": (
                "windows-empty-window-raw",
                lambda lkg: lkg["processes"][0].__setitem__("phase", "warmup"),
            ),
            "startup flag missing": (
                "windows-first-present-raw",
                lambda lkg: lkg["processes"][0].__setitem__(
                    "benchmark_startup", False
                ),
            ),
        }
        for name, (artifact_type, mutate) in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, "windows-memory-go")[
                    "windows-memory-go"
                ]
                self.mutate_artifact(
                    root,
                    manifest,
                    artifact_type,
                    lambda artifact, mutate=mutate: mutate(
                        artifact["groups"][0]["lkg"]
                    ),
                )
                decision = self.checker.validate_gate(
                    CONTRACT_PATH, "windows-memory-go", manifest
                )
                self.assertFalse(decision["ok"], decision)

    def test_fragment_must_cover_every_current_artifact_id(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fragment = self.make_fragment(root, "attribution-ready")
            self.split_raw_entry_by_process(
                fragment,
                "attribution-matrix-raw",
                "attribution-matrix-aggregate",
            )
            manifest = root / "manifest.json"
            self.assembler.assemble(
                CONTRACT_PATH,
                "attribution-ready",
                root,
                [fragment],
                manifest,
            )

            fragment_data = json.loads(fragment.read_text(encoding="utf-8"))
            raw_entries = [
                entry
                for entry in fragment_data["entries"]
                if entry["artifact_type"] == "attribution-matrix-raw"
            ]
            keep_id = raw_entries[0]["artifact_id"]
            fragment_data["entries"] = [
                entry
                for entry in fragment_data["entries"]
                if entry["artifact_type"] != "attribution-matrix-raw"
                or entry["artifact_id"] == keep_id
            ]
            write_json(fragment, fragment_data)
            manifest_data = json.loads(manifest.read_text(encoding="utf-8"))
            manifest_data["fragments"][0]["sha256"] = sha256(fragment)
            write_json(manifest, manifest_data)

            decision = self.checker.validate_gate(
                CONTRACT_PATH, "attribution-ready", manifest
            )
            self.assertFalse(decision["ok"])
            self.assertTrue(
                any("artifact ID" in item for item in decision["violations"]),
                decision["violations"],
            )

    def test_negative_candidate_and_lkg_measurements_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "attribution-ready")["attribution-ready"]
            fragment = self.make_fragment(root, "windows-memory-go")
            fragment_data = json.loads(fragment.read_text(encoding="utf-8"))
            entry = next(
                item
                for item in fragment_data["entries"]
                if item["artifact_type"] == "windows-empty-window-raw"
            )
            artifact_path = fragment.parent / entry["path"]
            artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
            group = artifact["groups"][0]
            for process in group["processes"]:
                process["samples"] = [-1] * 10
                process["representative"] = -1
            group["statistics"] = {"p50": -1, "p95": -1, "max": -1}
            for process in group["lkg"]["processes"]:
                process["samples"] = [-1] * 10
                process["representative"] = -1
            group["lkg"]["statistics"] = {"p50": -1, "p95": -1, "max": -1}
            group["lkg"]["relative_regression_ratios"] = {
                "p50": 1.0,
                "p95": 1.0,
                "max": 1.0,
            }
            write_json(artifact_path, artifact)
            entry["sha256"] = sha256(artifact_path)
            entry["size_bytes"] = artifact_path.stat().st_size
            write_json(fragment, fragment_data)

            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "windows-memory-go",
                    root,
                    [fragment],
                    root / "windows-memory-go.json",
                    prior_manifest=prior,
                )
            self.assertIn("non-negative", str(error.exception))

    def test_cross_platform_memory_requires_recomputed_same_platform_lkg_raw(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "windows-memory-go")["windows-memory-go"]
            fragment = self.make_fragment(root, "cross-platform-go")
            fragment_data = json.loads(fragment.read_text(encoding="utf-8"))

            raw_entry = next(
                item
                for item in fragment_data["entries"]
                if item["artifact_type"] == "linux-pss-raw"
            )
            raw_path = fragment.parent / raw_entry["path"]
            raw = json.loads(raw_path.read_text(encoding="utf-8"))
            group = raw["groups"][0]
            huge = 10**15
            for process in group["processes"]:
                process["samples"] = [huge] * 10
                process["representative"] = huge
            group["statistics"] = {"p50": huge, "p95": huge, "max": huge}
            group["lkg"]["source_sha"] = "d" * 40
            group["lkg"]["relative_regression_ratios"] = {
                "p50": 0,
                "p95": 0,
                "max": 0,
            }
            write_json(raw_path, raw)
            raw_entry["sha256"] = sha256(raw_path)
            raw_entry["size_bytes"] = raw_path.stat().st_size

            comparison_entry = next(
                item
                for item in fragment_data["entries"]
                if item["artifact_type"] == "linux-lkg-comparison"
            )
            comparison_path = fragment.parent / comparison_entry["path"]
            comparison = json.loads(
                comparison_path.read_text(encoding="utf-8")
            )
            comparison["group_statistics"] = [
                {"p50": huge, "p95": huge, "max": huge}
            ]
            write_json(comparison_path, comparison)
            comparison_entry["sha256"] = sha256(comparison_path)
            comparison_entry["size_bytes"] = comparison_path.stat().st_size
            write_json(fragment, fragment_data)

            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "cross-platform-go",
                    root,
                    [fragment],
                    root / "cross-platform-go.json",
                    prior_manifest=prior,
                )
            self.assertTrue(
                "immutable lkg_rssh_ref" in str(error.exception).lower()
                or "relative regression" in str(error.exception).lower()
            )

    def test_r2_and_r3_product_wrappers_cannot_replace_current_epoch_raw(self) -> None:
        cases = [
            ("dual-source-verified", "extraction-ready"),
            ("split-complete", "dual-source-verified"),
        ]
        for state, prior_state in cases:
            with self.subTest(state=state), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                prior = self.build_chain(root, prior_state)[prior_state]
                fragment = self.make_fragment(root, state)
                fragment_data = json.loads(fragment.read_text(encoding="utf-8"))
                raw_entries = [
                    entry
                    for entry in fragment_data["entries"]
                    if entry["role"] == "raw"
                ]
                for entry in raw_entries:
                    (fragment.parent / entry["path"]).unlink()
                fragment_data["entries"] = [
                    entry
                    for entry in fragment_data["entries"]
                    if entry not in raw_entries
                ]
                write_json(fragment, fragment_data)

                with self.assertRaises(self.assembler.EvidenceError) as error:
                    self.assembler.assemble(
                        CONTRACT_PATH,
                        state,
                        root,
                        [fragment],
                        root / f"{state}.json",
                        prior_manifest=prior,
                    )
                self.assertIn("raw", str(error.exception).lower())

    def test_rssh_epoch_refs_bind_real_certified_and_deletion_commits(self) -> None:
        cases = [
            ("dual-source-verified", "extraction-ready", "r2_ref"),
            ("split-complete", "dual-source-verified", "r3_ref"),
        ]
        for state, prior_state, field in cases:
            with self.subTest(state=state, field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                prior = self.build_chain(root, prior_state)[prior_state]
                fragment = self.make_fragment(root, state)
                data = json.loads(fragment.read_text(encoding="utf-8"))
                data["rssh"][field] = LKG_SHA
                data["epoch_id"] = self.checker.certification_epoch_id(
                    state,
                    data["certified_commit"],
                    data["rssh"],
                    data["rterm"],
                )
                subject_name = f"rssh.{field}"
                for entry in data["entries"]:
                    if subject_name in entry["subject_refs"]:
                        entry["subject_refs"][subject_name] = LKG_SHA
                    entry["cohort_id"] = self.checker.cohort_id(entry)
                    artifact_path = fragment.parent / entry["path"]
                    artifact = json.loads(
                        artifact_path.read_text(encoding="utf-8")
                    )
                    rules = self.contract["result_claims"].get(
                        entry["artifact_type"], {}
                    )
                    for claim_name, rule in rules.items():
                        if rule.get("kind") == "epoch-ref" and rule.get(
                            "path"
                        ) == subject_name:
                            artifact["claims"][claim_name] = LKG_SHA
                    write_json(artifact_path, artifact)
                    entry["sha256"] = sha256(artifact_path)
                    entry["size_bytes"] = artifact_path.stat().st_size
                write_json(fragment, data)

                with self.assertRaises(self.assembler.EvidenceError) as error:
                    self.assembler.assemble(
                        CONTRACT_PATH,
                        state,
                        root,
                        [fragment],
                        root / f"{state}.json",
                        prior_manifest=prior,
                    )
                self.assertIn("certified commit", str(error.exception).lower())

    def test_epoch_refs_reject_non_commit_objects_and_bad_deletion_ancestry(self) -> None:
        for bad_ref in ("0" * 40, BLOB_SHA):
            with self.subTest(r2_ref=bad_ref), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                prior = self.build_chain(root, "extraction-ready")["extraction-ready"]
                fragment = self.make_fragment(root, "dual-source-verified")
                self.rewrite_fragment_epoch_ref(fragment, "rssh", "r2_ref", bad_ref)
                with self.assertRaises(self.assembler.EvidenceError) as error:
                    self.assembler.assemble(
                        CONTRACT_PATH,
                        "dual-source-verified",
                        root,
                        [fragment],
                        root / "dual-source-verified.json",
                        prior_manifest=prior,
                    )
                self.assertIn("commit", str(error.exception).lower())

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "dual-source-verified")[
                "dual-source-verified"
            ]
            fragment = self.make_fragment(root, "split-complete")
            self.rewrite_fragment_epoch_ref(
                fragment, "rssh", "r3_deletion_ref", LKG_SHA
            )
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "split-complete",
                    root,
                    [fragment],
                    root / "split-complete.json",
                    prior_manifest=prior,
                )
            self.assertIn("r3_deletion_ref", str(error.exception))

    def test_r1_drift_and_cross_platform_wrapper_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "cross-platform-go")["cross-platform-go"]
            fragment = self.make_fragment(root, "extraction-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            data["rterm"]["r1_ref"] = LKG_SHA
            data["epoch_id"] = self.checker.certification_epoch_id(
                "extraction-ready",
                data["certified_commit"],
                data["rssh"],
                data["rterm"],
            )
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError):
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "extraction-ready",
                    root,
                    [fragment],
                    root / "extraction-ready.json",
                    prior_manifest=prior,
                )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            prior = self.build_chain(root, "cross-platform-go")["cross-platform-go"]
            fragment = self.make_fragment(root, "extraction-ready")
            data = json.loads(fragment.read_text(encoding="utf-8"))
            disguised = [
                entry
                for entry in data["entries"]
                if entry["artifact_type"] == "rterm-standalone-ci"
                and entry["platform"] != "windows-x86_64"
            ]
            for entry in disguised:
                artifact_path = fragment.parent / entry["path"]
                artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
                artifact["identity"]["platform"] = "cross-platform"
                write_json(artifact_path, artifact)
                entry["platform"] = "cross-platform"
                entry["sha256"] = sha256(artifact_path)
                entry["size_bytes"] = artifact_path.stat().st_size
                entry["cohort_id"] = self.checker.cohort_id(entry)
            write_json(fragment, data)
            with self.assertRaises(self.assembler.EvidenceError) as error:
                self.assembler.assemble(
                    CONTRACT_PATH,
                    "extraction-ready",
                    root,
                    [fragment],
                    root / "extraction-ready.json",
                    prior_manifest=prior,
                )
            self.assertIn("platform does not match the artifact contract", str(error.exception))

    def test_remote_authorization_deletion_and_both_releases_are_required(self) -> None:
        cases = [
            (
                "dual-source-verified",
                "remote-publication-proof",
                lambda artifact: artifact["claims"].__setitem__("authorization_count", 1),
                "authorization_count",
            ),
            (
                "split-complete",
                "local-rterm-deletion-proof",
                lambda artifact: artifact["claims"].__setitem__("deleted_path_count", 6),
                "deleted_path_count",
            ),
            (
                "split-complete",
                "rterm-protected-release",
                lambda artifact: artifact["claims"].__setitem__("protected", False),
                "protected",
            ),
            (
                "split-complete",
                "rssh-protected-release",
                lambda artifact: artifact["claims"].__setitem__("protected", False),
                "protected",
            ),
        ]
        for state, artifact_type, mutate, claim in cases:
            with self.subTest(artifact_type=artifact_type), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                manifest = self.build_chain(root, state)[state]
                self.mutate_artifact(root, manifest, artifact_type, mutate)
                decision = self.checker.validate_gate(CONTRACT_PATH, state, manifest)
                self.assertFalse(decision["ok"])
                self.assertTrue(
                    any(claim in item for item in decision["violations"]),
                    decision["violations"],
                )

    def build_chain(self, root: Path, target_state: str) -> dict[str, Path]:
        manifests: dict[str, Path] = {}
        prior: Path | None = None
        for state in self.contract["states"][1:]:
            fragment = self.make_fragment(root, state)
            output = root / f"{state}.json"
            self.assembler.assemble(
                CONTRACT_PATH,
                state,
                root,
                [fragment],
                output,
                prior_manifest=prior,
            )
            manifests[state] = output
            prior = output
            if state == target_state:
                return manifests
        raise AssertionError(f"unknown target state {target_state}")

    def make_fragment(self, root: Path, state: str) -> Path:
        fragment_dir = root / "fragments" / state
        entries = []
        raw_payloads: dict[str, list[tuple[str, dict]]] = {}
        rssh_epoch, rterm_epoch = self.epochs_for_state(state)
        for artifact_type in self.contract["new_artifacts_by_state"][state]:
            policy = self.contract["artifact_policies"][artifact_type]
            platforms = policy.get("platforms", [policy.get("platform", "repository")])
            for platform in platforms:
                run_id = f"{state}-{platform}-run"
                identity = {
                    "source_sha": SOURCE_SHA,
                    "platform": platform,
                    "run_id": run_id,
                }
                if policy["binary_identity"]:
                    identity["binary_hashes"] = {"rssh.exe": BINARY_SHA}
                if policy["runner_identity"]:
                    identity["runner_fingerprint_sha256"] = RUNNER_SHA
                artifact_id = (
                    artifact_type
                    if len(platforms) == 1
                    else f"{artifact_type}/{platform}"
                )
                payload = self.make_payload(
                    artifact_type,
                    artifact_id,
                    policy,
                    identity,
                    raw_payloads,
                    rssh_epoch,
                    rterm_epoch,
                )
                artifact = fragment_dir / f"{artifact_id.replace('/', '--')}.json"
                write_json(artifact, payload)
                entry = {
                    "artifact_type": artifact_type,
                    "artifact_id": artifact_id,
                    "role": {
                        "raw-metric": "raw",
                        "aggregate": "aggregate",
                        "result": "proof",
                    }[policy["content_kind"]],
                    "scope": state,
                    "payload_schema": payload["schema"],
                    "path": artifact.name,
                    "sha256": sha256(artifact),
                    "size_bytes": artifact.stat().st_size,
                    "producing_command": f"test-producer --artifact {artifact_type}",
                    "producing_argv": ["test-producer", "--artifact", artifact_type],
                    "subject_refs": self.subject_refs_for_state(
                        state, rssh_epoch, rterm_epoch
                    ),
                    "children": payload.get("raw_children", []),
                    **identity,
                }
                entry["cohort_id"] = self.checker.cohort_id(entry)
                entries.append(entry)
                if policy["content_kind"] == "raw-metric":
                    raw_payloads.setdefault(artifact_type, []).append((artifact_id, payload))

        fragment = fragment_dir / "artifact-manifest-fragment.json"
        write_json(
            fragment,
            {
                "schema": "rssh.stage7-artifact-manifest-fragment/v1",
                "requested_state": state,
                "certified_commit": SOURCE_SHA,
                "epoch_id": self.checker.certification_epoch_id(
                    state, SOURCE_SHA, rssh_epoch, rterm_epoch
                ),
                "rssh": rssh_epoch,
                "rterm": rterm_epoch,
                "entries": entries,
            },
        )
        return fragment

    def make_payload(
        self,
        artifact_type: str,
        artifact_id: str,
        policy: dict,
        identity: dict,
        raw_payloads: dict[str, list[tuple[str, dict]]],
        rssh_epoch: dict | None,
        rterm_epoch: dict | None,
    ) -> dict:
        kind = policy["content_kind"]
        if kind == "raw-metric":
            return self.make_metric_payload(artifact_type, policy, identity)
        if kind == "aggregate":
            raw_children = [
                (raw_id, raw)
                for raw_child_type in policy["raw_children"]
                for raw_id, raw in raw_payloads[raw_child_type]
            ]
            return {
                "schema": "rssh.stage7.metric-aggregate/v1",
                "identity": identity,
                "ok": True,
                "raw_children": sorted(raw_id for raw_id, _ in raw_children),
                "group_statistics": [
                    group["statistics"]
                    for _, raw in raw_children
                    for group in raw["groups"]
                ],
            }
        claims = {
            name: self.claim_value(rule, rssh_epoch, rterm_epoch)
            for name, rule in self.contract["result_claims"][artifact_type].items()
        }
        payload = {
            "schema": "rssh.stage7.result/v1",
            "identity": identity,
            "ok": True,
            "proof": artifact_type,
            "claims": claims,
        }
        if artifact_type == "runner-fingerprint":
            payload["fingerprint_sha256"] = RUNNER_SHA
        elif artifact_type == "font-catalog-fingerprint":
            specimens = self.font_functional_specimens()
            payload["functional_specimens"] = specimens
            payload["catalog_fingerprint_sha256"] = canonical_sha256(specimens)
        elif artifact_type == "local-two-bare-git-source-proof":
            payload.update(
                {
                    "bare_repository_count": 2,
                    "source_refs": [SOURCE_SHA, LKG_SHA],
                    "immutable": True,
                    "git_object_store_proof": local_two_bare_proof(),
                }
            )
        elif artifact_type == "rterm-extraction-manifest":
            inventory = owned_projection_inventory()
            payload["owned_projection_inventory"] = inventory
            payload["claims"]["owned_path_count"] = len(
                inventory["path_mappings"]
            )
            payload["claims"]["owned_root_count"] = sum(
                len(inventory["root_rules"][key])
                for key in ("required", "future_required")
            )
            payload["claims"]["bootstrap_inventory_complete"] = True
            payload["claims"]["manifest_sha256"] = inventory[
                "inventory_sha256"
            ]
        elif artifact_type == "source-to-filtered-history-map":
            payload.update(
                {
                    "commit_map_proof": source_to_filtered_map_proof(),
                    "bootstrap_projection_proof": bootstrap_projection_proof(),
                    "git_object_store_proof": rterm_object_store_proof(),
                }
            )
        elif artifact_type == "rterm-external-source-proof":
            source_map = source_to_filtered_map_proof()
            payload.update(
                {
                    "source_to_filtered_map_sha256": source_map["map_sha256"],
                    "tree_projection_sha256": source_map[
                        "tree_projection_proof"
                    ]["projection_sha256"],
                    "bootstrap_projection_sha256": bootstrap_projection_proof()[
                        "projection_sha256"
                    ],
                    "git_object_store_proof": rterm_object_store_proof(),
                }
            )
        elif artifact_type == "windows-release-build-provenance":
            payload.update({"profile": "release", "locked": True})
        elif artifact_type == "windows-loopback-native-ssh":
            payload["coverage"] = [
                "unknown-host-key",
                "changed-host-key",
                "secret-masking",
                "resize",
                "cancel",
                "disconnect",
                "reconnect",
            ]
        elif artifact_type == "windows-secret-scan":
            payload.update(
                {
                    "hits": 0,
                    "scopes": [
                        "stdout",
                        "stderr",
                        "markers",
                        "json",
                        "session-log",
                        "snapshot",
                    ],
                }
            )
        return payload

    def font_functional_specimens(self) -> list[dict]:
        records = []
        for mode in ("current", "shared", "lazy"):
            for specimen in ("cjk", "emoji"):
                records.append(
                    {
                        "requested_font_mode": mode,
                        "actual_font_mode": mode,
                        "requested_font_specimen": specimen,
                        "actual_font_specimen": specimen,
                        "requested_backend": "auto",
                        "actual_backend": "dx12",
                        "activation_latency_ms": 0.009,
                        "activation_latency_gate": "report-only",
                        "retained_source_bytes": 1_000_000,
                        "recovery_retained_source_bytes": 1_000_000,
                        "indexed_source_count": 3,
                        "active_source_count": 2,
                        "catalog_builds": 2,
                        "generation": 2,
                        "recovery_generation": 2,
                        "frame_catalog_generation": 2,
                        "frame_generation_consistent": True,
                        "tofu_count": 0,
                        "index_fingerprint_sha256": "1" * 64,
                        "catalog_fingerprint_sha256": "2" * 64,
                        "ordered_catalog_fingerprint_sha256": "3" * 64,
                    }
                )
        return records

    def make_metric_payload(self, artifact_type: str, policy: dict, identity: dict) -> dict:
        mode = policy["sampling_mode"]
        protocol = self.metric_protocol(policy)
        metric = policy["metric"]
        value = 100 if metric == "first_present_ms" else 1_000_000
        group_names = policy.get("required_groups", [artifact_type])
        if "matrix_stages" in policy:
            group_names = [
                f"{backend}/{stage}"
                for backend in policy["matrix_backends"]
                for stage in policy["matrix_stages"]
            ]
        groups = []
        for group_name in group_names:
            group_value = value
            if artifact_type == "font-ownership-raw":
                group_value = {
                    "current-copied/ascii": 300_000_000,
                    "shared-all/ascii": 300_000_000 - 67_108_864,
                    "lazy/ascii": 300_000_000 - 67_108_864 - 33_554_432,
                }[group_name]
            requested_backend = policy.get("requested_backend", "auto")
            if "matrix_stages" in policy:
                requested_backend, stage = group_name.split("/", 1)
            group: dict = {
                "name": group_name,
                "metric": metric,
                "sampling_mode": mode,
                "requested_backend": requested_backend,
                "final_renderer": policy.get("final_renderer", "gpu"),
            }
            if "matrix_stages" in policy:
                group["support_status"] = "supported"
            if mode == "startup-marker":
                group["processes"] = [
                    {
                        "process_id": f"{group_name}-process-{index:02d}",
                        "phase": "measured",
                        "benchmark_startup": True,
                        "marker_count": 1,
                        "value": group_value,
                    }
                    for index in range(30)
                ]
            else:
                add_backend_identity = True
                if "matrix_stages" in policy:
                    add_backend_identity = policy["matrix_stages"].index(stage) >= 2
                if add_backend_identity:
                    group["actual_backend"] = (
                        "dx12" if requested_backend == "auto" else requested_backend
                    )
                    group["adapter_identity"] = "test-adapter"
                group["owner_ready_marker"] = policy.get(
                    "owner_ready_marker", "owner_ready"
                )
                group["stabilization_ms"] = 5_000
                group["sample_interval_ms"] = 100
                group["processes"] = [
                    {
                        "process_id": f"{group_name}-process-{index:02d}",
                        "phase": "measured",
                        "samples": [group_value] * 10,
                        "representative": group_value,
                    }
                    for index in range(30)
                ]
            if policy.get("connection_state"):
                group["connection_state"] = policy["connection_state"]
            group["statistics"] = {
                "p50": group_value,
                "p95": group_value,
                "max": group_value,
            }
            if policy.get("same_machine_lkg"):
                lkg_processes = []
                for index in range(30):
                    process = {"process_id": f"lkg-{group_name}-{index:02d}"}
                    if mode == "startup-marker":
                        process.update(
                            {
                                "phase": "measured",
                                "benchmark_startup": True,
                                "marker_count": 1,
                                "value": group_value,
                            }
                        )
                    else:
                        process.update(
                            {
                                "phase": "measured",
                                "samples": [group_value] * 10,
                                "representative": group_value,
                            }
                        )
                    lkg_processes.append(process)
                group["lkg"] = {
                    "source_sha": LKG_SHA,
                    "binary_hashes": {"rssh.exe": "a" * 64},
                    "runner_fingerprint_sha256": identity["runner_fingerprint_sha256"],
                    "platform": identity["platform"],
                    "requested_backend": requested_backend,
                    "warmups": 5,
                    "warmup_process_ids": [
                        f"lkg-warmup-{group_name}-{index}" for index in range(5)
                    ],
                    "measured_cold_processes": 30,
                    "timeout_seconds": 60,
                    "protocol": copy.deepcopy(protocol),
                    "processes": lkg_processes,
                    "statistics": {
                        "p50": group_value,
                        "p95": group_value,
                        "max": group_value,
                    },
                    "relative_regression_ratios": {"p50": 1.0, "p95": 1.0, "max": 1.0},
                }
                if mode == "residence":
                    group["lkg"]["actual_backend"] = group["actual_backend"]
                    group["lkg"]["adapter_identity"] = group["adapter_identity"]
            groups.append(group)
        return {
            "schema": "rssh.stage7.metric-raw/v1",
            "identity": identity,
            "warmups": 5,
            "warmup_process_ids": [f"warmup-{index}" for index in range(5)],
            "measured_cold_processes": 30,
            "timeout_seconds": 60,
            "protocol": protocol,
            "groups": groups,
        }

    def metric_protocol(self, policy: dict) -> dict:
        protocol = {
            "warmups": 5,
            "measured_cold_processes": 30,
            "timeout_seconds": 60,
            "cross_process_percentiles": "nearest-rank",
            "maximum": "raw-maximum",
            "sampling_mode": policy["sampling_mode"],
        }
        if policy["sampling_mode"] == "startup-marker":
            protocol.update(
                {
                    "samples_per_process": 1,
                    "stabilization_ms": 0,
                    "benchmark_startup": True,
                    "exit_immediately_after_cpu_bootstrap_present": True,
                }
            )
        else:
            protocol.update(
                {
                    "samples_per_process": 10,
                    "stabilization_ms": 5_000,
                    "sample_interval_ms": 100,
                    "process_representative": "nearest-rank-p50",
                    "flattening_for_percentiles": "forbidden",
                    "owner_ready_marker": policy["owner_ready_marker"],
                }
            )
        return protocol

    def epochs_for_state(self, state: str) -> tuple[dict | None, dict | None]:
        index = self.contract["states"].index(state)
        if index < self.contract["states"].index("extraction-ready"):
            return None, None
        rssh = {
            "r0_ref": PARENT_SHA,
            "r2_ref": None,
            "r3_deletion_ref": None,
            "r3_ref": None,
        }
        rterm = {"filtered_boundary_ref": FILTERED_SHA, "r1_ref": R1_SHA}
        if index >= self.contract["states"].index("dual-source-verified"):
            rssh["r2_ref"] = SOURCE_SHA
        if index >= self.contract["states"].index("split-complete"):
            rssh["r3_deletion_ref"] = SOURCE_SHA
            rssh["r3_ref"] = SOURCE_SHA
        return rssh, rterm

    def claim_value(self, rule: dict, rssh: dict | None, rterm: dict | None):
        kind = rule["kind"]
        if kind == "exact":
            return copy.deepcopy(rule["value"])
        if kind == "exact-set":
            return rule["value"]
        if kind == "full-sha":
            return SOURCE_SHA
        if kind == "sha256":
            return "9" * 64
        if kind == "non-empty-string":
            return "test-value"
        if kind == "non-empty-list":
            return ["test-command"]
        if kind == "integer-min":
            return rule["value"]
        if kind == "number-max":
            return min(1.0, rule["value"])
        if kind == "https-url":
            return "https://github.com/lcxinc/R-Term.git"
        if kind == "epoch-ref":
            namespace, field = rule["path"].split(".", 1)
            return {"rssh": rssh, "rterm": rterm}[namespace][field]
        raise AssertionError(f"unknown claim rule {rule}")

    def subject_refs_for_state(
        self, state: str, rssh: dict | None, rterm: dict | None
    ) -> dict[str, str]:
        result: dict[str, str] = {}
        requirements = self.contract["epoch_requirements_by_state"][state]
        for namespace, epoch in (("rssh", rssh), ("rterm", rterm)):
            fields = requirements[namespace]
            if fields is None:
                continue
            for field in fields:
                result[f"{namespace}.{field}"] = epoch[field]
        return result

    def mutate_artifact(
        self,
        root: Path,
        manifest: Path,
        artifact_type: str,
        mutate,
    ) -> None:
        manifest_data = json.loads(manifest.read_text(encoding="utf-8"))
        entry = next(
            item
            for item in manifest_data["entries"]
            if item["artifact_type"] == artifact_type
        )
        artifact_path = root / entry["path"]
        artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
        mutate(artifact)
        write_json(artifact_path, artifact)
        artifact_hash = sha256(artifact_path)
        artifact_size = artifact_path.stat().st_size
        artifact_id = entry["artifact_id"]

        fragment_hashes: dict[Path, str] = {}
        for fragment in root.rglob("artifact-manifest-fragment.json"):
            fragment_data = json.loads(fragment.read_text(encoding="utf-8"))
            changed = False
            for fragment_entry in fragment_data.get("entries", []):
                if fragment_entry.get("artifact_id") == artifact_id:
                    fragment_entry["sha256"] = artifact_hash
                    fragment_entry["size_bytes"] = artifact_size
                    changed = True
            if changed:
                write_json(fragment, fragment_data)
                fragment_hashes[fragment.resolve()] = sha256(fragment)

        manifests: list[tuple[int, Path, dict]] = []
        for candidate in root.glob("*.json"):
            data = json.loads(candidate.read_text(encoding="utf-8"))
            state = data.get("certified_state")
            if data.get("schema") == "rssh.stage7-evidence-manifest/v1" and state in self.contract["states"]:
                manifests.append((self.contract["states"].index(state), candidate, data))
        for _, candidate, data in sorted(manifests):
            changed = False
            for candidate_entry in data["entries"]:
                if candidate_entry["artifact_id"] == artifact_id:
                    candidate_entry["sha256"] = artifact_hash
                    candidate_entry["size_bytes"] = artifact_size
                    changed = True
            for fragment_ref in data["fragments"]:
                fragment_path = (candidate.parent / fragment_ref["path"]).resolve()
                if fragment_path in fragment_hashes:
                    fragment_ref["sha256"] = fragment_hashes[fragment_path]
                    changed = True
            prior_ref = data.get("prior_manifest")
            if prior_ref is not None:
                prior_path = (candidate.parent / prior_ref["path"]).resolve()
                prior_ref["sha256"] = sha256(prior_path)
                changed = True
            if changed:
                write_json(candidate, data)

    def rewrite_fragment_source(self, fragment: Path, source_sha: str) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        data["certified_commit"] = source_sha
        data["epoch_id"] = self.checker.certification_epoch_id(
            data["requested_state"], source_sha, data["rssh"], data["rterm"]
        )
        for entry in data["entries"]:
            entry["source_sha"] = source_sha
            entry["cohort_id"] = self.checker.cohort_id(entry)
            artifact = fragment.parent / entry["path"]
            payload = json.loads(artifact.read_text(encoding="utf-8"))
            payload["identity"]["source_sha"] = source_sha
            if entry["artifact_type"] == "local-two-bare-git-source-proof":
                payload["source_refs"] = [source_sha, LKG_SHA]
                payload["git_object_store_proof"] = git_object_store_proof(
                    [
                        complete_git_repository_snapshot(
                            "candidate",
                            {"candidate": source_sha, "lkg_boundary": LKG_SHA},
                            history_boundaries=[LKG_SHA],
                        ),
                        complete_git_repository_snapshot(
                            "lkg",
                            {"lkg": LKG_SHA},
                            history_boundaries=[LKG_SHA],
                        ),
                    ]
                )
            write_json(artifact, payload)
            entry["sha256"] = sha256(artifact)
            entry["size_bytes"] = artifact.stat().st_size
        write_json(fragment, data)

    def rewrite_fragment_epoch_ref(
        self,
        fragment: Path,
        namespace: str,
        field: str,
        value: str,
    ) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        data[namespace][field] = value
        data["epoch_id"] = self.checker.certification_epoch_id(
            data["requested_state"],
            data["certified_commit"],
            data["rssh"],
            data["rterm"],
        )
        subject_name = f"{namespace}.{field}"
        for entry in data["entries"]:
            if subject_name in entry["subject_refs"]:
                entry["subject_refs"][subject_name] = value
            entry["cohort_id"] = self.checker.cohort_id(entry)
            artifact_path = fragment.parent / entry["path"]
            artifact = json.loads(artifact_path.read_text(encoding="utf-8"))
            for claim_name, rule in self.contract["result_claims"].get(
                entry["artifact_type"], {}
            ).items():
                if rule.get("kind") == "epoch-ref" and rule.get("path") == subject_name:
                    artifact["claims"][claim_name] = value
            write_json(artifact_path, artifact)
            entry["sha256"] = sha256(artifact_path)
            entry["size_bytes"] = artifact_path.stat().st_size
        write_json(fragment, data)

    def split_raw_entry_by_process(
        self, fragment: Path, raw_type: str, aggregate_type: str
    ) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        raw_entry = next(
            entry for entry in data["entries"] if entry["artifact_type"] == raw_type
        )
        aggregate_entry = next(
            entry
            for entry in data["entries"]
            if entry["artifact_type"] == aggregate_type
        )
        raw_path = fragment.parent / raw_entry["path"]
        raw_payload = json.loads(raw_path.read_text(encoding="utf-8"))
        shards = []
        child_ids = []
        for index in range(30):
            shard_payload = copy.deepcopy(raw_payload)
            shard_payload["identity"]["run_id"] = f"process-run-{index:02d}"
            for group in shard_payload["groups"]:
                group["processes"] = [group["processes"][index]]
                group.pop("statistics", None)
            shard_entry = copy.deepcopy(raw_entry)
            shard_entry["artifact_id"] = f"{raw_type}/process-{index:02d}"
            shard_entry["run_id"] = shard_payload["identity"]["run_id"]
            shard_entry["cohort_id"] = self.checker.cohort_id(shard_entry)
            shard_path = fragment.parent / f"{raw_type}--process-{index:02d}.json"
            write_json(shard_path, shard_payload)
            shard_entry["path"] = shard_path.name
            shard_entry["sha256"] = sha256(shard_path)
            shard_entry["size_bytes"] = shard_path.stat().st_size
            shards.append(shard_entry)
            child_ids.append(shard_entry["artifact_id"])
        raw_path.unlink()
        aggregate_path = fragment.parent / aggregate_entry["path"]
        aggregate_payload = json.loads(aggregate_path.read_text(encoding="utf-8"))
        aggregate_payload["raw_children"] = sorted(child_ids)
        write_json(aggregate_path, aggregate_payload)
        aggregate_entry["children"] = sorted(child_ids)
        aggregate_entry["sha256"] = sha256(aggregate_path)
        aggregate_entry["size_bytes"] = aggregate_path.stat().st_size
        data["entries"] = [
            entry for entry in data["entries"] if entry is not raw_entry
        ] + shards
        write_json(fragment, data)

    def split_metric_entry_with_different_warmups(
        self, fragment: Path, raw_type: str
    ) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        raw_entry = next(
            entry for entry in data["entries"] if entry["artifact_type"] == raw_type
        )
        raw_path = fragment.parent / raw_entry["path"]
        raw_payload = json.loads(raw_path.read_text(encoding="utf-8"))
        shards = []
        for index in range(30):
            shard_payload = copy.deepcopy(raw_payload)
            shard_payload["identity"]["run_id"] = f"process-run-{index:02d}"
            shard_payload["warmup_process_ids"] = [
                f"warmup-{index:02d}-{warmup}" for warmup in range(5)
            ]
            for group in shard_payload["groups"]:
                group["processes"] = [group["processes"][index]]
                group.pop("statistics", None)
                if index:
                    group.pop("lkg", None)
            shard_entry = copy.deepcopy(raw_entry)
            shard_entry["artifact_id"] = f"{raw_type}/process-{index:02d}"
            shard_entry["run_id"] = shard_payload["identity"]["run_id"]
            shard_entry["cohort_id"] = self.checker.cohort_id(shard_entry)
            shard_path = fragment.parent / f"{raw_type}--process-{index:02d}.json"
            write_json(shard_path, shard_payload)
            shard_entry["path"] = shard_path.name
            shard_entry["sha256"] = sha256(shard_path)
            shard_entry["size_bytes"] = shard_path.stat().st_size
            shards.append(shard_entry)
        raw_path.unlink()
        data["entries"] = [
            entry for entry in data["entries"] if entry is not raw_entry
        ] + shards
        write_json(fragment, data)

    def split_raw_entry_by_group_with_different_warmups(
        self, fragment: Path, raw_type: str, aggregate_type: str
    ) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        raw_entry = next(
            entry for entry in data["entries"] if entry["artifact_type"] == raw_type
        )
        aggregate_entry = next(
            entry
            for entry in data["entries"]
            if entry["artifact_type"] == aggregate_type
        )
        raw_path = fragment.parent / raw_entry["path"]
        raw_payload = json.loads(raw_path.read_text(encoding="utf-8"))
        shards = []
        child_ids = []
        for index, group in enumerate(raw_payload["groups"]):
            shard_payload = copy.deepcopy(raw_payload)
            shard_payload["identity"]["run_id"] = f"group-run-{index:02d}"
            shard_payload["warmup_process_ids"] = [
                f"group-{index:02d}-warmup-{warmup}" for warmup in range(5)
            ]
            shard_payload["groups"] = [copy.deepcopy(group)]
            shard_entry = copy.deepcopy(raw_entry)
            shard_entry["artifact_id"] = f"{raw_type}/group-{index:02d}"
            shard_entry["run_id"] = shard_payload["identity"]["run_id"]
            shard_entry["cohort_id"] = self.checker.cohort_id(shard_entry)
            shard_path = fragment.parent / f"{raw_type}--group-{index:02d}.json"
            write_json(shard_path, shard_payload)
            shard_entry["path"] = shard_path.name
            shard_entry["sha256"] = sha256(shard_path)
            shard_entry["size_bytes"] = shard_path.stat().st_size
            shards.append(shard_entry)
            child_ids.append(shard_entry["artifact_id"])
        raw_path.unlink()
        aggregate_path = fragment.parent / aggregate_entry["path"]
        aggregate = json.loads(aggregate_path.read_text(encoding="utf-8"))
        aggregate["raw_children"] = sorted(child_ids)
        write_json(aggregate_path, aggregate)
        aggregate_entry["children"] = sorted(child_ids)
        aggregate_entry["sha256"] = sha256(aggregate_path)
        aggregate_entry["size_bytes"] = aggregate_path.stat().st_size
        data["entries"] = [
            entry for entry in data["entries"] if entry is not raw_entry
        ] + shards
        write_json(fragment, data)

    def set_diagnostic_unsupported_suffix(
        self,
        fragment: Path,
        *,
        backend: str,
        first_stage: str,
        reason: str,
    ) -> None:
        data = json.loads(fragment.read_text(encoding="utf-8"))
        raw_entry = next(
            entry
            for entry in data["entries"]
            if entry["artifact_type"] == "attribution-matrix-raw"
        )
        aggregate_entry = next(
            entry
            for entry in data["entries"]
            if entry["artifact_type"] == "attribution-matrix-aggregate"
        )
        raw_path = fragment.parent / raw_entry["path"]
        raw = json.loads(raw_path.read_text(encoding="utf-8"))
        stages = self.contract["artifact_policies"]["attribution-matrix-raw"][
            "matrix_stages"
        ]
        start = stages.index(first_stage)
        unsupported_count = 0
        for group in raw["groups"]:
            group_backend, group_stage = group["name"].split("/", 1)
            if group_backend != backend or stages.index(group_stage) < start:
                continue
            unsupported_count += 1
            name = group["name"]
            metric = group["metric"]
            sampling_mode = group["sampling_mode"]
            requested_backend = group["requested_backend"]
            group.clear()
            group.update(
                {
                    "name": name,
                    "metric": metric,
                    "sampling_mode": sampling_mode,
                    "requested_backend": requested_backend,
                    "support_status": "unsupported",
                    "unsupported_reason": reason,
                    "unsupported_at_stage": first_stage,
                }
            )
        write_json(raw_path, raw)
        raw_entry["sha256"] = sha256(raw_path)
        raw_entry["size_bytes"] = raw_path.stat().st_size
        aggregate_path = fragment.parent / aggregate_entry["path"]
        aggregate = json.loads(aggregate_path.read_text(encoding="utf-8"))
        aggregate["group_statistics"] = aggregate["group_statistics"][
            :-unsupported_count
        ]
        write_json(aggregate_path, aggregate)
        aggregate_entry["sha256"] = sha256(aggregate_path)
        aggregate_entry["size_bytes"] = aggregate_path.stat().st_size
        write_json(fragment, data)


if __name__ == "__main__":
    unittest.main()
