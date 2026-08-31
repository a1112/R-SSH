# Stage 7 Protected CI Gate Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Allow the complete Stage 7 Gate 0 proof to run for an explicitly selected branch on the protected Windows performance runner and produce an authoritative `attribution-ready` decision.

**Architecture:** Extend the existing release workflow with one false-by-default `stage7_gate_only` manual input. Reuse the fixed-performance prerequisite, protected environment, fixed-runner labels, and concurrency group; skip unrelated packaging for opt-in Stage 7 runs, then execute all four proof producers, assemble their fragments, validate the manifest, and upload the complete evidence root even on failure.

**Tech Stack:** GitHub Actions YAML, PowerShell, Python `unittest`, existing Stage 7 proof scripts.

---

### Task 1: Add the failing protected-workflow contract test

**Files:**
- Modify: `scripts/ci/tests/test_check_stage7_split_gate.py:1104`

**Step 1: Write the failing test**

Add `test_attribution_ci_runs_the_complete_protected_gate` next to
`test_attribution_matrix`. Read `.github/workflows/release.yml`, isolate the
Stage 7 job, and require:

```python
self.assertIn("stage7_gate_only:", workflow)
self.assertIn("type: boolean", workflow)
self.assertIn("default: false", workflow)
self.assertIn("inputs.stage7_gate_only", workflow)
self.assertIn("timeout-minutes: 360", stage7_job)
self.assertIn("environment: performance", stage7_job)
self.assertIn("runs-on: [self-hosted, Windows, X64, rssh-performance]", stage7_job)
self.assertIn("run-stage7-attribution-deterministic-tests.ps1", stage7_job)
self.assertIn("prove-rterm-external-source.py", stage7_job)
self.assertIn("assemble-stage7-evidence.py", stage7_job)
self.assertIn("check-stage7-split-gate.py", stage7_job)
self.assertIn("artifacts/stage7-gate0", stage7_job)
self.assertIn("if: always()", stage7_job)
```

Also isolate `build-package` and require its job condition to reject a
Stage 7-only dispatch.

**Step 2: Run the test to verify RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_ci_runs_the_complete_protected_gate -v
```

Expected: FAIL because the input, complete proof steps, extended timeout, and
package skip condition do not exist.

**Step 3: Commit the RED test**

```powershell
git add scripts/ci/tests/test_check_stage7_split_gate.py
git commit -m "test(ci): require complete protected Stage 7 gate"
```

### Task 2: Implement the opt-in complete Gate 0 workflow

**Files:**
- Modify: `.github/workflows/release.yml:4-5`
- Modify: `.github/workflows/release.yml:13-15`
- Modify: `.github/workflows/release.yml:435-499`

**Step 1: Add the typed dispatch input**

Define:

```yaml
on:
  workflow_dispatch:
    inputs:
      stage7_gate_only:
        description: Run the complete Stage 7 Gate 0 proof for this ref
        required: false
        type: boolean
        default: false
```

**Step 2: Guard branch access to the protected jobs**

Extend the `fixed-performance` and `stage7-attribution-matrix` job conditions
so a non-default branch is accepted only for a manual dispatch with
`inputs.stage7_gate_only == true`. Preserve tag and ordinary default-branch
behavior.

Add this job condition to `build-package`:

```yaml
if: github.event_name != 'workflow_dispatch' || inputs.stage7_gate_only != true
```

**Step 3: Complete the Stage 7 evidence flow**

Increase the Stage 7 timeout to `360`. After the attribution matrix, add the
existing deterministic-test, external-source, assembly, and validator commands
with these output paths:

```text
artifacts/stage7-gate0/tests
artifacts/stage7-gate0/external
artifacts/stage7-gate0/stage7-evidence-manifest.json
```

Assembly must consume all four artifact-manifest fragments and request
`attribution-ready`. Upload `artifacts/stage7-gate0` under the existing
`if: always()` step.

**Step 4: Run the focused test to verify GREEN**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_ci_runs_the_complete_protected_gate -v
```

Expected: PASS.

**Step 5: Run adjacent Stage 7 tests**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_matrix scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_process_shards_defer_statistics_to_the_aggregate -v
```

Expected: 2 tests pass.

**Step 6: Commit the workflow implementation**

```powershell
git add .github/workflows/release.yml
git diff --cached --check
git commit -m "ci: run complete Stage 7 gate on protected refs"
```

### Task 3: Verify, push, and dispatch the immutable branch

**Files:**
- Verify: `.github/workflows/release.yml`
- Verify: `scripts/ci/tests/test_check_stage7_split_gate.py`

**Step 1: Run deterministic verification**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate -v
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: all tests pass, formatting and diff checks exit zero, and the
worktree is clean after committed changes.

**Step 2: Validate workflow syntax**

Parse `.github/workflows/release.yml` using the repository's available YAML
parser without rewriting the file.

Expected: valid YAML with a Boolean
`on.workflow_dispatch.inputs.stage7_gate_only` input.

**Step 3: Push the branch**

Unset the invalid process-local `GH_TOKEN` override, push
`codex/stage7-split-readiness`, and verify the remote ref resolves to local
`HEAD`.

**Step 4: Dispatch the protected Gate**

Run:

```powershell
gh workflow run release.yml --ref codex/stage7-split-readiness -f stage7_gate_only=true
```

Resolve the new run ID and verify the intended fixed-performance and Stage 7
path is eligible. The protected environment may require approval.

**Step 5: Monitor to the terminal decision**

Wait for the Stage 7 job. Download the complete evidence artifact and verify:

- the run's `headSha` equals the pushed immutable commit;
- the job used the `rssh-performance` Windows runner labels;
- all four fragments and `stage7-evidence-manifest.json` exist;
- the validator output is exactly `attribution-ready`.

Do not begin Task 10 or claim Gate 0 success before these checks pass.
