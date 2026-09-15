import json
import hashlib
import os
import subprocess
import sys
import unittest
import zipfile
import tarfile

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
            env={**os.environ, "CARGO_TARGET_DIR": str(self.root / "cargo-target"), "CARGO_NET_OFFLINE": "true", "TMPDIR": str(self.root), "TEMP": str(self.root), "TMP": str(self.root)})

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

    def configure_artifact(self, body=None):
        self.contract["consumer_artifacts"] = [{
            "after_command": 0, "path": "debug/rssh-app{exe_suffix}",
        }]
        self.write("verify.py", body if body is not None else (
            "import os\nfrom pathlib import Path\n"
            "p = Path(os.environ['CARGO_TARGET_DIR']) / 'debug' / ('rssh-app.exe' if os.name == 'nt' else 'rssh-app')\n"
            "p.parent.mkdir(parents=True, exist_ok=True)\n"
            "p.write_bytes(os.environ['RTERM_REHEARSAL_MODE'].encode())\n"
        ))
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("artifact contract")

    def configure_package(self):
        self.configure_artifact()
        self.contract["consumer_package"] = "native-unsigned-v1"
        for relative in ("scripts/ci/package-native.ps1", "scripts/ci/package-native.sh",
                         "packaging/rssh-console.cmd", "packaging/rssh-console.sh", "packaging/Info.plist"):
            self.write(relative, (fixtures.ROOT / relative).read_text(encoding="utf-8"))
        for relative in ("README.md", "LICENSE", "examples/rssh-profiles.toml",
                         "tests/fixtures/fonts/MANIFEST.tsv", "tests/fixtures/fonts/LICENSES/test.txt"):
            self.write(relative, "fixture packaging resource\n")
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("native package contract")

    def test_native_packages_retain_archive_hash_and_matching_binary_for_both_profiles(self):
        self.configure_package()
        result = self.rehearse()
        self.assertEqual(result.returncode, 0, result.stderr)
        for mode in ("candidate", "rollback"):
            evidence = json.loads((self.output / f"{mode}.json").read_text())
            self.assertIn("package", evidence)
            package = evidence["package"]
            archive = self.output / package["path"]
            self.assertTrue(package["ok"])
            self.assertEqual(package["sha256"], hashlib.sha256(archive.read_bytes()).hexdigest())
            self.assertEqual(package["binary_sha256"], evidence["artifacts"][0]["sha256"])
            member = "payload/" + package["binary"]
            if archive.suffix == ".zip":
                with zipfile.ZipFile(archive) as bundle:
                    binary = bundle.read(member)
            else:
                with tarfile.open(archive) as bundle:
                    binary = bundle.extractfile(member).read()
            self.assertEqual(binary, mode.encode())

    def test_packaging_failure_cannot_report_success(self):
        self.configure_package()
        for relative in ("scripts/ci/package-native.ps1", "scripts/ci/package-native.sh"):
            self.write(relative, "exit 7\n")
        self.candidate = self.commit("failed packaging")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertFalse(evidence["package"]["ok"])

    def test_product_contract_requires_native_package(self):
        contract = json.loads((fixtures.ROOT / "scripts/ci/rterm-release-contract.json").read_text())
        self.assertEqual(contract.get("consumer_package"), "native-unsigned-v1")

    def test_rollback_cannot_change_retained_candidate_package(self):
        self.configure_package()
        self.write("verify.py", (self.repo / "verify.py").read_text() + (
            "if os.environ['RTERM_REHEARSAL_MODE'] == 'rollback':\n"
            f"    for p in Path({str(self.output / 'candidate-package')!r}).glob('*-unsigned.*'): p.write_bytes(b'tampered')\n"
        ))
        self.candidate = self.commit("rollback tampers retained candidate archive")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertFalse(evidence["package"]["ok"])

    def test_malformed_package_manifest_retains_failure_evidence(self):
        self.configure_package()
        ps = "scripts/ci/package-native.ps1"
        self.write(ps, (self.repo / ps).read_text().replace(
            "$artifactPath =", '[IO.File]::WriteAllText((Join-Path $packageRootPath "manifest.json"), "[]")\n$artifactPath ='))
        sh = "scripts/ci/package-native.sh"
        self.write(sh, (self.repo / sh).read_text().replace(
            "package_parent=", "printf '[]' > \"$package_root/manifest.json\"\npackage_parent="))
        self.candidate = self.commit("malformed package manifest")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence_path = self.output / "candidate.json"
        self.assertTrue(evidence_path.exists(), result.stderr)
        evidence = json.loads(evidence_path.read_text())
        self.assertFalse(evidence["ok"])
        self.assertFalse(evidence["package"]["ok"])

    def test_artifact_hashes_are_retained_per_profile_before_shared_target_overwrite(self):
        self.configure_artifact()
        result = self.rehearse()
        self.assertEqual(result.returncode, 0, result.stderr)
        for mode in ("candidate", "rollback"):
            evidence = json.loads((self.output / f"{mode}.json").read_text())
            self.assertIn("artifacts", evidence)
            self.assertEqual(len(evidence["artifacts"]), 1)
            artifact = evidence["artifacts"][0]
            self.assertEqual(artifact["sha256"], hashlib.sha256(mode.encode()).hexdigest())
            self.assertEqual(artifact["size_bytes"], len(mode))
            self.assertEqual(artifact["after_command"], 0)

    def test_missing_required_artifact_fails_and_retains_evidence(self):
        self.configure_artifact("pass\n")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertEqual(evidence["commands"][-1]["kind"], "artifact-identity")

    def test_rollback_cannot_borrow_candidate_artifact(self):
        self.configure_artifact()
        self.write("verify.py", (self.repo / "verify.py").read_text().replace(
            "p.write_bytes(os.environ['RTERM_REHEARSAL_MODE'].encode())",
            "if os.environ['RTERM_REHEARSAL_MODE'] == 'candidate': p.write_bytes(b'candidate')"))
        self.candidate = self.commit("rollback does not produce artifact")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "rollback.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertEqual(evidence["artifacts"], [])

    def test_redirected_output_cannot_borrow_stale_declared_artifact(self):
        self.configure_artifact()
        self.write("verify.py", (self.repo / "verify.py").read_text().replace(
            "/ 'debug' /", "/ 'other-triple' / 'debug' /"))
        self.candidate = self.commit("redirected build output")
        stale = self.root / "cargo-target/debug" / ("rssh-app.exe" if os.name == "nt" else "rssh-app")
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b'old build')
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertEqual(evidence["artifacts"], [])

    def test_recovery_directory_link_cannot_move_artifact_outside_owned_work(self):
        self.configure_artifact()
        outside = self.root / "outside"
        outside.mkdir()
        self.write("contracts/probe/probe.py", (
            "import os, subprocess\nfrom pathlib import Path\n"
            "link = Path.cwd().parents[2] / 'candidate-prior-artifacts'\n"
            f"outside = Path({str(outside)!r})\n"
            "if os.name == 'nt':\n"
            "    subprocess.run(['powershell.exe', '-NoProfile', '-NonInteractive', '-Command', 'New-Item -ItemType Junction -Path $env:RSSH_TEST_LINK -Target $env:RSSH_TEST_TARGET | Out-Null'], env={**os.environ, 'RSSH_TEST_LINK': str(link), 'RSSH_TEST_TARGET': str(outside)}, check=True)\n"
            "else: link.symlink_to(outside, target_is_directory=True)\n"
        ))
        self.candidate = self.commit("linked recovery directory")
        stale = self.root / "cargo-target/debug" / ("rssh-app.exe" if os.name == "nt" else "rssh-app")
        stale.parent.mkdir(parents=True)
        stale.write_bytes(b'old build')
        result = self.rehearse()
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertEqual(evidence["commands"][0]["returncode"], 0, result.stderr)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(list(outside.iterdir()), [])
        self.assertEqual(stale.read_bytes(), b'old build')

    def test_artifact_changed_by_later_consumer_command_is_rejected(self):
        self.configure_artifact()
        self.write("mutate.py", (self.repo / "verify.py").read_text().replace(
            "os.environ['RTERM_REHEARSAL_MODE'].encode()", "b'changed'"))
        self.contract["consumer_commands"].append([sys.executable, "mutate.py"])
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("mutating built artifact")
        result = self.rehearse()
        self.assertNotEqual(result.returncode, 0)
        evidence = json.loads((self.output / "candidate.json").read_text())
        self.assertFalse(evidence["ok"])
        self.assertEqual(evidence["commands"][-1]["kind"], "artifact-identity")

    def test_product_contract_requires_built_executable_identity(self):
        contract = json.loads((fixtures.ROOT / "scripts/ci/rterm-release-contract.json").read_text())
        self.assertEqual(contract.get("consumer_artifacts"), [{
            "after_command": 4, "path": "debug/rssh-app{exe_suffix}",
        }])

    def test_real_cargo_build_regenerates_declared_output_with_cached_dependencies(self):
        self.configure_artifact()
        self.contract["consumer_commands"] = [["cargo", "build", "--locked", "-p", "rssh-app"]]
        self.write("scripts/ci/rterm-release-contract.json", json.dumps(self.contract))
        self.candidate = self.commit("actual cargo artifact")
        result = self.rehearse()
        self.assertEqual(result.returncode, 0, result.stderr)
        rollback = json.loads((self.output / "rollback.json").read_text())
        self.assertEqual(len(rollback["prior_artifacts"]), 1)
        self.assertEqual(len(rollback["artifacts"]), 1)
        self.assertGreater(rollback["artifacts"][0]["size_bytes"], 0)

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
