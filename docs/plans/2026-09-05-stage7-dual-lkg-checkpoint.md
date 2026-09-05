# Stage 7 dual-LKG checkpoint

Status: Task 12 is still **NO-GO**. This checkpoint is not a product certification
or permission to extract/publish R-Term.

## Approved baseline separation

The original `lkg_rssh_ref`,
`21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6`, remains the immutable R-SSH
history/extraction boundary. It cannot run the new packaged-product GUI probe.

The new `product_lkg_ref`,
`4cbee13c591ded5ccfc6b0aec68f2b33143528c1`, contains the probe before the font
and GPU resource optimizations. All product latency/memory comparisons, including
later R2/R3 epochs, use this reference. Both references are pinned full SHAs.
Absolute budgets, the 1.05 regression limit and the 5+30/60 protocol are unchanged.
Updating the complete contract digest invalidates evidence assembled under the
previous contract; historical evidence must not be relabelled as current GO.

Only six previously unpushed commits were reordered above remote base
`0e190289e24bc12c6d621e47f1560f9afaf5bb9d`. The final tracked tree was verified
identical to the pre-reorder tree before restoring the unfinished tooling edits.

| Original commit | Reordered commit | Purpose |
| --- | --- | --- |
| `63e4bdea` | `4cbee13c` | Product GUI probe; new product baseline |
| `72a27c69` | `76f05d3d` | Historical Gate 0 evidence |
| `3ac0276c` | `06dccde7` | Lazy platform fonts |
| `5e60c67d` | `0372700b` | Lazy GPU image resources |
| `a1f07f92` | `8ae88a30` | Product probe design |
| `97c3eee6` | `a8b0aafc` | Product probe implementation plan |

Recovery copies remain on branch `codex/stage7-before-dual-lkg-20260905` and
stash `a4dde0eeeb7b28e2d34dfc554d2fb459d6ba1fb5`. The remote branch has not been
force-rewritten.

## Verification observed so far

- Exact baseline build succeeded with
  `cargo build --locked --release -p rssh-app --no-default-features --features production-gui`.
- Baseline empty-window product smoke presented CPU then Vulkan GPU and exited
  normally. Its single short-stabilization sample was 311,963,648 bytes PWS.
  This is functional-only evidence, not the contracted sampling protocol or GO.
- Baseline SSH1 repeatedly reached a logged SSH connected state but timed out
  awaiting a GPU-presented frame. A non-invasive debugger stack found the GPU
  initialization worker inside NVIDIA `NvMemMapStoragex!TotalDiskUsage` through
  D3D12 device creation, while the UI thread waited for events. The underlying
  driver/cache trigger is not resolved. No backend override or extended timeout
  may be used to manufacture product evidence.
- New dual-LKG tests were observed failing against the old validator and then
  passing after the change. The seven primary product metric types accept the
  product reference and reject the historical reference as a metric baseline.
- The runner's existing focused tests plus the new dual-LKG case passed (13
  tests). This does not validate the complete product certification pipeline.
- Five focused gate tests passed, including complete-contract digest freezing,
  recomputed LKG ratios and the original history-ancestor boundary.
- `cargo fmt --all -- --check` and all 23 tests in
  `performance_scorecard_contract` passed. The five modified PowerShell scripts
  parsed successfully; the native runner passed `bash -n`; `git diff --check`
  passed. No repository-local `target` directory was created.
- GitHub's repository runner inventory returned `total_count: 0`. A successful
  ordinary hosted CI run would not replace protected fixed-runner certification.

## Outstanding review blockers in the uncommitted Task 12 tooling

1. Replace the Windows hard-coded secret-word scan with an actual fixture-secret
   scan of stdout, stderr, markers, metrics JSON, session log and visible
   snapshots. Missing scope or any hit must fail. Run this separately from
   performance cohorts, against the packaged candidate GUI. Do not emit secrets
   or their digests in evidence. Proposed opt-in launcher API: `--secret-scan`,
   restricted to `--product-gui --scenario ssh1`, with an optional
   `rssh.stage7/secret-scan/v1` receipt.
2. Bind release-build provenance to **both production-gui builds** and the actual
   packaged/archived executable hashes, not merely the diagnostic build receipt.
3. Retain complete functional command inputs, exit status and hashed outputs in
   re-verifiable evidence. Currently logs live below the disposable work root;
   result booleans alone do not meet the plan's hashed-input requirement.
4. Resolve native executables through `manifest.json.artifact.binary`. macOS has
   both a top-level shell wrapper and the real Mach-O; `find ... -name rssh-app`
   may hash the wrapper. Check build/package/archive equality and path safety.
5. Keep original diagnostic records separately from normalized records, including
   on ratio/absolute gate failure. Do not delete the evidence with build scratch.
6. Add a packaged native SSH GUI functional run on Unix. Existing
   `openssh_loopback` tests exercise CLI SSH, not the GUI.
7. Complete deterministic verification and another independent review before
   publishing the coordinator/workflow changes. Then run protected Windows and
   native-platform certification; only valid evidence can unblock Task 13.

The initially suspected PowerShell functional working-directory bug was
**withdrawn**: the shared bounded-process harness reads `$repositoryRoot` through
dynamic scope, and a real two-hop process test confirmed the candidate cwd.

Parallel implementation attempts for the remaining blockers hit the account
usage limit before making file changes. Their suggested interfaces are not
implemented. Do not mark these items complete based on this checkpoint.
