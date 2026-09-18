#!/usr/bin/env python3
"""Derive the fail-closed Stage 7 split state from immutable raw evidence."""

from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Iterable, NoReturn


CONTRACT_SCHEMA = "rssh.stage7-split-contract/v1"
MANIFEST_SCHEMA = "rssh.stage7-evidence-manifest/v1"
FRAGMENT_SCHEMA = "rssh.stage7-artifact-manifest-fragment/v1"
RAW_SCHEMA = "rssh.stage7.metric-raw/v1"
AGGREGATE_SCHEMA = "rssh.stage7.metric-aggregate/v1"
RESULT_SCHEMA = "rssh.stage7.result/v1"
GIT_STORE_SCHEMA = "rssh.stage7.git-object-store-proof/v1"
GIT_MAP_SCHEMA = "rssh.stage7.source-to-filtered-map-proof/v1"
REPLAYABLE_BARE_SCHEMA = "rssh.stage7.replayable-bare-repository/v1"
TREE_SNAPSHOT_SCHEMA = "rssh.stage7.filtered-tree-snapshot/v1"
TREE_PROJECTION_SCHEMA = "rssh.stage7.tree-projection-proof/v1"
BOOTSTRAP_PROJECTION_SCHEMA = "rssh.stage7.bootstrap-projection-proof/v1"
CROSS_REPOSITORY_ARTIFACT_TYPES = frozenset(
    {
        "source-to-filtered-history-map",
        "rterm-external-source-proof",
        "rterm-extraction-manifest",
    }
)
STATES = (
    "blocked",
    "attribution-ready",
    "windows-memory-go",
    "cross-platform-go",
    "extraction-ready",
    "dual-source-verified",
    "split-complete",
)
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
FROZEN_LKG = "21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6"
FROZEN_PRODUCT_LKG = "4cbee13c591ded5ccfc6b0aec68f2b33143528c1"
FROZEN_CONTRACT_SHA256 = "edf01edef3dd9f4d940d11c75a3951df4919cd12e7b9322683f2fa4e79ab4e7d"
MAX_JSON_BYTES = 768 * 1024 * 1024
JSON_READ_CHUNK_BYTES = 1024 * 1024
MAX_GIT_OBJECT_BYTES = 16 * 1024 * 1024
MAX_GIT_OBJECT_BASE64_CHARS = 4 * ((MAX_GIT_OBJECT_BYTES + 2) // 3)
MAX_REPLAY_FILE_BYTES = 20 * 1024 * 1024
MAX_REPLAY_FILE_BASE64_CHARS = 4 * ((MAX_REPLAY_FILE_BYTES + 2) // 3)
MAX_REPLAY_TOTAL_BYTES = 192 * 1024 * 1024
MAX_GIT_CLOSURE_TOTAL_BYTES = 192 * 1024 * 1024
MAX_FILTERED_TREE_TOTAL_BYTES = 16 * 1024 * 1024
MAX_LOOSE_OBJECT_BYTES = MAX_GIT_OBJECT_BYTES + 64
MAX_PARSED_TREE_ENTRIES = 262_144
MAX_FLATTENED_TREE_NODES = 65_536
MAX_FLATTENED_TREE_LEAVES = 262_144
MAX_EXPANDED_TREE_PATH_BYTES = 64 * 1024 * 1024
WINDOWS_RESERVED_NAMES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    *(f"COM{index}" for index in range(1, 10)),
    *(f"LPT{index}" for index in range(1, 10)),
    *(f"COM{index}" for index in "¹²³"),
    *(f"LPT{index}" for index in "¹²³"),
}
FROZEN_GATES = {
    "first_present_p50_ms_max": 400,
    "first_present_p95_ms_max": 500,
    "first_frame_private_bytes_p95_max": 57_671_680,
    "first_frame_private_bytes_max_exclusive": 62_914_560,
    "empty_window_private_working_set_p95_max": 47_185_920,
    "ssh1_private_working_set_p95_max": 62_914_560,
    "gpu_steady_bytes_max": 268_435_456,
    "relative_regression_ratio_max": 1.05,
}
FROZEN_WINDOWS_DETERMINISTIC_SUITE = [
    {"id": "format", "argv": ["cargo", "fmt", "--all", "--", "--check"], "exit_code": 0},
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
FROZEN_DIAGNOSTIC_OUTCOMES = {
    "statuses": ["supported", "unsupported"],
    "required_product_unsupported": "forbidden",
    "reason_pattern": "^[a-z0-9][a-z0-9-]*$",
    "stage_semantics": "supported-prefix-then-unsupported-suffix",
}
PROJECT_RESOURCE_SCHEMA = "rssh.project-owned-resources/v1"
PROJECT_RESOURCE_NUMERIC_FIELDS = (
    "cpu_staging_bytes",
    "cpu_surface_count",
    "cpu_present_count",
    "instance_count",
    "surface_count",
    "adapter_count",
    "device_count",
    "queue_count",
    "surface_configure_count",
    "surface_acquire_count",
    "clear_present_count",
    "pipeline_count",
    "pipeline_layout_count",
    "materialized_buffer_count",
    "retained_font_bytes",
    "inactive_font_bytes",
    "indexed_font_count",
    "active_font_count",
    "catalog_builds",
    "catalog_generation",
    "glyph_atlas_bytes",
    "raster_cache_bytes",
    "image_texture_bytes",
    "snapshot_bytes",
    "instance_buffer_bytes",
    "upload_buffer_bytes",
    "total_allocated_buffer_bytes",
    "total_allocated_texture_bytes",
    "base_text_renderer_materialization_count",
    "cursor_text_renderer_materialization_count",
    "config_load_count",
    "config_watcher_count",
    "pty_start_count",
    "ssh_start_count",
    "post_ready_task_count",
)
PROJECT_RESOURCE_FIELDS = frozenset((*PROJECT_RESOURCE_NUMERIC_FIELDS, "backend", "adapter_name"))
ATTRIBUTION_STAGES = (
    "cpu-window",
    "instance-surface",
    "adapter-device",
    "configured-surface-clear",
    "layer-pipelines",
    "fixture-font-text",
    "platform-font-index",
    "full-frame",
)
FROZEN_OWNED_PROJECTION_REQUIRED = [
    ("crates/rterm-types", "crates/rterm-types"),
    ("crates/rssh-terminal", "crates/rterm-terminal"),
    ("crates/rssh-runtime", "crates/rterm-runtime"),
    ("crates/rterm-fonts", "crates/rterm-fonts"),
    ("crates/rterm-render-core", "crates/rterm-render-core"),
    ("crates/rterm-render-cpu", "crates/rterm-render-cpu"),
    ("crates/rterm-render-wgpu", "crates/rterm-render-wgpu"),
    ("vendor/glyphon-0.12.0", "vendor/glyphon-0.12.0"),
    ("vendor/gpu-allocator-0.28.0", "vendor/gpu-allocator-0.28.0"),
    ("tests/fixtures/fonts", "tests/fixtures/fonts"),
    ("tests/fixtures/task10_legacy_trace_pack.gz", "tests/fixtures/task10_legacy_trace_pack.gz"),
    ("tests/fixtures/task10_trace_provenance.json", "tests/fixtures/task10_trace_provenance.json"),
    ("tests/task10_fixture_trace_support.rs", "tests/task10_fixture_trace_support.rs"),
    ("tests/task10_frozen_trace_pack.rs", "tests/task10_frozen_trace_pack.rs"),
    ("tests/task10_runtime_trace_codec.rs", "tests/task10_runtime_trace_codec.rs"),
    ("tests/task10_terminal_trace_codec.rs", "tests/task10_terminal_trace_codec.rs"),
    ("tests/task10_test_body_digest.rs", "tests/task10_test_body_digest.rs"),
    ("scripts/ci/check-task10-provenance.py", "scripts/ci/check-task10-provenance.py"),
    ("scripts/ci/task10_rust_test_body.py", "scripts/ci/task10_rust_test_body.py"),
    (
        "scripts/ci/tests/test_check_task10_provenance.py",
        "scripts/ci/tests/test_check_task10_provenance.py",
    ),
    ("scripts/ci/check-rterm-release-contract.py", "scripts/ci/check-rterm-release-contract.py"),
    ("scripts/ci/rehearse-rterm-consumer.py", "scripts/ci/rehearse-rterm-consumer.py"),
    ("scripts/ci/rterm-release-contract.json", "scripts/ci/rterm-release-contract.json"),
    (
        "scripts/ci/run-rterm-release-comparison.ps1",
        "scripts/ci/run-rterm-release-comparison.ps1",
    ),
    (
        "scripts/ci/tests/test_check_rterm_release_contract.py",
        "scripts/ci/tests/test_check_rterm_release_contract.py",
    ),
    (
        "scripts/ci/tests/test_rehearse_rterm_consumer.py",
        "scripts/ci/tests/test_rehearse_rterm_consumer.py",
    ),
    ("docs/release/rterm-api-compatibility.md", "docs/release/rterm-api-compatibility.md"),
    ("docs/release/rterm-history-paths.txt", "docs/release/rterm-history-paths.txt"),
    ("LICENSE", "LICENSE"),
    ("NOTICE", "NOTICE"),
]
FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED = [
    ("release/rterm-bootstrap", "release/rterm-bootstrap")
]
FROZEN_BOOTSTRAP_TEMPLATE_MAPPINGS = [
    ("release/rterm-bootstrap/Cargo.toml", "Cargo.toml"),
    ("release/rterm-bootstrap/rust-toolchain.toml", "rust-toolchain.toml"),
    ("release/rterm-bootstrap/.gitignore", ".gitignore"),
    ("release/rterm-bootstrap/.gitattributes", ".gitattributes"),
    ("release/rterm-bootstrap/README.md", "README.md"),
    ("release/rterm-bootstrap/CONTRIBUTING.md", "CONTRIBUTING.md"),
    ("release/rterm-bootstrap/SECURITY.md", "SECURITY.md"),
    ("release/rterm-bootstrap/LICENSE", "LICENSE"),
    ("release/rterm-bootstrap/NOTICE", "NOTICE"),
    ("release/rterm-bootstrap/deny.toml", "deny.toml"),
    ("release/rterm-bootstrap/.github/workflows/ci.yml", ".github/workflows/ci.yml"),
    (
        "release/rterm-bootstrap/contracts/rterm-consumer/Cargo.toml",
        "contracts/rterm-consumer/Cargo.toml",
    ),
    ("release/rterm-bootstrap/docs/release-policy.md", "docs/release-policy.md"),
]
TOP_LEVEL_FIELDS = {
    "schema",
    "contract_sha256",
    "requested_state",
    "certified_state",
    "certified_commit",
    "epoch_id",
    "rssh",
    "rterm",
    "created_by",
    "prior_manifest",
    "fragments",
    "entries",
}
ENTRY_REQUIRED = {
    "artifact_type",
    "artifact_id",
    "role",
    "scope",
    "payload_schema",
    "path",
    "sha256",
    "size_bytes",
    "producing_command",
    "producing_argv",
    "source_sha",
    "subject_refs",
    "platform",
    "run_id",
    "cohort_id",
    "children",
}
ENTRY_OPTIONAL = {
    "binary_hashes",
    "runner_fingerprint_sha256",
    "certification_eligible",
}
_SAFE_GIT_CONTROL_SNAPSHOTS: dict[str, tuple[Path, tuple[Any, ...]]] = {}
_POISONED_GIT_ROOTS: set[str] = set()
_STRICT_GIT_REPLAY_CACHE: set[str] = set()


def nearest_rank(values: Iterable[int | float], percentile: float) -> int | float:
    ordered = sorted(values)
    if not ordered:
        raise ValueError("nearest_rank requires at least one value")
    if not 0 < percentile <= 1:
        raise ValueError("percentile must be in (0, 1]")
    index = max(1, math.ceil(len(ordered) * percentile)) - 1
    return ordered[index]


def process_representative(samples: Iterable[int | float]) -> int | float:
    return nearest_rank(samples, 0.50)


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_sha256(value: Any) -> str:
    digest = hashlib.sha256()
    encoder = json.JSONEncoder(sort_keys=True, separators=(",", ":"))
    for chunk in encoder.iterencode(value):
        digest.update(chunk.encode("utf-8"))
    return digest.hexdigest()


def runner_canonical_sha256(value: Any) -> str:
    """Hash the cross-language runner cohort protocol without ASCII escaping."""

    def validate(item: Any, label: str) -> None:
        if isinstance(item, dict):
            for key, child in item.items():
                if not isinstance(key, str):
                    raise TypeError(f"{label} contains a non-string object key")
                validate(child, f"{label}.{key}")
        elif isinstance(item, list):
            for index, child in enumerate(item):
                validate(child, f"{label}[{index}]")
        elif not isinstance(item, (str, int, bool)) or isinstance(item, float):
            raise TypeError(
                f"{label} contains a value outside integer/bool/string runner protocol"
            )

    validate(value, "runner fields")
    digest = hashlib.sha256()
    encoder = json.JSONEncoder(
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    )
    for chunk in encoder.iterencode(value):
        digest.update(chunk.encode("utf-8"))
    return digest.hexdigest()


def certification_epoch_id(
    state: str, certified_commit: str, rssh_epoch: Any, rterm_epoch: Any
) -> str:
    return canonical_sha256(
        {
            "state": state,
            "certified_commit": certified_commit,
            "rssh": rssh_epoch,
            "rterm": rterm_epoch,
        }
    )


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


def strict_json_loads(text: str) -> Any:
    def reject_constant(value: str) -> NoReturn:
        raise ValueError(f"non-finite JSON number {value} is forbidden")

    def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key {key}")
            result[key] = value
        return result

    return json.loads(
        text,
        parse_constant=reject_constant,
        object_pairs_hook=unique_object,
    )


def read_bounded_json_text(path: Path) -> str:
    encoded = bytearray()
    with path.open("rb") as source:
        while len(encoded) <= MAX_JSON_BYTES:
            remaining = MAX_JSON_BYTES + 1 - len(encoded)
            chunk = source.read(min(JSON_READ_CHUNK_BYTES, remaining))
            if not chunk:
                break
            encoded.extend(chunk)
    if len(encoded) > MAX_JSON_BYTES:
        raise ValueError(f"JSON input exceeds the {MAX_JSON_BYTES}-byte size limit")
    return encoded.decode("utf-8")


def read_json(path: Path, label: str, violations: list[str]) -> dict[str, Any] | None:
    try:
        value = strict_json_loads(read_bounded_json_text(path))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError, ValueError) as error:
        violations.append(f"{label}: cannot parse JSON: {error}")
        return None
    if not isinstance(value, dict):
        violations.append(f"{label}: JSON root must be an object")
        return None
    return value


SCHEMA_KEYWORDS = {
    "$schema",
    "$id",
    "$defs",
    "$ref",
    "title",
    "type",
    "const",
    "enum",
    "oneOf",
    "required",
    "properties",
    "additionalProperties",
    "propertyNames",
    "minProperties",
    "items",
    "minItems",
    "uniqueItems",
    "minLength",
    "pattern",
    "minimum",
}


def schema_ref(root_schema: dict[str, Any], reference: str) -> Any:
    if not reference.startswith("#/"):
        raise ValueError("only local frozen-schema references are supported")
    value: Any = root_schema
    for component in reference[2:].split("/"):
        key = component.replace("~1", "/").replace("~0", "~")
        if not isinstance(value, dict) or key not in value:
            raise ValueError(f"unresolved frozen-schema reference {reference}")
        value = value[key]
    return value


def json_equal(left: Any, right: Any) -> bool:
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return set(left) == set(right) and all(
            json_equal(left[key], right[key]) for key in left
        )
    if isinstance(left, list):
        return len(left) == len(right) and all(
            json_equal(a, b) for a, b in zip(left, right)
        )
    return left == right


def validate_json_schema(
    instance: Any,
    schema: Any,
    root_schema: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    if not isinstance(schema, dict):
        violations.append(f"{label}: frozen schema node must be an object")
        return
    unknown_keywords = set(schema) - SCHEMA_KEYWORDS
    if unknown_keywords:
        violations.append(
            f"{label}: frozen schema contains unsupported keywords {sorted(unknown_keywords)}"
        )
        return
    if "$ref" in schema:
        try:
            referenced = schema_ref(root_schema, schema["$ref"])
        except (TypeError, ValueError) as error:
            violations.append(f"{label}: {error}")
            return
        validate_json_schema(instance, referenced, root_schema, label, violations)
    if "oneOf" in schema:
        branches = schema["oneOf"]
        if not isinstance(branches, list) or not branches:
            violations.append(f"{label}: frozen oneOf must be a non-empty array")
        else:
            matches = 0
            for branch in branches:
                branch_violations: list[str] = []
                validate_json_schema(
                    instance, branch, root_schema, label, branch_violations
                )
                if not branch_violations:
                    matches += 1
            if matches != 1:
                violations.append(f"{label}: value must match exactly one frozen schema branch")
    expected_type = schema.get("type")
    if expected_type is not None:
        type_matches = {
            "object": isinstance(instance, dict),
            "array": isinstance(instance, list),
            "string": isinstance(instance, str),
            "integer": isinstance(instance, int) and not isinstance(instance, bool),
            "boolean": isinstance(instance, bool),
            "null": instance is None,
        }
        if expected_type not in type_matches:
            violations.append(f"{label}: frozen schema uses unsupported type {expected_type!r}")
            return
        if not type_matches[expected_type]:
            violations.append(f"{label}: value is not frozen type {expected_type}")
            return
    if "const" in schema and not json_equal(instance, schema["const"]):
        violations.append(f"{label}: value differs from frozen const")
    if "enum" in schema and not any(json_equal(instance, item) for item in schema["enum"]):
        violations.append(f"{label}: value is outside the frozen enum")
    if isinstance(instance, dict):
        required = schema.get("required", [])
        if not isinstance(required, list) or not all(isinstance(item, str) for item in required):
            violations.append(f"{label}: frozen required must be a string array")
            required = []
        for name in required:
            if name not in instance:
                violations.append(f"{label}: missing frozen required property {name}")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            violations.append(f"{label}: frozen properties must be an object")
            properties = {}
        for name, value in instance.items():
            if name in properties:
                validate_json_schema(
                    value,
                    properties[name],
                    root_schema,
                    f"{label}.{name}",
                    violations,
                )
                continue
            additional = schema.get("additionalProperties", True)
            if additional is False:
                violations.append(f"{label}: unknown frozen property {name}")
            elif isinstance(additional, dict):
                validate_json_schema(
                    value,
                    additional,
                    root_schema,
                    f"{label}.{name}",
                    violations,
                )
            elif additional is not True:
                violations.append(f"{label}: unsupported additionalProperties rule")
        property_names = schema.get("propertyNames")
        if property_names is not None:
            for name in instance:
                validate_json_schema(
                    name,
                    property_names,
                    root_schema,
                    f"{label} property name",
                    violations,
                )
        minimum_properties = schema.get("minProperties")
        if minimum_properties is not None and len(instance) < minimum_properties:
            violations.append(f"{label}: object has fewer than minProperties")
    if isinstance(instance, list):
        minimum_items = schema.get("minItems")
        if minimum_items is not None and len(instance) < minimum_items:
            violations.append(f"{label}: array has fewer than minItems")
        if schema.get("uniqueItems") is True:
            canonical = [json.dumps(item, sort_keys=True, separators=(",", ":")) for item in instance]
            if len(canonical) != len(set(canonical)):
                violations.append(f"{label}: array items are not unique")
        if "items" in schema:
            for index, value in enumerate(instance):
                validate_json_schema(
                    value,
                    schema["items"],
                    root_schema,
                    f"{label}[{index}]",
                    violations,
                )
    if isinstance(instance, str):
        minimum_length = schema.get("minLength")
        if minimum_length is not None and len(instance) < minimum_length:
            violations.append(f"{label}: string is shorter than minLength")
        pattern = schema.get("pattern")
        if pattern is not None:
            try:
                matches = re.search(pattern, instance) is not None
            except (re.error, TypeError) as error:
                violations.append(f"{label}: invalid frozen pattern: {error}")
            else:
                if not matches:
                    violations.append(f"{label}: string does not match frozen pattern")
    if (
        "minimum" in schema
        and isinstance(instance, int)
        and not isinstance(instance, bool)
        and instance < schema["minimum"]
    ):
        violations.append(f"{label}: integer is below frozen minimum")


def is_full_sha(value: Any) -> bool:
    return isinstance(value, str) and FULL_SHA.fullmatch(value) is not None


def is_sha256(value: Any) -> bool:
    return isinstance(value, str) and SHA256.fullmatch(value) is not None


def windows_path_components_are_safe(parts: Iterable[str]) -> bool:
    for part in parts:
        if (
            not part
            or part.endswith((" ", "."))
            or any(character in part for character in '<>:"|?*')
            or any(ord(character) < 32 for character in part)
            or part.split(".", 1)[0].upper() in WINDOWS_RESERVED_NAMES
        ):
            return False
    return True


def contained_file(
    base: Path,
    value: Any,
    label: str,
    violations: list[str],
    *,
    must_exist: bool = True,
) -> Path | None:
    if not isinstance(value, str) or not value:
        violations.append(f"{label}: path must be a non-empty relative string")
        return None
    if "\\" in value or PureWindowsPath(value).drive or value.startswith("/"):
        violations.append(f"{label}: path is not contained beneath the manifest directory")
        return None
    pure = PurePosixPath(value)
    if (
        pure.is_absolute()
        or any(part in {"", ".", ".."} for part in pure.parts)
        or not windows_path_components_are_safe(pure.parts)
    ):
        violations.append(f"{label}: path is not contained beneath the manifest directory")
        return None
    base_resolved = base.resolve()
    candidate = (base / Path(*pure.parts)).resolve()
    try:
        candidate.relative_to(base_resolved)
    except ValueError:
        violations.append(f"{label}: path is not contained beneath the manifest directory")
        return None
    cursor = base / Path(*pure.parts)
    while True:
        if cursor.is_symlink() or (
            hasattr(cursor, "is_junction") and cursor.is_junction()
        ):
            violations.append(f"{label}: symlink/reparse-point evidence paths are forbidden")
            return None
        if cursor == base or cursor.parent == cursor:
            break
        cursor = cursor.parent
    if must_exist and not candidate.is_file():
        violations.append(f"{label}: referenced file does not exist: {value}")
        return None
    return candidate


def validate_contract(contract_path: Path) -> tuple[dict[str, Any] | None, list[str]]:
    violations: list[str] = []
    contract = read_json(contract_path, "contract", violations)
    if contract is None:
        return None, violations
    if canonical_sha256(contract) != FROZEN_CONTRACT_SHA256:
        violations.append("contract differs from the complete frozen contract digest")
    if contract.get("schema") != CONTRACT_SCHEMA:
        violations.append(f"contract schema must be {CONTRACT_SCHEMA}")
    if contract.get("initial_state") != "blocked":
        violations.append("contract initial_state must be blocked")
    if tuple(contract.get("states", [])) != STATES:
        violations.append("contract state order is not the frozen Stage 7 order")
    if contract.get("lkg_rssh_ref") != FROZEN_LKG:
        violations.append("lkg_rssh_ref must be the frozen full R-SSH SHA")
    if contract.get("product_lkg_ref") != FROZEN_PRODUCT_LKG:
        violations.append("product_lkg_ref must be the frozen probe-only product SHA")
    if contract.get("windows_product_gates") != FROZEN_GATES:
        violations.append("Windows product gates differ from the approved frozen values")
    if contract.get("windows_backends") != {
        "required_product": ["auto"],
        "diagnostic_only": ["dx12", "vulkan", "gl"],
    }:
        violations.append("Windows product and diagnostic backend sets are not frozen")
    if contract.get("diagnostic_probe_outcomes") != FROZEN_DIAGNOSTIC_OUTCOMES:
        violations.append("diagnostic supported/unsupported outcome semantics are not frozen")
    expected_owned_inventory = {
        "required": [
            {"source_prefix": source, "filtered_prefix": filtered}
            for source, filtered in FROZEN_OWNED_PROJECTION_REQUIRED
        ],
        "future_required": [
            {"source_prefix": source, "filtered_prefix": filtered}
            for source, filtered in FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED
        ],
    }
    if contract.get("owned_projection_inventory") != expected_owned_inventory:
        violations.append("R-Term owned source/rename inventory differs from the frozen contract")
    expected_bootstrap_templates = [
        {"source_path": source, "filtered_path": filtered}
        for source, filtered in FROZEN_BOOTSTRAP_TEMPLATE_MAPPINGS
    ]
    if contract.get("bootstrap_template_mappings") != expected_bootstrap_templates:
        violations.append("R-Term bootstrap template source/target mappings are not frozen")
    protocol = contract.get("protocol")
    if not isinstance(protocol, dict) or protocol != {
        "warmups": 5,
        "measured_cold_processes": 30,
        "timeout_seconds": 60,
        "cross_process_percentiles": "nearest-rank",
        "process_representative": "nearest-rank-p50",
        "maximum": "raw-maximum",
    }:
        violations.append("measurement protocol is not the frozen 5+30 nearest-rank protocol")
    if contract.get("relative_regression_statistics") != ["p50", "p95", "max"]:
        violations.append("relative regression must be recomputed for p50, p95, and raw maximum")
    if not json_equal(
        contract.get("windows_deterministic_suite"), FROZEN_WINDOWS_DETERMINISTIC_SUITE
    ):
        violations.append("Windows deterministic suite differs from the exact approved order")
    deterministic_rule = (
        contract.get("result_claims", {})
        .get("windows-deterministic-suite", {})
        .get("exact_suite")
    )
    if not json_equal(
        deterministic_rule,
        {"kind": "exact", "value": FROZEN_WINDOWS_DETERMINISTIC_SUITE},
    ):
        violations.append("Windows deterministic suite result rule is not exact")
    sampling = contract.get("sampling")
    if not isinstance(sampling, dict):
        violations.append("sampling contracts must be an object")
    else:
        startup = sampling.get("startup", {})
        residence = sampling.get("residence", {})
        if not (
            startup.get("marker") == "first_frame_memory"
            and startup.get("samples_per_process") == 1
            and startup.get("stabilization_ms") == 0
            and startup.get("exit_immediately_after_cpu_bootstrap_present") is True
            and startup.get("final_renderer") == "cpu"
            and startup.get("backend_identity") == "forbidden"
        ):
            violations.append("startup sampling must use one CPU-bootstrap marker and no residence sampling")
        if not (
            residence.get("owner_ready_marker_required") is True
            and residence.get("stabilization_ms") == 5_000
            and residence.get("sample_interval_ms") == 100
            and residence.get("samples_per_process") == 10
            and residence.get("process_representative") == "nearest-rank-p50"
            and residence.get("flattening_for_percentiles") == "forbidden"
        ):
            violations.append("residence sampling must retain 30 process medians and all raw maxima")

    artifact_types = contract.get("artifact_types")
    policies = contract.get("artifact_policies")
    multiplicity = contract.get("artifact_multiplicity")
    result_claims = contract.get("result_claims")
    epoch_requirements = contract.get("epoch_requirements_by_state")
    new_by_state = contract.get("new_artifacts_by_state")
    required_by_state = contract.get("required_artifacts_by_state")
    if not isinstance(artifact_types, list) or not all(
        isinstance(value, str) and value for value in artifact_types
    ):
        violations.append("artifact_types must be a closed non-empty string inventory")
        artifact_types = []
    if len(set(artifact_types)) != len(artifact_types):
        violations.append("artifact_types contains duplicates")
    if not isinstance(policies, dict) or set(policies) != set(artifact_types):
        violations.append("artifact_policies must cover exactly the closed artifact inventory")
        policies = {}
    if not isinstance(multiplicity, dict) or set(multiplicity) != {"singleton", "multiple"}:
        violations.append("artifact_multiplicity must explicitly partition singleton and multiple types")
    else:
        singleton = multiplicity.get("singleton")
        multiple = multiplicity.get("multiple")
        if not isinstance(singleton, list) or not isinstance(multiple, dict):
            violations.append("artifact multiplicity inventories have invalid shapes")
        else:
            singleton_set = set(singleton)
            multiple_set = set(multiple)
            if singleton_set.intersection(multiple_set) or singleton_set.union(multiple_set) != set(artifact_types):
                violations.append("artifact multiplicity must partition the closed artifact inventory")
            for artifact_type, rule in multiple.items():
                if not isinstance(rule, dict) or not isinstance(rule.get("minimum"), int) or rule["minimum"] < 1:
                    violations.append(f"{artifact_type}: multiple artifact rule needs a positive minimum")
                platforms = rule.get("required_platforms", []) if isinstance(rule, dict) else []
                if platforms and (not isinstance(platforms, list) or len(platforms) != len(set(platforms))):
                    violations.append(f"{artifact_type}: required platforms must be a unique list")
    result_types = {
        artifact_type
        for artifact_type, policy in policies.items()
        if isinstance(policy, dict) and policy.get("content_kind") == "result"
    }
    if not isinstance(result_claims, dict) or set(result_claims) != result_types:
        violations.append("result_claims must define type-specific claims for every result artifact")
    expected_font_reductions = [
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
    ]
    font_aggregate_policy = policies.get("font-ownership-aggregate", {})
    if font_aggregate_policy.get("p50_reductions") != expected_font_reductions:
        violations.append("font ownership p50 reduction rules differ from the frozen proof contract")
    expected_font_raw_policy = {
        "content_kind": "raw-metric",
        "sampling_mode": "residence",
        "metric": "windows_private_working_set_bytes",
        "platform": "windows-x86_64",
        "binary_identity": True,
        "runner_identity": True,
        "certification_eligible": True,
        "owner_ready_marker": "font_ownership_ready",
        "warmup_process_count": 15,
        "warmups_per_group": 5,
        "font_resource_evidence": True,
        "required_groups": [
            "current-copied/ascii",
            "shared-all/ascii",
            "lazy/ascii",
        ],
    }
    if policies.get("font-ownership-raw") != expected_font_raw_policy:
        violations.append("font ownership raw cohort/content policy differs from the frozen proof contract")
    font_catalog_policy = policies.get("font-catalog-fingerprint", {})
    if font_catalog_policy.get("frame_generation_scope") != "per-specimen-record":
        violations.append("font frame generation scope must be per-specimen-record")
    certification_artifacts = {
        artifact_type
        for artifact_type, policy in policies.items()
        if isinstance(policy, dict) and policy.get("certification_eligible") is True
    }
    if certification_artifacts != {
        "font-ownership-raw",
        "font-ownership-aggregate",
        "runner-fingerprint",
        "font-catalog-fingerprint",
    }:
        violations.append(
            "font certification eligibility policy differs from the frozen artifact set"
        )
    expected_font_claims = {
        "catalog_policy_version": {"kind": "non-empty-string"},
        "ordered_sources_hashed": {"kind": "exact", "value": True},
        "functional_specimen_count": {"kind": "exact", "value": 6},
        "zero_tofu": {"kind": "exact", "value": True},
        "single_frame_generation": {"kind": "exact", "value": True},
        "recovery_retained_bytes_stable": {"kind": "exact", "value": True},
        "same_actual_backend": {"kind": "exact", "value": True},
        "activation_latency_report_only": {"kind": "exact", "value": True},
    }
    if not isinstance(result_claims, dict) or result_claims.get("font-catalog-fingerprint") != expected_font_claims:
        violations.append("font functional specimen claims differ from the frozen proof contract")
    if not isinstance(epoch_requirements, dict) or set(epoch_requirements) != set(STATES):
        violations.append("epoch_requirements_by_state must cover every state")
    if not isinstance(new_by_state, dict) or set(new_by_state) != set(STATES):
        violations.append("new_artifacts_by_state must cover every state")
        new_by_state = {}
    if not isinstance(required_by_state, dict) or set(required_by_state) != set(STATES):
        violations.append("required_artifacts_by_state must cover every state")
        required_by_state = {}
    cumulative: list[str] = []
    for state in STATES:
        new = new_by_state.get(state, [])
        required = required_by_state.get(state, [])
        if not isinstance(new, list) or len(new) != len(set(new)):
            violations.append(f"{state}: new artifact set must be a unique list")
            new = []
        if any(item not in artifact_types for item in new):
            violations.append(f"{state}: new artifact set contains an unknown type")
        cumulative.extend(new)
        if required != cumulative:
            violations.append(f"{state}: required artifacts are not the exact cumulative state set")
    if cumulative != artifact_types:
        violations.append("state artifact deltas do not exactly form artifact_types")

    schema_path = contract_path.with_name("stage7-evidence-manifest.schema.json")
    schema = read_json(schema_path, "evidence manifest schema", violations)
    expected_schema_digest = contract.get("evidence_manifest_schema_sha256")
    if (
        not is_sha256(expected_schema_digest)
        or not schema_path.is_file()
        or file_sha256(schema_path) != expected_schema_digest
    ):
        violations.append("evidence manifest schema SHA-256 differs from the frozen contract")
    try:
        schema_types = schema["$defs"]["entry"]["properties"]["artifact_type"]["enum"] if schema else []
        schema_const = schema["properties"]["schema"]["const"] if schema else None
    except (KeyError, TypeError):
        schema_types = []
        schema_const = None
    if schema_const != MANIFEST_SCHEMA:
        violations.append("evidence manifest schema does not freeze the v1 schema identifier")
    if schema_types != artifact_types:
        violations.append("evidence manifest schema artifact enum differs from the contract")
    return contract, violations


def verify_hash(path: Path, expected: Any, label: str, violations: list[str]) -> bool:
    if not is_sha256(expected):
        violations.append(f"{label}: SHA-256 must be 64 lowercase hexadecimal characters")
        return False
    observed = file_sha256(path)
    if observed != expected:
        violations.append(f"{label}: SHA-256 mismatch: expected {expected}, observed {observed}")
        return False
    return True


def validate_binary_hashes(value: Any, label: str, violations: list[str]) -> bool:
    if not isinstance(value, dict) or not value:
        violations.append(f"{label}: binary identity is required")
        return False
    valid = True
    for name, digest in value.items():
        if (
            not isinstance(name, str)
            or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name) is None
            or not is_sha256(digest)
        ):
            violations.append(f"{label}: binary hashes must be named SHA-256 values")
            valid = False
    return valid


def identity_from_entry(entry: dict[str, Any]) -> dict[str, Any]:
    identity = {
        "source_sha": entry.get("source_sha"),
        "platform": entry.get("platform"),
        "run_id": entry.get("run_id"),
    }
    if "binary_hashes" in entry:
        identity["binary_hashes"] = entry["binary_hashes"]
    if "runner_fingerprint_sha256" in entry:
        identity["runner_fingerprint_sha256"] = entry["runner_fingerprint_sha256"]
    return identity


def cohort_identity(identity: dict[str, Any]) -> dict[str, Any]:
    return {
        field: identity.get(field)
        for field in (
            "source_sha",
            "binary_hashes",
            "runner_fingerprint_sha256",
            "platform",
        )
        if field in identity
    }


def recomputed_statistics(representatives: list[int | float], raw: list[int | float]) -> dict[str, int | float]:
    return {
        "p50": nearest_rank(representatives, 0.50),
        "p95": nearest_rank(representatives, 0.95),
        "max": max(raw),
    }


def numeric(value: Any) -> bool:
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
        and value >= 0
    )


def reject_unknown_fields(
    value: Any,
    allowed: set[str],
    label: str,
    violations: list[str],
) -> None:
    if not isinstance(value, dict):
        return
    unknown = set(value) - allowed
    if unknown:
        violations.append(f"{label}: unknown fields are forbidden: {sorted(unknown)}")


def decode_git_object_record(
    record: Any, label: str, violations: list[str]
) -> tuple[str | None, str | None, bytes | None]:
    fields = {"oid", "object_type", "body_base64"}
    if not isinstance(record, dict) or set(record) != fields:
        violations.append(f"{label}: bounded Git object fields are not closed")
        return None, None, None
    oid = record.get("oid")
    object_type = record.get("object_type")
    if not is_full_sha(oid):
        violations.append(f"{label}: Git object OID must be a full SHA")
    if object_type not in {"commit", "tree", "blob"}:
        violations.append(f"{label}: Git object type must be commit, tree, or blob")
    if not is_full_sha(oid) or object_type not in {"commit", "tree", "blob"}:
        return oid if is_full_sha(oid) else None, object_type, None
    encoded = record.get("body_base64")
    if not isinstance(encoded, str):
        violations.append(f"{label}: bounded raw Git object is missing")
        return oid if is_full_sha(oid) else None, object_type, None
    raw = decode_canonical_base64(
        encoded,
        MAX_GIT_OBJECT_BYTES,
        MAX_GIT_OBJECT_BASE64_CHARS,
        f"{label}: bounded raw Git object",
        violations,
    )
    if raw is None:
        return oid if is_full_sha(oid) else None, object_type, None
    if object_type in {"commit", "tree", "blob"}:
        observed = hashlib.sha1(
            f"{object_type} {len(raw)}\0".encode("ascii") + raw,
            usedforsecurity=False,
        ).hexdigest()
        if observed != oid:
            violations.append(f"{label}: recomputed Git object SHA does not match its OID")
    return oid if is_full_sha(oid) else None, object_type, raw


def decode_canonical_base64(
    encoded: Any,
    max_decoded_bytes: int,
    max_encoded_characters: int,
    label: str,
    violations: list[str],
) -> bytes | None:
    if not isinstance(encoded, str):
        violations.append(f"{label} is missing")
        return None
    if len(encoded) > max_encoded_characters:
        violations.append(f"{label} exceeds its encoded size limit")
        return None
    try:
        decoded = base64.b64decode(encoded, validate=True)
    except (ValueError, binascii.Error):
        violations.append(f"{label} is not canonical base64")
        return None
    if (
        len(decoded) > max_decoded_bytes
        or base64.b64encode(decoded).decode("ascii") != encoded
    ):
        violations.append(f"{label} is not canonical base64 or exceeds its decoded size limit")
        return None
    return decoded


def declared_canonical_base64_length(encoded: Any) -> int | None:
    """Return the decoded length shape without allocating decoded bytes."""
    if not isinstance(encoded, str) or len(encoded) % 4 != 0:
        return None
    first_padding = encoded.find("=")
    if first_padding < 0:
        padding = 0
    else:
        padding = len(encoded) - first_padding
        if padding > 2 or encoded[first_padding:] != "=" * padding:
            return None
    return (len(encoded) // 4) * 3 - padding


def bounded_zlib_decompress(
    compressed: bytes,
    label: str,
    violations: list[str],
) -> bytes | None:
    try:
        decompressor = zlib.decompressobj()
        output = decompressor.decompress(compressed, MAX_LOOSE_OBJECT_BYTES + 1)
        if len(output) > MAX_LOOSE_OBJECT_BYTES or decompressor.unconsumed_tail:
            violations.append(f"{label}: decompressed size limit exceeded")
            return None
    except zlib.error:
        violations.append(f"{label}: data is not valid zlib")
        return None
    if len(output) > MAX_LOOSE_OBJECT_BYTES:
        violations.append(f"{label}: decompressed size limit exceeded")
        return None
    if not decompressor.eof or decompressor.unused_data or decompressor.unconsumed_tail:
        violations.append(f"{label}: zlib data is trailing or truncated")
        return None
    return output


def parse_git_commit_body(
    raw: bytes, label: str, violations: list[str]
) -> tuple[list[str], str | None]:
    raw_header, separator, _message = raw.partition(b"\n\n")
    try:
        header = raw_header.decode("ascii", errors="strict").splitlines()
    except UnicodeDecodeError:
        header = []
    if (
        not separator
        or not header
        or not re.fullmatch(r"tree [0-9a-f]{40}", header[0])
        or not any(line.startswith("author ") for line in header)
        or not any(line.startswith("committer ") for line in header)
    ):
        violations.append(f"{label}: bounded Git commit object has invalid commit headers")
    parents = [line.removeprefix("parent ") for line in header if line.startswith("parent ")]
    if not all(is_full_sha(parent) for parent in parents):
        violations.append(f"{label}: commit parent header is not a full SHA")
    tree_oid = header[0].removeprefix("tree ") if header else None
    return parents, tree_oid


def validate_git_commit_object(
    record: Any, label: str, violations: list[str]
) -> tuple[str | None, list[str], str | None]:
    oid, object_type, raw = decode_git_object_record(record, label, violations)
    if object_type != "commit" or raw is None:
        violations.append(f"{label}: bounded Git object must be a readable commit")
        return oid, [], None
    parents, tree_oid = parse_git_commit_body(raw, label, violations)
    return oid, parents, tree_oid


def validate_materialized_git_replay(
    materialized: dict[str, bytes],
    refs: dict[str, str],
    snapshot_digest: str,
    label: str,
    violations: list[str],
) -> None:
    if snapshot_digest in _STRICT_GIT_REPLAY_CACHE:
        return
    environment = clean_git_environment()
    try:
        with tempfile.TemporaryDirectory(prefix="rssh-stage7-git-replay-") as temporary:
            bare = Path(temporary) / "proof.git"
            for relative_path, body in materialized.items():
                target = bare.joinpath(*PurePosixPath(relative_path).parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(body)
            fsck = subprocess.run(
                [
                    "git",
                    f"--git-dir={bare}",
                    "fsck",
                    "--strict",
                    "--no-reflogs",
                ],
                cwd=temporary,
                env=environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
                timeout=120,
            )
            if fsck.returncode != 0:
                violations.append(
                    f"{label}: materialized replay does not pass git fsck --strict"
                )
                return
            for logical_name in sorted(refs):
                archive = subprocess.run(
                    [
                        "git",
                        f"--git-dir={bare}",
                        "archive",
                        "--format=tar",
                        f"refs/heads/stage7-proof/{logical_name}",
                    ],
                    cwd=temporary,
                    env=environment,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                    check=False,
                    timeout=120,
                )
                if archive.returncode != 0:
                    violations.append(
                        f"{label}: materialized replay cannot archive certified ref {logical_name}"
                    )
                    return
    except (OSError, subprocess.TimeoutExpired) as error:
        violations.append(
            f"{label}: strict Git replay could not complete: {type(error).__name__}"
        )
        return
    _STRICT_GIT_REPLAY_CACHE.add(snapshot_digest)


def validate_replayable_bare_repository(
    snapshot: Any,
    refs: dict[str, str],
    history_boundaries: list[str],
    objects_by_oid: dict[str, dict[str, Any]],
    label: str,
    violations: list[str],
) -> str | None:
    violation_count = len(violations)
    fields = {"schema", "files", "snapshot_sha256"}
    if not isinstance(snapshot, dict) or set(snapshot) != fields:
        violations.append(f"{label}: replayable bare repository fields are not closed")
        return None
    if snapshot.get("schema") != REPLAYABLE_BARE_SCHEMA:
        violations.append(f"{label}: replayable bare repository schema mismatch")
    files = snapshot.get("files")
    if not isinstance(files, list) or not 4 <= len(files) <= 65_536:
        violations.append(f"{label}: bounded replayable bare repository file inventory is required")
        files = []
    elif files != sorted(files, key=lambda item: item.get("path", "") if isinstance(item, dict) else ""):
        violations.append(f"{label}: replayable bare repository files must be deterministically sorted")
    materialized: dict[str, bytes] = {}
    total_bytes = 0
    for index, record in enumerate(files):
        record_label = f"{label} replay file {index}"
        if not isinstance(record, dict) or set(record) != {"path", "body_base64"}:
            violations.append(f"{record_label}: replay file fields are not closed")
            continue
        path = record.get("path")
        if not safe_git_path(path) or path in materialized:
            violations.append(f"{record_label}: replay file path is unsafe or duplicate")
            continue
        encoded = record.get("body_base64")
        declared_bytes = declared_canonical_base64_length(encoded)
        if (
            declared_bytes is not None
            and total_bytes > MAX_REPLAY_TOTAL_BYTES - declared_bytes
        ):
            violations.append(f"{label}: replayable bare repository exceeds 192 MiB")
            return None
        body = decode_canonical_base64(
            encoded,
            MAX_REPLAY_FILE_BYTES,
            MAX_REPLAY_FILE_BASE64_CHARS,
            f"{record_label}: replay file body",
            violations,
        )
        if body is None:
            continue
        if total_bytes > MAX_REPLAY_TOTAL_BYTES - len(body):
            violations.append(f"{label}: replayable bare repository exceeds 192 MiB")
            return None
        total_bytes += len(body)
        materialized[path] = body

    canonical_config = (
        b"[core]\n"
        b"\trepositoryformatversion = 0\n"
        b"\tfilemode = false\n"
        b"\tbare = true\n"
    )
    expected_files = {"HEAD", "config"}
    expected_files.update(f"refs/heads/stage7-proof/{logical}" for logical in refs)
    expected_files.update(f"objects/{oid[:2]}/{oid[2:]}" for oid in objects_by_oid)
    if history_boundaries:
        expected_files.add("shallow")
    if set(materialized) != expected_files:
        violations.append(
            f"{label}: replayable bare repository must contain the exact closed reachable object store"
        )
    if materialized.get("config") != canonical_config:
        violations.append(f"{label}: replayed bare config is not the closed safe config")
    first_ref = f"refs/heads/stage7-proof/{sorted(refs)[0]}" if refs else ""
    if materialized.get("HEAD") != f"ref: {first_ref}\n".encode("ascii"):
        violations.append(f"{label}: replayed HEAD is not a bounded symbolic ref")
    for logical_name, oid in refs.items():
        if materialized.get(f"refs/heads/stage7-proof/{logical_name}") != f"{oid}\n".encode(
            "ascii"
        ):
            violations.append(f"{label}: replayed ref {logical_name} does not bind its commit")
    expected_shallow = "".join(f"{oid}\n" for oid in history_boundaries).encode("ascii")
    if history_boundaries and materialized.get("shallow") != expected_shallow:
        violations.append(f"{label}: replayed shallow boundary does not bind the frozen history boundary")

    for oid, record in objects_by_oid.items():
        loose = materialized.get(f"objects/{oid[:2]}/{oid[2:]}")
        if loose is None:
            continue
        object_bytes = bounded_zlib_decompress(
            loose,
            f"{label}: replayed loose object {oid}",
            violations,
        )
        if object_bytes is None:
            continue
        object_type = record["object_type"]
        decoded_expected = record["raw"]
        expected_object = (
            f"{object_type} {len(decoded_expected)}\0".encode("ascii") + decoded_expected
        )
        if (
            object_bytes != expected_object
            or hashlib.sha1(object_bytes, usedforsecurity=False).hexdigest() != oid
        ):
            violations.append(f"{label}: replayed loose Git object {oid} does not recompute")

    digest = snapshot.get("snapshot_sha256")
    digest_material = {key: snapshot[key] for key in fields - {"snapshot_sha256"}}
    if not is_sha256(digest) or digest != canonical_sha256(digest_material):
        violations.append(f"{label}: replayable bare repository digest does not recompute")
        return None
    if len(violations) == violation_count:
        validate_materialized_git_replay(
            materialized,
            refs,
            digest,
            label,
            violations,
        )
    return digest


def validate_git_object_store_proof(
    proof: Any,
    expected_refs: dict[str, dict[str, Any]],
    label: str,
    violations: list[str],
) -> dict[str, dict[str, Any]]:
    if not isinstance(proof, dict) or set(proof) != {
        "schema",
        "object_format",
        "repositories",
    }:
        violations.append(f"{label}: bounded Git object-store proof fields are not closed")
        return {}
    if proof.get("schema") != GIT_STORE_SCHEMA or proof.get("object_format") != "sha1":
        violations.append(f"{label}: Git object-store proof schema/object format mismatch")
    repositories = proof.get("repositories")
    if not isinstance(repositories, list) or len(repositories) != len(expected_refs):
        violations.append(f"{label}: exact bounded repository inventory is required")
        return {}
    repositories_by_role: dict[str, dict[str, Any]] = {}
    fingerprints: set[str] = set()
    replayable_stores: set[str] = set()
    for index, repository in enumerate(repositories):
        repository_label = f"{label} repository {index}"
        fields = {
            "role",
            "bare",
            "alternates",
            "refs",
            "history_boundaries",
            "git_objects",
            "bare_repository_snapshot",
            "snapshot_sha256",
        }
        if not isinstance(repository, dict) or set(repository) != fields:
            violations.append(
                f"{repository_label}: repository reachable object closure fields are not closed"
            )
            continue
        role = repository.get("role")
        if role not in expected_refs or role in repositories_by_role:
            violations.append(f"{repository_label}: repository role is missing, duplicate, or unexpected")
            continue
        if repository.get("bare") is not True or repository.get("alternates") != []:
            violations.append(f"{repository_label}: bare repository without object alternates is required")
        refs = repository.get("refs")
        if (
            not isinstance(refs, dict)
            or refs != expected_refs[role]
            or not all(is_full_sha(item) for item in refs.values())
        ):
            violations.append(f"{repository_label}: repository refs do not exactly bind certified subjects")
            refs = expected_refs[role]
        history_boundaries = repository.get("history_boundaries")
        if (
            not isinstance(history_boundaries, list)
            or history_boundaries != sorted(set(history_boundaries))
            or not all(is_full_sha(item) for item in history_boundaries)
            or not set(history_boundaries).issubset(set(refs.values()))
        ):
            violations.append(
                f"{repository_label}: history boundaries must be sorted unique certified refs"
            )
            history_boundaries = []
        objects = repository.get("git_objects")
        if not isinstance(objects, list) or not 1 <= len(objects) <= 65_536:
            violations.append(f"{repository_label}: bounded reachable Git object inventory is required")
            objects = []
        elif objects != sorted(objects, key=lambda item: item.get("oid", "") if isinstance(item, dict) else ""):
            violations.append(f"{repository_label}: Git object inventory must be deterministically sorted")
        objects_by_oid: dict[str, dict[str, Any]] = {}
        graph: dict[str, dict[str, Any]] = {}
        trees: dict[str, list[dict[str, str]]] = {}
        blobs: set[str] = set()
        total_decoded_bytes = 0
        tree_entry_budget: dict[str, int | bool] = {
            "entries": 0,
            "exceeded": False,
        }
        for object_index, record in enumerate(objects):
            encoded = record.get("body_base64") if isinstance(record, dict) else None
            declared_bytes = declared_canonical_base64_length(encoded)
            if (
                declared_bytes is not None
                and total_decoded_bytes
                > MAX_GIT_CLOSURE_TOTAL_BYTES - declared_bytes
            ):
                violations.append(
                    f"{repository_label}: reachable Git object closure exceeds 192 MiB"
                )
                return {}
            if declared_bytes is not None:
                total_decoded_bytes += declared_bytes
            oid, object_type, raw = decode_git_object_record(
                record,
                f"{repository_label} object {object_index}",
                violations,
            )
            if oid is None or object_type not in {"commit", "tree", "blob"} or raw is None:
                continue
            if (
                declared_bytes is None
                and total_decoded_bytes > MAX_GIT_CLOSURE_TOTAL_BYTES - len(raw)
            ):
                violations.append(
                    f"{repository_label}: reachable Git object closure exceeds 192 MiB"
                )
                return {}
            if declared_bytes is None:
                total_decoded_bytes += len(raw)
            if oid in objects_by_oid:
                violations.append(f"{repository_label}: duplicate Git object OID")
                continue
            objects_by_oid[oid] = {"object_type": object_type, "raw": raw}
            if object_type == "commit":
                parents, tree_oid = parse_git_commit_body(
                    raw, f"{repository_label} commit {oid}", violations
                )
                graph[oid] = {"parents": parents, "tree": tree_oid}
            elif object_type == "tree":
                trees[oid] = parse_raw_git_tree(
                    raw,
                    f"{repository_label} tree {oid}",
                    violations,
                    tree_entry_budget,
                )
                if tree_entry_budget["exceeded"]:
                    return {}
            else:
                blobs.add(oid)

        reachable: set[str] = set()
        pending = [(oid, "commit") for oid in refs.values()]
        while pending:
            oid, expected_type = pending.pop()
            if oid in reachable:
                actual = objects_by_oid.get(oid, {}).get("object_type")
                if actual != expected_type:
                    violations.append(
                        f"{repository_label}: reachable object {oid} type mismatch"
                    )
                continue
            record = objects_by_oid.get(oid)
            if record is None:
                violations.append(
                    f"{repository_label}: reachable object closure is missing {expected_type} {oid}"
                )
                continue
            if record["object_type"] != expected_type:
                violations.append(
                    f"{repository_label}: reachable object {oid} is not {expected_type}"
                )
                continue
            reachable.add(oid)
            if expected_type == "commit":
                commit = graph.get(oid, {})
                tree_oid = commit.get("tree")
                if is_full_sha(tree_oid):
                    pending.append((tree_oid, "tree"))
                else:
                    violations.append(f"{repository_label}: commit {oid} has no readable tree")
                if oid not in history_boundaries:
                    pending.extend((parent, "commit") for parent in commit.get("parents", []))
            elif expected_type == "tree":
                for entry in trees.get(oid, []):
                    if entry["mode"] == "160000":
                        violations.append(
                            f"{repository_label}: gitlink objects are forbidden in a self-contained proof"
                        )
                        continue
                    pending.append((entry["oid"], entry["object_type"]))
        if set(objects_by_oid) != reachable:
            violations.append(
                f"{repository_label}: every and only the complete reachable commit/tree/blob closure is required"
            )
        if not set(history_boundaries).issubset(reachable):
            violations.append(f"{repository_label}: history boundary is not reachable")
        replayable_store = validate_replayable_bare_repository(
            repository.get("bare_repository_snapshot"),
            refs,
            history_boundaries,
            objects_by_oid,
            repository_label,
            violations,
        )
        if replayable_store in replayable_stores:
            violations.append(
                f"{label}: independent replayable bare repository stores must be distinct"
            )
        elif replayable_store is not None:
            replayable_stores.add(replayable_store)
        snapshot = {
            key: repository[key]
            for key in (
                "bare",
                "alternates",
                "history_boundaries",
                "git_objects",
                "bare_repository_snapshot",
            )
        }
        fingerprint = repository.get("snapshot_sha256")
        if not is_sha256(fingerprint) or fingerprint != canonical_sha256(snapshot):
            violations.append(f"{repository_label}: repository snapshot fingerprint does not recompute")
        elif fingerprint in fingerprints:
            violations.append(f"{label}: independent repositories must have distinct snapshots")
        else:
            fingerprints.add(fingerprint)
        repositories_by_role[role] = {
            "refs": refs,
            "history_boundaries": history_boundaries,
            "commits": graph,
            "trees": trees,
            "blobs": blobs,
        }
    if set(repositories_by_role) != set(expected_refs):
        violations.append(f"{label}: repository roles do not match the frozen proof contract")
    return repositories_by_role


def safe_git_path(value: Any) -> bool:
    if not isinstance(value, str) or not value or "\\" in value or "\x00" in value:
        return False
    posix = PurePosixPath(value)
    windows = PureWindowsPath(value)
    return (
        not posix.is_absolute()
        and not windows.is_absolute()
        and not windows.drive
        and all(part not in {"", ".", ".."} for part in posix.parts)
        and windows_path_components_are_safe(posix.parts)
    )


def parse_raw_git_tree(
    raw: bytes,
    label: str,
    violations: list[str],
    entry_budget: dict[str, int | bool] | None = None,
) -> list[dict[str, str]]:
    entries: list[dict[str, str]] = []
    names: set[str] = set()
    sort_keys: list[bytes] = []
    cursor = 0
    while cursor < len(raw):
        space = raw.find(b" ", cursor)
        nul = raw.find(b"\0", space + 1) if space >= 0 else -1
        if space <= cursor or nul <= space + 1 or nul + 21 > len(raw):
            violations.append(f"{label}: malformed bounded raw Git tree entry")
            return []
        if entry_budget is not None:
            entries_used = int(entry_budget.get("entries", 0))
            if entries_used >= MAX_PARSED_TREE_ENTRIES:
                entry_budget["exceeded"] = True
                violations.append(f"{label}: global raw Git tree entry budget exceeded")
                return []
            entry_budget["entries"] = entries_used + 1
        try:
            mode = raw[cursor:space].decode("ascii", errors="strict")
            name = raw[space + 1 : nul].decode("utf-8", errors="strict")
        except UnicodeDecodeError:
            violations.append(f"{label}: Git tree mode/name must be canonical ASCII/UTF-8")
            return []
        oid = raw[nul + 1 : nul + 21].hex()
        object_type = {
            "40000": "tree",
            "100644": "blob",
            "100755": "blob",
            "120000": "blob",
            "160000": "commit",
        }.get(mode)
        if object_type is None:
            violations.append(f"{label}: unsupported Git tree mode {mode!r}")
        if not safe_git_path(name) or "/" in name:
            violations.append(f"{label}: unsafe Git tree entry name")
        if name in names:
            violations.append(f"{label}: duplicate Git tree entry name {name!r}")
        names.add(name)
        sort_keys.append(name.encode("utf-8") + (b"/" if mode == "40000" else b""))
        entries.append(
            {
                "mode": mode,
                "name": name,
                "object_type": object_type or "invalid",
                "oid": oid,
            }
        )
        cursor = nul + 21
    if not entries:
        violations.append(f"{label}: bounded raw Git tree cannot be empty")
    if sort_keys != sorted(sort_keys):
        violations.append(f"{label}: Git tree entries are not in canonical byte order")
    return entries


def validate_filtered_tree_snapshot(
    snapshot: Any,
    expected_root_tree_oid: Any,
    label: str,
    violations: list[str],
) -> dict[str, dict[str, str]]:
    fields = {"schema", "root_tree_oid", "tree_objects", "snapshot_sha256"}
    if not isinstance(snapshot, dict) or set(snapshot) != fields:
        violations.append(f"{label}: filtered tree snapshot fields are not closed")
        return {}
    if snapshot.get("schema") != TREE_SNAPSHOT_SCHEMA:
        violations.append(f"{label}: filtered tree snapshot schema mismatch")
    root_tree_oid = snapshot.get("root_tree_oid")
    if not is_full_sha(root_tree_oid) or root_tree_oid != expected_root_tree_oid:
        violations.append(f"{label}: filtered root tree does not bind the raw filtered commit")
    records = snapshot.get("tree_objects")
    if not isinstance(records, list) or not 1 <= len(records) <= 4096:
        violations.append(f"{label}: bounded filtered tree object inventory is required")
        records = []
    tree_graph: dict[str, list[dict[str, str]]] = {}
    total_bytes = 0
    tree_entry_budget: dict[str, int | bool] = {
        "entries": 0,
        "exceeded": False,
    }
    for index, record in enumerate(records):
        record_label = f"{label} tree object {index}"
        if not isinstance(record, dict) or set(record) != {
            "oid",
            "object_type",
            "body_base64",
        }:
            violations.append(f"{record_label}: raw tree object fields are not closed")
            continue
        oid = record.get("oid")
        if not is_full_sha(oid) or record.get("object_type") != "tree":
            violations.append(f"{record_label}: full tree OID and tree type are required")
        encoded = record.get("body_base64")
        declared_bytes = declared_canonical_base64_length(encoded)
        if (
            declared_bytes is not None
            and total_bytes > MAX_FILTERED_TREE_TOTAL_BYTES - declared_bytes
        ):
            violations.append(f"{label}: bounded tree inventory exceeds 16 MiB")
            return {}
        raw = decode_canonical_base64(
            encoded,
            MAX_GIT_OBJECT_BYTES,
            MAX_GIT_OBJECT_BASE64_CHARS,
            f"{record_label}: raw tree bytes",
            violations,
        )
        if not raw:
            if raw == b"":
                violations.append(f"{record_label}: raw tree bytes cannot be empty")
            continue
        if total_bytes > MAX_FILTERED_TREE_TOTAL_BYTES - len(raw):
            violations.append(f"{label}: bounded tree inventory exceeds 16 MiB")
            return {}
        total_bytes += len(raw)
        observed = hashlib.sha1(
            f"tree {len(raw)}\0".encode("ascii") + raw,
            usedforsecurity=False,
        ).hexdigest()
        if observed != oid:
            violations.append(f"{record_label}: recomputed Git tree SHA does not match")
        if oid in tree_graph:
            violations.append(f"{label}: duplicate raw tree object OID")
        tree_graph[oid] = parse_raw_git_tree(
            raw,
            record_label,
            violations,
            tree_entry_budget,
        )
        if tree_entry_budget["exceeded"]:
            return {}

    leaves: dict[str, dict[str, str]] = {}
    reachable: set[str] = set()

    if is_full_sha(root_tree_oid):
        pending: list[tuple[str, str, frozenset[str], int]] = [
            (root_tree_oid, "", frozenset(), 0)
        ]
        expanded_nodes = 0
        expanded_path_bytes = 0
        while pending:
            tree_oid, prefix, ancestors, depth = pending.pop()
            expanded_nodes += 1
            if expanded_nodes > MAX_FLATTENED_TREE_NODES:
                violations.append(f"{label}: filtered tree expansion budget exceeded")
                return {}
            if depth > 256:
                violations.append(f"{label}: filtered tree depth exceeds 256")
                continue
            if tree_oid in ancestors:
                violations.append(f"{label}: filtered tree graph contains a cycle")
                continue
            entries = tree_graph.get(tree_oid)
            if entries is None:
                violations.append(
                    f"{label}: filtered tree references a missing raw subtree {tree_oid}"
                )
                continue
            reachable.add(tree_oid)
            next_ancestors = ancestors | {tree_oid}
            for entry in reversed(entries):
                next_path_bytes = (
                    len(prefix.encode("utf-8"))
                    + (1 if prefix else 0)
                    + len(entry["name"].encode("utf-8"))
                )
                if (
                    expanded_path_bytes
                    > MAX_EXPANDED_TREE_PATH_BYTES - next_path_bytes
                ):
                    violations.append(f"{label}: filtered tree path byte budget exceeded")
                    return {}
                expanded_path_bytes += next_path_bytes
                path = f"{prefix}/{entry['name']}" if prefix else entry["name"]
                if entry["object_type"] == "tree":
                    if len(pending) >= MAX_FLATTENED_TREE_NODES:
                        violations.append(f"{label}: filtered tree expansion budget exceeded")
                        return {}
                    pending.append((entry["oid"], path, next_ancestors, depth + 1))
                elif entry["object_type"] in {"blob", "commit"}:
                    if len(leaves) >= MAX_FLATTENED_TREE_LEAVES and path not in leaves:
                        violations.append(f"{label}: filtered tree leaf budget exceeded")
                        return {}
                    if path in leaves:
                        violations.append(f"{label}: duplicate filtered leaf path {path}")
                    leaves[path] = {
                        "mode": entry["mode"],
                        "object_type": entry["object_type"],
                        "object_oid": entry["oid"],
                    }
    if set(tree_graph) != reachable:
        violations.append(f"{label}: tree snapshot contains unreachable or missing tree objects")
    digest = snapshot.get("snapshot_sha256")
    digest_material = {
        key: snapshot[key]
        for key in fields - {"snapshot_sha256"}
    }
    if not is_sha256(digest) or digest != canonical_sha256(digest_material):
        violations.append(f"{label}: filtered tree snapshot hash does not recompute")
    return leaves


def git_tree_identity_and_leaves(
    root: Path,
    commit: str,
    label: str,
    violations: list[str],
) -> tuple[str | None, dict[str, dict[str, str]]]:
    if git_repository_has_history_overrides(root):
        violations.append(f"{label}: R0 tree cannot be read from an overridden Git history")
        return None, {}
    environment = clean_git_environment()
    tree = subprocess.run(
        ["git", "rev-parse", f"{commit}^{{tree}}"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    listing = subprocess.run(
        ["git", "ls-tree", "-r", "--full-tree", "-z", commit],
        cwd=root,
        check=False,
        capture_output=True,
        env=environment,
    )
    tree_oid = tree.stdout.strip()
    if tree.returncode != 0 or not is_full_sha(tree_oid) or listing.returncode != 0:
        violations.append(f"{label}: immutable R0 ls-tree evidence cannot be recomputed")
        return None, {}
    leaves: dict[str, dict[str, str]] = {}
    try:
        encoded_records = listing.stdout.split(b"\0")
        for encoded in encoded_records:
            if not encoded:
                continue
            metadata, encoded_path = encoded.split(b"\t", 1)
            mode, object_type, oid = metadata.decode("ascii", errors="strict").split(" ")
            path = encoded_path.decode("utf-8", errors="strict")
            if (
                mode not in {"100644", "100755", "120000", "160000"}
                or object_type not in {"blob", "commit"}
                or not is_full_sha(oid)
                or not safe_git_path(path)
                or path in leaves
            ):
                raise ValueError
            leaves[path] = {
                "mode": mode,
                "object_type": object_type,
                "object_oid": oid,
            }
    except (UnicodeDecodeError, ValueError):
        violations.append(f"{label}: immutable R0 ls-tree output is malformed or ambiguous")
        return tree_oid, {}
    if not leaves:
        violations.append(f"{label}: immutable R0 ls-tree output is empty")
    return tree_oid, leaves


def derive_owned_projection_mappings(
    source_leaves: dict[str, dict[str, str]],
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> list[dict[str, str]]:
    inventory = contract.get("owned_projection_inventory")
    if not isinstance(inventory, dict) or set(inventory) != {
        "required",
        "future_required",
    }:
        violations.append(f"{label}: frozen owned projection root inventory is unavailable")
        return []
    mappings: list[dict[str, str]] = []
    seen_source: set[str] = set()
    seen_filtered: set[str] = set()
    bootstrap_templates = contract.get("bootstrap_template_mappings")
    expected_bootstrap = {
        item.get("source_path"): item.get("filtered_path")
        for item in bootstrap_templates
        if isinstance(item, dict)
    } if isinstance(bootstrap_templates, list) else {}
    bootstrap_prefix = FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED[0][0]
    for category in ("required", "future_required"):
        rules = inventory.get(category)
        if not isinstance(rules, list):
            violations.append(f"{label}: {category} owned root rules must be a list")
            continue
        for index, rule in enumerate(rules):
            rule_label = f"{label} {category} root {index}"
            if not isinstance(rule, dict) or set(rule) != {
                "source_prefix",
                "filtered_prefix",
            }:
                violations.append(f"{rule_label}: owned root rule fields are not closed")
                continue
            source_prefix = rule.get("source_prefix")
            filtered_prefix = rule.get("filtered_prefix")
            if not safe_git_path(source_prefix) or not safe_git_path(filtered_prefix):
                violations.append(f"{rule_label}: owned root source/filtered prefix is unsafe")
                continue
            matches = sorted(
                path
                for path in source_leaves
                if path == source_prefix or path.startswith(source_prefix + "/")
            )
            if not matches:
                violations.append(
                    f"{rule_label}: immutable R0 is missing required owned root {source_prefix}"
                )
                continue
            if source_prefix == bootstrap_prefix:
                observed = set(matches)
                expected = set(expected_bootstrap)
                for missing in sorted(expected - observed):
                    violations.append(
                        f"{rule_label}: immutable R0 is missing bootstrap template {missing}"
                    )
                for unexpected in sorted(observed - expected):
                    violations.append(
                        f"{rule_label}: immutable R0 has undeclared bootstrap template {unexpected}"
                    )
                matches = sorted(expected & observed)
            for source_path in matches:
                filtered_path = (
                    expected_bootstrap[source_path]
                    if source_prefix == bootstrap_prefix
                    else filtered_prefix + source_path[len(source_prefix) :]
                )
                filtered_overlap = (
                    filtered_path in seen_filtered and source_prefix != bootstrap_prefix
                )
                if source_path in seen_source or filtered_overlap:
                    violations.append(
                        f"{label}: frozen owned roots overlap or rename to the same path"
                    )
                    continue
                seen_source.add(source_path)
                seen_filtered.add(filtered_path)
                mappings.append(
                    {
                        "source_path": source_path,
                        "filtered_path": filtered_path,
                        **source_leaves[source_path],
                    }
                )
    return sorted(
        mappings, key=lambda record: (record["filtered_path"], record["source_path"])
    )


def validate_owned_projection_inventory(
    inventory: Any,
    root: Path,
    r0_ref: Any,
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> tuple[list[dict[str, str]], str | None]:
    fields = {
        "schema",
        "r0_ref",
        "root_rules",
        "bootstrap_template_mappings",
        "path_mappings",
        "bootstrap_inventory_complete",
        "inventory_sha256",
    }
    if not isinstance(inventory, dict) or set(inventory) != fields:
        violations.append(f"{label}: owned projection inventory fields are not closed")
        return [], None
    if inventory.get("schema") != "rssh.stage7.owned-projection-inventory/v1":
        violations.append(f"{label}: owned projection inventory schema mismatch")
    if inventory.get("r0_ref") != r0_ref:
        violations.append(f"{label}: owned projection inventory does not bind certified R0")
    if inventory.get("root_rules") != contract.get("owned_projection_inventory"):
        violations.append(f"{label}: owned projection root rules drift from the frozen contract")
    if inventory.get("bootstrap_template_mappings") != contract.get(
        "bootstrap_template_mappings"
    ):
        violations.append(f"{label}: bootstrap template mappings drift from the frozen contract")
    _source_tree, source_leaves = git_tree_identity_and_leaves(
        root, r0_ref, label, violations
    )
    expected_mappings = derive_owned_projection_mappings(
        source_leaves, contract, label, violations
    )
    if inventory.get("path_mappings") != expected_mappings:
        violations.append(
            f"{label}: owned path/rename inventory must enumerate every and only frozen R0 leaf"
        )
    expected_bootstrap_pairs = set(FROZEN_BOOTSTRAP_TEMPLATE_MAPPINGS)
    declared_mappings = inventory.get("path_mappings")
    if not isinstance(declared_mappings, list):
        declared_mappings = []
    observed_bootstrap_pairs = {
        (mapping["source_path"], mapping["filtered_path"])
        for mapping in declared_mappings
        if isinstance(mapping, dict)
        and isinstance(mapping.get("source_path"), str)
        and isinstance(mapping.get("filtered_path"), str)
        and mapping["source_path"].startswith(
            FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED[0][0] + "/"
        )
    }
    if (
        inventory.get("bootstrap_inventory_complete") is not True
        or observed_bootstrap_pairs != expected_bootstrap_pairs
    ):
        violations.append(f"{label}: bootstrap/template inventory is incomplete at immutable R0")
    digest = inventory.get("inventory_sha256")
    digest_material = {
        key: inventory[key]
        for key in fields - {"inventory_sha256"}
    }
    if not is_sha256(digest) or digest != canonical_sha256(digest_material):
        violations.append(f"{label}: owned projection inventory digest does not recompute")
        digest = None
    return expected_mappings, digest


def validate_tree_projection_proof(
    proof: Any,
    root: Path,
    r0_ref: Any,
    filtered_boundary_ref: Any,
    filtered_tree_oid: Any,
    label: str,
    violations: list[str],
    expected_owned_mappings: list[dict[str, str]] | None = None,
) -> tuple[str | None, str | None]:
    fields = {
        "schema",
        "r0_ref",
        "filtered_boundary_ref",
        "source_root_tree_oid",
        "extraction_manifest_sha256",
        "filtered_tree_snapshot",
        "path_mappings",
        "projection_sha256",
    }
    if not isinstance(proof, dict) or set(proof) != fields:
        violations.append(f"{label}: R0-to-filtered tree projection fields are not closed")
        return None, None
    if proof.get("schema") != TREE_PROJECTION_SCHEMA:
        violations.append(f"{label}: tree projection schema mismatch")
    if proof.get("r0_ref") != r0_ref or proof.get("filtered_boundary_ref") != filtered_boundary_ref:
        violations.append(f"{label}: tree projection does not bind the certified R0/filtered refs")
    source_tree_oid, source_leaves = git_tree_identity_and_leaves(
        root, r0_ref, label, violations
    )
    if proof.get("source_root_tree_oid") != source_tree_oid:
        violations.append(f"{label}: source root tree does not match immutable R0")
    filtered_leaves = validate_filtered_tree_snapshot(
        proof.get("filtered_tree_snapshot"),
        filtered_tree_oid,
        label,
        violations,
    )
    mappings = proof.get("path_mappings")
    if not isinstance(mappings, list) or not 1 <= len(mappings) <= 100_000:
        violations.append(f"{label}: bounded declared source-path/rename map is required")
        mappings = []
    mapped_source: set[str] = set()
    mapped_filtered: set[str] = set()
    mapping_fields = {
        "source_path",
        "filtered_path",
        "mode",
        "object_type",
        "object_oid",
    }
    for index, mapping in enumerate(mappings):
        mapping_label = f"{label} path mapping {index}"
        if not isinstance(mapping, dict) or set(mapping) != mapping_fields:
            violations.append(f"{mapping_label}: path mapping fields are not closed")
            continue
        source_path = mapping.get("source_path")
        filtered_path = mapping.get("filtered_path")
        if not safe_git_path(source_path) or not safe_git_path(filtered_path):
            violations.append(f"{mapping_label}: source/filtered path is unsafe")
            continue
        if source_path in mapped_source or filtered_path in mapped_filtered:
            violations.append(f"{label}: projection paths must be one-to-one and unique")
        mapped_source.add(source_path)
        mapped_filtered.add(filtered_path)
        identity = {
            "mode": mapping.get("mode"),
            "object_type": mapping.get("object_type"),
            "object_oid": mapping.get("object_oid"),
        }
        if source_leaves.get(source_path) != identity:
            violations.append(f"{mapping_label}: source path/tree/blob identity is not in immutable R0")
        if filtered_leaves.get(filtered_path) != identity:
            violations.append(f"{mapping_label}: filtered path/tree/blob identity was rewritten")
    if mapped_filtered != set(filtered_leaves):
        violations.append(f"{label}: projection map must cover every and only filtered leaf")
    if expected_owned_mappings is not None and mappings != expected_owned_mappings:
        violations.append(
            f"{label}: projection path mappings do not equal the closed owned inventory"
        )
    extraction_manifest_sha256 = proof.get("extraction_manifest_sha256")
    if not is_sha256(extraction_manifest_sha256):
        violations.append(f"{label}: extraction manifest identity must be a SHA-256")
        extraction_manifest_sha256 = None
    projection_sha256 = proof.get("projection_sha256")
    projection_material = {
        key: proof[key]
        for key in fields - {"projection_sha256"}
    }
    if not is_sha256(projection_sha256) or projection_sha256 != canonical_sha256(
        projection_material
    ):
        violations.append(f"{label}: deterministic tree projection digest does not recompute")
        projection_sha256 = None
    return projection_sha256, extraction_manifest_sha256


def validate_commit_map_proof(
    proof: Any,
    root: Path,
    r0_ref: Any,
    filtered_boundary_ref: Any,
    filtered_tree_oid: Any,
    label: str,
    violations: list[str],
) -> tuple[str | None, int, str | None, str | None]:
    fields = {
        "schema",
        "records",
        "map_sha256",
        "source_refs_before",
        "source_refs_after",
        "tree_projection_proof",
    }
    if not isinstance(proof, dict) or set(proof) != fields:
        violations.append(f"{label}: source-to-filtered commit map fields are not closed")
        return None, 0, None, None
    if proof.get("schema") != GIT_MAP_SCHEMA:
        violations.append(f"{label}: source-to-filtered commit map schema mismatch")
    records = proof.get("records")
    if not isinstance(records, list) or not 1 <= len(records) <= 4096:
        violations.append(f"{label}: bounded source-to-filtered commit records are required")
        records = []
    seen: set[tuple[str, str]] = set()
    for index, record in enumerate(records):
        if not isinstance(record, dict) or set(record) != {"source_oid", "filtered_oid"}:
            violations.append(f"{label}: commit map record {index} fields are not closed")
            continue
        pair = (record.get("source_oid"), record.get("filtered_oid"))
        if not all(is_full_sha(item) for item in pair) or pair in seen:
            violations.append(f"{label}: commit map records must contain unique full SHA pairs")
        seen.add(pair)
    expected_pair = (r0_ref, filtered_boundary_ref)
    if seen != {expected_pair}:
        violations.append(f"{label}: commit map must exactly bind R0 to the filtered boundary")
    digest = proof.get("map_sha256")
    if not is_sha256(digest) or digest != canonical_sha256(records):
        violations.append(f"{label}: commit map digest does not recompute")
        digest = None
    expected_source_refs = {"r0": r0_ref}
    if (
        proof.get("source_refs_before") != expected_source_refs
        or proof.get("source_refs_after") != expected_source_refs
    ):
        violations.append(f"{label}: immutable source refs changed across filtering")
    projection_digest, extraction_manifest_digest = validate_tree_projection_proof(
        proof.get("tree_projection_proof"),
        root,
        r0_ref,
        filtered_boundary_ref,
        filtered_tree_oid,
        label,
        violations,
    )
    return digest, len(records), projection_digest, extraction_manifest_digest


def validate_rterm_object_store(
    proof: Any,
    manifest: dict[str, Any],
    label: str,
    violations: list[str],
) -> dict[str, dict[str, Any]]:
    rssh = manifest.get("rssh")
    rterm = manifest.get("rterm")
    if not isinstance(rssh, dict) or not isinstance(rterm, dict):
        violations.append(f"{label}: certified R0/filtered/R1 epoch is required")
        return {}
    r0_ref = rssh.get("r0_ref")
    filtered_ref = rterm.get("filtered_boundary_ref")
    r1_ref = rterm.get("r1_ref")
    repositories = validate_git_object_store_proof(
        proof,
        {
            "rssh-source": {"r0": r0_ref},
            "rterm-filtered": {
                "filtered_boundary": filtered_ref,
                "r1": r1_ref,
            },
        },
        label,
        violations,
    )
    source_repository = repositories.get("rssh-source", {})
    filtered_repository = repositories.get("rterm-filtered", {})
    if source_repository.get("history_boundaries") != [r0_ref]:
        violations.append(f"{label}: bounded R-SSH R0 proof must stop exactly at certified R0")
    if filtered_repository.get("history_boundaries") != []:
        violations.append(f"{label}: filtered R-Term proof must be a closed root history")
    filtered_graph = filtered_repository.get("commits", {})
    r1_record = filtered_graph.get(r1_ref, {})
    if filtered_ref == r1_ref or r1_record.get("parents") != [filtered_ref]:
        violations.append(
            f"{label}: R1 raw commit must have the filtered boundary as its unique parent"
        )
    return repositories


def git_graph_is_ancestor(
    commits: dict[str, dict[str, Any]], ancestor: Any, descendant: Any
) -> bool:
    if not is_full_sha(ancestor) or not is_full_sha(descendant):
        return False
    pending = [descendant]
    seen: set[str] = set()
    while pending:
        commit = pending.pop()
        if commit == ancestor:
            return True
        if commit in seen:
            continue
        seen.add(commit)
        record = commits.get(commit)
        if isinstance(record, dict):
            pending.extend(record.get("parents", []))
    return False


def flatten_git_tree(
    root_tree_oid: Any,
    trees: dict[str, list[dict[str, str]]],
    label: str,
    violations: list[str],
) -> dict[str, dict[str, str]]:
    if not is_full_sha(root_tree_oid) or root_tree_oid not in trees:
        violations.append(f"{label}: authenticated commit tree is unreadable")
        return {}
    leaves: dict[str, dict[str, str]] = {}
    pending: list[tuple[str, str, int, frozenset[str]]] = [
        (root_tree_oid, "", 0, frozenset())
    ]
    expanded_nodes = 0
    expanded_path_bytes = 0
    while pending:
        tree_oid, prefix, depth, ancestors = pending.pop()
        expanded_nodes += 1
        if expanded_nodes > MAX_FLATTENED_TREE_NODES:
            violations.append(f"{label}: authenticated tree expansion budget exceeded")
            return {}
        if depth > 256 or tree_oid in ancestors:
            violations.append(f"{label}: authenticated tree is cyclic or exceeds depth 256")
            continue
        next_ancestors = ancestors | {tree_oid}
        for entry in reversed(trees.get(tree_oid, [])):
            next_path_bytes = (
                len(prefix.encode("utf-8"))
                + (1 if prefix else 0)
                + len(entry["name"].encode("utf-8"))
            )
            if (
                expanded_path_bytes
                > MAX_EXPANDED_TREE_PATH_BYTES - next_path_bytes
            ):
                violations.append(f"{label}: authenticated tree path byte budget exceeded")
                return {}
            expanded_path_bytes += next_path_bytes
            path = f"{prefix}/{entry['name']}" if prefix else entry["name"]
            if entry["object_type"] == "tree":
                if len(pending) >= MAX_FLATTENED_TREE_NODES:
                    violations.append(f"{label}: authenticated tree expansion budget exceeded")
                    return {}
                pending.append((entry["oid"], path, depth + 1, next_ancestors))
                continue
            if entry["mode"] == "160000":
                violations.append(f"{label}: gitlinks are forbidden in bootstrap trees")
                continue
            if path in leaves:
                violations.append(f"{label}: authenticated tree contains duplicate leaf path")
                continue
            if len(leaves) >= MAX_FLATTENED_TREE_LEAVES:
                violations.append(f"{label}: authenticated tree leaf budget exceeded")
                return {}
            leaves[path] = {
                "mode": entry["mode"],
                "object_type": entry["object_type"],
                "object_oid": entry["oid"],
            }
    return leaves


def validate_bootstrap_projection_proof(
    proof: Any,
    expected_template_mappings: list[dict[str, str]],
    filtered_repository: dict[str, Any],
    filtered_ref: Any,
    r1_ref: Any,
    label: str,
    violations: list[str],
) -> str | None:
    fields = {
        "schema",
        "filtered_boundary_ref",
        "r1_ref",
        "filtered_tree_oid",
        "r1_tree_oid",
        "template_mappings",
        "generated_files",
        "projection_sha256",
    }
    if not isinstance(proof, dict) or set(proof) != fields:
        violations.append(f"{label}: exact R1 bootstrap projection proof fields are not closed")
        return None
    if proof.get("schema") != BOOTSTRAP_PROJECTION_SCHEMA:
        violations.append(f"{label}: R1 bootstrap projection schema mismatch")
    if (
        proof.get("filtered_boundary_ref") != filtered_ref
        or proof.get("r1_ref") != r1_ref
    ):
        violations.append(f"{label}: bootstrap projection does not bind filtered/R1 refs")
    commits = filtered_repository.get("commits", {})
    trees = filtered_repository.get("trees", {})
    blobs = filtered_repository.get("blobs", set())
    filtered_commit = commits.get(filtered_ref, {})
    r1_commit = commits.get(r1_ref, {})
    filtered_tree_oid = filtered_commit.get("tree")
    r1_tree_oid = r1_commit.get("tree")
    if proof.get("filtered_tree_oid") != filtered_tree_oid:
        violations.append(f"{label}: bootstrap projection filtered tree does not match commit")
    if proof.get("r1_tree_oid") != r1_tree_oid:
        violations.append(f"{label}: bootstrap projection R1 tree does not match commit")
    templates = proof.get("template_mappings")
    if not json_equal(templates, expected_template_mappings):
        violations.append(f"{label}: bootstrap projection must contain the exact thirteen templates")
        templates = []
    filtered_leaves = flatten_git_tree(
        filtered_tree_oid, trees, f"{label} filtered tree", violations
    )
    r1_leaves = flatten_git_tree(r1_tree_oid, trees, f"{label} R1 tree", violations)
    generated = proof.get("generated_files")
    generated_fields = {"filtered_path", "mode", "object_type", "object_oid"}
    generated_paths = ["Cargo.lock", "contracts/rterm-consumer/Cargo.lock"]
    valid_generated: list[dict[str, str]] = []
    if not isinstance(generated, list) or len(generated) != 2:
        violations.append(f"{label}: bootstrap projection requires exactly two generated lockfiles")
    else:
        for index, item in enumerate(generated):
            if not isinstance(item, dict) or set(item) != generated_fields:
                violations.append(f"{label}: generated lockfile {index} fields are not closed")
                continue
            if (
                item.get("filtered_path") not in generated_paths
                or item.get("mode") != "100644"
                or item.get("object_type") != "blob"
                or item.get("object_oid") not in blobs
            ):
                violations.append(f"{label}: generated lockfile {index} is not a closed R1 blob")
                continue
            valid_generated.append(item)
        if [item["filtered_path"] for item in valid_generated] != generated_paths:
            violations.append(f"{label}: generated lockfile paths/order differ from the contract")
    expected_r1 = {path: dict(identity) for path, identity in filtered_leaves.items()}
    for mapping in expected_template_mappings:
        expected_r1[mapping["filtered_path"]] = {
            "mode": mapping["mode"],
            "object_type": mapping["object_type"],
            "object_oid": mapping["object_oid"],
        }
    for item in valid_generated:
        if item["filtered_path"] in filtered_leaves:
            violations.append(f"{label}: generated lockfile already existed at filtered boundary")
        expected_r1[item["filtered_path"]] = {
            "mode": item["mode"],
            "object_type": item["object_type"],
            "object_oid": item["object_oid"],
        }
    if r1_leaves != expected_r1:
        violations.append(
            f"{label}: authenticated R1 tree is not the exact filtered tree plus bootstrap product"
        )
    digest = proof.get("projection_sha256")
    digest_material = {key: proof[key] for key in fields - {"projection_sha256"}}
    if not is_sha256(digest) or digest != canonical_sha256(digest_material):
        violations.append(f"{label}: deterministic bootstrap projection digest does not recompute")
        return None
    return digest


def validate_cross_repository_proof_set(
    entries_by_type: dict[str, list[dict[str, Any]]],
    artifacts: dict[str, dict[str, Any]],
    violations: list[str],
) -> None:
    def singleton_payload(artifact_type: str) -> dict[str, Any] | None:
        entries = entries_by_type.get(artifact_type, [])
        if len(entries) != 1:
            return None
        return artifacts.get(entries[0].get("artifact_id"))

    history_map = singleton_payload("source-to-filtered-history-map")
    external = singleton_payload("rterm-external-source-proof")
    if history_map is None and external is None:
        return
    if history_map is None or external is None:
        violations.append("R-Term source map and external object-store proof must be certified together")
        return
    map_proof = history_map.get("commit_map_proof")
    map_digest = map_proof.get("map_sha256") if isinstance(map_proof, dict) else None
    if external.get("source_to_filtered_map_sha256") != map_digest:
        violations.append("R-Term external proof does not bind the recomputed source-to-filtered map")
    if external.get("git_object_store_proof") != history_map.get("git_object_store_proof"):
        violations.append("R-Term history map and external proof object-store snapshots drift")
    projection = map_proof.get("tree_projection_proof") if isinstance(map_proof, dict) else None
    projection_digest = (
        projection.get("projection_sha256") if isinstance(projection, dict) else None
    )
    if external.get("tree_projection_sha256") != projection_digest:
        violations.append("R-Term external proof does not bind the recomputed tree projection")
    bootstrap_projection = history_map.get("bootstrap_projection_proof")
    bootstrap_projection_digest = (
        bootstrap_projection.get("projection_sha256")
        if isinstance(bootstrap_projection, dict)
        else None
    )
    if external.get("bootstrap_projection_sha256") != bootstrap_projection_digest:
        violations.append("R-Term external proof does not bind the R1 bootstrap projection")
    extraction = singleton_payload("rterm-extraction-manifest")
    extraction_claims = extraction.get("claims") if isinstance(extraction, dict) else None
    owned_inventory = (
        extraction.get("owned_projection_inventory")
        if isinstance(extraction, dict)
        else None
    )
    extraction_digest = (
        owned_inventory.get("inventory_sha256")
        if isinstance(owned_inventory, dict)
        else None
    )
    if (
        not isinstance(projection, dict)
        or projection.get("extraction_manifest_sha256") != extraction_digest
    ):
        violations.append("R0-to-filtered projection is not bound to the extraction manifest")
    inventory_paths = (
        owned_inventory.get("path_mappings", [])
        if isinstance(owned_inventory, dict)
        else []
    )
    historical_paths = [
        mapping
        for mapping in inventory_paths
        if isinstance(mapping, dict)
        and not mapping.get("source_path", "").startswith(
            FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED[0][0] + "/"
        )
    ]
    bootstrap_paths = [
        mapping
        for mapping in inventory_paths
        if isinstance(mapping, dict)
        and mapping.get("source_path", "").startswith(
            FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED[0][0] + "/"
        )
    ]
    if not isinstance(projection, dict) or not json_equal(
        projection.get("path_mappings"), historical_paths
    ):
        violations.append("R0-to-filtered projection does not equal the historical owned inventory")
    if not isinstance(bootstrap_projection, dict) or not json_equal(
        bootstrap_projection.get("template_mappings"), bootstrap_paths
    ):
        violations.append("R1 bootstrap projection does not equal the template inventory")
    inventory_mappings = inventory_paths
    if not isinstance(extraction_claims, dict) or (
        extraction_claims.get("manifest_sha256") != extraction_digest
        or extraction_claims.get("owned_path_count") != len(inventory_mappings)
    ):
        violations.append("R-Term extraction claims do not recompute from the owned inventory")


def retain_post_cross_repository_artifacts(
    entries_by_type: dict[str, list[dict[str, Any]]],
    artifacts: dict[str, dict[str, Any]],
    contract: dict[str, Any],
) -> dict[str, dict[str, Any]]:
    """Keep only payloads consumed after cross-repository proof validation."""
    policies = contract.get("artifact_policies", {})
    retained_ids: set[Any] = set()
    for artifact_type, entries in entries_by_type.items():
        policy = policies.get(artifact_type, {})
        if artifact_type in {"runner-fingerprint", "font-catalog-fingerprint"} or policy.get("content_kind") == "aggregate":
            retained_ids.update(entry.get("artifact_id") for entry in entries)
    return {
        artifact_id: payload
        for artifact_id, payload in artifacts.items()
        if artifact_id in retained_ids
    }


def artifact_payload_needed_after_individual_validation(
    artifact_type: str,
    policy: dict[str, Any],
) -> bool:
    return (
        artifact_type in CROSS_REPOSITORY_ARTIFACT_TYPES
        or artifact_type in {"runner-fingerprint", "font-catalog-fingerprint"}
        or policy.get("content_kind") == "aggregate"
    )


def validate_statistics(
    reported: Any,
    recomputed: dict[str, int | float],
    label: str,
    violations: list[str],
) -> None:
    if reported != recomputed:
        violations.append(f"{label}: reported statistics do not match recomputed raw statistics")


def expected_metric_protocol(
    policy: dict[str, Any], contract: dict[str, Any]
) -> dict[str, Any]:
    protocol = {
        "warmups": contract["protocol"]["warmups"],
        "measured_cold_processes": contract["protocol"]["measured_cold_processes"],
        "timeout_seconds": contract["protocol"]["timeout_seconds"],
        "cross_process_percentiles": contract["protocol"][
            "cross_process_percentiles"
        ],
        "maximum": contract["protocol"]["maximum"],
        "sampling_mode": policy["sampling_mode"],
    }
    if policy["sampling_mode"] == "startup-marker":
        startup = contract["sampling"]["startup"]
        protocol.update(
            {
                "samples_per_process": startup["samples_per_process"],
                "stabilization_ms": startup["stabilization_ms"],
                "benchmark_startup": True,
                "exit_immediately_after_cpu_bootstrap_present": startup[
                    "exit_immediately_after_cpu_bootstrap_present"
                ],
            }
        )
    else:
        residence = contract["sampling"]["residence"]
        protocol.update(
            {
                "samples_per_process": residence["samples_per_process"],
                "stabilization_ms": residence["stabilization_ms"],
                "sample_interval_ms": residence["sample_interval_ms"],
                "process_representative": residence["process_representative"],
                "flattening_for_percentiles": residence[
                    "flattening_for_percentiles"
                ],
                "owner_ready_marker": policy["owner_ready_marker"],
            }
        )
    return protocol


def validate_project_owned_resource_metrics_v1(
    summary: Any,
    stage: str,
    label: str,
    violations: list[str],
    *,
    expected_backend: str | None = None,
    expected_adapter_name: str | None = None,
) -> None:
    """Validate the closed ProjectOwnedResourceMetricsV1 stage row.

    The application owns the same boundary in Rust.  Keeping the validator in
    the evidence checker prevents a runner from replacing an owner marker with
    a generic delay or from silently carrying a later-stage allocation into an
    earlier attribution cell.
    """
    if not isinstance(summary, dict):
        violations.append(f"{label}: ProjectOwnedResourceMetricsV1 row must be an object")
        return
    if stage not in ATTRIBUTION_STAGES:
        violations.append(f"{label}: attribution stage is outside the frozen matrix")
        return
    unknown = set(summary) - PROJECT_RESOURCE_FIELDS
    if unknown:
        violations.append(f"{label}: resource row has unknown fields {sorted(unknown)}")
    for field in PROJECT_RESOURCE_NUMERIC_FIELDS:
        value = summary.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            violations.append(f"{label}: resource field {field} must be a non-negative JSON integer")

    stage_index = ATTRIBUTION_STAGES.index(stage)
    allowed_by_stage = [
        "cpu_staging_bytes",
        "cpu_surface_count",
        "cpu_present_count",
    ]
    if stage_index >= 1:
        allowed_by_stage += ["instance_count", "surface_count"]
    if stage_index >= 2:
        allowed_by_stage += ["adapter_count", "device_count", "queue_count"]
    if stage_index >= 3:
        allowed_by_stage += ["surface_configure_count", "surface_acquire_count", "clear_present_count"]
    if stage_index >= 4:
        allowed_by_stage += [
            "pipeline_count",
            "pipeline_layout_count",
            "materialized_buffer_count",
            "total_allocated_buffer_bytes",
        ]
    if stage_index >= 5:
        allowed_by_stage += [
            "retained_font_bytes",
            "active_font_count",
            "catalog_builds",
            "catalog_generation",
            "glyph_atlas_bytes",
            "raster_cache_bytes",
            "instance_buffer_bytes",
            "upload_buffer_bytes",
            "total_allocated_texture_bytes",
            "base_text_renderer_materialization_count",
            "cursor_text_renderer_materialization_count",
        ]
    if stage_index >= 6:
        allowed_by_stage.append("indexed_font_count")
    if stage_index >= 7:
        allowed_by_stage.append("snapshot_bytes")
    for field in PROJECT_RESOURCE_NUMERIC_FIELDS:
        value = summary.get(field)
        if isinstance(value, int) and not isinstance(value, bool) and value != 0 and field not in allowed_by_stage:
            violations.append(f"{label}: resource field {field} must remain zero at {stage}")

    def require_exact(field: str, expected: int) -> None:
        if summary.get(field) != expected:
            violations.append(f"{label}: resource field {field} must be {expected}")

    def require_positive(field: str) -> None:
        value = summary.get(field)
        if not isinstance(value, int) or isinstance(value, bool) or value == 0:
            violations.append(f"{label}: resource field {field} must be positive")

    require_positive("cpu_staging_bytes")
    require_exact("cpu_surface_count", 1)
    require_exact("cpu_present_count", 1)
    if stage_index >= 1:
        require_exact("instance_count", 1)
        require_exact("surface_count", 1)
    if stage_index >= 2:
        for field in ("adapter_count", "device_count", "queue_count"):
            require_exact(field, 1)
        backend = summary.get("backend")
        adapter_name = summary.get("adapter_name")
        if backend not in {"dx12", "vulkan", "gl"}:
            violations.append(f"{label}: resource backend is required from adapter-device onward")
        if not isinstance(adapter_name, str) or not adapter_name:
            violations.append(f"{label}: resource adapter_name is required from adapter-device onward")
        if expected_backend is not None and backend != expected_backend:
            violations.append(f"{label}: resource backend differs from renderer identity")
        if expected_adapter_name is not None and adapter_name != expected_adapter_name:
            violations.append(f"{label}: resource adapter_name differs from renderer identity")
    elif "backend" in summary or "adapter_name" in summary:
        violations.append(f"{label}: resource backend and adapter_name must be absent before adapter-device")
    if stage_index >= 3:
        require_exact("surface_configure_count", 1)
        require_exact("clear_present_count", 1)
        require_exact("surface_acquire_count", 3 if stage_index >= 7 else 2 if stage_index >= 5 else 1)
    if stage_index >= 4:
        require_exact("pipeline_count", 1)
        require_exact("pipeline_layout_count", 1)
        require_exact("materialized_buffer_count", 1)
        if summary.get("total_allocated_buffer_bytes") == 0:
            violations.append(f"{label}: total_allocated_buffer_bytes must be positive")
    if stage_index >= 5:
        for field in (
            "retained_font_bytes",
            "active_font_count",
            "catalog_builds",
            "catalog_generation",
            "glyph_atlas_bytes",
        ):
            require_positive(field)
        if summary.get("total_allocated_texture_bytes") != summary.get("glyph_atlas_bytes", 0) + summary.get("image_texture_bytes", 0):
            violations.append(f"{label}: total_allocated_texture_bytes does not equal glyph plus image bytes")
        text_count = 2 if stage_index >= 7 else 1
        require_exact("base_text_renderer_materialization_count", text_count)
        require_exact("cursor_text_renderer_materialization_count", 0)
    if stage_index >= 6:
        require_positive("indexed_font_count")
        require_exact("inactive_font_bytes", 0)
    if stage_index >= 7:
        require_exact("image_texture_bytes", 0)
        require_positive("snapshot_bytes")


def validate_lkg(
    lkg: Any,
    candidate: dict[str, int | float],
    policy: dict[str, Any],
    candidate_metadata: dict[str, Any],
    candidate_identity: dict[str, Any],
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    if not isinstance(lkg, dict):
        violations.append(f"{label}: same-machine immutable LKG samples are required")
        return
    lkg_fields = {
        "source_sha",
        "binary_hashes",
        "runner_fingerprint_sha256",
        "platform",
        "requested_backend",
        "warmups",
        "warmup_process_ids",
        "measured_cold_processes",
        "timeout_seconds",
        "protocol",
        "processes",
        "statistics",
        "relative_regression_ratios",
    }
    if policy["sampling_mode"] == "residence":
        lkg_fields.update({"actual_backend", "adapter_identity"})
    reject_unknown_fields(lkg, lkg_fields, f"{label} LKG", violations)
    if (
        lkg.get("warmups") != contract["protocol"]["warmups"]
        or lkg.get("measured_cold_processes")
        != contract["protocol"]["measured_cold_processes"]
        or lkg.get("timeout_seconds") != contract["protocol"]["timeout_seconds"]
    ):
        violations.append(f"{label}: LKG must retain the exact 5+30/60 protocol fields")
    lkg_warmup_ids = lkg.get("warmup_process_ids")
    if (
        not isinstance(lkg_warmup_ids, list)
        or len(lkg_warmup_ids) != contract["protocol"]["warmups"]
        or len(set(lkg_warmup_ids)) != contract["protocol"]["warmups"]
        or not all(isinstance(item, str) and item for item in lkg_warmup_ids)
    ):
        violations.append(f"{label}: LKG must retain exactly five unique warmup identities")
        lkg_warmup_ids = []
    if lkg.get("protocol") != expected_metric_protocol(policy, contract):
        violations.append(f"{label}: LKG full sampling protocol differs from the frozen contract")
    if lkg.get("source_sha") != contract["product_lkg_ref"]:
        violations.append(f"{label}: LKG source must equal immutable product_lkg_ref")
    if not validate_binary_hashes(lkg.get("binary_hashes"), f"{label} LKG", violations):
        return
    if lkg.get("runner_fingerprint_sha256") != candidate_identity.get("runner_fingerprint_sha256"):
        violations.append(f"{label}: LKG runner fingerprint cohort mismatch")
    if lkg.get("platform") != candidate_identity.get("platform"):
        violations.append(f"{label}: LKG platform cohort mismatch")
    if lkg.get("requested_backend") != candidate_metadata.get("requested_backend"):
        violations.append(f"{label}: LKG requested backend mismatch")
    mode = policy["sampling_mode"]
    if mode == "residence":
        if lkg.get("actual_backend") != candidate_metadata.get("actual_backend"):
            violations.append(f"{label}: LKG actual backend mismatch")
        if lkg.get("adapter_identity") != candidate_metadata.get("adapter_identity"):
            violations.append(f"{label}: LKG adapter identity mismatch")
    processes = lkg.get("processes")
    if not isinstance(processes, list) or len(processes) != 30:
        violations.append(f"{label}: LKG must retain 30 process-cold raw records")
        return
    process_ids: set[str] = set()
    representatives: list[int | float] = []
    raw: list[int | float] = []
    for index, process in enumerate(processes):
        if not isinstance(process, dict):
            violations.append(f"{label}: LKG process {index} is not an object")
            continue
        process_fields = (
            {"process_id", "phase", "benchmark_startup", "marker_count", "value"}
            if mode == "startup-marker"
            else {"process_id", "phase", "samples", "representative"}
        )
        reject_unknown_fields(
            process, process_fields, f"{label} LKG process {index}", violations
        )
        process_id = process.get("process_id")
        if not isinstance(process_id, str) or not process_id or process_id in process_ids:
            violations.append(f"{label}: LKG process IDs must be non-empty and unique")
        else:
            process_ids.add(process_id)
        if process.get("phase") != "measured" or process_id in lkg_warmup_ids:
            violations.append(f"{label}: LKG warmup records cannot be mixed into measured samples")
        if mode == "startup-marker":
            value = process.get("value")
            if (
                not numeric(value)
                or process.get("marker_count") != 1
                or process.get("benchmark_startup") is not True
            ):
                violations.append(
                    f"{label}: LKG startup process must carry exactly one non-negative marker value"
                )
                continue
            representatives.append(value)
            raw.append(value)
        else:
            samples = process.get("samples")
            if not isinstance(samples, list) or len(samples) != 10 or not all(numeric(item) for item in samples):
                violations.append(
                    f"{label}: LKG residence process must retain ten non-negative raw samples"
                )
                continue
            representative = process_representative(samples)
            if process.get("representative") != representative:
                violations.append(f"{label}: LKG process representative is not nearest-rank p50")
            representatives.append(representative)
            raw.extend(samples)
    if len(representatives) != 30:
        return
    expected = recomputed_statistics(representatives, raw)
    validate_statistics(lkg.get("statistics"), expected, f"{label} LKG", violations)
    reported_ratios = lkg.get("relative_regression_ratios")
    if not isinstance(reported_ratios, dict) or set(reported_ratios) != {"p50", "p95", "max"}:
        violations.append(f"{label}: p50/p95/max relative regression ratios are required")
        return
    for statistic in ("p50", "p95", "max"):
        if expected[statistic] == 0:
            violations.append(f"{label}: LKG {statistic} cannot be zero")
            continue
        ratio = candidate[statistic] / expected[statistic]
        reported_ratio = reported_ratios.get(statistic)
        if not numeric(reported_ratio) or not math.isclose(reported_ratio, ratio, rel_tol=1e-12, abs_tol=1e-12):
            violations.append(f"{label}: {statistic} relative regression ratio does not match raw LKG values")
        if ratio > contract["windows_product_gates"]["relative_regression_ratio_max"]:
            violations.append(f"{label}: {statistic} relative regression threshold violated ({ratio:.6f})")


def validate_thresholds(
    statistics: dict[str, int | float],
    policy: dict[str, Any],
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    gates = contract["windows_product_gates"]
    thresholds = policy.get("thresholds", {})
    for statistic, gate_name in thresholds.items():
        gate = gates[gate_name]
        if statistic == "max_exclusive":
            if statistics["max"] >= gate:
                violations.append(f"{label}: max threshold is exclusive and was violated")
        elif statistic == "max":
            if statistics["max"] > gate:
                violations.append(f"{label}: max threshold violated")
        elif statistic == "p50_max":
            if statistics["p50"] > gate:
                violations.append(f"{label}: p50 threshold violated")
        elif statistic == "p95_max":
            if statistics["p95"] > gate:
                violations.append(f"{label}: p95 threshold violated")


def is_font_diagnostic_run_id(value: Any) -> bool:
    return isinstance(value, str) and re.fullmatch(r"empty-window-[0-9]+-[0-9]+", value) is not None


def validate_font_resource_evidence(
    value: Any,
    expected_mode: str | None,
    expected_specimen: str,
    label: str,
    violations: list[str],
) -> dict[str, Any] | None:
    resource_label = f"{label} font_resources"
    if not isinstance(value, dict):
        violations.append(f"{resource_label}: complete resource evidence is required")
        return None
    fields = {
        "mode",
        "specimen",
        "retained_source_bytes",
        "indexed_source_count",
        "active_source_count",
        "initial_catalog_source_count",
        "catalog_builds",
        "generation",
        "recovery_retained_source_bytes",
        "recovery_generation",
        "activation_latency_micros",
        "tofu_count",
        "frame_catalog_generation",
        "frame_generation_consistent",
        "index_fingerprint_sha256",
        "catalog_fingerprint_sha256",
        "ordered_catalog_fingerprint_sha256",
        "font_inventory_fingerprint_sha256",
        "font_index_policy_version",
    }
    inventory_fields = {
        "font_inventory_fingerprint_sha256",
        "font_index_policy_version",
    }
    fields.update(inventory_fields)
    reject_unknown_fields(value, fields, resource_label, violations)
    required = fields.difference(inventory_fields)
    missing = required.difference(value)
    if missing:
        violations.append(
            f"{resource_label}: missing fields {', '.join(sorted(missing))}"
        )
        return None
    if value.get("mode") != expected_mode or value.get("specimen") != expected_specimen:
        violations.append(f"{resource_label}: mode/specimen identity mismatch")
    integer_fields = (
        "retained_source_bytes",
        "indexed_source_count",
        "active_source_count",
        "initial_catalog_source_count",
        "catalog_builds",
        "generation",
        "recovery_retained_source_bytes",
        "recovery_generation",
        "activation_latency_micros",
        "tofu_count",
        "frame_catalog_generation",
    )
    if any(
        not isinstance(value.get(field), int)
        or isinstance(value.get(field), bool)
        or value[field] < 0
        for field in integer_fields
    ):
        violations.append(f"{resource_label}: counters must be finite non-negative integers")
        return None
    retained = value["retained_source_bytes"]
    indexed = value["indexed_source_count"]
    active = value["active_source_count"]
    initial = value["initial_catalog_source_count"]
    builds = value["catalog_builds"]
    generation = value["generation"]
    if retained <= 0 or not (indexed >= active > 0) or not (1 <= initial <= active):
        violations.append(f"{resource_label}: retained/indexed/active/initial counters are invalid")
    if builds != generation or builds != active - initial + 1:
        violations.append(f"{resource_label}: catalog builds do not match the independently recorded initial batch")
    if (
        value["recovery_retained_source_bytes"] != retained
        or value["recovery_generation"] != generation
    ):
        violations.append(f"{resource_label}: recovery epoch is inconsistent")
    if (
        value["tofu_count"] != 0
        or value["frame_catalog_generation"] != generation
        or value["frame_generation_consistent"] is not True
    ):
        violations.append(f"{resource_label}: tofu/frame generation evidence is inconsistent")
    if expected_mode == "shared" and not (initial == active and builds == 1):
        violations.append(f"{resource_label}: SharedAll counter shape is invalid")
    if expected_mode == "lazy" and not (initial == active == builds == 1):
        violations.append(f"{resource_label}: Lazy ASCII counter shape is invalid")
    for field in (
        "index_fingerprint_sha256",
        "catalog_fingerprint_sha256",
        "ordered_catalog_fingerprint_sha256",
    ):
        if not is_sha256(value.get(field)):
            violations.append(f"{resource_label}: {field} must be an irreversible SHA-256")
    if expected_mode in {"current", "shared"}:
        if not is_sha256(value.get("font_inventory_fingerprint_sha256")):
            violations.append(
                f"{resource_label}: full inventory fingerprint must be an irreversible SHA-256"
            )
        policy_version = value.get("font_index_policy_version")
        if (
            not isinstance(policy_version, int)
            or isinstance(policy_version, bool)
            or policy_version <= 0
        ):
            violations.append(f"{resource_label}: font index policy version must be positive")
    elif any(field in value for field in inventory_fields):
        violations.append(
            f"{resource_label}: Lazy evidence must not pretend to materialize the full font inventory"
        )
    recursively_reject_path_keys(value, resource_label, violations)
    return value


def validate_metric_artifact(
    artifact: dict[str, Any],
    policy: dict[str, Any],
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> list[dict[str, Any]]:
    reject_unknown_fields(
        artifact,
        {
            "schema",
            "identity",
            "warmups",
            "warmup_process_ids",
            "measured_cold_processes",
            "timeout_seconds",
            "protocol",
            "groups",
            "certification_eligible",
        },
        label,
        violations,
    )
    if artifact.get("schema") != RAW_SCHEMA:
        violations.append(f"{label}: raw metric schema must be {RAW_SCHEMA}")
    if policy.get("certification_eligible") is True and artifact.get(
        "certification_eligible"
    ) is not True:
        violations.append(f"{label}: certification_eligible must be true")
    font_resource_evidence = policy.get("font_resource_evidence") is True
    expected_warmups = policy.get("warmup_process_count", 5)
    if artifact.get("warmups") != expected_warmups or artifact.get("measured_cold_processes") != 30:
        violations.append(
            f"{label}: raw metric must retain exactly {expected_warmups} warmups and 30 measured processes"
        )
    warmup_ids = artifact.get("warmup_process_ids")
    if (
        not isinstance(warmup_ids, list)
        or len(warmup_ids) != expected_warmups
        or len(set(warmup_ids)) != expected_warmups
        or not all(isinstance(item, str) and item for item in warmup_ids)
    ):
        violations.append(
            f"{label}: {expected_warmups} distinct warmup process IDs must be retained separately"
        )
        warmup_ids = []
    elif font_resource_evidence and not all(is_font_diagnostic_run_id(item) for item in warmup_ids):
        violations.append(f"{label}: all 15 warmups must retain actual diagnostic run IDs")
    if artifact.get("timeout_seconds") != 60:
        violations.append(f"{label}: timeout must be 60 seconds")
    if artifact.get("protocol") != expected_metric_protocol(policy, contract):
        violations.append(f"{label}: full sampling protocol differs from the frozen contract")
    groups = artifact.get("groups")
    if not isinstance(groups, list) or not groups:
        violations.append(f"{label}: raw metric must contain non-empty groups")
        return []
    raw_groups: list[dict[str, Any]] = []
    font_group_warmups: list[set[str]] = []
    for index, group_value in enumerate(groups):
        group_label = f"{label} group {index}"
        if not isinstance(group_value, dict):
            violations.append(f"{group_label}: group must be an object")
            continue
        group = group_value
        mode = policy["sampling_mode"]
        reject_unknown_fields(
            group,
            {
                "name",
                "metric",
                "sampling_mode",
                "requested_backend",
                "final_renderer",
                "processes",
                "statistics",
                "actual_backend",
                "adapter_identity",
                "connection_state",
                "owner_ready_marker",
                "stabilization_ms",
                "sample_interval_ms",
                "lkg",
                "support_status",
                "unsupported_reason",
                "unsupported_at_stage",
                "warmup_process_ids",
            },
            group_label,
            violations,
        )
        if group.get("sampling_mode") != mode:
            violations.append(f"{group_label}: sampling mode mismatch")
        if group.get("metric") != policy["metric"]:
            violations.append(f"{group_label}: metric is not approved for this artifact")
        if "flattened_samples" in group or "samples" in group:
            violations.append(f"{group_label}: flattened residence samples are forbidden")
        matrix_status: str | None = None
        matrix_stage: str | None = None
        if "matrix_stages" in policy:
            expected_names = {
                f"{backend}/{stage}"
                for backend in policy["matrix_backends"]
                for stage in policy["matrix_stages"]
            }
            name = group.get("name")
            if name not in expected_names:
                violations.append(f"{group_label}: matrix stage/backend cell is outside the frozen inventory")
            else:
                backend, stage = name.split("/", 1)
                matrix_stage = stage
                matrix_status = group.get("support_status")
                if matrix_status not in set(
                    contract["diagnostic_probe_outcomes"]["statuses"]
                ):
                    violations.append(
                        f"{group_label}: matrix support_status must be supported or unsupported"
                    )
                if group.get("requested_backend") != backend:
                    violations.append(f"{group_label}: matrix requested backend does not match its cell")
                stage_index = policy["matrix_stages"].index(stage)
                if matrix_status == "unsupported":
                    unsupported_fields = {
                        "name",
                        "metric",
                        "sampling_mode",
                        "requested_backend",
                        "support_status",
                        "unsupported_reason",
                        "unsupported_at_stage",
                    }
                    reject_unknown_fields(
                        group, unsupported_fields, group_label, violations
                    )
                    reason = group.get("unsupported_reason")
                    unsupported_at = group.get("unsupported_at_stage")
                    if backend not in contract["windows_backends"]["diagnostic_only"]:
                        violations.append(
                            f"{group_label}: required auto product probe cannot be unsupported"
                        )
                    if not isinstance(reason, str) or re.fullmatch(
                        contract["diagnostic_probe_outcomes"]["reason_pattern"],
                        reason,
                    ) is None:
                        violations.append(
                            f"{group_label}: unsupported diagnostic requires a stable reason code"
                        )
                    if unsupported_at not in policy["matrix_stages"]:
                        violations.append(
                            f"{group_label}: unsupported_at_stage is outside the frozen stages"
                        )
                    elif policy["matrix_stages"].index(unsupported_at) > stage_index:
                        violations.append(
                            f"{group_label}: unsupported_at_stage occurs after the target stage"
                        )
                    raw_groups.append(
                        {
                            "name": name,
                            "status": "unsupported",
                            "unsupported_reason": reason,
                            "unsupported_at_stage": unsupported_at,
                            "process_ids": set(),
                            "representatives": [],
                            "raw": [],
                            "reported_statistics": None,
                            "lkg": None,
                            "identity": artifact.get("identity"),
                            "warmup_process_ids": tuple(warmup_ids),
                            "protocol_fields": (
                                artifact.get("warmups"),
                                artifact.get("measured_cold_processes"),
                                artifact.get("timeout_seconds"),
                            ),
                            "protocol": artifact.get("protocol"),
                            "metadata": {
                                "metric": group.get("metric"),
                                "sampling_mode": group.get("sampling_mode"),
                                "requested_backend": group.get("requested_backend"),
                                "support_status": "unsupported",
                                "unsupported_reason": reason,
                                "unsupported_at_stage": unsupported_at,
                            },
                        }
                    )
                    continue
                if any(
                    field in group
                    for field in ("unsupported_reason", "unsupported_at_stage")
                ):
                    violations.append(
                        f"{group_label}: supported matrix cell cannot carry unsupported fields"
                    )
                if stage_index < 2:
                    if "actual_backend" in group or "adapter_identity" in group:
                        violations.append(f"{group_label}: pre-adapter stage cannot report adapter/backend identity")
                else:
                    actual = group.get("actual_backend")
                    expected_actual = (
                        contract["windows_backends"]["diagnostic_only"]
                        if backend == "auto"
                        else [backend]
                    )
                    if actual not in expected_actual or not isinstance(group.get("adapter_identity"), str) or not group["adapter_identity"]:
                        violations.append(f"{group_label}: adapter/backend identity is missing or mismatched")
        elif any(
            field in group
            for field in ("support_status", "unsupported_reason", "unsupported_at_stage")
        ):
            violations.append(f"{group_label}: support outcome fields are matrix-only")
        if "requested_backend" in policy and group.get("requested_backend") != policy["requested_backend"]:
            violations.append(f"{group_label}: required product backend mismatch")
        if "final_renderer" in policy and group.get("final_renderer") != policy["final_renderer"]:
            violations.append(f"{group_label}: final renderer mismatch")
        if "connection_state" in policy and group.get("connection_state") != policy["connection_state"]:
            violations.append(f"{group_label}: connection state mismatch")
        group_warmup_ids = warmup_ids
        if font_resource_evidence:
            group_warmup_ids = group.get("warmup_process_ids")
            expected_group_warmups = policy.get("warmups_per_group")
            if (
                not isinstance(group_warmup_ids, list)
                or len(group_warmup_ids) != expected_group_warmups
                or len(set(group_warmup_ids)) != expected_group_warmups
                or not all(
                    isinstance(item, str) and is_font_diagnostic_run_id(item)
                    for item in group_warmup_ids
                )
            ):
                violations.append(
                    f"{group_label}: must retain exactly five actual warmup process IDs"
                )
                group_warmup_ids = []
            else:
                font_group_warmups.append(set(group_warmup_ids))
        processes = group.get("processes")
        if not isinstance(processes, list) or not 1 <= len(processes) <= 30:
            if mode == "residence" and isinstance(group.get("flattened_samples"), list):
                violations.append(f"{group_label}: flattened 300-point residence statistics are forbidden")
            else:
                violations.append(f"{group_label}: each raw shard must retain between one and 30 process records")
            continue
        process_ids: set[str] = set()
        representatives: list[int | float] = []
        raw: list[int | float] = []
        font_processes: list[dict[str, Any]] = []
        for process_index, process_value in enumerate(processes):
            process_label = f"{group_label} process {process_index}"
            if not isinstance(process_value, dict):
                violations.append(f"{process_label}: record must be an object")
                continue
            process_fields = (
                {"process_id", "phase", "benchmark_startup", "marker_count", "value"}
                if mode == "startup-marker"
                else {"process_id", "phase", "samples", "representative"}
            )
            if "matrix_stages" in policy:
                process_fields.update(
                    {
                        "round_index",
                        "attribution_stage",
                        "resource_summary_schema",
                        "resource_summary",
                    }
                )
            if font_resource_evidence:
                process_fields.update({"round_index", "font_resources"})
            reject_unknown_fields(
                process_value, process_fields, process_label, violations
            )
            process_id = process_value.get("process_id")
            if not isinstance(process_id, str) or not process_id or process_id in process_ids:
                violations.append(f"{process_label}: process_id must be non-empty and unique")
            else:
                process_ids.add(process_id)
            if font_resource_evidence and not is_font_diagnostic_run_id(process_id):
                violations.append(f"{process_label}: process_id must be an actual diagnostic run ID")
            if process_value.get("phase") != "measured" or process_id in warmup_ids:
                violations.append(f"{process_label}: warmup records cannot be mixed into measured samples")
            if "matrix_stages" in policy:
                if matrix_stage is None:
                    continue
                round_index = process_value.get("round_index")
                if not isinstance(round_index, int) or isinstance(round_index, bool) or not 1 <= round_index <= 30:
                    violations.append(f"{process_label}: round_index must be an integer from 1 through 30")
                if process_value.get("attribution_stage") != matrix_stage:
                    violations.append(f"{process_label}: attribution_stage must be the exact owner stage")
                if process_value.get("resource_summary_schema") != PROJECT_RESOURCE_SCHEMA:
                    violations.append(f"{process_label}: resource_summary_schema must be {PROJECT_RESOURCE_SCHEMA}")
                validate_project_owned_resource_metrics_v1(
                    process_value.get("resource_summary"),
                    matrix_stage,
                    f"{process_label} resource_summary",
                    violations,
                    expected_backend=group.get("actual_backend")
                    if policy["matrix_stages"].index(matrix_stage) >= 2
                    else None,
                )
            if font_resource_evidence:
                round_index = process_value.get("round_index")
                if not isinstance(round_index, int) or isinstance(round_index, bool) or not 1 <= round_index <= 30:
                    violations.append(f"{process_label}: round_index must be an integer from 1 through 30")
                group_name = group.get("name")
                expected_font_mode = {
                    "current-copied/ascii": "current",
                    "shared-all/ascii": "shared",
                    "lazy/ascii": "lazy",
                }.get(group_name)
                resources = validate_font_resource_evidence(
                    process_value.get("font_resources"),
                    expected_font_mode,
                    "ascii",
                    process_label,
                    violations,
                )
                if resources is not None and isinstance(round_index, int):
                    font_processes.append(
                        {
                            "round_index": round_index,
                            "process_id": process_id,
                            "font_resources": resources,
                        }
                    )
            if mode == "startup-marker":
                if any(field in process_value for field in ("samples", "residence_samples", "representative")):
                    violations.append(f"{process_label}: startup evidence cannot contain residence samples")
                if process_value.get("benchmark_startup") is not True or process_value.get("marker_count") != 1:
                    violations.append(f"{process_label}: startup must contribute exactly one benchmark marker")
                value = process_value.get("value")
                if not numeric(value):
                    violations.append(
                        f"{process_label}: marker value must be non-negative numeric"
                    )
                    continue
                representatives.append(value)
                raw.append(value)
            else:
                samples = process_value.get("samples")
                if not isinstance(samples, list) or len(samples) != 10 or not all(numeric(item) for item in samples):
                    violations.append(
                        f"{process_label}: residence evidence must retain ten non-negative raw samples"
                    )
                    continue
                representative = process_value.get("representative")
                expected_representative = process_representative(samples)
                if representative != expected_representative:
                    violations.append(f"{process_label}: representative is not nearest-rank p50")
                representatives.append(expected_representative)
                raw.extend(samples)
        if len(representatives) != len(processes):
            continue
        if mode == "startup-marker":
            if any(
                field in group
                for field in ("actual_backend", "backend_identity", "adapter_identity")
            ):
                violations.append(f"{group_label}: startup CPU bootstrap cannot report GPU backend identity")
            if group.get("final_renderer") != "cpu":
                violations.append(f"{group_label}: startup final renderer must be CPU")
        else:
            if group.get("stabilization_ms") != 5_000 or group.get("sample_interval_ms") != 100:
                violations.append(f"{group_label}: residence stabilization/interval contract mismatch")
            if group.get("owner_ready_marker") != policy.get("owner_ready_marker"):
                violations.append(f"{group_label}: owner-specific ready marker mismatch")
            if "requested_backend" in policy:
                actual = group.get("actual_backend")
                if actual not in contract["windows_backends"]["diagnostic_only"]:
                    violations.append(f"{group_label}: actual product backend identity is missing or unsupported")
        statistics = recomputed_statistics(representatives, raw)
        if "statistics" in group:
            validate_statistics(group.get("statistics"), statistics, group_label, violations)
        raw_groups.append(
            {
                "name": group.get("name"),
                "status": matrix_status or "supported",
                "unsupported_reason": None,
                "unsupported_at_stage": None,
                "process_ids": process_ids,
                "representatives": representatives,
                "raw": raw,
                "reported_statistics": group.get("statistics"),
                "lkg": group.get("lkg"),
                "identity": artifact.get("identity"),
                "warmup_process_ids": tuple(group_warmup_ids),
                "artifact_warmup_process_ids": tuple(warmup_ids),
                "font_processes": font_processes,
                "protocol_fields": (
                    artifact.get("warmups"),
                    artifact.get("measured_cold_processes"),
                    artifact.get("timeout_seconds"),
                ),
                "protocol": artifact.get("protocol"),
                "metadata": {
                    field: group.get(field)
                    for field in (
                        "metric",
                        "sampling_mode",
                        "requested_backend",
                        "final_renderer",
                        "actual_backend",
                        "adapter_identity",
                        "connection_state",
                        "owner_ready_marker",
                        "stabilization_ms",
                        "sample_interval_ms",
                        "support_status",
                    )
                },
            }
        )
    if font_resource_evidence and warmup_ids:
        warmup_union = set().union(*font_group_warmups) if font_group_warmups else set()
        if (
            len(font_group_warmups) != len(policy.get("required_groups", []))
            or sum(len(group_ids) for group_ids in font_group_warmups) != len(warmup_union)
            or warmup_union != set(warmup_ids)
        ):
            violations.append(
                f"{label}: per-mode warmup cohorts must be disjoint and their union must equal the 15 top-level run IDs"
            )
    return raw_groups


def validate_font_ownership_cross_mode_content(
    artifact_type: str,
    groups: dict[str, dict[str, Any]],
    artifact_warmup_process_ids: tuple[str, ...] | None,
    violations: list[str],
) -> None:
    expected_names = (
        "current-copied/ascii",
        "shared-all/ascii",
        "lazy/ascii",
    )
    if any(name not in groups for name in expected_names):
        return
    group_warmups = [set(groups[name]["warmup_process_ids"]) for name in expected_names]
    warmup_union = set().union(*group_warmups)
    if (
        artifact_warmup_process_ids is None
        or any(len(group) != 5 for group in group_warmups)
        or sum(len(group) for group in group_warmups) != len(warmup_union)
        or warmup_union != set(artifact_warmup_process_ids)
    ):
        violations.append(
            f"artifact_type {artifact_type}: three disjoint five-process warmup cohorts must cover the 15 actual run IDs"
        )
    by_mode: dict[str, dict[int, dict[str, Any]]] = {}
    for name in expected_names:
        rounds: dict[int, dict[str, Any]] = {}
        for record in groups[name]["font_processes"]:
            round_index = record["round_index"]
            if round_index in rounds:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: duplicate font proof round_index"
                )
            else:
                rounds[round_index] = record["font_resources"]
        if set(rounds) != set(range(1, 31)):
            violations.append(
                f"artifact_type {artifact_type} group {name}: font resource evidence must cover rounds 1 through 30 exactly once"
            )
        by_mode[name] = rounds
    current = by_mode["current-copied/ascii"]
    shared = by_mode["shared-all/ascii"]
    for round_index in sorted(set(current).intersection(shared)):
        current_resources = current[round_index]
        shared_resources = shared[round_index]
        for field in (
            "indexed_source_count",
            "active_source_count",
            "index_fingerprint_sha256",
            "catalog_fingerprint_sha256",
            "ordered_catalog_fingerprint_sha256",
            "font_inventory_fingerprint_sha256",
            "font_index_policy_version",
        ):
            missing_modes = [
                mode
                for mode, resources in (
                    ("CurrentCopied", current_resources),
                    ("SharedAll", shared_resources),
                )
                if field not in resources
            ]
            if missing_modes:
                violations.append(
                    f"artifact_type {artifact_type} round {round_index}: "
                    f"{'/'.join(missing_modes)} missing required {field}"
                )
                continue
            if current_resources.get(field) != shared_resources.get(field):
                violations.append(
                    f"artifact_type {artifact_type} round {round_index}: CurrentCopied/SharedAll {field} differs"
                )
        current_retained = current_resources.get("retained_source_bytes")
        shared_retained = shared_resources.get("retained_source_bytes")
        if (
            isinstance(current_retained, int)
            and not isinstance(current_retained, bool)
            and isinstance(shared_retained, int)
            and not isinstance(shared_retained, bool)
            and current_retained != 2 * shared_retained
        ):
            violations.append(
                f"artifact_type {artifact_type} round {round_index}: CurrentCopied retained bytes must equal exactly twice SharedAll"
            )


def combine_metric_shards(
    artifact_type: str,
    shards: list[tuple[str, list[dict[str, Any]]]],
    policy: dict[str, Any],
    contract: dict[str, Any],
    violations: list[str],
) -> list[dict[str, int | float]]:
    groups: dict[str, dict[str, Any]] = {}
    artifact_warmup_process_ids: tuple[str, ...] | None = None
    artifact_protocol_fields: tuple[Any, ...] | None = None
    artifact_protocol: dict[str, Any] | None = None
    for artifact_id, raw_groups in shards:
        for group in raw_groups:
            candidate_artifact_warmups = group.get(
                "artifact_warmup_process_ids", group["warmup_process_ids"]
            )
            if artifact_warmup_process_ids is None:
                artifact_warmup_process_ids = candidate_artifact_warmups
                artifact_protocol_fields = group["protocol_fields"]
                artifact_protocol = group["protocol"]
            else:
                if artifact_warmup_process_ids != candidate_artifact_warmups:
                    violations.append(
                        f"artifact_type {artifact_type}: every shard and group must share one exact top-level warmup cohort"
                    )
                if artifact_protocol_fields != group["protocol_fields"]:
                    violations.append(
                        f"artifact_type {artifact_type}: protocol fields drift across groups or shards"
                    )
                if artifact_protocol != group["protocol"]:
                    violations.append(
                        f"artifact_type {artifact_type}: full protocol drifts across groups or shards"
                    )
            name = group.get("name")
            if not isinstance(name, str) or not name:
                violations.append(f"artifact {artifact_id}: metric group name must be non-empty")
                continue
            combined = groups.setdefault(
                name,
                {
                    "metadata": group["metadata"],
                    "process_ids": set(),
                    "representatives": [],
                    "raw": [],
                    "reported_statistics": [],
                    "lkg": [],
                    "identity": group["identity"],
                    "warmup_process_ids": group["warmup_process_ids"],
                    "artifact_warmup_process_ids": candidate_artifact_warmups,
                    "font_processes": [],
                    "protocol_fields": group["protocol_fields"],
                    "protocol": group["protocol"],
                    "status": group["status"],
                    "unsupported_reason": group["unsupported_reason"],
                    "unsupported_at_stage": group["unsupported_at_stage"],
                    "outcome_records": 0,
                },
            )
            if combined["status"] != group["status"]:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: supported/unsupported shard outcome drift"
                )
            if (
                combined["unsupported_reason"] != group["unsupported_reason"]
                or combined["unsupported_at_stage"] != group["unsupported_at_stage"]
            ):
                violations.append(
                    f"artifact_type {artifact_type} group {name}: unsupported diagnostic identity drift"
                )
            combined["outcome_records"] += 1
            if combined["metadata"] != group["metadata"]:
                violations.append(f"artifact_type {artifact_type} group {name}: shard metadata identity drift")
            if cohort_identity(combined["identity"]) != cohort_identity(group["identity"]):
                violations.append(f"artifact_type {artifact_type} group {name}: candidate source/binary/runner cohort drift")
            if combined["warmup_process_ids"] != group["warmup_process_ids"]:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: raw shards do not share the exact five warmup identities"
                )
            if combined["protocol_fields"] != group["protocol_fields"]:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: raw shard protocol fields drift"
                )
            if combined["protocol"] != group["protocol"]:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: raw shard full protocol drift"
                )
            overlap = combined["process_ids"].intersection(group["process_ids"])
            if overlap:
                violations.append(f"artifact_type {artifact_type} group {name}: duplicate process IDs across raw shards")
            combined["process_ids"].update(group["process_ids"])
            combined["representatives"].extend(group["representatives"])
            combined["raw"].extend(group["raw"])
            combined["font_processes"].extend(group.get("font_processes", []))
            if group["reported_statistics"] is not None:
                combined["reported_statistics"].append(group["reported_statistics"])
            if group["lkg"] is not None:
                combined["lkg"].append(group["lkg"])
    required_groups = policy.get("required_groups")
    if "matrix_stages" in policy:
        required_groups = [
            f"{backend}/{stage}"
            for backend in policy["matrix_backends"]
            for stage in policy["matrix_stages"]
        ]
    if required_groups is not None and set(groups) != set(required_groups):
        violations.append(f"artifact_type {artifact_type}: raw group inventory differs from the frozen contract")
    if "matrix_stages" in policy:
        stages = policy["matrix_stages"]
        for backend in policy["matrix_backends"]:
            outcomes = [groups.get(f"{backend}/{stage}") for stage in stages]
            if any(outcome is None for outcome in outcomes):
                continue
            statuses = [outcome["status"] for outcome in outcomes]
            if backend == "auto":
                if any(status != "supported" for status in statuses):
                    violations.append(
                        f"artifact_type {artifact_type}: required auto product matrix cannot be unsupported"
                    )
                continue
            if statuses[0] != "supported":
                violations.append(
                    f"artifact_type {artifact_type}: diagnostic {backend} cpu-window must be supported"
                )
            if "unsupported" in statuses:
                first = statuses.index("unsupported")
                if statuses != ["supported"] * first + ["unsupported"] * (
                    len(stages) - first
                ):
                    violations.append(
                        f"artifact_type {artifact_type}: diagnostic {backend} unsupported outcome must be a suffix"
                    )
                suffix = outcomes[first:]
                reasons = {outcome["unsupported_reason"] for outcome in suffix}
                stopped_at = {outcome["unsupported_at_stage"] for outcome in suffix}
                if len(reasons) != 1 or stopped_at != {stages[first]}:
                    violations.append(
                        f"artifact_type {artifact_type}: diagnostic {backend} unsupported suffix identity drift"
                    )
    if policy.get("font_resource_evidence") is True:
        validate_font_ownership_cross_mode_content(
            artifact_type, groups, artifact_warmup_process_ids, violations
        )
    results: list[dict[str, int | float]] = []
    group_order = required_groups or sorted(groups)
    for name in group_order:
        if name not in groups:
            continue
        combined = groups[name]
        if combined["status"] == "unsupported":
            if combined["outcome_records"] != 1:
                violations.append(
                    f"artifact_type {artifact_type} group {name}: unsupported diagnostic cell must appear exactly once"
                )
            continue
        representatives = combined["representatives"]
        raw = combined["raw"]
        if len(representatives) != 30 or len(combined["process_ids"]) != 30:
            violations.append(f"artifact_type {artifact_type} group {name}: exactly 30 distinct process-cold records are required")
            continue
        if combined["process_ids"].intersection(combined["warmup_process_ids"]):
            violations.append(
                f"artifact_type {artifact_type} group {name}: measured process IDs overlap warmups"
            )
        statistics = recomputed_statistics(representatives, raw)
        for reported in combined["reported_statistics"]:
            if reported != statistics:
                violations.append(f"artifact_type {artifact_type} group {name}: reported statistics do not match the complete raw cohort")
        if policy.get("same_machine_lkg"):
            lkg_records = combined["lkg"]
            if len(lkg_records) != 1:
                violations.append(f"artifact_type {artifact_type} group {name}: exactly one immutable LKG cohort is required")
            else:
                validate_lkg(
                    lkg_records[0],
                    statistics,
                    policy,
                    combined["metadata"],
                    combined["identity"],
                    contract,
                    f"artifact_type {artifact_type} group {name}",
                    violations,
                )
        elif combined["lkg"]:
            violations.append(f"artifact_type {artifact_type} group {name}: undeclared LKG samples are forbidden")
        validate_thresholds(
            statistics,
            policy,
            contract,
            f"artifact_type {artifact_type} group {name}",
            violations,
        )
        results.append(statistics)
    return results


def epoch_value(manifest: dict[str, Any], path: str) -> Any:
    value: Any = manifest
    for component in path.split("."):
        if not isinstance(value, dict):
            return None
        value = value.get(component)
    return value


def validate_claim_rule(
    name: str,
    value: Any,
    rule: dict[str, Any],
    manifest: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    kind = rule.get("kind")
    expected = rule.get("value")
    valid = False
    if kind == "exact":
        valid = json_equal(value, expected)
    elif kind == "exact-set":
        valid = isinstance(value, list) and set(value) == set(expected) and len(value) == len(set(value))
    elif kind == "full-sha":
        valid = is_full_sha(value)
    elif kind == "sha256":
        valid = is_sha256(value)
    elif kind == "non-empty-string":
        valid = isinstance(value, str) and bool(value.strip())
    elif kind == "non-empty-list":
        valid = isinstance(value, list) and bool(value)
    elif kind == "integer-min":
        valid = isinstance(value, int) and not isinstance(value, bool) and value >= expected
    elif kind == "number-max":
        valid = numeric(value) and value <= expected
    elif kind == "https-url":
        valid = isinstance(value, str) and value.startswith("https://") and len(value) > len("https://")
    elif kind == "epoch-ref":
        expected = epoch_value(manifest, rule.get("path", ""))
        valid = is_full_sha(expected) and value == expected
    if not valid:
        violations.append(f"{label}: claim {name} does not satisfy {kind}")


def validate_runner_fingerprint(
    artifact: dict[str, Any], label: str, violations: list[str]
) -> None:
    source = artifact.get("source")
    if source not in {"host-probe", "fixture"}:
        violations.append(f"{label}: runner source must be host-probe or fixture")
    if artifact.get("certification_eligible") is True and source != "host-probe":
        violations.append(f"{label}: certification requires host-probe runner evidence")
    if artifact.get("complete") is not True:
        violations.append(f"{label}: runner fingerprint must be complete")
    for field in ("producer_script_sha256", "collector_script_sha256"):
        if not is_sha256(artifact.get(field)):
            violations.append(f"{label}: {field} must be a SHA-256")
    if artifact.get("collector_timeout_seconds") != 60:
        violations.append(
            f"{label}: collector timeout must be the 60-second process contract"
        )

    fields = artifact.get("fields")
    if not isinstance(fields, dict):
        violations.append(f"{label}: complete normalized runner fields are required")
        return
    top_fields = {
        "os",
        "gpu",
        "memory",
        "displays",
        "power_plan",
        "session",
        "locale",
        "fonts",
        "cold_cache_policy",
    }
    reject_unknown_fields(fields, top_fields, f"{label} fields", violations)
    if set(fields) != top_fields:
        violations.append(f"{label}: runner fields are incomplete")

    def exact_object(name: str, expected: set[str]) -> dict[str, Any]:
        value = fields.get(name)
        if not isinstance(value, dict):
            violations.append(f"{label}: fields.{name} must be an object")
            return {}
        reject_unknown_fields(value, expected, f"{label} fields.{name}", violations)
        if set(value) != expected:
            violations.append(f"{label}: fields.{name} is incomplete")
        return value

    os_fields = exact_object(
        "os", {"version", "build_number", "build_revision", "architecture"}
    )
    if any(
        not isinstance(os_fields.get(name), str) or not os_fields[name].strip()
        for name in ("version", "build_number", "architecture")
    ) or (
        not isinstance(os_fields.get("build_revision"), int)
        or isinstance(os_fields.get("build_revision"), bool)
        or os_fields.get("build_revision", -1) < 0
    ):
        violations.append(f"{label}: fields.os contains invalid normalized values")
    if os_fields.get("architecture") not in {"x86", "x86_64", "arm", "arm64"}:
        violations.append(f"{label}: fields.os architecture must be a stable ASCII enum")

    gpu = exact_object(
        "gpu", {"vendor_id", "device_id", "driver_version", "wddm_version"}
    )
    if any(
        not isinstance(gpu.get(name), int)
        or isinstance(gpu.get(name), bool)
        or gpu.get(name, 0) <= 0
        for name in ("vendor_id", "device_id")
    ) or any(
        not isinstance(gpu.get(name), str) or not gpu[name].strip()
        for name in ("driver_version", "wddm_version")
    ):
        violations.append(f"{label}: fields.gpu contains invalid selected-adapter values")
    if not isinstance(gpu.get("driver_version"), str) or re.fullmatch(
        r"[0-9]+(?:\.[0-9]+){1,3}", gpu.get("driver_version", "")
    ) is None:
        violations.append(f"{label}: fields.gpu driver version is not normalized")
    if not isinstance(gpu.get("wddm_version"), str) or re.fullmatch(
        r"WDDM [0-9]+\.[0-9]+", gpu.get("wddm_version", "")
    ) is None:
        violations.append(f"{label}: fields.gpu WDDM version is not normalized")

    memory = exact_object("memory", {"physical_bytes", "pagefile_mode"})
    if (
        not isinstance(memory.get("physical_bytes"), int)
        or isinstance(memory.get("physical_bytes"), bool)
        or memory.get("physical_bytes", 0) <= 0
        or memory.get("pagefile_mode")
        not in {"automatic-managed", "manual", "disabled"}
    ):
        violations.append(f"{label}: fields.memory contains invalid values")

    displays = fields.get("displays")
    normalized_displays: list[dict[str, Any]] = []
    display_keys = {"width_px", "height_px", "dpi_x", "dpi_y", "primary"}
    if not isinstance(displays, list) or not displays:
        violations.append(f"{label}: fields.displays must cover every active display")
    else:
        for index, display in enumerate(displays):
            if not isinstance(display, dict):
                violations.append(f"{label}: fields.displays[{index}] must be an object")
                continue
            reject_unknown_fields(
                display, display_keys, f"{label} fields.displays[{index}]", violations
            )
            if set(display) != display_keys or any(
                not isinstance(display.get(name), int)
                or isinstance(display.get(name), bool)
                or display.get(name, 0) <= 0
                for name in ("width_px", "height_px", "dpi_x", "dpi_y")
            ) or not isinstance(display.get("primary"), bool):
                violations.append(f"{label}: fields.displays[{index}] is invalid")
                continue
            normalized_displays.append(display)
        expected_order = sorted(
            normalized_displays,
            key=lambda item: (
                not item["primary"],
                item["width_px"],
                item["height_px"],
                item["dpi_x"],
                item["dpi_y"],
            ),
        )
        if normalized_displays != expected_order:
            violations.append(f"{label}: fields.displays must be canonically sorted")
        if sum(1 for display in normalized_displays if display["primary"]) != 1:
            violations.append(f"{label}: fields.displays must contain exactly one primary")

    power = exact_object("power_plan", {"guid"})
    if not isinstance(power.get("guid"), str) or re.fullmatch(
        r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        power.get("guid", ""),
    ) is None:
        violations.append(f"{label}: fields.power_plan.guid must be a normalized GUID")
    session = exact_object("session", {"kind"})
    if session.get("kind") not in {"local", "remote"}:
        violations.append(f"{label}: fields.session.kind must be local or remote")
    locale = exact_object("locale", {"culture", "ui_culture", "system_locale"})
    if any(
        not isinstance(locale.get(name), str) or not locale[name].strip()
        for name in ("culture", "ui_culture", "system_locale")
    ):
        violations.append(f"{label}: fields.locale must be complete")
    fonts = exact_object(
        "fonts", {"inventory_fingerprint_sha256", "index_policy_version"}
    )
    if not is_sha256(fonts.get("inventory_fingerprint_sha256")) or (
        not isinstance(fonts.get("index_policy_version"), int)
        or isinstance(fonts.get("index_policy_version"), bool)
        or fonts.get("index_policy_version", 0) <= 0
    ):
        violations.append(f"{label}: fields.fonts must bind inventory and policy")
    cache = exact_object(
        "cold_cache_policy", {"process_cold_start", "os_file_cache"}
    )
    if cache != {
        "process_cold_start": True,
        "os_file_cache": "unmodified-no-explicit-flush",
    }:
        violations.append(f"{label}: fields.cold_cache_policy overstates cache handling")

    fingerprint = artifact.get("fingerprint_sha256")
    try:
        expected_fingerprint = runner_canonical_sha256(fields)
    except TypeError as error:
        expected_fingerprint = None
        violations.append(f"{label}: {error}")
    if not is_sha256(fingerprint) or fingerprint != expected_fingerprint:
        violations.append(f"{label}: canonical runner fingerprint does not match fields")
    identity = artifact.get("identity")
    if isinstance(identity, dict) and "runner_fingerprint_sha256" in identity:
        if identity.get("runner_fingerprint_sha256") != fingerprint:
            violations.append(
                f"{label}: optional embedded runner identity does not match canonical fingerprint"
            )


def validate_external_source_evidence(
    artifact: dict[str, Any],
    artifact_type: str,
    label: str,
    violations: list[str],
) -> None:
    """Validate the optional rich evidence emitted by the Stage 8 proof tool.

    Historical local proof fixtures intentionally contain only the frozen minimal
    fields.  Rich fields are therefore validated as a closed bundle only when the
    ``mode`` discriminator is present.
    """

    if "mode" not in artifact:
        return
    mode = artifact.get("mode")
    expected_mode = "synthesize" if artifact_type == "local-two-bare-git-source-proof" else "canonical"
    if mode != expected_mode:
        violations.append(f"{label}: external-source proof mode does not match artifact type")
    for field in (
        "candidate_ref",
        "source_switch_ref",
        "rollback_ref",
        "candidate_tree_sha256",
        "metadata_sha256",
        "metadata_raw_sha256",
        "lockfile_sha256",
    ):
        value = artifact.get(field)
        valid = is_full_sha(value) if field.endswith("_ref") else is_sha256(value)
        if not valid:
            violations.append(f"{label}: rich source proof field {field} must be an immutable digest")
    source_refs = artifact.get("source_refs")
    if not isinstance(source_refs, list) or len(source_refs) != 2 or not all(is_full_sha(item) for item in source_refs):
        violations.append(f"{label}: rich source proof source_refs must contain two full SHAs")
    if artifact.get("immutable") is not True or artifact.get("bare_repository_count") != 2:
        violations.append(f"{label}: rich source proof must bind two immutable bare repositories")
    remotes = artifact.get("bare_repositories")
    if not isinstance(remotes, dict) or set(remotes) != {"candidate", "consumer"}:
        violations.append(f"{label}: rich source proof must identify candidate and consumer remotes")
    else:
        identities: set[str] = set()
        for role, value in remotes.items():
            if not isinstance(value, dict) or not isinstance(value.get("identity"), str) or not is_sha256(value["identity"]):
                violations.append(f"{label}: bare repository {role} identity must be a SHA-256")
            else:
                identities.add(value["identity"])
        if len(identities) != 2:
            violations.append(f"{label}: candidate and consumer bare repositories must be distinct")
    commands = artifact.get("commands")
    switch_index = artifact.get("source_switch_command_count")
    if not isinstance(commands, list) or not commands or not isinstance(switch_index, int) or not 0 <= switch_index < len(commands):
        violations.append(f"{label}: rich source proof command boundary is invalid")
    else:
        for index, command in enumerate(commands):
            if not isinstance(command, dict) or not isinstance(command.get("argv"), list):
                violations.append(f"{label}: command {index} is not a closed command record")
                continue
            argv = command["argv"]
            if index >= switch_index and "--locked" not in argv:
                violations.append(f"{label}: post-switch command {index} is not --locked")
            if index >= switch_index and "generate-lockfile" in argv:
                violations.append(f"{label}: post-switch cargo generate-lockfile is forbidden")
    metadata = artifact.get("metadata")
    sources = metadata.get("rterm_sources") if isinstance(metadata, dict) else None
    candidate_ref = artifact.get("candidate_ref")
    if not isinstance(sources, dict) or len(sources) != 7 or not all(
        isinstance(value, str) and value.startswith("git+") and is_full_sha(candidate_ref) and f"#{candidate_ref}" in value
        for value in sources.values()
    ):
        violations.append(f"{label}: metadata must bind all seven R-Term packages to candidate_ref")
    vendors = artifact.get("vendor_resolutions")
    consumer_root = artifact.get("consumer_root")
    if not isinstance(vendors, dict) or set(vendors) != {"glyphon", "gpu-allocator"} or not isinstance(consumer_root, str):
        violations.append(f"{label}: vendor resolutions must contain both consumer-root packages")
    else:
        root_path = Path(consumer_root).resolve()
        for name, value in vendors.items():
            manifest_path = value.get("manifest_path") if isinstance(value, dict) else None
            if not isinstance(manifest_path, str):
                violations.append(f"{label}: vendor resolution {name} lacks a manifest path")
                continue
            try:
                Path(manifest_path).resolve().relative_to(root_path)
            except ValueError:
                violations.append(f"{label}: vendor resolution {name} escapes consumer_root")
    baseline = artifact.get("baseline")
    rollback = artifact.get("rollback")
    if not isinstance(baseline, dict) or not isinstance(rollback, dict):
        violations.append(f"{label}: baseline and rollback hashes are required")
    elif baseline.get("manifest_sha256") != rollback.get("manifest_sha256") or baseline.get("lockfile_sha256") != rollback.get("lockfile_sha256"):
        violations.append(f"{label}: rollback does not restore baseline lockfile and manifest hashes")
    if mode == "canonical" and artifact.get("r1_ref") != candidate_ref:
        violations.append(f"{label}: canonical r1_ref must equal candidate_ref")


def validate_result_artifact(
    artifact_type: str,
    artifact: dict[str, Any],
    contract: dict[str, Any],
    manifest: dict[str, Any],
    root: Path,
    label: str,
    violations: list[str],
) -> None:
    allowed = {"schema", "identity", "ok", "proof", "claims"}
    policy = contract["artifact_policies"].get(artifact_type, {})
    if policy.get("certification_eligible") is True:
        allowed.add("certification_eligible")
    allowed.update(
        {
            "runner-fingerprint": {
                "source",
                "complete",
                "fields",
                "fingerprint_sha256",
                "producer_script_sha256",
                "collector_script_sha256",
                "collector_timeout_seconds",
            },
            "font-catalog-fingerprint": {
                "catalog_fingerprint_sha256",
                "functional_specimens",
            },
            "local-two-bare-git-source-proof": {
                "bare_repository_count",
                "source_refs",
                "immutable",
                "git_object_store_proof",
                "mode",
                "candidate_ref",
                "candidate_tree_sha256",
                "source_switch_ref",
                "rollback_ref",
                "candidate_repository",
                "consumer_repository",
                "consumer_root",
                "consumer_workspace",
                "consumer_manifest",
                "consumer_lockfile",
                "bare_repositories",
                "baseline",
                "source_switch",
                "rollback",
                "source_switch_command_count",
                "commands",
                "metadata",
                "metadata_sha256",
                "metadata_raw_sha256",
                "vendor_resolutions",
                "worktree_hashes",
                "lockfile_sha256",
                "post_commit_cargo_generate_lockfile",
                "bare_remote_count",
            },
            "rterm-extraction-manifest": {"owned_projection_inventory"},
            "source-to-filtered-history-map": {
                "commit_map_proof",
                "bootstrap_projection_proof",
                "git_object_store_proof",
            },
            "rterm-external-source-proof": {
                "source_to_filtered_map_sha256",
                "tree_projection_sha256",
                "bootstrap_projection_sha256",
                "git_object_store_proof",
                "immutable",
                "mode",
                "candidate_ref",
                "r1_ref",
                "candidate_tree_sha256",
                "source_switch_ref",
                "rollback_ref",
                "source_refs",
                "candidate_repository",
                "consumer_repository",
                "consumer_root",
                "consumer_workspace",
                "consumer_manifest",
                "consumer_lockfile",
                "bare_repositories",
                "baseline",
                "source_switch",
                "rollback",
                "source_switch_command_count",
                "commands",
                "metadata",
                "metadata_sha256",
                "metadata_raw_sha256",
                "vendor_resolutions",
                "worktree_hashes",
                "lockfile_sha256",
                "post_commit_cargo_generate_lockfile",
                "bare_remote_count",
                "bare_repository_count",
            },
            "windows-release-build-provenance": {"profile", "locked"},
            "windows-loopback-native-ssh": {"coverage"},
            "windows-secret-scan": {"hits", "scopes"},
        }.get(artifact_type, set())
    )
    reject_unknown_fields(artifact, allowed, label, violations)
    if policy.get("certification_eligible") is True and artifact.get(
        "certification_eligible"
    ) is not True:
        violations.append(f"{label}: certification_eligible must be true")
    if artifact.get("schema") != RESULT_SCHEMA or artifact.get("ok") is not True:
        violations.append(f"{label}: result artifact must be a successful {RESULT_SCHEMA} object")
        return
    if artifact.get("proof") != artifact_type:
        violations.append(f"{label}: result proof discriminator must equal artifact_type")
    claims = artifact.get("claims")
    rules = contract["result_claims"].get(artifact_type, {})
    if not isinstance(claims, dict) or set(claims) != set(rules):
        violations.append(f"{label}: type-specific claim set is incomplete or contains unknown claims")
    else:
        for name, rule in rules.items():
            validate_claim_rule(name, claims.get(name), rule, manifest, label, violations)
    validate_external_source_evidence(artifact, artifact_type, label, violations)
    if artifact_type == "runner-fingerprint":
        validate_runner_fingerprint(artifact, label, violations)
    elif artifact_type == "font-catalog-fingerprint":
        validate_font_functional_specimens(artifact, contract, label, violations)
    elif artifact_type == "local-two-bare-git-source-proof":
        refs = artifact.get("source_refs")
        proof_source = artifact.get("identity", {}).get("source_sha")
        if artifact.get("bare_repository_count") != 2 or artifact.get("immutable") is not True:
            violations.append(f"{label}: immutable local two-bare-Git proof is incomplete")
        if not isinstance(refs, list) or len(refs) != 2 or not all(is_full_sha(item) for item in refs):
            violations.append(f"{label}: source proof refs must be two full immutable SHAs")
        elif set(refs) != {proof_source, contract["lkg_rssh_ref"]}:
            violations.append(
                f"{label}: source proof must bind the certified candidate and immutable R-SSH LKG"
            )
        repositories = artifact.get("git_object_store_proof")
        repositories_by_role = validate_git_object_store_proof(
            repositories,
            {
                "candidate": {
                    "candidate": proof_source,
                    "lkg_boundary": contract["lkg_rssh_ref"],
                },
                "lkg": {"lkg": contract["lkg_rssh_ref"]},
            },
            label,
            violations,
        )
        candidate_repository = repositories_by_role.get("candidate", {})
        lkg_repository = repositories_by_role.get("lkg", {})
        if candidate_repository.get("history_boundaries") != [contract["lkg_rssh_ref"]]:
            violations.append(f"{label}: candidate proof must use the frozen LKG as its boundary")
        if lkg_repository.get("history_boundaries") != [contract["lkg_rssh_ref"]]:
            violations.append(f"{label}: LKG proof must stop exactly at the frozen LKG")
        if not git_graph_is_ancestor(
            candidate_repository.get("commits", {}),
            contract["lkg_rssh_ref"],
            proof_source,
        ):
            violations.append(f"{label}: frozen LKG is not an ancestor of the certified candidate")
    elif artifact_type == "rterm-extraction-manifest":
        rssh = manifest.get("rssh", {})
        expected_mappings, inventory_digest = validate_owned_projection_inventory(
            artifact.get("owned_projection_inventory"),
            root,
            rssh.get("r0_ref") if isinstance(rssh, dict) else None,
            contract,
            label,
            violations,
        )
        expected_root_count = len(FROZEN_OWNED_PROJECTION_REQUIRED) + len(
            FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED
        )
        if claims.get("owned_path_count") != len(expected_mappings):
            violations.append(f"{label}: owned_path_count does not match the complete inventory")
        if claims.get("owned_root_count") != expected_root_count:
            violations.append(f"{label}: owned_root_count does not match the frozen roots")
        if claims.get("manifest_sha256") != inventory_digest:
            violations.append(f"{label}: manifest_sha256 does not bind the owned inventory")
    elif artifact_type == "source-to-filtered-history-map":
        rssh = manifest.get("rssh", {})
        rterm = manifest.get("rterm", {})
        repositories_by_role = validate_rterm_object_store(
            artifact.get("git_object_store_proof"), manifest, label, violations
        )
        filtered_ref = (
            rterm.get("filtered_boundary_ref") if isinstance(rterm, dict) else None
        )
        filtered_record = (
            repositories_by_role.get("rterm-filtered", {})
            .get("commits", {})
            .get(filtered_ref, {})
        )
        _source_tree, source_leaves = git_tree_identity_and_leaves(
            root,
            rssh.get("r0_ref") if isinstance(rssh, dict) else None,
            label,
            violations,
        )
        expected_owned_mappings = derive_owned_projection_mappings(
            source_leaves, contract, label, violations
        )
        expected_bootstrap_mappings = [
            mapping
            for mapping in expected_owned_mappings
            if mapping["source_path"].startswith(
                FROZEN_OWNED_PROJECTION_FUTURE_REQUIRED[0][0] + "/"
            )
        ]
        validate_bootstrap_projection_proof(
            artifact.get("bootstrap_projection_proof"),
            expected_bootstrap_mappings,
            repositories_by_role.get("rterm-filtered", {}),
            filtered_ref,
            rterm.get("r1_ref") if isinstance(rterm, dict) else None,
            label,
            violations,
        )
        _digest, mapping_count, _projection_digest, _extraction_digest = validate_commit_map_proof(
            artifact.get("commit_map_proof"),
            root,
            rssh.get("r0_ref") if isinstance(rssh, dict) else None,
            filtered_ref,
            filtered_record.get("tree"),
            label,
            violations,
        )
        if claims.get("mapping_count") != mapping_count:
            violations.append(f"{label}: claimed mapping_count does not match bounded raw records")
    elif artifact_type == "rterm-external-source-proof":
        if not is_sha256(artifact.get("source_to_filtered_map_sha256")):
            violations.append(f"{label}: source-to-filtered map digest is required")
        if not is_sha256(artifact.get("tree_projection_sha256")):
            violations.append(f"{label}: tree projection identity is required")
        validate_rterm_object_store(
            artifact.get("git_object_store_proof"), manifest, label, violations
        )
    elif artifact_type == "windows-release-build-provenance":
        if artifact.get("profile") != "release" or artifact.get("locked") is not True:
            violations.append(f"{label}: one locked Windows release-build provenance is required")
    elif artifact_type == "windows-loopback-native-ssh":
        expected = {
            "unknown-host-key",
            "changed-host-key",
            "secret-masking",
            "resize",
            "cancel",
            "disconnect",
            "reconnect",
        }
        if set(artifact.get("coverage", [])) != expected:
            violations.append(f"{label}: loopback SSH coverage is incomplete")
    elif artifact_type == "windows-secret-scan":
        expected = {"stdout", "stderr", "markers", "json", "session-log", "snapshot"}
        if artifact.get("hits") != 0 or set(artifact.get("scopes", [])) != expected:
            violations.append(f"{label}: secret scan must have zero hits over every required scope")


def recursively_reject_path_keys(value: Any, label: str, violations: list[str]) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if "path" in key.casefold():
                violations.append(f"{label}: functional specimen path key is forbidden: {key}")
            recursively_reject_path_keys(child, label, violations)
    elif isinstance(value, list):
        for child in value:
            recursively_reject_path_keys(child, label, violations)


def validate_font_functional_specimens(
    artifact: dict[str, Any],
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    specimens = artifact.get("functional_specimens")
    if not isinstance(specimens, list) or len(specimens) != 6:
        violations.append(f"{label}: exactly six functional specimens are required")
        return
    recursively_reject_path_keys(specimens, label, violations)
    expected_pairs = [
        (mode, specimen)
        for mode in ("current", "shared", "lazy")
        for specimen in ("cjk", "emoji")
    ]
    fields = {
        "requested_font_mode",
        "actual_font_mode",
        "requested_font_specimen",
        "actual_font_specimen",
        "requested_backend",
        "actual_backend",
        "activation_latency_ms",
        "activation_latency_gate",
        "retained_source_bytes",
        "recovery_retained_source_bytes",
        "indexed_source_count",
        "active_source_count",
        "initial_catalog_source_count",
        "catalog_builds",
        "generation",
        "recovery_generation",
        "frame_catalog_generation",
        "frame_generation_consistent",
        "tofu_count",
        "index_fingerprint_sha256",
        "catalog_fingerprint_sha256",
        "ordered_catalog_fingerprint_sha256",
        "font_inventory_fingerprint_sha256",
        "font_index_policy_version",
    }
    valid_backends = set(contract.get("windows_backends", {}).get("diagnostic_only", []))
    actual_backends: set[Any] = set()
    derived = {
        "functional_specimen_count": len(specimens),
        "zero_tofu": True,
        "single_frame_generation": True,
        "recovery_retained_bytes_stable": True,
        "same_actual_backend": True,
        "activation_latency_report_only": True,
    }
    for index, (record, (mode, specimen)) in enumerate(zip(specimens, expected_pairs)):
        record_label = f"{label} functional specimen {index}"
        if not isinstance(record, dict):
            violations.append(f"{record_label}: specimen must be an object")
            continue
        reject_unknown_fields(record, fields, record_label, violations)
        if (
            record.get("requested_font_mode") != mode
            or record.get("actual_font_mode") != mode
            or record.get("requested_font_specimen") != specimen
            or record.get("actual_font_specimen") != specimen
        ):
            violations.append(f"{record_label}: mode/specimen fallback or inventory drift")
        if record.get("requested_backend") != "auto":
            violations.append(f"{record_label}: requested backend must be auto")
        actual_backend = record.get("actual_backend")
        if isinstance(actual_backend, str):
            actual_backends.add(actual_backend)
        if not isinstance(actual_backend, str) or actual_backend not in valid_backends:
            violations.append(f"{record_label}: actual backend is missing or unsupported")
        latency = record.get("activation_latency_ms")
        if not numeric(latency):
            violations.append(f"{record_label}: activation latency must be finite and non-negative")
        if record.get("activation_latency_gate") != "report-only":
            derived["activation_latency_report_only"] = False
            violations.append(f"{record_label}: activation latency must remain report-only")
        tofu_count = record.get("tofu_count")
        if not isinstance(tofu_count, int) or isinstance(tofu_count, bool) or tofu_count != 0:
            derived["zero_tofu"] = False
            violations.append(f"{record_label}: tofu count must be zero")
        generation = record.get("generation")
        frame_generation = record.get("frame_catalog_generation")
        active = record.get("active_source_count")
        initial = record.get("initial_catalog_source_count")
        if (
            not isinstance(generation, int)
            or isinstance(generation, bool)
            or generation <= 0
            or not isinstance(record.get("catalog_builds"), int)
            or isinstance(record.get("catalog_builds"), bool)
            or record.get("catalog_builds") != generation
            or not isinstance(record.get("recovery_generation"), int)
            or isinstance(record.get("recovery_generation"), bool)
            or record.get("recovery_generation") != generation
            or not isinstance(active, int)
            or isinstance(active, bool)
            or not isinstance(initial, int)
            or isinstance(initial, bool)
            or not 1 <= initial <= active
            or generation != active - initial + 1
        ):
            violations.append(
                f"{record_label}: catalog builds/generation must match the independently recorded initial batch"
            )
        if mode == "shared" and not (initial == active and generation == 1):
            violations.append(f"{record_label}: SharedAll counter shape is invalid")
        if mode == "lazy" and not (initial == 1 and active == generation == 2):
            violations.append(f"{record_label}: Lazy activated specimen counter shape is invalid")
        if (
            not isinstance(frame_generation, int)
            or isinstance(frame_generation, bool)
            or frame_generation != generation
            or record.get("frame_generation_consistent") is not True
        ):
            derived["single_frame_generation"] = False
            violations.append(f"{record_label}: frame generation must equal the resource generation")
        retained = record.get("retained_source_bytes")
        if not isinstance(retained, int) or isinstance(retained, bool) or retained < 0:
            violations.append(f"{record_label}: retained source bytes must be a non-negative integer")
        recovery_retained = record.get("recovery_retained_source_bytes")
        if (
            not isinstance(recovery_retained, int)
            or isinstance(recovery_retained, bool)
            or recovery_retained != retained
        ):
            derived["recovery_retained_bytes_stable"] = False
            violations.append(f"{record_label}: recovery retained bytes duplicated or drifted")
        indexed = record.get("indexed_source_count")
        active = record.get("active_source_count")
        if (
            not isinstance(indexed, int)
            or isinstance(indexed, bool)
            or not isinstance(active, int)
            or isinstance(active, bool)
            or not indexed >= active > 0
        ):
            violations.append(f"{record_label}: indexed/active source counts must satisfy indexed >= active > 0")
        for fingerprint in (
            "index_fingerprint_sha256",
            "catalog_fingerprint_sha256",
            "ordered_catalog_fingerprint_sha256",
        ):
            if not is_sha256(record.get(fingerprint)):
                violations.append(f"{record_label}: {fingerprint} must be a SHA-256")
        if mode in {"current", "shared"}:
            if not is_sha256(record.get("font_inventory_fingerprint_sha256")):
                violations.append(
                    f"{record_label}: full font inventory fingerprint must be a SHA-256"
                )
            policy_version = record.get("font_index_policy_version")
            if (
                not isinstance(policy_version, int)
                or isinstance(policy_version, bool)
                or policy_version <= 0
            ):
                violations.append(
                    f"{record_label}: font index policy version must be positive"
                )
        elif any(
            field in record
            for field in (
                "font_inventory_fingerprint_sha256",
                "font_index_policy_version",
            )
        ):
            violations.append(
                f"{record_label}: Lazy evidence must not claim a full font inventory"
            )
    if len(actual_backends) != 1:
        derived["same_actual_backend"] = False
        violations.append(f"{label}: functional specimens must use one actual backend")
    claims = artifact.get("claims")
    if isinstance(claims, dict):
        for name, value in derived.items():
            if claims.get(name) != value:
                violations.append(f"{label}: claim {name} does not match derived functional evidence")
    fingerprint = artifact.get("catalog_fingerprint_sha256")
    expected_fingerprint = canonical_sha256(specimens)
    if fingerprint != expected_fingerprint:
        violations.append(f"{label}: canonical functional specimen digest mismatch")

    full_inventory_records = [
        record
        for record in specimens
        if isinstance(record, dict)
        and record.get("actual_font_mode") in {"current", "shared"}
    ]
    inventory_pairs = {
        (
            record.get("font_inventory_fingerprint_sha256"),
            record.get("font_index_policy_version"),
        )
        for record in full_inventory_records
    }
    if len(full_inventory_records) != 4 or len(inventory_pairs) != 1:
        violations.append(
            f"{label}: CurrentCopied/SharedAll functional evidence must bind one full font inventory"
        )


def validate_font_runner_inventory_binding(
    runner: dict[str, Any],
    raw_groups: list[dict[str, Any]],
    catalog: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    runner_fields = runner.get("fields")
    if not isinstance(runner_fields, dict):
        violations.append(f"{label}: complete runner fields are required for inventory binding")
        return
    runner_fonts = runner_fields.get("fonts")
    if not isinstance(runner_fonts, dict):
        violations.append(f"{label}: runner fields.fonts is required for inventory binding")
        return
    expected_fingerprint = runner_fonts.get("inventory_fingerprint_sha256")
    expected_policy = runner_fonts.get("index_policy_version")
    if not is_sha256(expected_fingerprint) or (
        not isinstance(expected_policy, int)
        or isinstance(expected_policy, bool)
        or expected_policy <= 0
    ):
        violations.append(
            f"{label}: runner font inventory digest and policy are required for binding"
        )
        return
    evidence: list[tuple[str, Any]] = []
    for group in raw_groups:
        if not isinstance(group, dict):
            continue
        processes = group.get("font_processes")
        if not isinstance(processes, list):
            continue
        for process in processes:
            if not isinstance(process, dict):
                continue
            resources = process.get("font_resources")
            if not isinstance(resources, dict):
                continue
            if resources.get("mode") in {"current", "shared"}:
                evidence.append(("raw", resources))
    specimens = catalog.get("functional_specimens")
    if isinstance(specimens, list):
        for record in specimens:
            if isinstance(record, dict) and record.get("actual_font_mode") in {
                "current",
                "shared",
            }:
                evidence.append(("functional", record))
    if not evidence:
        violations.append(f"{label}: no CurrentCopied/SharedAll inventory evidence is available")
        return
    for evidence_kind, resources in evidence:
        if (
            resources.get("font_inventory_fingerprint_sha256")
            != expected_fingerprint
            or resources.get("font_index_policy_version") != expected_policy
        ):
            violations.append(
                f"{label}: {evidence_kind} font inventory does not match runner fingerprint fields"
            )


def validate_font_ownership_reductions(
    statistics: list[dict[str, int | float]],
    policy: dict[str, Any],
    violations: list[str],
) -> None:
    reductions = policy.get("p50_reductions")
    if not isinstance(reductions, list):
        violations.append("font ownership aggregate is missing p50 reduction rules")
        return
    ordered_groups: list[str] = []
    for reduction in reductions:
        for field in ("minuend_group", "subtrahend_group"):
            group = reduction.get(field) if isinstance(reduction, dict) else None
            if isinstance(group, str) and group not in ordered_groups:
                ordered_groups.append(group)
    if len(statistics) != len(ordered_groups):
        violations.append("font ownership aggregate statistics do not match the frozen group inventory")
        return
    statistics_by_group = dict(zip(ordered_groups, statistics))
    for reduction in reductions:
        minuend = reduction["minuend_group"]
        subtrahend = reduction["subtrahend_group"]
        minimum = reduction["minimum_bytes"]
        observed = statistics_by_group[minuend]["p50"] - statistics_by_group[subtrahend]["p50"]
        if observed < minimum:
            violations.append(
                f"font ownership minimum p50 reduction violated for {minuend} -> {subtrahend}: {observed} < {minimum}"
            )


def validate_entry_shape(
    entry: Any,
    policies: dict[str, Any],
    label: str,
    violations: list[str],
) -> dict[str, Any] | None:
    if not isinstance(entry, dict):
        violations.append(f"{label}: entry must be an object")
        return None
    unknown = set(entry) - ENTRY_REQUIRED - ENTRY_OPTIONAL
    missing = ENTRY_REQUIRED - set(entry)
    if unknown:
        violations.append(f"{label}: entry has unknown fields: {sorted(unknown)}")
    if missing:
        violations.append(f"{label}: entry is missing required fields: {sorted(missing)}")
    artifact_type = entry.get("artifact_type")
    if artifact_type not in policies:
        violations.append(f"{label}: artifact_type is outside the closed inventory")
        return entry
    policy = policies[artifact_type]
    artifact_id = entry.get("artifact_id")
    if not isinstance(artifact_id, str) or not re.fullmatch(r"[a-z0-9][a-z0-9._/-]*", artifact_id):
        violations.append(f"{label}: artifact_id must be a stable relative identifier")
    expected_role = {
        "raw-metric": "raw",
        "aggregate": "aggregate",
        "result": "proof",
    }.get(policy.get("content_kind"))
    if entry.get("role") != expected_role:
        violations.append(f"{label}: role does not match artifact content kind")
    expected_payload_schema = {
        "raw-metric": RAW_SCHEMA,
        "aggregate": AGGREGATE_SCHEMA,
        "result": RESULT_SCHEMA,
    }.get(policy.get("content_kind"))
    if entry.get("payload_schema") != expected_payload_schema:
        violations.append(f"{label}: payload_schema does not match artifact content kind")
    if entry.get("scope") not in STATES[1:]:
        violations.append(f"{label}: scope must be a certifiable Stage 7 state")
    if not isinstance(entry.get("size_bytes"), int) or isinstance(entry.get("size_bytes"), bool) or entry["size_bytes"] < 1:
        violations.append(f"{label}: size_bytes must be a positive integer")
    argv = entry.get("producing_argv")
    if not isinstance(argv, list) or not argv or not all(isinstance(item, str) and item for item in argv):
        violations.append(f"{label}: producing_argv must be embedded as a non-empty string array")
    subject_refs = entry.get("subject_refs")
    if not isinstance(subject_refs, dict) or not all(
        isinstance(name, str)
        and re.fullmatch(r"(?:rssh|rterm)\.[a-z0-9_]+", name)
        and is_full_sha(value)
        for name, value in subject_refs.items()
    ):
        violations.append(f"{label}: subject_refs must contain only full immutable repository refs")
    children = entry.get("children")
    children_are_strings = isinstance(children, list) and all(
        isinstance(item, str) and re.fullmatch(r"[a-z0-9][a-z0-9._/-]*", item)
        for item in children
    )
    if not children_are_strings or len(children) != len(set(children)):
        violations.append(f"{label}: children must be a unique artifact_id array")
    elif expected_role != "aggregate" and children:
        violations.append(f"{label}: only aggregate entries may declare raw children")
    if not is_full_sha(entry.get("source_sha")):
        violations.append(f"{label}: source_sha must be a full immutable commit SHA")
    if not isinstance(entry.get("producing_command"), str) or not entry["producing_command"].strip():
        violations.append(f"{label}: producing_command must be embedded and non-empty")
    allowed_platforms = policy.get("platforms", [policy.get("platform")])
    if entry.get("platform") not in allowed_platforms:
        violations.append(f"{label}: platform does not match the artifact contract")
    if not isinstance(entry.get("run_id"), str) or re.fullmatch(
        r"[A-Za-z0-9][A-Za-z0-9._-]*", entry["run_id"]
    ) is None:
        violations.append(f"{label}: run_id must match the frozen manifest schema")
    if not is_sha256(entry.get("cohort_id")) or entry.get("cohort_id") != cohort_id(entry):
        violations.append(f"{label}: cohort_id does not bind the certification identity")
    if policy.get("binary_identity"):
        validate_binary_hashes(entry.get("binary_hashes"), label, violations)
    elif "binary_hashes" in entry:
        validate_binary_hashes(entry["binary_hashes"], label, violations)
    if policy.get("runner_identity"):
        if not is_sha256(entry.get("runner_fingerprint_sha256")):
            violations.append(f"{label}: runner fingerprint identity is required")
    elif "runner_fingerprint_sha256" in entry and not is_sha256(entry["runner_fingerprint_sha256"]):
        violations.append(f"{label}: runner fingerprint must be a SHA-256")
    if policy.get("certification_eligible") is True and entry.get(
        "certification_eligible"
    ) is not True:
        violations.append(f"{label}: certification_eligible must be true")
    if not is_sha256(entry.get("sha256")):
        violations.append(f"{label}: artifact SHA-256 must be embedded in the manifest")
    return entry


def entry_equivalent(
    current: dict[str, Any],
    current_base: Path,
    prior: dict[str, Any],
    prior_base: Path,
) -> bool:
    current_copy = {key: value for key, value in current.items() if key != "path"}
    prior_copy = {key: value for key, value in prior.items() if key != "path"}
    if current_copy != prior_copy:
        return False
    try:
        return (current_base / current["path"]).resolve() == (prior_base / prior["path"]).resolve()
    except (KeyError, TypeError):
        return False


def repository_root(contract_path: Path) -> Path:
    return contract_path.resolve().parents[2]


def clean_git_environment() -> dict[str, str]:
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.upper().startswith("GIT_")
    }
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    environment["GIT_CONFIG_NOSYSTEM"] = "1"
    environment["GIT_CONFIG_GLOBAL"] = os.devnull
    return environment


def path_control_fingerprint(path: Path, recursive: bool = False) -> tuple[Any, ...]:
    if not path.exists() and not path.is_symlink():
        return (str(path), "absent")
    if path.is_symlink():
        return (str(path), "symlink")
    status = path.stat()
    identity = (status.st_dev, status.st_ino)
    if path.is_file():
        if status.st_size > 4 * 1024 * 1024:
            return (str(path), "oversized", identity, status.st_size)
        return (str(path), "file", identity, hashlib.sha256(path.read_bytes()).hexdigest())
    if not path.is_dir():
        return (str(path), "special", identity)
    if not recursive:
        return (str(path), "directory", identity)
    children: list[tuple[Any, ...]] = []
    for index, child in enumerate(sorted(path.rglob("*"), key=lambda item: str(item).casefold())):
        if index >= 4096:
            return (str(path), "oversized-directory", identity)
        relative = child.relative_to(path).as_posix()
        if child.is_symlink():
            children.append((relative, "symlink"))
        elif child.is_file():
            child_status = child.stat()
            if child_status.st_size > 4 * 1024 * 1024:
                children.append((relative, "oversized", child_status.st_size))
            else:
                children.append(
                    (relative, "file", hashlib.sha256(child.read_bytes()).hexdigest())
                )
        elif child.is_dir():
            children.append((relative, "directory"))
        else:
            children.append((relative, "special"))
    return (str(path), "directory", identity, tuple(children))


def git_control_signature(root: Path, objects_path: Path) -> tuple[Any, ...]:
    common_dir = objects_path.parent
    root_marker = root / ".git"
    return (
        path_control_fingerprint(root_marker),
        path_control_fingerprint(common_dir),
        path_control_fingerprint(objects_path),
        path_control_fingerprint(common_dir / "shallow"),
        path_control_fingerprint(common_dir / "info" / "grafts"),
        path_control_fingerprint(objects_path / "info" / "alternates"),
        path_control_fingerprint(common_dir / "refs" / "replace", recursive=True),
        path_control_fingerprint(common_dir / "packed-refs"),
    )


def git_repository_has_history_overrides(root: Path) -> bool:
    root_key = str(root.resolve()).casefold()
    if root_key in _POISONED_GIT_ROOTS:
        return True
    cached = _SAFE_GIT_CONTROL_SNAPSHOTS.get(root_key)
    if cached is not None:
        objects_path, expected_signature = cached
        try:
            current_signature = git_control_signature(root, objects_path)
        except OSError:
            _POISONED_GIT_ROOTS.add(root_key)
            return True
        if current_signature != expected_signature:
            _POISONED_GIT_ROOTS.add(root_key)
            return True
        return False
    environment = clean_git_environment()
    git_path = subprocess.run(
        ["git", "rev-parse", "--git-path", "objects"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    if git_path.returncode != 0 or not git_path.stdout.strip():
        return True
    objects_path = Path(git_path.stdout.strip())
    if not objects_path.is_absolute():
        objects_path = root / objects_path
    git_dir = objects_path.parent
    control_paths = (
        git_dir / "shallow",
        git_dir / "info" / "grafts",
        objects_path / "info" / "alternates",
    )
    try:
        for path in control_paths:
            if path.is_symlink() or (path.exists() and path.stat().st_size > 0):
                return True
    except OSError:
        return True
    replacements = subprocess.run(
        ["git", "for-each-ref", "--format=%(refname)", "refs/replace"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    if replacements.returncode != 0 or bool(replacements.stdout.strip()):
        return True
    shallow = subprocess.run(
        ["git", "rev-parse", "--is-shallow-repository"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=environment,
    )
    if shallow.returncode != 0 or shallow.stdout.strip() != "false":
        return True
    try:
        signature = git_control_signature(root, objects_path)
    except OSError:
        return True
    _SAFE_GIT_CONTROL_SNAPSHOTS[root_key] = (objects_path, signature)
    return False


def git_commit_available(root: Path, commit: str) -> bool:
    if git_repository_has_history_overrides(root):
        return False
    result = subprocess.run(
        ["git", "cat-file", "-t", commit],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        env=clean_git_environment(),
    )
    return result.returncode == 0 and result.stdout.strip() == "commit"


def git_is_ancestor(root: Path, prior: str, current: str) -> bool:
    if git_repository_has_history_overrides(root):
        return False
    result = subprocess.run(
        ["git", "merge-base", "--is-ancestor", prior, current],
        cwd=root,
        check=False,
        capture_output=True,
        env=clean_git_environment(),
    )
    return result.returncode == 0


def validate_epoch_git_bindings(
    manifest: dict[str, Any],
    state: str,
    contract: dict[str, Any],
    root: Path,
    label: str,
    violations: list[str],
) -> None:
    state_index = STATES.index(state)
    if state_index < STATES.index("extraction-ready"):
        return
    if git_repository_has_history_overrides(root):
        violations.append(
            f"{label}: local Git repository has shallow, graft, replacement, alternate, or unreadable history controls"
        )
    certified_commit = manifest.get("certified_commit")
    rssh = manifest.get("rssh")
    rterm = manifest.get("rterm")
    if not isinstance(rssh, dict):
        return
    required = contract["epoch_requirements_by_state"][state]["rssh"]
    for field in required:
        value = rssh.get(field)
        if is_full_sha(value) and not git_commit_available(root, value):
            violations.append(f"{label}: rssh.{field} is not an available immutable commit")
    r0_ref = rssh.get("r0_ref")
    if all(is_full_sha(value) for value in (r0_ref, certified_commit)) and (
        r0_ref == certified_commit or not git_is_ancestor(root, r0_ref, certified_commit)
    ):
        violations.append(
            f"{label}: rssh.r0_ref must be a strict ancestor of the certified commit"
        )
    if state == "dual-source-verified" and rssh.get("r2_ref") != certified_commit:
        violations.append(f"{label}: rssh.r2_ref must equal the dual-source certified commit")
    if state == "split-complete":
        if rssh.get("r3_ref") != certified_commit:
            violations.append(f"{label}: rssh.r3_ref must equal the split certified commit")
        r2_ref = rssh.get("r2_ref")
        deletion_ref = rssh.get("r3_deletion_ref")
        r3_ref = rssh.get("r3_ref")
        if all(is_full_sha(value) for value in (r2_ref, deletion_ref)) and not git_is_ancestor(
            root, r2_ref, deletion_ref
        ):
            violations.append(f"{label}: rssh.r3_deletion_ref is not descended from rssh.r2_ref")
        if all(is_full_sha(value) for value in (deletion_ref, r3_ref)) and not git_is_ancestor(
            root, deletion_ref, r3_ref
        ):
            violations.append(f"{label}: rssh.r3_deletion_ref is not an ancestor of rssh.r3_ref")
    if isinstance(rterm, dict) and rterm.get("r1_ref") == contract["lkg_rssh_ref"]:
        violations.append(f"{label}: rterm.r1_ref must be distinct from the frozen R-SSH LKG")
    if isinstance(rterm, dict):
        for field in ("filtered_boundary_ref", "r1_ref"):
            value = rterm.get(field)
            if is_full_sha(value) and git_commit_available(root, value):
                violations.append(
                    f"{label}: rterm.{field} must belong to the bounded external object domain, not the R-SSH object database"
                )


def validate_epoch_shape(
    manifest: dict[str, Any],
    state: str,
    contract: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    requirements = contract["epoch_requirements_by_state"][state]
    for namespace in ("rssh", "rterm"):
        value = manifest.get(namespace)
        required = requirements[namespace]
        if required is None:
            if value is not None:
                violations.append(f"{label}: {namespace} epoch must remain null before it is certified")
            continue
        expected_fields = (
            {"r0_ref", "r2_ref", "r3_deletion_ref", "r3_ref"}
            if namespace == "rssh"
            else {"filtered_boundary_ref", "r1_ref"}
        )
        if not isinstance(value, dict) or set(value) != expected_fields:
            violations.append(f"{label}: {namespace} epoch has invalid fields")
            continue
        for field in expected_fields:
            item = value[field]
            if field in required:
                if not is_full_sha(item):
                    violations.append(f"{label}: {namespace}.{field} must be a full immutable SHA")
            elif item is not None:
                violations.append(f"{label}: {namespace}.{field} is not certified at {state}")


def validate_epoch_progression(
    current: dict[str, Any],
    prior: dict[str, Any],
    label: str,
    violations: list[str],
) -> None:
    for namespace in ("rssh", "rterm"):
        prior_epoch = prior.get(namespace)
        current_epoch = current.get(namespace)
        if prior_epoch is None:
            continue
        if not isinstance(current_epoch, dict):
            violations.append(f"{label}: {namespace} epoch was removed")
            continue
        for field, value in prior_epoch.items():
            if value is not None and current_epoch.get(field) != value:
                violations.append(f"{label}: {namespace}.{field} changed after certification")


def artifact_scope(contract: dict[str, Any], artifact_type: str) -> str | None:
    for state in STATES[1:]:
        if artifact_type in contract["new_artifacts_by_state"][state]:
            return state
    return None


def expected_subject_refs(
    manifest: dict[str, Any], contract: dict[str, Any], scope: str
) -> dict[str, str]:
    result: dict[str, str] = {}
    requirements = contract["epoch_requirements_by_state"][scope]
    for namespace in ("rssh", "rterm"):
        fields = requirements[namespace]
        epoch = manifest.get(namespace)
        if fields is None or not isinstance(epoch, dict):
            continue
        for field in fields:
            value = epoch.get(field)
            if is_full_sha(value):
                result[f"{namespace}.{field}"] = value
    return result


def validate_fragment(
    fragment_path: Path,
    manifest_base: Path,
    state: str,
    certified_commit: str,
    rssh_epoch: Any,
    rterm_epoch: Any,
    contract: dict[str, Any],
    manifest_entries: dict[str, dict[str, Any]],
    referenced: set[Path],
    violations: list[str],
) -> tuple[set[str], set[str]]:
    fragment = read_json(fragment_path, f"fragment {fragment_path.name}", violations)
    if fragment is None:
        return set(), set()
    if set(fragment) != {"schema", "requested_state", "certified_commit", "epoch_id", "rssh", "rterm", "entries"}:
        violations.append(f"fragment {fragment_path.name}: fields are not the frozen fragment shape")
    if fragment.get("schema") != FRAGMENT_SCHEMA:
        violations.append(f"fragment {fragment_path.name}: schema mismatch")
    if fragment.get("requested_state") != state:
        violations.append(f"fragment {fragment_path.name}: requested state mismatch")
    if fragment.get("certified_commit") != certified_commit:
        violations.append(f"fragment {fragment_path.name}: certified commit identity drift")
    if fragment.get("rssh") != rssh_epoch or fragment.get("rterm") != rterm_epoch:
        violations.append(f"fragment {fragment_path.name}: repository epoch identity drift")
    if fragment.get("epoch_id") != certification_epoch_id(
        state, certified_commit, rssh_epoch, rterm_epoch
    ):
        violations.append(f"fragment {fragment_path.name}: epoch_id identity drift")
    entries = fragment.get("entries")
    if not isinstance(entries, list) or not entries:
        violations.append(f"fragment {fragment_path.name}: entries must be non-empty")
        return set(), set()
    observed_ids: set[str] = set()
    observed_types: set[str] = set()
    type_counts: dict[str, int] = {}
    for index, entry_value in enumerate(entries):
        label = f"fragment {fragment_path.name} entry {index}"
        entry = validate_entry_shape(entry_value, contract["artifact_policies"], label, violations)
        if entry is None:
            continue
        artifact_type = entry.get("artifact_type")
        artifact_id = entry.get("artifact_id")
        expected_scope = artifact_scope(contract, artifact_type)
        if entry.get("scope") != expected_scope:
            violations.append(f"{label}: scope does not match the artifact's certification state")
        elif entry.get("subject_refs") != expected_subject_refs(
            {"rssh": rssh_epoch, "rterm": rterm_epoch}, contract, expected_scope
        ):
            violations.append(f"{label}: subject_refs drift from the manifest repository epoch")
        if artifact_id in observed_ids:
            violations.append(f"{label}: duplicate artifact_id {artifact_id}")
        observed_ids.add(artifact_id)
        observed_types.add(artifact_type)
        type_counts[artifact_type] = type_counts.get(artifact_type, 0) + 1
        path = contained_file(fragment_path.parent, entry.get("path"), label, violations)
        if path is None:
            continue
        referenced.add(path)
        verify_hash(path, entry.get("sha256"), label, violations)
        manifest_entry = manifest_entries.get(artifact_id)
        if manifest_entry is None:
            violations.append(f"{label}: artifact_id is absent from the assembled manifest")
            continue
        fragment_copy = {key: value for key, value in entry.items() if key != "path"}
        manifest_copy = {key: value for key, value in manifest_entry.items() if key != "path"}
        manifest_path = contained_file(
            manifest_base, manifest_entry.get("path"), f"manifest {artifact_id}", violations
        )
        if fragment_copy != manifest_copy or manifest_path != path:
            violations.append(f"{label}: assembled entry differs from its source fragment")
    singleton = set(contract["artifact_multiplicity"]["singleton"])
    for artifact_type, count in type_counts.items():
        if artifact_type in singleton and count > 1:
            violations.append(f"fragment {fragment_path.name}: singleton artifact_type {artifact_type} is duplicated")
    return observed_ids, observed_types


def validate_manifest_recursive(
    contract_path: Path,
    contract: dict[str, Any],
    contract_digest: str,
    manifest_path: Path,
    expected_state: str,
    top_base: Path,
    referenced: set[Path],
    visited: set[Path],
    violations: list[str],
) -> tuple[dict[str, Any] | None, dict[str, dict[str, Any]]]:
    resolved_manifest = manifest_path.resolve()
    if resolved_manifest in visited:
        violations.append("predecessor manifest chain contains a cycle")
        return None, {}
    try:
        resolved_manifest.relative_to(top_base.resolve())
    except ValueError:
        violations.append("predecessor manifest is outside the bounded manifest root")
        return None, {}
    visited.add(resolved_manifest)
    referenced.add(resolved_manifest)
    manifest = read_json(resolved_manifest, f"manifest {resolved_manifest.name}", violations)
    if manifest is None:
        return None, {}
    schema = read_json(
        contract_path.with_name("stage7-evidence-manifest.schema.json"),
        "evidence manifest schema",
        violations,
    )
    if schema is not None:
        validate_json_schema(
            manifest,
            schema,
            schema,
            f"manifest {resolved_manifest.name}",
            violations,
        )
    if set(manifest) != TOP_LEVEL_FIELDS:
        violations.append(f"manifest {resolved_manifest.name}: fields do not match the frozen schema")
    if manifest.get("schema") != MANIFEST_SCHEMA:
        violations.append(f"manifest {resolved_manifest.name}: schema must be {MANIFEST_SCHEMA}")
    if manifest.get("contract_sha256") != contract_digest:
        violations.append(f"manifest {resolved_manifest.name}: contract SHA-256 identity drift")
    if manifest.get("requested_state") != expected_state or manifest.get("certified_state") != expected_state:
        violations.append(f"manifest {resolved_manifest.name}: requested state does not match evidence state {expected_state}")
    if manifest.get("created_by") != "assemble-stage7-evidence.py":
        violations.append(f"manifest {resolved_manifest.name}: untrusted manifest producer")
    certified_commit = manifest.get("certified_commit")
    if not is_full_sha(certified_commit):
        violations.append(f"manifest {resolved_manifest.name}: certified commit must be a full SHA")
    elif not git_commit_available(repository_root(contract_path), certified_commit):
        violations.append(f"manifest {resolved_manifest.name}: certified commit is unavailable in Git")
    validate_epoch_shape(
        manifest,
        expected_state,
        contract,
        f"manifest {resolved_manifest.name}",
        violations,
    )
    validate_epoch_git_bindings(
        manifest,
        expected_state,
        contract,
        repository_root(contract_path),
        f"manifest {resolved_manifest.name}",
        violations,
    )
    expected_epoch_id = certification_epoch_id(
        expected_state, certified_commit, manifest.get("rssh"), manifest.get("rterm")
    )
    if manifest.get("epoch_id") != expected_epoch_id:
        violations.append(f"manifest {resolved_manifest.name}: epoch_id does not bind state/commit/repository refs")

    entries_value = manifest.get("entries")
    if not isinstance(entries_value, list) or not entries_value:
        violations.append(f"manifest {resolved_manifest.name}: entries must be non-empty")
        entries_value = []
    entries: dict[str, dict[str, Any]] = {}
    entries_by_type: dict[str, list[dict[str, Any]]] = {}
    artifacts: dict[str, dict[str, Any]] = {}
    artifact: dict[str, Any] | None = None
    raw_statistics: dict[str, list[dict[str, Any]]] = {}
    artifact_paths: dict[str, str] = {}
    for index, entry_value in enumerate(entries_value):
        label = f"manifest {resolved_manifest.name} entry {index}"
        entry = validate_entry_shape(entry_value, contract["artifact_policies"], label, violations)
        if entry is None:
            continue
        artifact_type = entry.get("artifact_type")
        artifact_id = entry.get("artifact_id")
        expected_scope = artifact_scope(contract, artifact_type)
        if entry.get("scope") != expected_scope:
            violations.append(f"{label}: scope does not match the artifact's certification state")
        elif entry.get("subject_refs") != expected_subject_refs(manifest, contract, expected_scope):
            violations.append(f"{label}: subject_refs drift from the manifest repository epoch")
        if artifact_id in entries:
            violations.append(f"{label}: duplicate artifact_id {artifact_id}")
            continue
        entries[artifact_id] = entry
        entries_by_type.setdefault(artifact_type, []).append(entry)
        path = contained_file(resolved_manifest.parent, entry.get("path"), label, violations)
        if path is None:
            continue
        path_key = str(path).casefold()
        if path_key in artifact_paths:
            violations.append(
                f"{label}: artifact path collides with {artifact_paths[path_key]} (including case-only collision)"
            )
        else:
            artifact_paths[path_key] = artifact_id
        referenced.add(path)
        if entry.get("size_bytes") != path.stat().st_size:
            violations.append(f"{label}: size_bytes does not match the referenced artifact")
        if not verify_hash(path, entry.get("sha256"), label, violations):
            continue
        artifact = read_json(path, f"artifact {artifact_id}", violations)
        if artifact is None:
            continue
        if artifact.get("schema") != entry.get("payload_schema"):
            violations.append(f"artifact {artifact_id}: payload schema differs from the manifest entry")
        if artifact.get("identity") != identity_from_entry(entry):
            violations.append(f"artifact {artifact_id}: embedded source/binary/runner identity drift")
        policy = contract["artifact_policies"].get(artifact_type, {})
        if artifact_payload_needed_after_individual_validation(artifact_type, policy):
            artifacts[artifact_id] = artifact
        if policy.get("content_kind") == "raw-metric":
            raw_statistics[artifact_id] = validate_metric_artifact(
                artifact, policy, contract, f"artifact {artifact_id}", violations
            )
        elif policy.get("content_kind") == "result":
            validate_result_artifact(
                artifact_type,
                artifact,
                contract,
                manifest,
                repository_root(contract_path),
                f"artifact {artifact_id}",
                violations,
            )

    validate_cross_repository_proof_set(entries_by_type, artifacts, violations)
    artifacts = retain_post_cross_repository_artifacts(
        entries_by_type,
        artifacts,
        contract,
    )
    artifact = None

    required = contract["required_artifacts_by_state"].get(expected_state, [])
    missing = [item for item in required if item not in entries_by_type]
    extra = [item for item in entries_by_type if item not in required]
    for artifact_type in missing:
        violations.append(f"manifest {resolved_manifest.name}: missing required artifact {artifact_type}")
    for artifact_type in extra:
        violations.append(f"manifest {resolved_manifest.name}: artifact {artifact_type} is not permitted at {expected_state}")
    singleton = set(contract["artifact_multiplicity"]["singleton"])
    multiple = contract["artifact_multiplicity"]["multiple"]
    for artifact_type, typed_entries in entries_by_type.items():
        if artifact_type in singleton and len(typed_entries) != 1:
            violations.append(f"manifest {resolved_manifest.name}: singleton artifact_type {artifact_type} is duplicated")
        if artifact_type in multiple:
            rule = multiple[artifact_type]
            if len(typed_entries) < rule["minimum"]:
                violations.append(f"manifest {resolved_manifest.name}: artifact_type {artifact_type} is below its minimum multiplicity")
            platforms = {entry.get("platform") for entry in typed_entries}
            for platform in rule.get("required_platforms", []):
                if platform not in platforms:
                    violations.append(f"manifest {resolved_manifest.name}: artifact_type {artifact_type} lacks platform cohort {platform}")

    prior_entries: dict[str, dict[str, Any]] = {}
    prior_manifest_data: dict[str, Any] | None = None
    state_index = STATES.index(expected_state) if expected_state in STATES else -1
    prior_value = manifest.get("prior_manifest")
    if state_index == 1:
        if prior_value is not None:
            violations.append("attribution-ready must not carry a predecessor manifest")
    elif state_index > 1:
        prior_state = STATES[state_index - 1]
        if not isinstance(prior_value, dict) or set(prior_value) != {
            "path",
            "sha256",
            "certified_state",
            "certified_commit",
        }:
            violations.append(f"manifest {resolved_manifest.name}: exactly one immediate predecessor is required")
        else:
            if prior_value.get("certified_state") != prior_state:
                violations.append(f"manifest {resolved_manifest.name}: predecessor is not the immediate predecessor state")
            prior_path = contained_file(
                resolved_manifest.parent,
                prior_value.get("path"),
                f"manifest {resolved_manifest.name} predecessor",
                violations,
            )
            if prior_path is not None:
                referenced.add(prior_path)
                if verify_hash(
                    prior_path,
                    prior_value.get("sha256"),
                    f"manifest {resolved_manifest.name} predecessor",
                    violations,
                ):
                    prior_manifest_data, prior_entries = validate_manifest_recursive(
                        contract_path,
                        contract,
                        contract_digest,
                        prior_path,
                        prior_state,
                        top_base,
                        referenced,
                        visited,
                        violations,
                    )
                    if prior_manifest_data is not None:
                        validate_epoch_progression(
                            manifest,
                            prior_manifest_data,
                            f"manifest {resolved_manifest.name}",
                            violations,
                        )
                        prior_commit = prior_manifest_data.get("certified_commit")
                        if prior_value.get("certified_commit") != prior_commit:
                            violations.append(f"manifest {resolved_manifest.name}: predecessor certified commit identity drift")
                        if is_full_sha(prior_commit) and is_full_sha(certified_commit):
                            if state_index <= STATES.index("cross-platform-go"):
                                if prior_commit != certified_commit:
                                    violations.append(f"manifest {resolved_manifest.name}: attribution, Windows, and cross-platform states must use the exact same candidate commit")
                            elif not git_is_ancestor(
                                repository_root(contract_path), prior_commit, certified_commit
                            ):
                                violations.append(f"manifest {resolved_manifest.name}: current certified commit is not descended from prior certified commit")
                        for artifact_id, prior_entry in prior_entries.items():
                            current_entry = entries.get(artifact_id)
                            if current_entry is None or not entry_equivalent(
                                current_entry,
                                resolved_manifest.parent,
                                prior_entry,
                                prior_path.parent,
                            ):
                                violations.append(f"manifest {resolved_manifest.name}: prior artifact {artifact_id} was removed or changed")
    elif state_index < 1:
        violations.append(f"manifest {resolved_manifest.name}: invalid certification state")

    fragments_value = manifest.get("fragments")
    fragment_ids: set[str] = set()
    fragment_types: set[str] = set()
    fragment_type_counts: dict[str, int] = {}
    fragment_paths: set[Path] = set()
    if not isinstance(fragments_value, list) or not fragments_value:
        violations.append(f"manifest {resolved_manifest.name}: at least one fragment is required")
    else:
        for index, fragment_ref in enumerate(fragments_value):
            label = f"manifest {resolved_manifest.name} fragment {index}"
            if not isinstance(fragment_ref, dict) or set(fragment_ref) != {"path", "sha256"}:
                violations.append(f"{label}: fragment reference shape is invalid")
                continue
            fragment_path = contained_file(
                resolved_manifest.parent, fragment_ref.get("path"), label, violations
            )
            if fragment_path is None:
                continue
            if fragment_path in fragment_paths:
                violations.append(f"{label}: duplicate fragment input")
                continue
            fragment_paths.add(fragment_path)
            referenced.add(fragment_path)
            if verify_hash(fragment_path, fragment_ref.get("sha256"), label, violations):
                observed_ids, observed_types = validate_fragment(
                    fragment_path,
                    resolved_manifest.parent,
                    expected_state,
                    certified_commit,
                    manifest.get("rssh"),
                    manifest.get("rterm"),
                    contract,
                    entries,
                    referenced,
                    violations,
                )
                overlap = fragment_ids.intersection(observed_ids)
                for artifact_id in sorted(overlap):
                    violations.append(f"{label}: duplicate artifact_id {artifact_id} across fragments")
                fragment_ids.update(observed_ids)
                for artifact_type in observed_types:
                    count = sum(
                        1
                        for artifact_id in observed_ids
                        if entries.get(artifact_id, {}).get("artifact_type") == artifact_type
                    )
                    fragment_type_counts[artifact_type] = fragment_type_counts.get(artifact_type, 0) + count
                fragment_types.update(observed_types)
    expected_new = set(contract["new_artifacts_by_state"].get(expected_state, []))
    expected_fragment_ids = {
        artifact_id
        for artifact_id, entry in entries.items()
        if entry.get("scope") == expected_state
    }
    if fragment_ids != expected_fragment_ids:
        for artifact_id in sorted(expected_fragment_ids - fragment_ids):
            violations.append(
                f"manifest {resolved_manifest.name}: fragments omit current artifact ID {artifact_id}"
            )
        for artifact_id in sorted(fragment_ids - expected_fragment_ids):
            violations.append(
                f"manifest {resolved_manifest.name}: fragments contain non-current artifact ID {artifact_id}"
            )
    if fragment_types != expected_new:
        for artifact_type in sorted(expected_new - fragment_types):
            violations.append(f"manifest {resolved_manifest.name}: fragments omit new artifact {artifact_type}")
        for artifact_type in sorted(fragment_types - expected_new):
            violations.append(f"manifest {resolved_manifest.name}: fragments contain non-current artifact {artifact_type}")
    for artifact_type, count in fragment_type_counts.items():
        if artifact_type in singleton and count > 1:
            violations.append(f"manifest {resolved_manifest.name}: singleton new artifact {artifact_type} is duplicated across fragments")

    combined_statistics_by_type: dict[str, list[dict[str, int | float]]] = {}
    for artifact_type, policy in contract["artifact_policies"].items():
        if policy.get("content_kind") != "raw-metric" or artifact_type not in entries_by_type:
            continue
        shards = [
            (entry["artifact_id"], raw_statistics.get(entry["artifact_id"], []))
            for entry in entries_by_type[artifact_type]
        ]
        combined_statistics_by_type[artifact_type] = combine_metric_shards(
            artifact_type, shards, policy, contract, violations
        )

    if all(
        artifact_type in entries_by_type
        for artifact_type in (
            "runner-fingerprint",
            "font-ownership-raw",
            "font-catalog-fingerprint",
        )
    ):
        runner_id = entries_by_type["runner-fingerprint"][0]["artifact_id"]
        catalog_id = entries_by_type["font-catalog-fingerprint"][0]["artifact_id"]
        font_raw_groups = [
            group
            for entry in entries_by_type["font-ownership-raw"]
            for group in raw_statistics.get(entry["artifact_id"], [])
        ]
        validate_font_runner_inventory_binding(
            artifacts.get(runner_id, {}),
            font_raw_groups,
            artifacts.get(catalog_id, {}),
            "font proof runner cohort",
            violations,
        )

    for artifact_type, policy in contract["artifact_policies"].items():
        if policy.get("content_kind") != "aggregate" or artifact_type not in entries_by_type:
            continue
        aggregate_entry = entries_by_type[artifact_type][0]
        aggregate_id = aggregate_entry["artifact_id"]
        aggregate = artifacts.get(aggregate_id, {})
        reject_unknown_fields(
            aggregate,
            {
                "schema",
                "identity",
                "ok",
                "raw_children",
                "group_statistics",
                "certification_eligible",
                # Stage attribution keeps the auditable reductions and
                # failure classifications alongside the contract-required
                # group_statistics.  They are report-only and never replace
                # recomputation from raw children.
                "representatives",
                "raw_maxima",
                "identities",
                "failure_classifications",
                "adjacent_stage_deltas",
                "source_tree_sha256",
                "runner_fingerprint_sha256",
            },
            f"artifact {aggregate_id}",
            violations,
        )
        if aggregate.get("schema") != AGGREGATE_SCHEMA or aggregate.get("ok") is not True:
            violations.append(f"artifact {aggregate_id}: aggregate schema/result is invalid")
            continue
        if policy.get("certification_eligible") is True and aggregate.get(
            "certification_eligible"
        ) is not True:
            violations.append(
                f"artifact {aggregate_id}: certification_eligible must be true"
            )
        children = aggregate.get("raw_children")
        expected_children = sorted(
            entry["artifact_id"]
            for child_type in policy.get("raw_children", [])
            for entry in entries_by_type.get(child_type, [])
        )
        if aggregate_entry.get("children") != expected_children:
            violations.append(f"artifact {aggregate_id}: manifest children do not cover every raw artifact_id")
        if not isinstance(children, list) or not children or children != expected_children:
            violations.append(f"artifact {aggregate_id}: summary requires exact raw child artifact IDs")
            continue
        expected_statistics: list[dict[str, int | float]] = []
        for child_type in policy.get("raw_children", []):
            if child_type not in combined_statistics_by_type:
                violations.append(f"artifact {aggregate_id}: raw child type {child_type} is absent or invalid")
            else:
                expected_statistics.extend(combined_statistics_by_type[child_type])
        if artifact_type == "font-ownership-aggregate":
            validate_font_ownership_reductions(expected_statistics, policy, violations)
        if aggregate.get("group_statistics") != expected_statistics:
            violations.append(f"artifact {aggregate_id}: summary statistics do not match raw children")

    new_entries = [entries[item] for item in fragment_ids if item in entries]
    identities: dict[str, dict[str, Any]] = {}
    for entry in new_entries:
        key = entry.get("platform")
        identity = identities.setdefault(key, {"source_sha": entry.get("source_sha")})
        if identity["source_sha"] != entry.get("source_sha"):
            violations.append(f"manifest {resolved_manifest.name}: source identity drift within certification epoch")
        for field in ("binary_hashes", "runner_fingerprint_sha256"):
            if field in entry:
                if field in identity and identity[field] != entry[field]:
                    violations.append(f"manifest {resolved_manifest.name}: {field} mismatch within certification epoch")
                identity[field] = entry[field]
        if entry.get("source_sha") != certified_commit:
            violations.append(f"manifest {resolved_manifest.name}: current artifact source differs from certified commit")

    if expected_state == "attribution-ready" and "runner-fingerprint" in entries_by_type:
        fingerprint_id = entries_by_type["runner-fingerprint"][0]["artifact_id"]
        fingerprint = artifacts.get(fingerprint_id, {}).get("fingerprint_sha256")
        for entry in new_entries:
            if contract["artifact_policies"][entry["artifact_type"]].get("runner_identity") and entry.get("runner_fingerprint_sha256") != fingerprint:
                violations.append(f"manifest {resolved_manifest.name}: runner fingerprint artifact mismatch")
    return manifest, entries


def validate_gate(
    contract_path: Path | str,
    requested_state: str,
    evidence_manifest: Path | str | None = None,
) -> dict[str, Any]:
    contract_path = Path(contract_path).resolve()
    contract, violations = validate_contract(contract_path)
    reason = (
        contract.get("no_go_reason")
        if contract is not None
        else "Stage 7 contract is invalid; physical repository split remains NO-GO."
    )
    if requested_state not in STATES:
        violations.append(f"unknown requested state: {requested_state}")
    if requested_state == "blocked":
        if evidence_manifest is not None:
            violations.append("blocked does not accept an evidence manifest")
        return {
            "ok": not violations,
            "go": False,
            "decision": "NO-GO",
            "state": "blocked",
            "reason": reason,
            "artifact_count": 0,
            "violations": violations,
        }
    if contract is None or requested_state not in STATES:
        return {
            "ok": False,
            "go": False,
            "decision": "NO-GO",
            "state": "blocked",
            "reason": reason,
            "artifact_count": 0,
            "violations": violations,
        }
    if evidence_manifest is None:
        violations.append(f"{requested_state} requires exactly one evidence manifest")
        entries: dict[str, Any] = {}
    else:
        manifest_path = Path(evidence_manifest).resolve()
        if not manifest_path.is_file():
            violations.append(f"evidence manifest does not exist: {manifest_path}")
            entries = {}
        else:
            referenced: set[Path] = set()
            _, entries = validate_manifest_recursive(
                contract_path,
                contract,
                file_sha256(contract_path),
                manifest_path,
                requested_state,
                manifest_path.parent,
                referenced,
                set(),
                violations,
            )
            actual_files = {path.resolve() for path in manifest_path.parent.rglob("*") if path.is_file()}
            for path in sorted(actual_files - referenced, key=lambda item: item.as_posix()):
                violations.append(
                    f"unreferenced evidence file beneath manifest directory: {path.relative_to(manifest_path.parent).as_posix()}"
                )
    return {
        "ok": not violations,
        "go": not violations,
        "decision": "GO" if not violations else "NO-GO",
        "state": requested_state if not violations else "blocked",
        "reason": f"Immutable raw evidence proves {requested_state}." if not violations else reason,
        "artifact_count": len(entries),
        "violations": violations,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--contract", required=True, type=Path)
    parser.add_argument("--requested-state", required=True)
    parser.add_argument("--evidence-manifest", type=Path)
    arguments = parser.parse_args()
    try:
        decision = validate_gate(
            arguments.contract, arguments.requested_state, arguments.evidence_manifest
        )
    except Exception as error:  # pragma: no cover - defensive CLI boundary
        decision = {
            "ok": False,
            "go": False,
            "decision": "NO-GO",
            "state": "blocked",
            "reason": "The validator failed closed while checking malformed evidence.",
            "artifact_count": 0,
            "violations": [f"validator failed closed: {type(error).__name__}"],
        }
    print(json.dumps(decision, sort_keys=True, separators=(",", ":")))
    return 0 if decision["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
