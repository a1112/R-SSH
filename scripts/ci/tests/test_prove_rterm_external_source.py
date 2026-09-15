from __future__ import annotations

import json
import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
PROOF_TOOL = REPOSITORY_ROOT / "scripts" / "ci" / "prove-rterm-external-source.py"
STAGE_CONTRACT = REPOSITORY_ROOT / "scripts" / "ci" / "stage7-split-contract.json"

PACKAGE_SPECS = [
    ("rterm-types", "crates/rterm-types", []),
    ("rterm-terminal", "crates/rterm-terminal", ["rterm-types"]),
    ("rterm-runtime", "crates/rterm-runtime", ["rterm-types", "rterm-terminal"]),
    ("rterm-fonts", "crates/rterm-fonts", []),
    (
        "rterm-render-core",
        "crates/rterm-render-core",
        ["rterm-types", "rterm-terminal", "rterm-fonts"],
    ),
    (
        "rterm-render-cpu",
        "crates/rterm-render-cpu",
        ["rterm-types", "rterm-fonts", "rterm-render-core"],
    ),
    (
        "rterm-render-wgpu",
        "crates/rterm-render-wgpu",
        ["rterm-types", "rterm-fonts", "rterm-render-core", "rterm-render-cpu"],
    ),
]


def run(*arguments: str, cwd: Path, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(arguments),
        cwd=cwd,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )


def git(repository: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    return run("git", "-C", str(repository), *arguments, cwd=repository)


def commit(repository: Path, message: str) -> str:
    result = git(repository, "add", ".")
    if result.returncode:
        raise AssertionError(result.stderr)
    result = git(repository, "commit", "--quiet", "-m", message)
    if result.returncode:
        raise AssertionError(result.stderr)
    result = git(repository, "rev-parse", "HEAD")
    if result.returncode:
        raise AssertionError(result.stderr)
    return result.stdout.strip()


def write(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def load_checker():
    specification = importlib.util.spec_from_file_location(
        "stage7_split_gate_for_external_source", REPOSITORY_ROOT / "scripts/ci/check-stage7-split-gate.py"
    )
    assert specification is not None and specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


class RTermExternalSourceProofTests(unittest.TestCase):
    def init_repository(self, path: Path) -> None:
        result = run("git", "init", "--quiet", "-b", "main", str(path), cwd=path.parent)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(git(path, "config", "user.name", "Stage 8 Test").returncode, 0)
        self.assertEqual(
            git(path, "config", "user.email", "stage8@example.invalid").returncode,
            0,
        )

    def package_manifest(self, name: str, dependencies: list[str]) -> str:
        lines = [
            "[package]",
            f'name = "{name}"',
            'version = "0.1.0"',
            'edition = "2021"',
            "",
            "[lib]",
            'path = "src/lib.rs"',
        ]
        if dependencies:
            lines.extend(["", "[dependencies]"])
            for dependency in dependencies:
                dependency_path = next(path for pkg, path, _ in PACKAGE_SPECS if pkg == dependency)
                lines.append(
                    f'{dependency} = {{ path = "../{Path(dependency_path).name if dependency_path.startswith("crates/") else dependency_path}" }}'
                )
        if name == "rterm-render-wgpu":
            lines.extend(["glyphon = \"0.1.0\""])
        return "\n".join(lines) + "\n"

    def create_workspace_packages(self, repository: Path) -> None:
        for name, relative, dependencies in PACKAGE_SPECS:
            package_path = repository / relative
            write(package_path / "Cargo.toml", self.package_manifest(name, dependencies))
            write(
                package_path / "src/lib.rs",
                f"pub const PACKAGE: &str = \"{name}\";\n",
            )
        write(
            repository / "vendor/glyphon-0.1.0/Cargo.toml",
            "[package]\nname = \"glyphon\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\ngpu-allocator = \"0.1.0\"\n",
        )
        write(
            repository / "vendor/glyphon-0.1.0/src/lib.rs",
            "pub fn glyph() -> gpu_allocator::Allocator { gpu_allocator::Allocator }\n",
        )
        write(
            repository / "vendor/gpu-allocator-0.1.0/Cargo.toml",
            "[package]\nname = \"gpu-allocator\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\nname = \"gpu_allocator\"\n",
        )
        write(
            repository / "vendor/gpu-allocator-0.1.0/src/lib.rs",
            "pub struct Allocator;\n",
        )
        write(
            repository / "Cargo.toml",
            "[workspace]\nresolver = \"2\"\nmembers = [\n"
            + "\n".join(f'    "{path}",' for _name, path, _deps in PACKAGE_SPECS)
            + "\n]\n\n[workspace.package]\nversion = \"0.1.0\"\n\n[patch.crates-io]\nglyphon = { path = \"vendor/glyphon-0.1.0\" }\ngpu-allocator = { path = \"vendor/gpu-allocator-0.1.0\" }\n",
        )
        write(repository / "rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n")

    def create_consumer(self, repository: Path) -> None:
        dependency_lines = []
        for name, relative, _deps in PACKAGE_SPECS:
            dependency_lines.append(
                f'{name} = {{ path = "../../{relative}", version = "0.1.0" }}'
            )
        write(
            repository / "contracts/rterm-consumer/Cargo.toml",
            "[package]\nname = \"rterm-consumer-contract\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n\n[dependencies]\n"
            + "\n".join(dependency_lines)
            + "\n\n[patch.crates-io]\nglyphon = { path = \"../../vendor/glyphon-0.1.0\" }\ngpu-allocator = { path = \"../../vendor/gpu-allocator-0.1.0\" }\n",
        )
        write(
            repository / "contracts/rterm-consumer/src/lib.rs",
            "pub fn consumer() { let _ = rterm_render_wgpu::PACKAGE; }\n",
        )
        env = os.environ.copy()
        env["CARGO_TERM_COLOR"] = "never"
        env["CARGO_TARGET_DIR"] = str(repository.parent / "consumer-target")
        generated = run(
            "cargo",
            "generate-lockfile",
            cwd=repository / "contracts/rterm-consumer",
            env=env,
        )
        self.assertEqual(generated.returncode, 0, generated.stderr)

    def make_fixture(self, root: Path) -> tuple[Path, Path, Path, dict[str, str]]:
        source = root / "rssh-source"
        source.mkdir()
        self.init_repository(source)
        self.create_workspace_packages(source)
        self.create_consumer(source)
        write(source / "product.txt", "consumer product\n")
        lkg = commit(source, "lkg")
        write(source / "product.txt", "current consumer product\n")
        source_ref = commit(source, "source head")
        self.assertEqual(git(source, "status", "--porcelain").stdout, "")

        candidate = root / "rterm-candidate"
        candidate.mkdir()
        self.init_repository(candidate)
        self.create_workspace_packages(candidate)
        candidate_env = os.environ.copy()
        candidate_env["CARGO_TARGET_DIR"] = str(candidate.parent / "candidate-target")
        generated = run("cargo", "generate-lockfile", cwd=candidate, env=candidate_env)
        self.assertEqual(generated.returncode, 0, generated.stderr)
        candidate_ref = commit(candidate, "candidate")

        contract = {
            "schema": "rssh.stage7/rterm-external-source-proof/v1",
            "source_repository": str(source),
            "lkg_rssh_ref": lkg,
            "candidate": {
                "package_paths": [relative for _name, relative, _deps in PACKAGE_SPECS],
                "vendor_paths": ["vendor/glyphon-0.1.0", "vendor/gpu-allocator-0.1.0"],
                "workspace_files": ["Cargo.toml", "rust-toolchain.toml"],
            },
            "consumer": {
                "path": "contracts/rterm-consumer",
                "manifest": "Cargo.toml",
                "lockfile": "Cargo.lock",
                "vendor_root": "vendor",
                "dependencies": [name for name, _path, _deps in PACKAGE_SPECS],
            },
            "metadata_command": ["cargo", "metadata", "--locked", "--format-version", "1"],
            "locked_commands": [["cargo", "check", "--locked"]],
        }
        contract_path = root / "contract.json"
        contract_path.write_text(json.dumps(contract, indent=2), encoding="utf-8")
        return source, candidate, contract_path, {
            "lkg": lkg,
            "source": source_ref,
            "candidate": candidate_ref,
        }

    def invoke(
        self,
        contract: Path,
        output: Path,
        *arguments: str,
    ) -> subprocess.CompletedProcess[str]:
        return run(
            sys.executable,
            str(PROOF_TOOL),
            "--contract",
            str(contract),
            "--output",
            str(output),
            *arguments,
            cwd=REPOSITORY_ROOT,
        )

    def test_modes_and_refs_are_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, candidate, contract, refs = self.make_fixture(root)
            cases = [
                ((), "exactly one proof mode"),
                (("--synthesize", "--candidate-repo", str(candidate), "--candidate-ref", refs["candidate"]), "mutually exclusive"),
                (("--candidate-repo", str(candidate), "--candidate-ref", "main"), "full 40-character"),
                (("--candidate-repo", str(candidate), "--candidate-ref", refs["candidate"][:8]), "full 40-character"),
            ]
            for arguments, expected in cases:
                with self.subTest(arguments=arguments):
                    result = self.invoke(contract, root / "failure", *arguments)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(expected, (result.stderr + result.stdout).lower())

    def test_candidate_must_be_clean_and_head_must_match_requested_sha(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _source, candidate, contract, refs = self.make_fixture(root)
            write(candidate / "dirty.txt", "not committed\n")
            dirty = self.invoke(
                contract,
                root / "dirty-output",
                "--candidate-repo",
                str(candidate),
                "--candidate-ref",
                refs["candidate"],
            )
            self.assertNotEqual(dirty.returncode, 0)
            self.assertIn("dirty", (dirty.stderr + dirty.stdout).lower())
            (candidate / "dirty.txt").unlink()
            self.assertEqual(git(candidate, "commit", "--quiet", "--allow-empty", "-m", "second candidate").returncode, 0)
            new_ref = git(candidate, "rev-parse", "HEAD").stdout.strip()
            mismatch = self.invoke(
                contract,
                root / "mismatch-output",
                "--candidate-repo",
                str(candidate),
                "--candidate-ref",
                refs["candidate"],
            )
            self.assertNotEqual(mismatch.returncode, 0)
            self.assertIn("head", (mismatch.stderr + mismatch.stdout).lower())
            self.assertNotEqual(new_ref, refs["candidate"])

    def test_synthesize_proves_git_sources_vendors_and_rollback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _source, _candidate, contract, refs = self.make_fixture(root)
            output = root / "synth-output"
            result = self.invoke(contract, output, "--synthesize")
            self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
            fragment = json.loads((output / "artifact-manifest-fragment.json").read_text())
            self.assertEqual(fragment["requested_state"], "attribution-ready")
            entry = fragment["entries"][0]
            payload = json.loads((output / entry["path"]).read_text())
            self.assertEqual(payload["mode"], "synthesize")
            self.assertFalse((output / "work").exists())
            self.assertEqual(entry["artifact_type"], "local-two-bare-git-source-proof")
            self.assertRegex(payload["candidate_ref"], r"^[0-9a-f]{40}$")
            self.assertRegex(payload["source_switch_ref"], r"^[0-9a-f]{40}$")
            self.assertRegex(payload["rollback_ref"], r"^[0-9a-f]{40}$")
            self.assertEqual(payload["source_refs"], [refs["source"], refs["lkg"]])
            self.assertNotEqual(
                payload["bare_repositories"]["candidate"]["identity"],
                payload["bare_repositories"]["consumer"]["identity"],
            )
            self.assertEqual(set(payload["metadata"]["rterm_sources"]), set(PACKAGE_SPECS[i][0] for i in range(7)))
            for package in payload["metadata"]["packages"]:
                if package["name"] in {name for name, _path, _deps in PACKAGE_SPECS}:
                    self.assertIn(f"#{payload['candidate_ref']}", package["source"])
            for name in ("glyphon", "gpu-allocator"):
                vendor_path = Path(payload["vendor_resolutions"][name]["manifest_path"]).resolve()
                self.assertTrue(str(vendor_path).lower().startswith(str(payload["consumer_root"]).lower()))
            self.assertEqual(payload["rollback"]["manifest_sha256"], payload["baseline"]["manifest_sha256"])
            self.assertEqual(payload["rollback"]["lockfile_sha256"], payload["baseline"]["lockfile_sha256"])
            post_commit = payload["commands"][payload["source_switch_command_count"] :]
            self.assertTrue(post_commit)
            self.assertTrue(all("--locked" in item["argv"] for item in post_commit))
            self.assertFalse(any("generate-lockfile" in item["argv"] for item in post_commit))
            violations: list[str] = []
            checker_contract = json.loads(STAGE_CONTRACT.read_text(encoding="utf-8"))
            checker_contract["lkg_rssh_ref"] = refs["lkg"]
            load_checker().validate_result_artifact(
                "local-two-bare-git-source-proof",
                payload,
                checker_contract,
                {},
                _source,
                "synthesized local source proof",
                violations,
            )
            self.assertEqual(violations, [], "\\n".join(violations))

    def test_canonical_mode_binds_r1_and_does_not_modify_candidate_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            _source, candidate, contract, refs = self.make_fixture(root)
            before = run("git", "-C", str(candidate), "rev-parse", "HEAD^{tree}", cwd=root).stdout.strip()
            output = root / "canonical-output"
            result = self.invoke(
                contract,
                output,
                "--candidate-repo",
                str(candidate),
                "--candidate-ref",
                refs["candidate"],
            )
            self.assertEqual(result.returncode, 0, result.stderr or result.stdout)
            after = run("git", "-C", str(candidate), "rev-parse", "HEAD^{tree}", cwd=root).stdout.strip()
            self.assertEqual(before, after)
            fragment = json.loads((output / "artifact-manifest-fragment.json").read_text())
            entry = fragment["entries"][0]
            payload = json.loads((output / entry["path"]).read_text())
            self.assertEqual(payload["mode"], "canonical")
            self.assertEqual(payload["r1_ref"], refs["candidate"])
            self.assertEqual(entry["artifact_type"], "rterm-external-source-proof")
            for package in payload["metadata"]["packages"]:
                if package["name"] in {name for name, _path, _deps in PACKAGE_SPECS}:
                    self.assertIn(f"#{refs['candidate']}", package["source"])

    def test_contract_rejects_escape_and_path_file_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, _candidate, contract_path, _refs = self.make_fixture(root)
            contract = json.loads(contract_path.read_text())
            contract["candidate"]["package_paths"][0] = "../escape"
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            escaped = self.invoke(contract_path, root / "escape-output", "--synthesize")
            self.assertNotEqual(escaped.returncode, 0)
            self.assertIn("contain", (escaped.stderr + escaped.stdout).lower())

            contract["candidate"]["package_paths"][0] = PACKAGE_SPECS[0][1]
            manifest = source / "contracts/rterm-consumer/Cargo.toml"
            original = manifest.read_text(encoding="utf-8")
            manifest.write_text(
                original.replace(
                    'rterm-types = { path = "../../crates/rterm-types", version = "0.1.0" }',
                    'rterm-types = { path = "../../crates/rterm-types", file = "../../outside", version = "0.1.0" }',
                ),
                encoding="utf-8",
            )
            commit(source, "invalid path file source")
            contract_path.write_text(json.dumps(contract), encoding="utf-8")
            invalid_source = self.invoke(contract_path, root / "path-file-output", "--synthesize")
            self.assertNotEqual(invalid_source.returncode, 0)
            self.assertIn("path+file", (invalid_source.stderr + invalid_source.stdout).lower())


if __name__ == "__main__":
    unittest.main()
