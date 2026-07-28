#!/usr/bin/env python3
"""Rebuild deterministic font subsets and verify their committed SHA-256."""

from __future__ import annotations

import argparse
import csv
import hashlib
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path, PureWindowsPath
from typing import Iterable, Mapping

import fontTools


FIXTURE_ROOT = Path(__file__).resolve().parent
MANIFEST_FIELDS = (
    "role",
    "file",
    "license",
    "license_file",
    "license_source",
    "license_sha256",
    "codepoints",
    "sequences",
    "gsub_features",
    "color",
    "source",
    "version",
    "subset_command",
)
EXPECTED_ROLES = {
    "arabic",
    "cjk",
    "color-emoji",
    "devanagari",
    "hebrew",
    "latin-ligature",
    "symbols-text",
}
WINDOWS_DEVICES = {
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "CONIN$",
    "CONOUT$",
}
WINDOWS_ILLEGAL = frozenset('<>:"|?*')


def validate_local_relative_path(value: str) -> tuple[str, ...]:
    def fail() -> None:
        raise ValueError(f"not a portable relative path: {value!r}")

    if not value or "\\" in value or PureWindowsPath(value).drive:
        fail()
    components = value.split("/")
    if any(component in {"", ".", ".."} for component in components):
        fail()
    for component in components:
        if (
            component.endswith((" ", "."))
            or any(character in WINDOWS_ILLEGAL or ord(character) < 32 for character in component)
            or is_windows_device(component)
        ):
            fail()
    return tuple(components)


def is_windows_device(component: str) -> bool:
    stem = component.split(".", maxsplit=1)[0].upper()
    if stem in WINDOWS_DEVICES:
        return True
    for prefix in ("COM", "LPT"):
        if stem.startswith(prefix):
            suffix = stem[len(prefix) :]
            if suffix in {*map(str, range(1, 10)), "\u00b9", "\u00b2", "\u00b3"}:
                return True
    return False


def resolve_under(root: Path, value: str) -> Path:
    components = validate_local_relative_path(value)
    resolved_root = root.resolve()
    resolved = resolved_root.joinpath(*components).resolve()
    if not resolved.is_relative_to(resolved_root):
        raise ValueError(f"path escapes root {resolved_root}: {value!r}")
    return resolved


def validate_manifest_uniqueness(rows: Iterable[Mapping[str, str]]) -> None:
    roles: set[str] = set()
    files: set[str] = set()
    for row in rows:
        role = row["role"]
        filename = row["file"]
        if role in roles:
            raise ValueError(f"duplicate role in MANIFEST.tsv: {role!r}")
        if filename in files:
            raise ValueError(f"duplicate file in MANIFEST.tsv: {filename!r}")
        roles.add(role)
        files.add(filename)


def load_manifest() -> list[dict[str, str]]:
    with (FIXTURE_ROOT / "MANIFEST.tsv").open(encoding="utf-8", newline="") as manifest:
        reader = csv.DictReader(manifest, dialect="excel-tab")
        if tuple(reader.fieldnames or ()) != MANIFEST_FIELDS:
            raise ValueError("MANIFEST.tsv header does not match the pinned schema")
        rows = list(reader)
    validate_manifest_uniqueness(rows)
    roles = {row["role"] for row in rows}
    if roles != EXPECTED_ROLES:
        raise ValueError(
            f"manifest role set differs: expected {sorted(EXPECTED_ROLES)}, got {sorted(roles)}"
        )
    for row in rows:
        validate_local_relative_path(row["file"])
        validate_local_relative_path(row["license_file"])
        validate_subset_command(row)
    return rows


def validate_subset_command(row: Mapping[str, str]) -> None:
    arguments = shlex.split(row["subset_command"])
    if len(arguments) < 3 or arguments[0] != "pyftsubset":
        raise ValueError(f"invalid subset command for {row['role']!r}")
    source = validate_local_relative_path(arguments[1])
    if len(source) != 2 or source[0] != "upstream":
        raise ValueError(f"subset source must be directly under upstream/: {arguments[1]!r}")
    options = arguments[2:]
    if any(not option.startswith("--") or option.startswith("@") for option in options):
        raise ValueError(f"unexpected positional or response-file argument for {row['role']!r}")
    forbidden_file_options = (
        "--gids-file=",
        "--glyphs-file=",
        "--text-file=",
        "--unicodes-file=",
    )
    if any(option.startswith(forbidden_file_options) for option in options):
        raise ValueError(f"file-valued subset option is forbidden for {row['role']!r}")
    output_options = [
        option.removeprefix("--output-file=")
        for option in options
        if option.startswith("--output-file=")
    ]
    if len(output_options) != 1:
        raise ValueError(f"subset command needs exactly one output for {row['role']!r}")
    validate_local_relative_path(output_options[0])
    expected_output = f"tests/fixtures/fonts/{row['file']}"
    if output_options[0] != expected_output:
        raise ValueError(
            f"subset output for {row['role']!r} must be {expected_output!r}"
        )


def read_checksums(expected_files: set[str]) -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in (FIXTURE_ROOT / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        checksum, filename = line.split("  ", maxsplit=1)
        validate_local_relative_path(filename)
        if filename in checksums:
            raise ValueError(f"duplicate SHA256SUMS path: {filename!r}")
        if len(checksum) != 64 or any(character not in "0123456789abcdef" for character in checksum):
            raise ValueError(f"invalid SHA-256 for {filename!r}")
        checksums[filename] = checksum
    if set(checksums) != expected_files:
        raise ValueError("SHA256SUMS file set differs from MANIFEST.tsv")
    return checksums


def rebuild(
    rows: list[dict[str, str]], upstream: Path, output_root: Path
) -> dict[str, Path]:
    outputs: dict[str, Path] = {}
    for row in rows:
        arguments = shlex.split(row["subset_command"])
        source_components = validate_local_relative_path(arguments[1])
        source = resolve_under(upstream, "/".join(source_components[1:]))
        if not source.is_file():
            raise ValueError(f"missing upstream source: {source}")
        output = resolve_under(output_root, row["file"])
        output.parent.mkdir(parents=True, exist_ok=True)
        arguments = [
            f"--output-file={output}" if argument.startswith("--output-file=") else argument
            for argument in arguments[2:]
        ]
        subprocess.run(
            [sys.executable, "-m", "fontTools.subset", str(source), *arguments],
            check=True,
        )
        outputs[row["file"]] = output
    return outputs


def validate_output_set(expected: set[str], outputs: Iterable[str | Path]) -> None:
    actual_items = [
        output.as_posix() if isinstance(output, Path) else output for output in outputs
    ]
    actual = set(actual_items)
    if len(actual) != len(actual_items) or actual != expected:
        raise ValueError(
            f"output set differs: expected {sorted(expected)}, got {sorted(actual)}"
        )


def verify(outputs: Mapping[str, Path], expected: Mapping[str, str]) -> None:
    validate_output_set(set(expected), outputs)
    mismatches = []
    for filename, output in outputs.items():
        actual = hashlib.sha256(output.read_bytes()).hexdigest()
        wanted = expected[filename]
        if actual != wanted:
            mismatches.append(f"{filename}: expected {wanted}, rebuilt {actual}")
    if mismatches:
        raise SystemExit("\n".join(mismatches))


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--upstream",
        required=True,
        type=Path,
        help="directory containing sources named as documented in README.md",
    )
    parser.add_argument(
        "--write",
        action="store_true",
        help="replace committed fixtures instead of rebuilding in a temporary directory",
    )
    args = parser.parse_args()

    rows = load_manifest()
    expected_files = {row["file"] for row in rows}
    expected_checksums = read_checksums(expected_files)
    pinned = (FIXTURE_ROOT / "FONTTOOLS_VERSION").read_text(encoding="utf-8").strip()
    if fontTools.__version__ != pinned:
        raise SystemExit(
            f"fonttools {pinned} is required; found {fontTools.__version__}"
        )

    if args.write:
        outputs = rebuild(rows, args.upstream.resolve(), FIXTURE_ROOT)
        verify(outputs, expected_checksums)
    else:
        with tempfile.TemporaryDirectory(prefix="rssh-font-fixtures-") as temporary:
            outputs = rebuild(rows, args.upstream.resolve(), Path(temporary))
            verify(outputs, expected_checksums)

    print(f"verified {len(outputs)} fixture(s) with fonttools {pinned}")


if __name__ == "__main__":
    main()
