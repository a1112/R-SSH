import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
CHECKER = REPOSITORY_ROOT / "scripts" / "ci" / "check-functional-matrix.py"


def load_checker():
    spec = importlib.util.spec_from_file_location("check_functional_matrix", CHECKER)
    if spec is None or spec.loader is None:
        raise AssertionError("load functional matrix checker")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def ndjson(scenario: str, target: str, outcome: str = "passed", attempt: int = 0) -> str:
    base = {
        "schema": 1,
        "run_id": {"scenario_id": scenario, "target": target, "attempt": attempt},
        "monotonic_ms": 0,
    }
    started = {**base, "sequence": 1, "event": "scenario_started", "capabilities": []}
    finished = {
        **base,
        "sequence": 2,
        "event": "scenario_finished",
        "outcome": outcome,
    }
    return json.dumps(started) + "\n" + json.dumps(finished) + "\n"


def playwright(project: str, status: str = "passed", retry: bool = False) -> dict:
    results = [{"status": "failed"}, {"status": status}] if retry else [{"status": status}]
    return {
        "config": {"projects": [{"name": project}]},
        "suites": [
            {
                "title": "terminal.spec.ts",
                "specs": [
                    {
                        "title": "opens a PTY",
                        "tests": [{"projectName": project, "results": results}],
                    }
                ],
            }
        ],
    }


class FunctionalMatrixCheckerTests(unittest.TestCase):
    def make_fixture(self):
        root = tempfile.TemporaryDirectory()
        path = Path(root.name)
        suite = path / "suite"
        evidence = path / "evidence"
        (suite / "scenarios").mkdir(parents=True)
        evidence.mkdir()
        (suite / "scenarios" / "cli.toml").write_text(
            'schema = 1\nid = "cli.real"\n', encoding="utf-8"
        )
        (suite / "scenarios" / "web.toml").write_text(
            'schema = 1\nid = "web.real"\n', encoding="utf-8"
        )
        matrix = suite / "matrix.toml"
        matrix.write_text(
            '''schema = 1
[[scenario_runs]]
scenario_id = "cli.real"
targets = ["linux", "windows"]

[[playwright_runs]]
scenario_id = "web.real"
identity = "terminal.spec.ts › opens a PTY"
projects = ["chromium", "firefox", "webkit"]
''',
            encoding="utf-8",
        )
        return root, suite, matrix, evidence

    def run_checker(self, suite: Path, matrix: Path, evidence: Path):
        return subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--suite",
                str(suite),
                "--matrix",
                str(matrix),
                "--evidence-root",
                str(evidence),
            ],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def test_accepts_exact_fixed_scenario_and_browser_matrix(self):
        root, suite, matrix, evidence = self.make_fixture()
        self.addCleanup(root.cleanup)
        (evidence / "cli-linux.ndjson").write_text(ndjson("cli.real", "linux"), encoding="utf-8")
        (evidence / "cli-windows.ndjson").write_text(
            ndjson("cli.real", "windows"), encoding="utf-8"
        )
        for project in ("chromium", "firefox", "webkit"):
            (evidence / f"web.{project}.playwright.json").write_text(
                json.dumps(playwright(project)), encoding="utf-8"
            )

        result = self.run_checker(suite, matrix, evidence)

        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report["scenario_runs"], 2)
        self.assertEqual(report["playwright_runs"], 3)

    def test_rejects_missing_duplicate_unapproved_and_retried_runs(self):
        root, suite, matrix, evidence = self.make_fixture()
        self.addCleanup(root.cleanup)
        (evidence / "cli-linux.ndjson").write_text(ndjson("cli.real", "linux"), encoding="utf-8")
        (evidence / "duplicate.ndjson").write_text(ndjson("cli.real", "linux"), encoding="utf-8")
        (evidence / "unapproved.ndjson").write_text(ndjson("cli.real", "freebsd"), encoding="utf-8")
        (evidence / "web.chromium.playwright.json").write_text(
            json.dumps(playwright("chromium", retry=True)), encoding="utf-8"
        )

        result = self.run_checker(suite, matrix, evidence)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("duplicate", result.stderr)
        self.assertIn("matrix", result.stderr)
        self.assertIn("retry", result.stderr)
        self.assertIn("missing", result.stderr)

    def test_repository_matrix_names_every_approved_scenario_once(self):
        checker = load_checker()
        errors = checker.validate_catalog(
            REPOSITORY_ROOT / "functional-tests",
            REPOSITORY_ROOT / "functional-tests" / "matrix.toml",
        )

        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
