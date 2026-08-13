import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "check-functional-hermeticity.py"
SPEC = importlib.util.spec_from_file_location("check_functional_hermeticity", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class FunctionalHermeticityTests(unittest.TestCase):
    def test_loopback_urls_and_addresses_are_allowed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.ts"
            path.write_text("http://127.0.0.1:0 http://localhost:7 ::1", encoding="utf-8")
            self.assertEqual(MODULE.check_file(path), [])

    def test_public_url_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.ts"
            path.write_text("https://example.com/socket", encoding="utf-8")
            errors = MODULE.check_file(path)
            self.assertEqual(len(errors), 1)
            self.assertIn("external URL", errors[0])

    def test_non_loopback_literal_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.rs"
            path.write_text('connect("192.0.2.10:22")', encoding="utf-8")
            errors = MODULE.check_file(path)
            self.assertEqual(len(errors), 1)
            self.assertIn("non-loopback address", errors[0])


if __name__ == "__main__":
    unittest.main()
