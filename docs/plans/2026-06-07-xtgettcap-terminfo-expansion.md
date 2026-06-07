# XTGETTCAP Terminfo Expansion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expand the console/native runtime XTGETTCAP response table with common xterm-compatible terminfo capabilities for keys, line/display controls, and alternate character set metadata.

**Architecture:** Keep the existing XTGETTCAP parser and response framing unchanged. Extend the duplicated `xtgettcap_value_hex` tables in `local.rs` and `terminal_runtime.rs` so console-hosted and native-window PTY paths answer the same capability names.

**Tech Stack:** Rust, existing `rssh-app` unit tests, xterm-style terminfo capability names and escape sequences.

---

### Task 1: Add Failing Tests for Common Terminfo Capabilities

**Files:**
- Modify: `crates/rssh-app/src/local.rs`
- Modify: `crates/rssh-app/src/terminal_runtime.rs`

**Step 1: Write the failing tests**

Add tests that query grouped XTGETTCAP capabilities:

- controls: `el`, `ed`, `el1`, `dch1`, `ich1`, `il1`, `dl1`, `cuu`, `cud`, `cub`, `cuf`, `hpa`, `vpa`, `smir`, `rmir`, `smam`, `rmam`
- keys: `kcuu1`, `kcud1`, `kcuf1`, `kcub1`, `khome`, `kend`, `kich1`, `kdch1`, `kpp`, `knp`, `kf1` through `kf12`
- ACS: `enacs`, `smacs`, `rmacs`, `acsc`

**Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test -p rssh-app xtgettcap_common -- --nocapture
```

Expected: failure because most new capability names currently return `DCS 0+r ST` or are omitted from grouped responses.

### Task 2: Implement Minimal XTGETTCAP Table Expansion

**Files:**
- Modify: `crates/rssh-app/src/local.rs`
- Modify: `crates/rssh-app/src/terminal_runtime.rs`

**Step 1: Extend `xtgettcap_value_hex` in both files**

Add exact byte responses using the existing `encode_ascii_hex` helper:

```rust
b"el" => Some(encode_ascii_hex(b"\x1b[K")),
b"ed" => Some(encode_ascii_hex(b"\x1b[J")),
b"kcuu1" => Some(encode_ascii_hex(b"\x1bOA")),
b"kf1" => Some(encode_ascii_hex(b"\x1bOP")),
```

Keep values conservative and xterm-compatible.

**Step 2: Run targeted tests**

Run:

```powershell
cargo test -p rssh-app xtgettcap_common -- --nocapture
```

Expected: pass.

### Task 3: Update User-Facing Docs

**Files:**
- Modify: `README.md`
- Modify: `docs/mvp-2-local-terminal.md`
- Modify: `docs/mvp-4-live-pty-window.md`

**Step 1: Update XTGETTCAP descriptions**

Mention common key capabilities, line/display edit controls, insert/autowrap controls, and ACS metadata.

**Step 2: Run docs-related validation through normal project gates**

No separate doc generator exists; use full verification.

### Task 4: Verify, Package, Commit

**Files:**
- Existing modified files only.

**Step 1: Run verification**

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p rssh-app
```

**Step 2: Refresh Windows package**

Copy the release exe, launcher, README, LICENSE, and profile example into `dist/R-SSH-windows-x64`, then regenerate `dist/R-SSH-windows-x64.zip`.

**Step 3: Run package smoke**

Run packaged `version`, `doctor`, `self-test`, `bench`, profile checks, `console --preflight`, and `rssh-console.cmd --preflight`.

**Step 4: Commit and push**

```powershell
git add README.md docs/mvp-2-local-terminal.md docs/mvp-4-live-pty-window.md crates/rssh-app/src/local.rs crates/rssh-app/src/terminal_runtime.rs docs/plans/2026-06-07-xtgettcap-terminfo-expansion.md
git commit -m "feat: expand xtgettcap terminfo responses"
git push origin main
```
