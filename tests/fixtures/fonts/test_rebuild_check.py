"""Security regression tests for rebuild_check.py."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("rebuild_check.py")
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("rssh_font_rebuild_check", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
REBUILD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REBUILD)


class RebuildSafetyTests(unittest.TestCase):
    def test_rejects_traversal_and_absolute_paths(self) -> None:
        for unsafe in ("../escape.ttf", "/escape.ttf", "C:/escape.ttf"):
            with self.subTest(path=unsafe):
                with self.assertRaisesRegex(ValueError, "portable relative path"):
                    REBUILD.validate_local_relative_path(unsafe)

    def test_rejects_windows_device_and_illegal_paths(self) -> None:
        for unsafe in (
            "COM\u00b9.txt",
            "LPT\u00b2.txt",
            "CONIN$.txt",
            "name.",
            "name ",
            "bad<name>.ttf",
            "nested\\escape.ttf",
        ):
            with self.subTest(path=unsafe):
                with self.assertRaisesRegex(ValueError, "portable relative path"):
                    REBUILD.validate_local_relative_path(unsafe)

    def test_rejects_six_of_seven_outputs(self) -> None:
        expected = {f"fixture-{index}.ttf" for index in range(7)}
        outputs = [Path(name) for name in sorted(expected)[:6]]
        with self.assertRaisesRegex(ValueError, "output set"):
            REBUILD.validate_output_set(expected, outputs)
        with self.assertRaisesRegex(ValueError, "output set"):
            REBUILD.validate_output_set(expected, [*expected, "fixture-extra.ttf"])

    def test_rejects_duplicate_manifest_roles_and_files(self) -> None:
        duplicate_role = [
            {"role": "latin", "file": "latin.ttf"},
            {"role": "latin", "file": "other.ttf"},
        ]
        with self.assertRaisesRegex(ValueError, "duplicate role"):
            REBUILD.validate_manifest_uniqueness(duplicate_role)

        duplicate_file = [
            {"role": "latin", "file": "same.ttf"},
            {"role": "cjk", "file": "same.ttf"},
        ]
        with self.assertRaisesRegex(ValueError, "duplicate file"):
            REBUILD.validate_manifest_uniqueness(duplicate_file)


if __name__ == "__main__":
    unittest.main()
