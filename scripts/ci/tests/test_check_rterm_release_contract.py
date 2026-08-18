import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
CHECKER = REPOSITORY_ROOT / "scripts" / "ci" / "check-rterm-release-contract.py"
CONTRACT = REPOSITORY_ROOT / "scripts" / "ci" / "rterm-release-contract.json"
HISTORY_MAP = REPOSITORY_ROOT / "docs" / "release" / "rterm-history-paths.txt"

EXPECTED_PACKAGES = {
    "rterm-types": "crates/rterm-types",
    "rterm-terminal": "crates/rssh-terminal",
    "rterm-runtime": "crates/rssh-runtime",
    "rterm-fonts": "crates/rterm-fonts",
    "rterm-render-core": "crates/rterm-render-core",
    "rterm-render-cpu": "crates/rterm-render-cpu",
    "rterm-render-wgpu": "crates/rterm-render-wgpu",
}


class RTermReleaseContractTests(unittest.TestCase):
    def run_checker(self, contract: Path = CONTRACT) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--repo-root",
                str(REPOSITORY_ROOT),
                "--contract",
                str(contract),
            ],
            cwd=REPOSITORY_ROOT,
            text=True,
            capture_output=True,
            check=False,
        )

    def test_repository_contract_is_complete_and_valid(self):
        result = self.run_checker()

        self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
        report = json.loads(result.stdout)
        self.assertTrue(report["ok"])
        self.assertEqual(report["checked_ref"], "HEAD")
        self.assertEqual(set(report["packages"]), set(EXPECTED_PACKAGES))
        self.assertEqual(set(report["vendor_trees"]), {"glyphon", "gpu-allocator"})
        self.assertEqual(report["violations"], [])

    def test_contract_declares_exact_public_packages_and_vendor_strategy(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))

        self.assertEqual(contract["schema_version"], 1)
        self.assertEqual(contract["api_compatibility_line"], "0.1")
        self.assertRegex(contract["last_known_good_rterm_ref"], r"^[0-9a-f]{40}$")
        self.assertEqual(
            contract["vendor_patch_strategy"], "consumer-root-path-patch"
        )
        self.assertEqual(
            {entry["name"]: entry["path"] for entry in contract["packages"]},
            EXPECTED_PACKAGES,
        )
        for package in contract["packages"]:
            self.assertEqual(package["version"], "0.1.0")
            self.assertFalse(
                any(name.startswith("rssh-") for name in package["dependencies"]),
                package["name"],
            )

    def test_checker_rejects_mutable_refs_version_mismatch_and_reverse_dependencies(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        contract["last_known_good_rterm_ref"] = "main"
        contract["packages"][0]["version"] = "0.2.0"
        contract["packages"][0]["dependencies"].append("rssh-core")

        with tempfile.TemporaryDirectory() as directory:
            mutated = Path(directory) / "contract.json"
            mutated.write_text(json.dumps(contract), encoding="utf-8")
            result = self.run_checker(mutated)

        self.assertNotEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        self.assertFalse(report["ok"])
        joined = "\n".join(report["violations"])
        self.assertIn("immutable 40-character lowercase commit", joined)
        self.assertIn("version", joined)
        self.assertIn("reverse dependency rssh-core", joined)

    def test_checker_rejects_missing_paths_and_vendor_tree_drift(self):
        contract = json.loads(CONTRACT.read_text(encoding="utf-8"))
        contract["packages"][0]["path"] = "crates/does-not-exist"
        contract["vendor_trees"][0]["tree"] = "0" * 40

        with tempfile.TemporaryDirectory() as directory:
            mutated = Path(directory) / "contract.json"
            mutated.write_text(json.dumps(contract), encoding="utf-8")
            result = self.run_checker(mutated)

        self.assertNotEqual(result.returncode, 0)
        report = json.loads(result.stdout)
        joined = "\n".join(report["violations"])
        self.assertIn("missing package path", joined)
        self.assertIn("vendor tree drift", joined)

    def test_history_map_preserves_terminal_runtime_fonts_and_renderer_paths(self):
        rows = {
            tuple(line.split("|"))
            for line in HISTORY_MAP.read_text(encoding="utf-8").splitlines()
            if line and not line.startswith("#")
        }

        self.assertIn(
            (
                "terminal",
                "crates/rssh-terminal",
                "crates/rssh-terminal",
            ),
            rows,
        )
        self.assertIn(
            (
                "runtime",
                "crates/rssh-app/src/terminal_runtime.rs",
                "crates/rssh-runtime/src/terminal.rs",
            ),
            rows,
        )
        self.assertIn(("fonts", "crates/rssh-fonts", "crates/rterm-fonts"), rows)
        self.assertIn(
            (
                "render-cpu",
                "crates/rssh-renderer/src/text.rs",
                "crates/rterm-render-cpu/src/text.rs",
            ),
            rows,
        )
        self.assertIn(
            (
                "render-wgpu",
                "crates/rssh-renderer/src/gpu",
                "crates/rterm-render-wgpu/src/gpu",
            ),
            rows,
        )


if __name__ == "__main__":
    unittest.main()
