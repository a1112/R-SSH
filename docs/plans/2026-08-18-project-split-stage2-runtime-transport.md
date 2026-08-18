# Stage 2 Runtime and Transport Split Implementation Plan

**Goal:** Make the terminal runtime an R-Term-owned, transport-neutral package while
moving concrete local PTY and native SSH adapters back to their protocol/platform
crates without changing session semantics.

**Architecture:** The package at `crates/rssh-runtime` is renamed to
`rterm-runtime` while its physical path remains stable for the frozen Task 10
provenance corpus. It continues to own `SessionTransport`, reader/writer/control/
interrupt ownership, pane workers, bounded mailboxes, terminal progression, and
test transports. `rssh-pty` and `rssh-ssh` gain opt-in `runtime-adapter` features
and own their concrete implementations. Consumers compose the abstract runtime
with one or both concrete adapters explicitly.

## Task 1: Lock the package boundary with failing architecture tests

**Files:**

- Modify: `scripts/ci/tests/test_check_rust_architecture.py`
- Modify: `crates/rssh-functional-tests/tests/behavior_catalog_contract.rs`

Add contracts that require:

- the runtime Cargo package to be named `rterm-runtime`;
- the runtime manifest and source tree to contain no `rssh-pty`, `rssh-ssh`,
  `local-transport`, `ssh-transport`, or `transport-adapters` ownership;
- local and SSH adapter modules plus `runtime-adapter` features to live in
  `rssh-pty` and `rssh-ssh`;
- the functional workflow to run abstract runtime, local adapter, and SSH adapter
  suites from their new owning packages.

Run the focused contracts first and record the expected RED result.

## Task 2: Rename the runtime package without moving frozen sources

**Files:**

- Modify: `crates/rssh-runtime/Cargo.toml`
- Modify: runtime/native/app manifests and Rust imports
- Modify: `Cargo.lock`
- Modify: CI commands and architecture expectations

Rename the package to `rterm-runtime`, remove the concrete transport feature
surface and dependencies, and migrate consumers to the `rterm_runtime` crate
name. Keep the directory and Task 10 files at their recorded paths.

Verify the abstract runtime suites (`api_contract`, fake transport, burst,
mailbox, pane worker, batching, equivalence, terminal delta) before moving the
adapters.

## Task 3: Move the local PTY runtime adapter to `rssh-pty`

**Files:**

- Create: `crates/rssh-pty/src/runtime_adapter.rs`
- Modify: `crates/rssh-pty/src/lib.rs`
- Modify: `crates/rssh-pty/Cargo.toml`
- Move: `crates/rssh-runtime/tests/local_transport.rs` to
  `crates/rssh-pty/tests/runtime_adapter.rs`
- Delete: `crates/rssh-runtime/src/transport/local.rs`
- Modify: app local/window composition imports

Expose `LocalPtyTransport`, `LocalPtyControl`, and `LocalPtyInterrupt` only under
the `runtime-adapter` feature. Preserve spawn, resize, exit, master-close, and
out-of-band interrupt behavior. Run the moved real-PTY and shutdown tests.

## Task 4: Move the SSH runtime adapter to `rssh-ssh`

**Files:**

- Create: `crates/rssh-ssh/src/runtime_adapter.rs`
- Modify: `crates/rssh-ssh/src/lib.rs`
- Modify: `crates/rssh-ssh/Cargo.toml`
- Move: `crates/rssh-runtime/tests/ssh_transport.rs` to
  `crates/rssh-ssh/tests/runtime_adapter.rs`
- Delete: `crates/rssh-runtime/src/transport/ssh.rs`
- Modify: app SSH composition imports

Expose `SshTransport`, reader/writer/control/interrupt adapters only under the
`runtime-adapter` feature. Preserve cancellation, partial writes, resize,
finish-input, exit status/signal, reconnect, and shutdown behavior. Run the moved
adapter suite and native loopback SSH suite.

## Task 5: Update composition, CI, and ownership documentation

**Files:**

- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-native/Cargo.toml`
- Modify: `.github/workflows/functional.yml`
- Modify: `README.md`
- Modify: architecture/functional contract tests

Make app composition opt into both concrete adapter features explicitly. Update
package commands to `rterm-runtime`, move adapter test commands to their owning
packages, and document the one-way dependency direction.

## Task 6: Verify the Stage 2 exit contract

Run:

```text
cargo tree -p rterm-runtime --all-features --locked
cargo test -p rterm-runtime --all-targets --all-features --locked
cargo test -p rssh-pty --all-targets --all-features --locked
cargo test -p rssh-ssh --all-targets --all-features --locked
cargo test -p rssh-app --all-targets --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
python scripts/ci/check-rust-architecture.py --policy scripts/ci/architecture-policy.json
python scripts/ci/check-task10-provenance.py
```

Inspect the runtime dependency tree and fail the stage if it contains
`portable-pty`, `russh`, `rssh-pty`, or `rssh-ssh`. Push a dedicated Stage 2 PR,
wait for required CI, and merge before starting Stage 3.
