# Built-in Color Scheme Alias Completeness Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make every canonical name and alias in the pinned WezTerm color-scheme data resolve through R-SSH's static built-in palette lookup.

**Architecture:** Retain the explicit `builtin_color_scheme_toml` match table in `window.rs`. Add a test that uses the pinned data as the expected name set, then add only the aliases the test identifies to their existing palette match arms. Unknown names stay unresolved.

**Tech Stack:** Rust 2024, Cargo test, pinned JSON data under `refs/wezterm`.

---

### Task 1: Add an upstream alias-completeness regression test

**Files:**
- Modify: `crates/rssh-app/src/window.rs` (test module beside existing color-scheme lookup tests)
- Reference: `refs/wezterm/docs/colorschemes/data.json`

**Step 1: Write the failing test**

Add a test that loads the pinned color-scheme JSON, iterates each scheme's canonical name plus aliases, and records every name for which `builtin_color_scheme_toml(name)` is `None`.

**Step 2: Run test to verify it fails**

Run: `cargo test -p rssh-app builtin_color_scheme_alias_completeness -- --exact`

Expected: FAIL, listing unmapped upstream aliases.

### Task 2: Map the missing aliases

**Files:**
- Modify: `crates/rssh-app/src/window.rs: builtin_color_scheme_toml`

**Step 1: Add the minimal mappings**

For each name listed by Task 1, add it as an alternate match pattern on the arm that already returns the same bundled TOML palette. Do not normalize or otherwise transform lookup input.

**Step 2: Run test to verify it passes**

Run: `cargo test -p rssh-app builtin_color_scheme_alias_completeness -- --exact`

Expected: PASS with zero missing canonical names or aliases.

### Task 3: Record and verify the bounded parity claim

**Files:**
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Update the tracker**

State that all aliases in the pinned upstream data resolve; retain dynamic scheme construction and lookup behavior outside this slice as open work.

**Step 2: Run the complete verification set**

Run: `cargo test -p rssh-app`; `cargo test --workspace`; `git diff --check`; `git fsck --no-dangling`.

Expected: all Cargo tests pass, no whitespace errors, and any pre-existing repository-object problem is reported separately from the source change.

**Step 3: Commit**

Run: `git add crates/rssh-app/src/window.rs docs/research/wezterm-parity-gap.md`; `git commit -m "feat: complete builtin color scheme aliases"`.
