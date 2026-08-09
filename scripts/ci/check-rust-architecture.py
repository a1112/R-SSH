#!/usr/bin/env python3
"""Enforce shrinking structural budgets for handwritten Rust sources."""

from __future__ import annotations

import argparse
import bisect
import fnmatch
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


RULES = (
    "file_lines",
    "struct_fields",
    "impl_lines",
    "function_lines",
    "rustfmt_skip",
    "unbounded_channels",
    "forbidden_dependencies",
)
UNBOUNDED_CHANNEL_PATTERNS = (
    re.compile(r"\b(?:std\s*::\s*sync\s*::\s*)?mpsc\s*::\s*channel\s*(?:::\s*<[^;{}()]*>)?\s*\("),
    re.compile(r"\bcrossbeam_channel\s*::\s*unbounded\s*(?:::\s*<[^;{}()]*>)?\s*\("),
    re.compile(r"\b(?:tokio\s*::\s*sync\s*::\s*)?mpsc\s*::\s*unbounded_channel\s*(?:::\s*<[^;{}()]*>)?\s*\("),
)


@dataclass(frozen=True)
class ItemMeasurement:
    item: str
    observed: int
    line: int


class LineMap:
    def __init__(self, source: str) -> None:
        self._newlines = [index for index, character in enumerate(source) if character == "\n"]

    def line(self, index: int) -> int:
        return bisect.bisect_right(self._newlines, index) + 1


def mask_rust_non_code(source: str) -> str:
    """Replace comments and literals with spaces while preserving positions."""
    masked = list(source)
    index = 0
    length = len(source)
    while index < length:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            if end == -1:
                end = length
            blank(masked, index, end)
            index = end
            continue
        if source.startswith("/*", index):
            end = nested_block_comment_end(source, index)
            blank(masked, index, end)
            index = end
            continue

        if source[index] in "br":
            raw_end = raw_string_end(source, index)
            if raw_end is not None:
                blank(masked, index, raw_end)
                index = raw_end
                continue
        if source[index] == '"':
            end = quoted_literal_end(source, index, '"')
            blank(masked, index, end)
            index = end
            continue
        if source[index] == "'":
            end = character_literal_end(source, index)
            if end is not None:
                blank(masked, index, end)
                index = end
                continue
        index += 1
    return "".join(masked)


def blank(buffer: list[str], start: int, end: int) -> None:
    for index in range(start, min(end, len(buffer))):
        if buffer[index] not in "\r\n":
            buffer[index] = " "


def nested_block_comment_end(source: str, start: int) -> int:
    depth = 1
    index = start + 2
    while index < len(source) and depth > 0:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    return index


def raw_string_end(source: str, start: int) -> int | None:
    index = start
    if source.startswith(("br", "rb"), index):
        index += 2
    elif source.startswith("r", index):
        index += 1
    else:
        return None
    hashes_start = index
    while index < len(source) and source[index] == "#" and index - hashes_start < 255:
        index += 1
    if index >= len(source) or source[index] != '"':
        return None
    hashes = source[hashes_start:index]
    terminator = '"' + hashes
    content_start = index + 1
    closing = source.find(terminator, content_start)
    return len(source) if closing == -1 else closing + len(terminator)


def quoted_literal_end(source: str, start: int, delimiter: str) -> int:
    index = start + 1
    while index < len(source):
        if source[index] == "\\":
            index += 2
        elif source[index] == delimiter:
            return index + 1
        else:
            index += 1
    return len(source)


def character_literal_end(source: str, start: int) -> int | None:
    index = start + 1
    if index >= len(source):
        return None
    if source[index] == "\\":
        index += 2
        if index < len(source) and source[index] == "u" and index + 1 < len(source) and source[index + 1] == "{":
            closing_brace = source.find("}", index + 2)
            if closing_brace == -1:
                return None
            index = closing_brace + 1
    else:
        index += 1
    return index + 1 if index < len(source) and source[index] == "'" else None


def matching_brace(masked: str, opening: int) -> int | None:
    depth = 1
    for index in range(opening + 1, len(masked)):
        character = masked[index]
        if character == "{":
            depth += 1
        elif character == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def mask_cfg_test_modules(masked: str) -> str:
    """Mask module bodies that are compiled only under an exact `cfg(test)`."""
    buffer = list(masked)
    pattern = re.compile(
        r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{"
    )
    for match in pattern.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_brace(masked, opening)
        if closing is not None:
            blank(buffer, match.start(), closing + 1)
    return "".join(buffer)


def measure_structs(masked: str, lines: LineMap) -> list[ItemMeasurement]:
    measurements: list[ItemMeasurement] = []
    pattern = re.compile(r"\bstruct\s+([A-Za-z_][A-Za-z0-9_]*)[^;{]*\{")
    for match in pattern.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_brace(masked, opening)
        if closing is None:
            continue
        fields = count_struct_fields(masked[opening + 1 : closing])
        measurements.append(ItemMeasurement(match.group(1), fields, lines.line(match.start())))
    return measurements


def count_struct_fields(body: str) -> int:
    fields = 0
    segment_start = 0
    round_depth = square_depth = brace_depth = angle_depth = 0
    for index, character in enumerate(body):
        if character == "(":
            round_depth += 1
        elif character == ")":
            round_depth = max(0, round_depth - 1)
        elif character == "[":
            square_depth += 1
        elif character == "]":
            square_depth = max(0, square_depth - 1)
        elif character == "{":
            brace_depth += 1
        elif character == "}":
            brace_depth = max(0, brace_depth - 1)
        elif character == "<":
            angle_depth += 1
        elif character == ">":
            angle_depth = max(0, angle_depth - 1)
        elif character == "," and not any((round_depth, square_depth, brace_depth, angle_depth)):
            if has_top_level_colon(body[segment_start:index]):
                fields += 1
            segment_start = index + 1
    if has_top_level_colon(body[segment_start:]):
        fields += 1
    return fields


def has_top_level_colon(segment: str) -> bool:
    round_depth = square_depth = brace_depth = angle_depth = 0
    for index, character in enumerate(segment):
        if character == "(":
            round_depth += 1
        elif character == ")":
            round_depth = max(0, round_depth - 1)
        elif character == "[":
            square_depth += 1
        elif character == "]":
            square_depth = max(0, square_depth - 1)
        elif character == "{":
            brace_depth += 1
        elif character == "}":
            brace_depth = max(0, brace_depth - 1)
        elif character == "<":
            angle_depth += 1
        elif character == ">":
            angle_depth = max(0, angle_depth - 1)
        elif character == ":" and not any((round_depth, square_depth, brace_depth, angle_depth)):
            previous = segment[index - 1] if index > 0 else ""
            following = segment[index + 1] if index + 1 < len(segment) else ""
            if previous != ":" and following != ":":
                return True
    return False


def measure_impls(masked: str, lines: LineMap) -> list[ItemMeasurement]:
    measurements: list[ItemMeasurement] = []
    pattern = re.compile(r"(?m)^(?:unsafe\s+)?impl\b[^;{]*\{")
    for match in pattern.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        closing = matching_brace(masked, opening)
        if closing is None:
            continue
        header = " ".join(masked[match.start() : opening].split())
        measurements.append(
            ItemMeasurement(
                header,
                lines.line(closing) - lines.line(match.start()) + 1,
                lines.line(match.start()),
            )
        )
    return measurements


def measure_functions(masked: str, lines: LineMap) -> list[ItemMeasurement]:
    measurements: list[ItemMeasurement] = []
    for match in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\b", masked):
        opening = function_body_opening(masked, match.end())
        if opening is None:
            continue
        closing = matching_brace(masked, opening)
        if closing is None:
            continue
        measurements.append(
            ItemMeasurement(
                match.group(1),
                lines.line(closing) - lines.line(match.start()) + 1,
                lines.line(match.start()),
            )
        )
    return measurements


def function_body_opening(masked: str, start: int) -> int | None:
    round_depth = square_depth = angle_depth = 0
    index = start
    while index < len(masked):
        character = masked[index]
        if character == "(":
            round_depth += 1
        elif character == ")":
            round_depth = max(0, round_depth - 1)
        elif character == "[":
            square_depth += 1
        elif character == "]":
            square_depth = max(0, square_depth - 1)
        elif character == "<":
            angle_depth += 1
        elif character == ">":
            angle_depth = max(0, angle_depth - 1)
        elif character == ";" and not any((round_depth, square_depth, angle_depth)):
            return None
        elif character == "{" and not any((round_depth, square_depth, angle_depth)):
            return index
        index += 1
    return None


def maximum(measurements: Iterable[ItemMeasurement]) -> ItemMeasurement:
    return max(measurements, key=lambda item: item.observed, default=ItemMeasurement("<none>", 0, 1))


def load_policy(path: Path) -> dict[str, Any]:
    policy = json.loads(path.read_text(encoding="utf-8"))
    if policy.get("version") != 1:
        raise ValueError("architecture policy version must be 1")
    missing = [rule for rule in RULES if rule not in policy.get("limits", {})]
    if missing:
        raise ValueError(f"architecture policy is missing limits: {', '.join(missing)}")
    return policy


def effective_limit(policy: dict[str, Any], relative: str, rule: str) -> int:
    budgets = policy.get("migration", {}).get("budgets", {})
    return int(budgets.get(relative, {}).get(rule, policy["limits"][rule]))


def validate_budgets(policy: dict[str, Any]) -> list[dict[str, Any]]:
    migration = policy.get("migration", {})
    ceilings = migration.get("initial_ceilings", {})
    budgets = migration.get("budgets", {})
    violations: list[dict[str, Any]] = []
    for relative, rules in budgets.items():
        for rule, budget_value in rules.items():
            if rule not in RULES:
                raise ValueError(f"unknown migration budget rule {rule!r} for {relative}")
            if relative not in ceilings or rule not in ceilings[relative]:
                raise ValueError(f"migration budget lacks an initial ceiling: {relative} {rule}")
            ceiling = int(ceilings[relative][rule])
            budget = int(budget_value)
            if budget > ceiling:
                violations.append(
                    violation(
                        "policy_budget",
                        relative,
                        rule,
                        budget,
                        ceiling,
                        1,
                    )
                )
    return violations


def collect_rust_files(root: Path, policy: dict[str, Any]) -> list[Path]:
    files: set[Path] = set()
    for configured_root in policy.get("roots", []):
        candidate = root / configured_root
        if candidate.is_file() and candidate.suffix == ".rs":
            files.add(candidate)
        elif candidate.is_dir():
            files.update(candidate.rglob("*.rs"))
    return sorted(files)


def is_generated(relative: str, policy: dict[str, Any]) -> bool:
    return any(fnmatch.fnmatch(relative, pattern) for pattern in policy.get("generated_globs", []))


def check_repository(root: Path, policy: dict[str, Any]) -> dict[str, Any]:
    violations = validate_budgets(policy)
    inventory: dict[str, Any] = {}
    for path in collect_rust_files(root, policy):
        relative = path.relative_to(root).as_posix()
        if is_generated(relative, policy):
            continue
        source = path.read_text(encoding="utf-8")
        masked = mask_rust_non_code(source)
        production_excluded = any(
            fnmatch.fnmatch(relative, pattern)
            for pattern in policy.get("production_excluded_globs", [])
        )
        production_masked = "" if production_excluded else mask_cfg_test_modules(masked)
        lines = LineMap(source)
        structs = measure_structs(masked, lines)
        impls = measure_impls(masked, lines)
        functions = measure_functions(masked, lines)
        rustfmt_matches = list(re.finditer(r"#\s*!?\s*\[\s*rustfmt\s*::\s*skip\s*\]", masked))
        channel_matches = [
            match
            for pattern in UNBOUNDED_CHANNEL_PATTERNS
            for match in pattern.finditer(production_masked)
        ]
        dependency_matches = forbidden_dependency_matches(relative, production_masked, policy)

        file_lines = len(source.splitlines())
        max_struct = maximum(structs)
        max_impl = maximum(impls)
        max_function = maximum(functions)
        inventory[relative] = {
            "file_lines": file_lines,
            "max_struct_fields": measurement_json(max_struct),
            "max_impl_lines": measurement_json(max_impl),
            "max_function_lines": measurement_json(max_function),
            "rustfmt_skip": len(rustfmt_matches),
            "unbounded_channels": len(channel_matches),
            "forbidden_dependencies": len(dependency_matches),
        }

        append_if_over(
            violations,
            "file_lines",
            relative,
            "<file>",
            file_lines,
            effective_limit(policy, relative, "file_lines"),
            1,
        )
        for rule, measurements in (
            ("struct_fields", structs),
            ("impl_lines", impls),
            ("function_lines", functions),
        ):
            limit = effective_limit(policy, relative, rule)
            for measurement in measurements:
                append_if_over(
                    violations,
                    rule,
                    relative,
                    measurement.item,
                    measurement.observed,
                    limit,
                    measurement.line,
                )
        append_count_violations(
            violations,
            "rustfmt_skip",
            relative,
            rustfmt_matches,
            effective_limit(policy, relative, "rustfmt_skip"),
            lines,
        )
        append_count_violations(
            violations,
            "unbounded_channels",
            relative,
            channel_matches,
            effective_limit(policy, relative, "unbounded_channels"),
            lines,
        )
        forbidden_limit = effective_limit(policy, relative, "forbidden_dependencies")
        if len(dependency_matches) > forbidden_limit:
            for pattern, index in dependency_matches:
                violations.append(
                    violation(
                        "forbidden_dependencies",
                        relative,
                        pattern,
                        len(dependency_matches),
                        forbidden_limit,
                        lines.line(index),
                    )
                )

    violations.sort(key=lambda entry: (entry["file"], entry["line"], entry["rule"], entry["item"]))
    return {
        "ok": not violations,
        "policy_version": policy["version"],
        "files_checked": len(inventory),
        "violations": violations,
        "inventory": inventory,
    }


def forbidden_dependency_matches(
    relative: str, masked: str, policy: dict[str, Any]
) -> list[tuple[str, int]]:
    matches: list[tuple[str, int]] = []
    for rule in policy.get("forbidden_dependencies", []):
        if not fnmatch.fnmatch(relative, rule["scope"]):
            continue
        for pattern in rule.get("patterns", []):
            start = 0
            while True:
                index = masked.find(pattern, start)
                if index == -1:
                    break
                matches.append((pattern, index))
                start = index + len(pattern)
    return matches


def append_count_violations(
    violations: list[dict[str, Any]],
    rule: str,
    relative: str,
    matches: list[re.Match[str]],
    limit: int,
    lines: LineMap,
) -> None:
    if len(matches) <= limit:
        return
    for match in matches:
        violations.append(
            violation(rule, relative, f"{rule}@{lines.line(match.start())}", len(matches), limit, lines.line(match.start()))
        )


def append_if_over(
    violations: list[dict[str, Any]],
    rule: str,
    relative: str,
    item: str,
    observed: int,
    limit: int,
    line: int,
) -> None:
    if observed > limit:
        violations.append(violation(rule, relative, item, observed, limit, line))


def violation(
    rule: str,
    relative: str,
    item: str,
    observed: int,
    limit: int,
    line: int,
) -> dict[str, Any]:
    return {
        "rule": rule,
        "file": relative,
        "item": item,
        "line": line,
        "observed": observed,
        "limit": limit,
        "message": f"{relative}:{line} {rule} {item!r} observed={observed} limit={limit}",
    }


def measurement_json(measurement: ItemMeasurement) -> dict[str, Any]:
    return {
        "item": measurement.item,
        "observed": measurement.observed,
        "line": measurement.line,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--policy", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    try:
        root = arguments.root.resolve(strict=True)
        policy = load_policy(arguments.policy.resolve(strict=True))
        report = check_repository(root, policy)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"architecture checker configuration error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, ensure_ascii=False, separators=(",", ":")))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
