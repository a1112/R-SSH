import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[3]
CHECKER = REPOSITORY_ROOT / "scripts" / "ci" / "check-rust-architecture.py"


class RustArchitectureCheckerTests(unittest.TestCase):
    def test_stage1_workspace_declares_types_domain_and_compatibility_facade(self):
        workspace = (REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        core = (REPOSITORY_ROOT / "crates" / "rssh-core" / "Cargo.toml").read_text(
            encoding="utf-8"
        )

        self.assertIn('"crates/rterm-types"', workspace)
        self.assertIn('"crates/rssh-domain"', workspace)
        self.assertIn("rterm-types", core)
        self.assertIn("rssh-domain", core)

    def test_stage1_foundation_manifests_obey_one_way_dependency_direction(self):
        types = (
            REPOSITORY_ROOT / "crates" / "rterm-types" / "Cargo.toml"
        ).read_text(encoding="utf-8")
        domain = (
            REPOSITORY_ROOT / "crates" / "rssh-domain" / "Cargo.toml"
        ).read_text(encoding="utf-8")

        self.assertNotIn("rssh-", types)
        self.assertIn("rterm-types", domain)
        for forbidden in ("rssh-app", "rssh-runtime", "rssh-renderer", "rssh-ssh"):
            self.assertNotIn(forbidden, domain)

    def test_stage1_renames_terminal_and_font_packages_without_losing_compatibility_aliases(self):
        workspace = (REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8")
        terminal = (
            REPOSITORY_ROOT / "crates" / "rssh-terminal" / "Cargo.toml"
        ).read_text(encoding="utf-8")
        fonts = (
            REPOSITORY_ROOT / "crates" / "rterm-fonts" / "Cargo.toml"
        ).read_text(encoding="utf-8")

        self.assertIn('"crates/rssh-terminal"', workspace)
        self.assertIn('"crates/rterm-fonts"', workspace)
        self.assertNotIn('"crates/rssh-fonts"', workspace)
        self.assertIn('name = "rterm-terminal"', terminal)
        self.assertIn('name = "rterm-fonts"', fonts)

    def test_deterministic_performance_uses_the_stage1_terminal_package_name(self):
        workflow = (
            REPOSITORY_ROOT / ".github" / "workflows" / "ci.yml"
        ).read_text(encoding="utf-8")

        self.assertIn(
            "cargo test --locked -p rterm-terminal "
            "batched_scroll_prune_matches_incremental_prune",
            workflow,
        )

    def test_stage1_foundational_crates_import_owned_types_without_the_core_facade(self):
        expectations = {
            "rssh-terminal": ("rterm-types",),
            "rssh-renderer": ("rterm-types",),
            "rssh-ssh": ("rterm-types",),
            "rssh-runtime": ("rterm-types", "rssh-domain"),
            "rssh-native": ("rterm-types", "rssh-domain"),
        }

        for crate, required in expectations.items():
            manifest = (REPOSITORY_ROOT / "crates" / crate / "Cargo.toml").read_text(
                encoding="utf-8"
            )
            production = manifest.split("[dev-dependencies]", maxsplit=1)[0]
            self.assertNotIn("rssh-core", production, crate)
            for package in required:
                self.assertIn(package, manifest, crate)

    def test_readme_documents_stage1_package_ownership_and_facade_policy(self):
        readme = (REPOSITORY_ROOT / "README.md").read_text(encoding="utf-8")

        for required in (
            "rterm-types",
            "rssh-domain",
            "rterm-terminal",
            "rterm-fonts",
            "rssh-core compatibility facade",
        ):
            self.assertIn(required, readme)

    def test_stage2_runtime_package_is_transport_neutral(self):
        manifest = (
            REPOSITORY_ROOT / "crates" / "rssh-runtime" / "Cargo.toml"
        ).read_text(encoding="utf-8")
        transport = (
            REPOSITORY_ROOT / "crates" / "rssh-runtime" / "src" / "transport.rs"
        ).read_text(encoding="utf-8")

        self.assertIn('name = "rterm-runtime"', manifest)
        for forbidden in (
            "rssh-pty",
            "rssh-ssh",
            "local-transport",
            "ssh-transport",
            "transport-adapters",
        ):
            self.assertNotIn(forbidden, manifest)
        self.assertNotIn("mod local", transport)
        self.assertNotIn("mod ssh", transport)
        self.assertFalse(
            (REPOSITORY_ROOT / "crates" / "rssh-runtime" / "src" / "transport" / "local.rs").exists()
        )
        self.assertFalse(
            (REPOSITORY_ROOT / "crates" / "rssh-runtime" / "src" / "transport" / "ssh.rs").exists()
        )

    def test_stage2_concrete_crates_own_opt_in_runtime_adapters(self):
        for crate in ("rssh-pty", "rssh-ssh"):
            root = REPOSITORY_ROOT / "crates" / crate
            manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
            library = (root / "src" / "lib.rs").read_text(encoding="utf-8")

            self.assertIn("runtime-adapter", manifest, crate)
            self.assertIn("rterm-runtime", manifest, crate)
            self.assertIn("runtime_adapter", library, crate)
            self.assertTrue((root / "src" / "runtime_adapter.rs").exists(), crate)
            self.assertTrue((root / "tests" / "runtime_adapter.rs").exists(), crate)

    def test_readme_documents_stage2_runtime_transport_ownership(self):
        readme = (REPOSITORY_ROOT / "README.md").read_text(encoding="utf-8")

        for required in (
            "rterm-runtime",
            "transport-neutral",
            "runtime-adapter",
            "rssh-pty",
            "rssh-ssh",
        ):
            self.assertIn(required, readme)

    def test_stage3_workspace_declares_core_cpu_wgpu_and_compatibility_packages(self):
        workspace = (REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8")

        for member in (
            '"crates/rterm-render-core"',
            '"crates/rterm-render-cpu"',
            '"crates/rterm-render-wgpu"',
            '"crates/rssh-renderer"',
        ):
            self.assertIn(member, workspace)

        packages = {
            "rterm-render-core": "rterm-render-core",
            "rterm-render-cpu": "rterm-render-cpu",
            "rterm-render-wgpu": "rterm-render-wgpu",
            "rssh-renderer": "rssh-renderer",
        }
        for directory, package in packages.items():
            manifest = (
                REPOSITORY_ROOT / "crates" / directory / "Cargo.toml"
            ).read_text(encoding="utf-8")
            self.assertIn(f'name = "{package}"', manifest)

    def test_stage3_renderer_core_and_cpu_manifests_are_backend_neutral(self):
        core = (
            REPOSITORY_ROOT / "crates" / "rterm-render-core" / "Cargo.toml"
        ).read_text(encoding="utf-8")
        cpu = (
            REPOSITORY_ROOT / "crates" / "rterm-render-cpu" / "Cargo.toml"
        ).read_text(encoding="utf-8")

        for forbidden in (
            "wgpu",
            "glyphon",
            "raw-window-handle",
            "image =",
            "rssh-app",
            "rssh-pty",
            "rssh-ssh",
            "winit",
            "tauri",
        ):
            self.assertNotIn(forbidden, core)
        self.assertIn("rterm-render-core", cpu)
        for forbidden in ("wgpu", "glyphon", "raw-window-handle"):
            self.assertNotIn(forbidden, cpu)

    def test_stage3_app_composes_owned_renderers_without_the_compatibility_facade(self):
        app = (REPOSITORY_ROOT / "crates" / "rssh-app" / "Cargo.toml").read_text(
            encoding="utf-8"
        )
        facade = (
            REPOSITORY_ROOT / "crates" / "rssh-renderer" / "Cargo.toml"
        ).read_text(encoding="utf-8")
        facade_library = (
            REPOSITORY_ROOT / "crates" / "rssh-renderer" / "src" / "lib.rs"
        ).read_text(encoding="utf-8")

        for package in (
            "rterm-render-core",
            "rterm-render-cpu",
            "rterm-render-wgpu",
        ):
            self.assertIn(package, app)
            self.assertIn(package, facade)
        self.assertNotIn("rssh-renderer =", app)
        self.assertNotIn("wgpu =", facade)
        self.assertNotIn("image =", facade)
        self.assertIn("pub use rterm_render_core", facade_library)
        self.assertIn("pub use rterm_render_cpu", facade_library)
        self.assertIn("pub use rterm_render_wgpu", facade_library)

    def test_readme_documents_stage3_renderer_ownership(self):
        readme = (REPOSITORY_ROOT / "README.md").read_text(encoding="utf-8")

        for required in (
            "rterm-render-core",
            "rterm-render-cpu",
            "rterm-render-wgpu",
            "rssh-renderer compatibility facade",
        ):
            self.assertIn(required, readme)

    def test_quality_workflow_runs_the_checked_in_architecture_policy(self):
        workflow = (REPOSITORY_ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )

        self.assertIn(
            "python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json",
            workflow,
        )

    def test_checked_in_policy_accepts_the_current_migration_budget(self):
        result = subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--root",
                str(REPOSITORY_ROOT),
                "--policy",
                str(REPOSITORY_ROOT / "scripts" / "ci" / "architecture-policy.json"),
            ],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(parse_report(result)["ok"])

    def test_reports_exact_structural_items_and_limits(self):
        source = """\
pub struct OversizedState {
    first: u8,
    second: u8,
}

impl OversizedState {
    pub fn oversized(&self) {
        let first = 1;
        let second = 2;
    }
}
"""
        result = run_checker(
            {"src/oversized.rs": source},
            policy(limits={
                "file_lines": 8,
                "struct_fields": 1,
                "impl_lines": 5,
                "function_lines": 3,
                "rustfmt_skip": 0,
                "unbounded_channels": 0,
                "forbidden_dependencies": 0,
            }),
        )

        self.assertEqual(result.returncode, 1, result.stderr)
        report = parse_report(result)
        violations = {entry["rule"]: entry for entry in report["violations"]}
        self.assertEqual(violations["file_lines"]["file"], "src/oversized.rs")
        self.assertEqual(violations["file_lines"]["observed"], 11)
        self.assertEqual(violations["file_lines"]["limit"], 8)
        self.assertEqual(violations["struct_fields"]["item"], "OversizedState")
        self.assertEqual(violations["struct_fields"]["observed"], 2)
        self.assertEqual(violations["impl_lines"]["item"], "impl OversizedState")
        self.assertEqual(violations["function_lines"]["item"], "oversized")

    def test_masks_comments_strings_raw_strings_and_character_literals(self):
        source = r'''// std::sync::mpsc::channel::<u8>(); and unmatched {
const NORMAL: &str = "crossbeam_channel::unbounded() }";
const RAW: &str = r###"tokio::sync::mpsc::unbounded_channel(); {"###;
const BYTE_RAW: &[u8] = br#"mpsc::channel()"#;

pub fn tiny() {
    let brace = '{';
    /* nested { /* mpsc::channel() */ } */
}
'''
        result = run_checker({"src/masked.rs": source}, policy())

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(parse_report(result)["ok"])

    def test_reports_rustfmt_skip_unbounded_channel_and_forbidden_dependency(self):
        files = {
            "src/app.rs": """\
#[rustfmt::skip]
fn spawn() {
    let (_tx, _rx) = std::sync::mpsc::channel::<Vec<u8>>();
}
""",
            "src/config_lifecycle.rs": "use crate::window::NativeConfigOverrides;\n",
        }
        configured = policy()
        configured["forbidden_dependencies"] = [
            {"scope": "src/config_lifecycle.rs", "patterns": ["crate::window"]}
        ]

        result = run_checker(files, configured)

        self.assertEqual(result.returncode, 1, result.stderr)
        rules = {entry["rule"] for entry in parse_report(result)["violations"]}
        self.assertEqual(
            rules,
            {"rustfmt_skip", "unbounded_channels", "forbidden_dependencies"},
        )

    def test_production_channel_rule_ignores_cfg_test_modules_and_test_only_files(self):
        files = {
            "src/module.rs": """\
pub fn production() {}

#[cfg(test)]
mod tests {
    fn helper() {
        let (_tx, _rx) = std::sync::mpsc::channel::<u8>();
    }
}
""",
            "src/module_tests.rs": """\
fn helper() {
    let (_tx, _rx) = crossbeam_channel::unbounded::<u8>();
}
""",
        }
        configured = policy()
        configured["production_excluded_globs"] = ["src/*_tests.rs"]

        result = run_checker(files, configured)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_generated_globs_skip_structural_checks(self):
        generated = "\n".join(["pub const VALUE: usize = 1;"] * 50)
        configured = policy(limits={
            "file_lines": 5,
            "struct_fields": 1,
            "impl_lines": 5,
            "function_lines": 3,
            "rustfmt_skip": 0,
            "unbounded_channels": 0,
            "forbidden_dependencies": 0,
        })
        configured["generated_globs"] = ["src/generated/**"]

        result = run_checker({"src/generated/table.rs": generated}, configured)

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_migration_budget_accepts_current_value_but_cannot_exceed_ceiling(self):
        source = "\n".join(["pub const VALUE: usize = 1;"] * 12)
        configured = policy(limits={
            "file_lines": 8,
            "struct_fields": 1,
            "impl_lines": 5,
            "function_lines": 3,
            "rustfmt_skip": 0,
            "unbounded_channels": 0,
            "forbidden_dependencies": 0,
        })
        configured["migration"] = {
            "initial_ceilings": {"src/legacy.rs": {"file_lines": 12}},
            "budgets": {"src/legacy.rs": {"file_lines": 12}},
        }

        accepted = run_checker({"src/legacy.rs": source}, configured)
        self.assertEqual(accepted.returncode, 0, accepted.stderr)

        configured["migration"]["budgets"]["src/legacy.rs"]["file_lines"] = 13
        rejected = run_checker({"src/legacy.rs": source}, configured)
        self.assertEqual(rejected.returncode, 1, rejected.stderr)
        violation = parse_report(rejected)["violations"][0]
        self.assertEqual(violation["rule"], "policy_budget")
        self.assertEqual(violation["observed"], 13)
        self.assertEqual(violation["limit"], 12)


def policy(limits=None):
    return {
        "version": 1,
        "roots": ["src"],
        "generated_globs": [],
        "limits": limits or {
            "file_lines": 8000,
            "struct_fields": 64,
            "impl_lines": 2000,
            "function_lines": 300,
            "rustfmt_skip": 0,
            "unbounded_channels": 0,
            "forbidden_dependencies": 0,
        },
        "migration": {"initial_ceilings": {}, "budgets": {}},
        "forbidden_dependencies": [],
    }


def run_checker(files, configured_policy):
    with tempfile.TemporaryDirectory(prefix="rssh-architecture-test-") as temporary:
        root = Path(temporary)
        for relative, contents in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(contents, encoding="utf-8")
        policy_path = root / "policy.json"
        policy_path.write_text(json.dumps(configured_policy), encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(CHECKER),
                "--root",
                str(root),
                "--policy",
                str(policy_path),
            ],
            check=False,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )


def parse_report(result):
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise AssertionError(
            f"checker did not emit JSON: stdout={result.stdout!r} stderr={result.stderr!r}"
        ) from error


if __name__ == "__main__":
    unittest.main()
