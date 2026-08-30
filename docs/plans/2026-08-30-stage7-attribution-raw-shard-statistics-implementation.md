# Stage 7 Attribution Raw-Shard Statistics Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make each per-process Stage 7 attribution raw artifact omit partial statistics so Gate assembly can independently recompute and verify the complete 30-process cohort.

**Architecture:** Keep raw samples and per-process representatives in each atomic shard, and keep derived p50/p95/max only in the aggregate. Do not change the validator, collection schedule, thresholds, backend behavior, or failure policy. After the source-only producer fix, regenerate all evidence under one new immutable commit.

**Tech Stack:** PowerShell, Python `unittest`, Rust contract tests, Stage 7 evidence assembler and validator.

---

### Task 1: Add a Failing Per-Process Shard Contract Test

**Files:**
- Modify: `scripts/ci/tests/test_check_stage7_split_gate.py`

**Step 1: Write the failing test**

Add a test that bounds the supported per-process raw-shard construction block
and rejects any local `statistics` claim while requiring the raw process:

```python
def test_attribution_process_shards_defer_statistics_to_the_aggregate(self) -> None:
    source = ATTRIBUTION_RUNNER_PATH.read_text(encoding="utf-8")
    start = source.index(
        '$rawId = "attribution-matrix-raw/$backend/$stage/process-{0:D3}"'
    )
    end = source.index("Write-AtomicJson", start)
    process_shard = source[start:end]

    self.assertIn("processes = @(\u0024process)", process_shard)
    self.assertNotIn("statistics =", process_shard)
    self.assertIn("$stats = New-Statistics -Processes $processes", source)
    self.assertIn("group_statistics = @($groupStatistics)", source)
```

Use the literal PowerShell `$process` spelling in the actual Python source.

**Step 2: Run the focused test to verify RED**

Run:

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_process_shards_defer_statistics_to_the_aggregate -v
```

Expected: FAIL because the current process-shard block contains
`statistics = ...`.

**Step 3: Commit the RED test**

```powershell
git add scripts/ci/tests/test_check_stage7_split_gate.py
git commit -m "test(ci): reject partial attribution shard statistics"
```

### Task 2: Remove Partial Statistics From the Producer

**Files:**
- Modify: `scripts/ci/run-stage7-attribution-matrix.ps1`

**Step 1: Apply the minimal producer fix**

Remove only this field from the supported per-process `$group` object:

```powershell
statistics = [ordered]@{ p50 = [UInt64] $process.representative; p95 = [UInt64] $process.representative; max = [UInt64] ($process.samples | Measure-Object -Maximum).Maximum }
```

Keep `New-Statistics -Processes $processes`, aggregate
`group_statistics`, representatives, and raw maxima unchanged.

**Step 2: Run the focused test to verify GREEN**

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_process_shards_defer_statistics_to_the_aggregate -v
```

Expected: PASS.

**Step 3: Run the existing matrix contract test**

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate.Stage7SplitGateTests.test_attribution_matrix -v
```

Expected: PASS.

**Step 4: Commit the producer fix**

```powershell
git add scripts/ci/run-stage7-attribution-matrix.ps1
git commit -m "fix(ci): defer attribution statistics to aggregate"
```

### Task 3: Run Deterministic Regression Verification

**Files:**
- Verify only.

**Step 1: Run all Stage 7 attribution Rust contracts**

```powershell
cargo test --locked -p rssh-app --test gpu_backend_memory_matrix_behavior stage7_attribution -j1
```

Expected: all Stage 7 attribution tests PASS.

**Step 2: Run the complete Python Gate test module**

```powershell
python -m unittest scripts.ci.tests.test_check_stage7_split_gate -v
```

Expected: PASS.

**Step 3: Run formatting and whitespace checks**

```powershell
cargo fmt --all -- --check
git diff --check
git status --short
```

Expected: checks exit 0 and the worktree is clean after the two commits.

### Task 4: Regenerate Gate 0 Evidence From the New Commit

**Files:**
- Archive the failed evidence under `L:\rssh-evidence\failed`.
- Generate new evidence under `L:\rssh-evidence\stage7-gate0`.

**Step 1: Archive the failed evidence recoverably**

Verify the exact resolved source and destination remain under
`L:\rssh-evidence`, then move the failed `1dc756f0` evidence to a unique archive
directory. Do not delete or overwrite it.

**Step 2: Start the complete Gate 0 pipeline**

From the new clean commit, run release prebuild, font proof, all 960 attribution
processes, deterministic attribution proof, external-source proof, four-fragment
assembly, and validation.

**Step 3: Verify the exact terminal state**

Expected final output:

```text
attribution-ready
```

Do not mutate prior evidence, skip a proof, begin Task 10, or claim Gate success
without that exact validator result.
