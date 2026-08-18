import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
REHEARSAL = REPOSITORY_ROOT / "scripts" / "ci" / "rehearse-rterm-consumer.py"


def run(*arguments: str, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(arguments),
        cwd=cwd,
        text=True,
        capture_output=True,
        check=False,
    )


class RTermConsumerRehearsalTests(unittest.TestCase):
    def make_fixture(self, root: Path, failing: bool = False) -> tuple[Path, Path, dict[str, str]]:
        repository = root / "fixture-repository"
        repository.mkdir()
        self.assertEqual(run("git", "init", "-b", "main", cwd=repository).returncode, 0)
        run("git", "config", "user.email", "stage6@example.invalid", cwd=repository)
        run("git", "config", "user.name", "Stage 6 Test", cwd=repository)

        (repository / "rterm" / "pkg").mkdir(parents=True)
        (repository / "vendor" / "dep").mkdir(parents=True)
        (repository / "rterm" / "pkg" / "value.txt").write_text(
            "rollback", encoding="utf-8"
        )
        (repository / "vendor" / "dep" / "value.txt").write_text(
            "rollback-vendor", encoding="utf-8"
        )
        (repository / "product.txt").write_text("consumer-product", encoding="utf-8")
        (repository / "probe.py").write_text(
            "from pathlib import Path\n"
            "assert Path('rterm/pkg/value.txt').read_text() in {'rollback', 'candidate'}\n",
            encoding="utf-8",
        )
        expected_exit = "raise SystemExit(7)" if failing else ""
        (repository / "verify.py").write_text(
            "import os\n"
            "from pathlib import Path\n"
            "expected = 'candidate' if os.environ['RTERM_REHEARSAL_MODE'] == 'candidate' else 'rollback'\n"
            "assert Path('rterm/pkg/value.txt').read_text() == expected\n"
            "assert Path('product.txt').read_text() == 'consumer-product'\n"
            f"{expected_exit}\n",
            encoding="utf-8",
        )
        run("git", "add", ".", cwd=repository)
        self.assertEqual(run("git", "commit", "-m", "lkg", cwd=repository).returncode, 0)
        lkg = run("git", "rev-parse", "HEAD", cwd=repository).stdout.strip()

        (repository / "consumer-only.txt").write_text("consumer", encoding="utf-8")
        run("git", "add", ".", cwd=repository)
        self.assertEqual(
            run("git", "commit", "-m", "consumer", cwd=repository).returncode, 0
        )
        consumer = run("git", "rev-parse", "HEAD", cwd=repository).stdout.strip()

        (repository / "rterm" / "pkg" / "value.txt").write_text(
            "candidate", encoding="utf-8"
        )
        (repository / "vendor" / "dep" / "value.txt").write_text(
            "candidate-vendor", encoding="utf-8"
        )
        run("git", "add", ".", cwd=repository)
        self.assertEqual(
            run("git", "commit", "-m", "candidate", cwd=repository).returncode, 0
        )
        candidate = run("git", "rev-parse", "HEAD", cwd=repository).stdout.strip()

        contract = {
            "last_known_good_rterm_ref": lkg,
            "packages": [
                {
                    "name": "rterm-types",
                    "path": "rterm/pkg",
                    "version": "0.1.0",
                    "dependencies": [],
                }
            ],
            "vendor_trees": [
                {"name": "dep", "path": "vendor/dep", "tree": "0" * 40}
            ],
            "standalone_probe": {
                "path": ".",
                "command": [sys.executable, "probe.py"],
            },
            "consumer_commands": [[sys.executable, "verify.py"]],
        }
        contract_path = root / "contract.json"
        contract_path.write_text(json.dumps(contract), encoding="utf-8")
        return repository, contract_path, {
            "candidate": candidate,
            "consumer": consumer,
            "rollback": lkg,
        }

    def rehearse(
        self, repository: Path, contract: Path, refs: dict[str, str], output: Path
    ) -> subprocess.CompletedProcess[str]:
        return run(
            sys.executable,
            str(REHEARSAL),
            "--repo",
            str(repository),
            "--contract",
            str(contract),
            "--candidate-ref",
            refs["candidate"],
            "--consumer-ref",
            refs["consumer"],
            "--output-dir",
            str(output),
            cwd=REPOSITORY_ROOT,
        )

    def test_candidate_and_rollback_use_clean_clones_and_record_immutable_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository, contract, refs = self.make_fixture(root)
            output = root / "evidence"

            result = self.rehearse(repository, contract, refs, output)

            self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
            for mode in ("candidate", "rollback"):
                evidence = json.loads(
                    (output / f"{mode}.json").read_text(encoding="utf-8")
                )
                self.assertTrue(evidence["ok"])
                self.assertRegex(evidence["source_commit"], r"^[0-9a-f]{40}$")
                self.assertEqual(evidence["consumer_commit"], refs["consumer"])
                self.assertEqual(evidence["overlay_paths"], ["rterm/pkg", "vendor/dep"])
                self.assertTrue(all(command["returncode"] == 0 for command in evidence["commands"]))
            self.assertFalse((output / "work").exists())

    def test_failure_is_propagated_and_keeps_checkouts_for_diagnosis(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository, contract, refs = self.make_fixture(root, failing=True)
            output = root / "evidence"

            result = self.rehearse(repository, contract, refs, output)

            self.assertNotEqual(result.returncode, 0)
            evidence = json.loads((output / "candidate.json").read_text(encoding="utf-8"))
            self.assertFalse(evidence["ok"])
            self.assertEqual(evidence["commands"][-1]["returncode"], 7)
            self.assertTrue((output / "work" / "candidate-consumer").is_dir())

    def test_overlay_paths_must_be_contained_and_cannot_own_product_crates(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository, contract_path, refs = self.make_fixture(root)
            for forbidden in ("../escape", "crates/rssh-app"):
                contract = json.loads(contract_path.read_text(encoding="utf-8"))
                contract["packages"][0]["path"] = forbidden
                contract_path.write_text(json.dumps(contract), encoding="utf-8")

                result = self.rehearse(repository, contract_path, refs, root / forbidden.replace("/", "_"))

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("refusing overlay path", result.stderr)


if __name__ == "__main__":
    unittest.main()
