# Runtime Test Ports Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add deterministic runtime clocks and scripted transports whose independently owned interrupt handle can release blocked readers and writers.

**Architecture:** `SessionTransport` splits into reader, writer, worker-owned control, and a cloneable external `SessionInterrupt`. Production code sees only the narrow `Clock`, `SessionTransport`, `SessionControl`, and `SessionInterrupt` traits; scripted implementations remain behind `cfg(any(test, feature = "test-support"))` and use `Mutex + Condvar` rather than channels or sleeps.

**Tech Stack:** Rust 1.89, `std::io`, `std::sync::{Arc, Mutex, Condvar}`, Cargo feature-gated test support.

---

### Task 1: Externally interruptible session contract

**Files:**
- Modify: `crates/rssh-runtime/src/transport.rs`
- Modify: `crates/rssh-runtime/src/lib.rs`
- Modify: `crates/rssh-runtime/tests/api_contract.rs`

**Step 1: Write the failing contract test**

Add a transport whose reader and writer can block while a separately cloned handle implements:

```rust
pub trait SessionInterrupt: Clone + Send + Sync + 'static {
    fn interrupt(&self) -> io::Result<()>;
}
```

Assert `SessionParts` exposes `interrupt`, `SessionTransport` has an `Interrupt` associated type, and the handle can be retained after reader/writer/control move to different owners.

**Step 2: Run the RED test**

Run: `cargo test --locked -p rssh-runtime --test api_contract session_transport_exposes_external_interrupt_ownership`

Expected: compile failure because `SessionInterrupt` and `SessionParts::interrupt` do not exist.

**Step 3: Implement the minimal contract**

Add the trait, the fourth `SessionParts<R, W, C, I>` member, a four-argument constructor, and `SessionTransport::Interrupt`. Document `interrupt` as fast, idempotent, and safe to call concurrently; it must cause blocked transport I/O to make progress toward an error or EOF.

**Step 4: Run the GREEN contract**

Run: `cargo test --locked -p rssh-runtime --test api_contract session_transport_exposes_external_interrupt_ownership`

Expected: one passing test.

### Task 2: Monotonic production and virtual clocks

**Files:**
- Create: `crates/rssh-runtime/src/clock.rs`
- Modify: `crates/rssh-runtime/src/lib.rs`
- Create: `crates/rssh-runtime/tests/fake_transport.rs`

**Step 1: Write failing clock tests**

Assert `SystemClock::now()` is monotonic and `VirtualClock` starts at a supplied `Instant`, clones share state, checked `advance(Duration)` changes all clones immediately, zero advance is valid, and overflow returns a typed error without changing time. Do not call `sleep`.

**Step 2: Run the RED test**

Run: `cargo test --locked -p rssh-runtime --test fake_transport virtual_clock`

Expected: compile failure because the clock API does not exist.

**Step 3: Implement the clocks**

Define `Clock: Clone + Send + Sync + 'static { fn now(&self) -> Instant; }`, zero-sized `SystemClock`, and feature-gated `VirtualClock` backed by an `Arc<Mutex<Instant>>`. Advance computes with `checked_add` before committing.

**Step 4: Run the GREEN clock tests**

Run: `cargo test --locked -p rssh-runtime --test fake_transport virtual_clock`

Expected: all virtual-clock tests pass without wall-clock waits.

### Task 3: Scripted transport and deterministic blocked-I/O interruption

**Files:**
- Create: `crates/rssh-runtime/src/testing.rs`
- Modify: `crates/rssh-runtime/src/lib.rs`
- Modify: `crates/rssh-runtime/Cargo.toml`
- Modify: `crates/rssh-runtime/tests/fake_transport.rs`

**Step 1: Write failing scripted tests**

Cover:

- partial reads and writes with exact retained suffixes;
- injected read/write error kinds;
- delayed EOF released by an explicit script step;
- independently blocked reader and writer;
- one cloned interrupt waking both blocked operations without sleep;
- resize, exit polling, begin-close calls, close errors, and ordered call logs;
- interrupt and begin-close idempotence;
- no accepted write after interruption.

Use a Condvar-backed observation helper so tests wait for `reader_blocked` / `writer_blocked` predicates rather than timing.

**Step 2: Run the RED suite**

Run: `cargo test --locked -p rssh-runtime --test fake_transport`

Expected: compile failures for the missing scripted types.

**Step 3: Implement narrow test support**

Add feature `test-support = []`. Export `testing` only with `cfg(any(test, feature = "test-support"))`. Store scripts, offsets, call logs, blocked flags, and interrupted/closed state under one mutex; use predicate loops for every Condvar wait and notify all waiters after script, close, or interrupt state changes. Compute/move fallible payloads before mutating shared accounting.

**Step 4: Run the GREEN suite and quality gates**

Run:

```powershell
cargo test --locked -p rssh-runtime --test fake_transport
cargo test --locked -p rssh-runtime
cargo clippy --locked -p rssh-runtime --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
```

Expected: all pass; production dependencies remain unchanged and no channel API appears in `rssh-runtime`.

**Step 5: Commit**

```powershell
git add crates/rssh-runtime docs/plans/2026-08-11-runtime-test-ports.md
git commit -m "test: add deterministic runtime transports and clock"
```
