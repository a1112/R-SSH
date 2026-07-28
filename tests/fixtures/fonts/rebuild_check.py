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
from pathlib import Path

import fontTools


FIXTURE_ROOT = Path(__file__).resolve().parent


def read_checksums() -> dict[str, str]:
    checksums: dict[str, str] = {}
    for line in (FIXTURE_ROOT / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        checksum, filename = line.split("  ", maxsplit=1)
        checksums[filename] = checksum
    return checksums


def rebuild(upstream: Path, output_root: Path) -> list[Path]:
    outputs: list[Path] = []
    with (FIXTURE_ROOT / "MANIFEST.tsv").open(encoding="utf-8", newline="") as manifest:
        for row in csv.DictReader(manifest, dialect="excel-tab"):
            arguments = shlex.split(row["subset_command"])
            arguments[1] = str(upstream / Path(arguments[1]).name)
            output = output_root / row["file"]
            arguments = [
                f"--output-file={output}" if argument.startswith("--output-file=") else argument
                for argument in arguments[1:]
            ]
            subprocess.run(
                [sys.executable, "-m", "fontTools.subset", *arguments],
                check=True,
            )
            outputs.append(output)
    return outputs


def verify(outputs: list[Path]) -> None:
    expected = read_checksums()
    mismatches = []
    for output in outputs:
        actual = hashlib.sha256(output.read_bytes()).hexdigest()
        wanted = expected[output.name]
        if actual != wanted:
            mismatches.append(f"{output.name}: expected {wanted}, rebuilt {actual}")
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

    pinned = (FIXTURE_ROOT / "FONTTOOLS_VERSION").read_text(encoding="utf-8").strip()
    if fontTools.__version__ != pinned:
        raise SystemExit(
            f"fonttools {pinned} is required; found {fontTools.__version__}"
        )

    if args.write:
        outputs = rebuild(args.upstream.resolve(), FIXTURE_ROOT)
        verify(outputs)
    else:
        with tempfile.TemporaryDirectory(prefix="rssh-font-fixtures-") as temporary:
            outputs = rebuild(args.upstream.resolve(), Path(temporary))
            verify(outputs)

    print(f"verified {len(outputs)} fixture(s) with fonttools {pinned}")


if __name__ == "__main__":
    main()
