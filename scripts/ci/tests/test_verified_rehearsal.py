import json
import os
import subprocess
import sys
import unittest

import test_prepare_rterm_consumer as fixtures


class VerifiedRehearsalTests(unittest.TestCase):
    write = fixtures.PrepareConsumerTests.write
    commit = fixtures.PrepareConsumerTests.commit

    def setUp(self):
        fixtures.PrepareConsumerTests.setUp(self)
        self.contract.update({
            "consumer_preparation": "verified-dual-adapter-v1",
            "consumer_prepare_command": ["cargo", "generate-lockfile"],
            "standalone_probe": {"path": "contracts/probe", "command": [sys.executable, "probe.py"]},
            "consumer_commands": [[sys.executable, "verify.py"]],
        })
        self.write("contracts/probe/probe.py", "print('standalone probe')\n")
        self.write("verify.py", "from pathlib import Path\nassert Path('crates/rssh-app/src/main.rs').read_text() == 'fn main() {}\\n'\n")
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("verified rehearsal contract")

    def rehearse(self):
        return subprocess.run([
            sys.executable, str(fixtures.ROOT / "scripts/ci/rehearse-rterm-consumer.py"),
            "--repo", str(self.repo), "--contract", str(self.contract_path),
            "--candidate-ref", self.candidate, "--consumer-ref", self.candidate,
            "--output-dir", str(self.output),
        ], capture_output=True, text=True, timeout=120,
            env={**os.environ, "CARGO_NET_OFFLINE": "true", "TMPDIR": str(self.root), "TEMP": str(self.root), "TMP": str(self.root)})

    def test_both_profiles_keep_receipts_and_all_consumer_commands(self):
        result = self.rehearse()
        self.assertEqual(result.returncode, 0, result.stderr)
        for mode, profile in (("candidate", "modern"), ("rollback", "legacy-0.1")):
            evidence = json.loads((self.output / f"{mode}.json").read_text())
            self.assertTrue(evidence["ok"])
            self.assertTrue(evidence["post_consumer_identity"]["ok"])
            receipt = evidence["preparation"]
            self.assertEqual(receipt["profile"], profile)
            self.assertFalse(receipt["compatibility_verified"])
            self.assertEqual(receipt["consumer_commit"], self.candidate)
            self.assertEqual(receipt["source_commit"], self.candidate if mode == "candidate" else self.lkg)
            self.assertEqual([c["argv"] for c in evidence["commands"] if c["kind"] == "consumer"], self.contract["consumer_commands"])
            self.assertEqual(len(receipt["source_trees"]), 9)
        self.assertEqual(fixtures.git(self.repo, "status", "--porcelain"), "")

    def test_consumer_mutation_cannot_report_success(self):
        self.write("verify.py", "from pathlib import Path\nPath('crates/rssh-app/src/main.rs').write_text('tampered')\n")
        self.candidate = self.commit("mutating consumer command")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertFalse(evidence["post_consumer_identity"]["ok"])

    def test_dirty_contract_is_rejected_before_any_probe_runs(self):
        sentinel = self.root / "probe-ran"
        self.contract["standalone_probe"]["command"] = [
            sys.executable, "-c",
            f"from pathlib import Path; Path({str(sentinel)!r}).touch()",
        ]
        for selector in ("verified-dual-adapter-v1", None, "absent"):
            with self.subTest(selector=selector):
                self.contract["consumer_preparation"] = selector
                if selector == "absent":
                    del self.contract["consumer_preparation"]
                self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
                result = self.rehearse()
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(sentinel.exists(), "unverified probe command executed")

    def test_frozen_source_and_lock_mutations_are_rejected(self):
        for index, path in enumerate(("crates/rterm-fonts/src/lib.rs", "Cargo.lock", "injected.txt")):
            with self.subTest(path=path):
                self.output = self.root / f"mutation-{index}"
                self.write("verify.py", f"from pathlib import Path\nPath({path!r}).write_text('tampered')\n")
                self.candidate = self.commit(f"mutating {index}")
                result = self.rehearse()
                self.assertNotEqual(result.returncode, 0)
                evidence = json.loads((self.output / "candidate.json").read_text())
                self.assertFalse(evidence["ok"])
                self.assertFalse(evidence["post_consumer_identity"]["ok"])

    def test_failed_consumer_retains_receipt_and_never_runs_later_commands(self):
        self.write("verify.py", "raise SystemExit(7)\n")
        self.contract["consumer_commands"].append([sys.executable, "-c", "raise SystemExit(99)"])
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("failed command")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertTrue(evidence["preparation"]["ok"])
        self.assertFalse(evidence["ok"])
        self.assertEqual(evidence["commands"][-1]["returncode"], 7)
        self.assertEqual(sum(c["kind"] == "consumer" for c in evidence["commands"]), 1)

    def test_repository_contract_requires_verified_preparation(self):
        contract = json.loads((fixtures.ROOT / "scripts/ci/rterm-release-contract.json").read_text())
        self.assertEqual(contract.get("consumer_preparation"), "verified-dual-adapter-v1")
        self.assertEqual(contract["last_known_good_rterm_ref"], "0e8ebd5de22758275cbb6a849c19c032268d7fac")
        self.assertEqual(contract["consumer_prepare_command"], ["cargo", "generate-lockfile"])

    def test_preparation_failure_retains_error_and_never_runs_consumer(self):
        self.write("crates/rssh-app/Cargo.toml", fixtures.APP.decode().replace('version = "0.1.0"', 'version = "99.0.0"'))
        self.candidate = self.commit("unresolvable dependency")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertIn("preparation", evidence)
        self.assertFalse(evidence["preparation"]["ok"])
        self.assertFalse(any(c["kind"] == "consumer" for c in evidence["commands"]))


if __name__ == "__main__":
    unittest.main()
