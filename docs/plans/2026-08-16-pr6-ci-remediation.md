# PR #6 CI Remediation Implementation Plan

> **Implementation note:** Execute this plan task-by-task and verify each gate before publishing.

**Goal:** Make PR #6 mergeable and restore all required CI checks to green.

**Architecture:** Rebase the GUI SSH feature branch onto the current `main` so it inherits the repository's dependency-security updates and current CI baseline. Resolve conflicts by preserving the feature behavior while adopting current base APIs, then verify the exact failing audit locally before pushing the rewritten branch with lease protection.

**Tech Stack:** Git, Rust/Cargo, npm, PowerShell, GitHub Actions

---

### Task 1: Preserve the CI failure baseline

**Files:**
- Inspect: `web/package-lock.json`

**Step 1: Record the failing check**

Run: `gh pr checks 6 --repo lcxinc/R-SSH`

Expected: `Dependency and license security` fails while the other checks pass.

**Step 2: Confirm the vulnerable lock entry**

Run: `npm --prefix web audit --audit-level=high`

Expected: FAIL for `nanoid` versions below `3.3.18`.

### Task 2: Update the feature branch

**Files:**
- Modify through conflict resolution: files reported by `git rebase pr6/main`

**Step 1: Rebase onto the fetched base tip**

Run: `git rebase pr6/main`

Expected: Either a clean rebase or explicit conflicts to resolve.

**Step 2: Resolve each conflict**

Preserve the GUI SSH feature behavior and tests while adopting the current `main` CI, dependency, and API baseline.

**Step 3: Continue until complete**

Run: `git rebase --continue`

Expected: the feature commits are replayed on `pr6/main` and `git status --short` is clean.

### Task 3: Verify the remediation

**Files:**
- Verify: `web/package-lock.json`
- Verify: `Cargo.lock`
- Verify: affected Rust sources and tests

**Step 1: Re-run the exact failing audit**

Run: `npm --prefix web audit --audit-level=high`

Expected: PASS with zero high-severity vulnerabilities.

**Step 2: Run Rust dependency policy checks**

Run: `cargo deny check`

Expected: PASS; duplicate-version warnings are permitted by the current policy.

**Step 3: Run formatting, lint, and relevant tests**

Run the repository's CI-equivalent commands for the changed Rust and web code.

Expected: all commands exit successfully.

### Task 4: Publish and monitor

**Files:**
- No additional source files unless verification exposes a regression.

**Step 1: Push the rewritten feature branch safely**

Run: `git push --force-with-lease=<branch>:<expected-old-sha> https://github.com/lcxinc/R-SSH.git HEAD:<branch>`

Expected: the remote branch advances to the verified rebased commit.

**Step 2: Monitor PR checks**

Run: `gh pr checks 6 --repo lcxinc/R-SSH --watch`

Expected: all required checks pass and the PR is no longer blocked by conflicts.
