# R-SSH Native Rust Terminal Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a native Rust SSH terminal client foundation with a testable terminal core, SSH/local PTY adapters, GPU renderer, and desktop session shell.

**Architecture:** Keep terminal state independent from connection and rendering. SSH and PTY adapters produce byte streams; the terminal core mutates grid and scrollback; the renderer consumes dirty regions; the app shell manages sessions, tabs, persistence, and user actions.

**Tech Stack:** Rust 2024, `winit`, `wgpu`, `cosmic-text`, `russh`, optional `libssh2`, Windows ConPTY, Unix PTY, SQLite, OS-backed key storage.

---

## Phase 0: Repository Baseline

### Task 0.1: Verify Workspace Skeleton

**Files:**
- Check: `Cargo.toml`
- Check: `crates/rssh-core/src/lib.rs`
- Check: `crates/rssh-terminal/src/lib.rs`
- Check: `crates/rssh-renderer/src/lib.rs`
- Check: `crates/rssh-ssh/src/lib.rs`
- Check: `crates/rssh-pty/src/lib.rs`
- Check: `crates/rssh-app/src/main.rs`

**Step 1: Run formatting**

Run: `cargo fmt --all -- --check`

Expected: PASS.

**Step 2: Run tests**

Run: `cargo test --workspace`

Expected: PASS.

**Step 3: Commit**

```bash
git add .
git commit -m "chore: initialize native rust workspace"
```

## Phase 1: Terminal Core MVP

### Task 1.1: Define Terminal Cell Model

**Files:**
- Modify: `crates/rssh-terminal/src/lib.rs`
- Test: `crates/rssh-terminal/src/lib.rs`

**Step 1: Write failing tests**

Add tests for:

- default cell has a space character
- cell stores foreground and background color placeholders
- grid can set and read a cell by row and column
- out-of-bounds reads return `None`

Run: `cargo test -p rssh-terminal`

Expected: FAIL because the cell attributes and accessors do not exist.

**Step 2: Implement minimal cell model**

Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub foreground: Color,
    pub background: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}
```

Add `TerminalGrid::get(row, column)` and `TerminalGrid::set(row, column, cell)`.

**Step 3: Run tests**

Run: `cargo test -p rssh-terminal`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-terminal
git commit -m "feat: add terminal cell model"
```

### Task 1.2: Add VT Stream Parser Boundary

**Files:**
- Create: `crates/rssh-terminal/src/parser.rs`
- Modify: `crates/rssh-terminal/src/lib.rs`
- Test: `crates/rssh-terminal/src/parser.rs`

**Step 1: Write failing tests**

Test plain text parsing:

```rust
#[test]
fn writes_plain_text_into_grid() {
    let mut terminal = Terminal::new(TerminalSize::new(10, 2));
    terminal.feed(b"abc");
    assert_eq!(terminal.grid().get(0, 0).unwrap().ch, 'a');
    assert_eq!(terminal.grid().get(0, 1).unwrap().ch, 'b');
    assert_eq!(terminal.grid().get(0, 2).unwrap().ch, 'c');
}
```

Run: `cargo test -p rssh-terminal writes_plain_text_into_grid`

Expected: FAIL because `Terminal` and `feed` do not exist.

**Step 2: Implement minimal parser**

Implement only printable ASCII and newline. Keep the parser struct isolated so a
real VT parser can replace it.

**Step 3: Run tests**

Run: `cargo test -p rssh-terminal`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-terminal
git commit -m "feat: parse basic terminal text"
```

### Task 1.3: Add Damage Tracking

**Files:**
- Modify: `crates/rssh-terminal/src/lib.rs`
- Modify: `crates/rssh-renderer/src/lib.rs`
- Test: `crates/rssh-terminal/src/lib.rs`

**Step 1: Write failing tests**

Assert that feeding `abc` marks row 0 columns 0-2 dirty.

Run: `cargo test -p rssh-terminal damage`

Expected: FAIL because terminal damage is not tracked.

**Step 2: Implement minimal damage API**

Return a `Vec<DamageRegion>` from feed operations or expose
`Terminal::take_damage()`.

**Step 3: Run tests**

Run: `cargo test -p rssh-terminal -p rssh-renderer`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-terminal crates/rssh-renderer
git commit -m "feat: track terminal damage regions"
```

## Phase 2: Local PTY MVP

### Task 2.1: Add PTY Trait

**Files:**
- Modify: `crates/rssh-pty/src/lib.rs`
- Test: `crates/rssh-pty/src/lib.rs`

**Step 1: Write failing tests**

Add tests for backend selection and command configuration validation.

Run: `cargo test -p rssh-pty`

Expected: FAIL because command configuration does not exist.

**Step 2: Implement config types**

Add `PtyCommand`, `PtySize`, and a `PtySession` trait with read/write/resize
boundaries. Do not spawn real shells yet.

**Step 3: Run tests**

Run: `cargo test -p rssh-pty`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-pty
git commit -m "feat: define pty session boundary"
```

### Task 2.2: Integrate Windows ConPTY First

**Files:**
- Modify: `crates/rssh-pty/Cargo.toml`
- Create: `crates/rssh-pty/src/windows.rs`
- Modify: `crates/rssh-pty/src/lib.rs`
- Test: `crates/rssh-pty/src/windows.rs`

**Step 1: Add dependency**

Evaluate `portable-pty` first because WezTerm already proves the shape. Add it
behind a platform implementation, not directly into app code.

**Step 2: Write integration test**

Spawn `cmd /C echo rssh` on Windows and assert the stream contains `rssh`.

Run: `cargo test -p rssh-pty -- --ignored`

Expected: FAIL before implementation.

**Step 3: Implement minimal PTY spawn**

Spawn, read, write, resize, and terminate through the trait.

**Step 4: Run tests**

Run: `cargo test -p rssh-pty -- --ignored`

Expected: PASS on Windows.

**Step 5: Commit**

```bash
git add crates/rssh-pty
git commit -m "feat: add windows conpty adapter"
```

## Phase 3: SSH Shell MVP

### Task 3.1: Define SSH Session Interface

**Files:**
- Modify: `crates/rssh-ssh/src/lib.rs`
- Test: `crates/rssh-ssh/src/lib.rs`

**Step 1: Write failing tests**

Validate host, username, port range, and terminal size.

Run: `cargo test -p rssh-ssh`

Expected: FAIL because validation does not exist.

**Step 2: Implement validation**

Return typed config errors without adding the network library yet.

**Step 3: Run tests**

Run: `cargo test -p rssh-ssh`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-ssh
git commit -m "feat: validate ssh session configuration"
```

### Task 3.2: Add russh Adapter

**Files:**
- Modify: `crates/rssh-ssh/Cargo.toml`
- Create: `crates/rssh-ssh/src/russh_client.rs`
- Modify: `crates/rssh-ssh/src/lib.rs`
- Test: `crates/rssh-ssh/tests/loopback.rs`

**Step 1: Write loopback fixture test**

Use a local test server or a mocked channel to prove that bytes from an SSH
channel can be read and bytes can be written back.

Run: `cargo test -p rssh-ssh loopback`

Expected: FAIL before adapter exists.

**Step 2: Implement adapter**

Support connect, authenticate by password or key path, request PTY, start shell,
read stream, write input, resize, keepalive, and close.

**Step 3: Run tests**

Run: `cargo test -p rssh-ssh`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-ssh
git commit -m "feat: add russh shell adapter"
```

## Phase 4: Renderer Prototype

### Task 4.1: Add Renderer Model Tests

**Files:**
- Modify: `crates/rssh-renderer/src/lib.rs`
- Test: `crates/rssh-renderer/src/lib.rs`

**Step 1: Write failing tests**

Test that damage regions are merged and empty regions are skipped.

Run: `cargo test -p rssh-renderer`

Expected: FAIL because merging does not exist.

**Step 2: Implement region merging**

Keep it simple: merge adjacent regions on the same row first.

**Step 3: Run tests**

Run: `cargo test -p rssh-renderer`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-renderer
git commit -m "feat: merge renderer damage regions"
```

### Task 4.2: Add wgpu Window Prototype

**Files:**
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-renderer/Cargo.toml`
- Create: `crates/rssh-renderer/src/wgpu_renderer.rs`
- Modify: `crates/rssh-renderer/src/lib.rs`
- Modify: `crates/rssh-app/src/main.rs`

**Step 1: Add smoke test boundary**

Keep GPU tests opt-in because CI machines vary. Add a constructor test that
validates configuration without requiring a physical adapter.

**Step 2: Implement window prototype**

Create a window, clear the frame, and draw a placeholder terminal rectangle.

**Step 3: Verify manually**

Run: `cargo run -p rssh-app`

Expected: a native window opens and stays responsive.

**Step 4: Commit**

```bash
git add crates/rssh-app crates/rssh-renderer
git commit -m "feat: add native renderer prototype"
```

## Phase 5: Session Runtime

### Task 5.1: Add Session Runtime Types

**Files:**
- Create: `crates/rssh-core/src/session.rs`
- Modify: `crates/rssh-core/src/lib.rs`
- Test: `crates/rssh-core/src/session.rs`

**Step 1: Write failing tests**

Test session lifecycle: created, connecting, connected, disconnected, closed.

Run: `cargo test -p rssh-core session`

Expected: FAIL.

**Step 2: Implement lifecycle model**

Add typed state transitions and reject invalid transitions.

**Step 3: Run tests**

Run: `cargo test -p rssh-core`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-core
git commit -m "feat: model session lifecycle"
```

### Task 5.2: Wire PTY to Terminal Core

**Files:**
- Modify: `crates/rssh-app/Cargo.toml`
- Modify: `crates/rssh-app/src/main.rs`
- Test: `crates/rssh-app/tests/local_pty.rs`

**Step 1: Write ignored integration test**

Spawn a local shell command, feed its bytes into `rssh-terminal`, and assert the
grid receives output.

Run: `cargo test -p rssh-app local_pty -- --ignored`

Expected: FAIL before wiring exists.

**Step 2: Implement runtime bridge**

Create an async task that reads PTY output and calls terminal feed.

**Step 3: Run tests**

Run: `cargo test -p rssh-app local_pty -- --ignored`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-app
git commit -m "feat: wire local pty into terminal runtime"
```

## Phase 6: Product Shell

### Task 6.1: Add Session Persistence

**Files:**
- Create: `crates/rssh-core/src/profile.rs`
- Create: `crates/rssh-app/src/storage.rs`
- Modify: `crates/rssh-app/Cargo.toml`
- Test: `crates/rssh-app/src/storage.rs`

**Step 1: Write failing tests**

Save and load a session profile without secrets.

Run: `cargo test -p rssh-app storage`

Expected: FAIL.

**Step 2: Implement SQLite storage**

Store host, port, username, terminal defaults, theme ID, proxy ID, and metadata.
Do not store passwords or private key passphrases.

**Step 3: Run tests**

Run: `cargo test -p rssh-app storage`

Expected: PASS.

**Step 4: Commit**

```bash
git add crates/rssh-app crates/rssh-core
git commit -m "feat: persist session profiles"
```

### Task 6.2: Add Secure Secret Boundary

**Files:**
- Create: `crates/rssh-app/src/secrets.rs`
- Modify: `crates/rssh-app/Cargo.toml`
- Test: `crates/rssh-app/src/secrets.rs`

**Step 1: Write tests with in-memory fake**

Store, read, and delete a secret through a trait.

Run: `cargo test -p rssh-app secrets`

Expected: FAIL.

**Step 2: Implement trait and fake**

Add `SecretStore` trait and `MemorySecretStore` for tests.

**Step 3: Add platform-backed implementation**

Use OS-backed storage later behind the same trait.

**Step 4: Commit**

```bash
git add crates/rssh-app
git commit -m "feat: define secret storage boundary"
```

## Phase 7: Advanced SSH Product Features

### Task 7.1: Add Known Hosts Handling

**Files:**
- Create: `crates/rssh-ssh/src/known_hosts.rs`
- Modify: `crates/rssh-ssh/src/lib.rs`
- Test: `crates/rssh-ssh/src/known_hosts.rs`

**Step 1: Write failing tests**

Test new host, matching host, changed host, and revoked host behavior.

Run: `cargo test -p rssh-ssh known_hosts`

Expected: FAIL.

**Step 2: Implement host key policy**

Make changed host keys blocking and explicit.

**Step 3: Commit**

```bash
git add crates/rssh-ssh
git commit -m "feat: add known hosts policy"
```

### Task 7.2: Add SFTP Boundary

**Files:**
- Create: `crates/rssh-ssh/src/sftp.rs`
- Modify: `crates/rssh-ssh/src/lib.rs`
- Test: `crates/rssh-ssh/src/sftp.rs`

**Step 1: Write failing tests**

Test list, stat, upload, download, and delete through a fake backend.

Run: `cargo test -p rssh-ssh sftp`

Expected: FAIL.

**Step 2: Implement backend trait**

Add trait first, then wire to `russh` after the shell path is stable.

**Step 3: Commit**

```bash
git add crates/rssh-ssh
git commit -m "feat: define sftp boundary"
```

### Task 7.3: Add Port Forwarding Boundary

**Files:**
- Create: `crates/rssh-ssh/src/forwarding.rs`
- Modify: `crates/rssh-ssh/src/lib.rs`
- Test: `crates/rssh-ssh/src/forwarding.rs`

**Step 1: Write failing tests**

Validate local, remote, and dynamic forward config.

Run: `cargo test -p rssh-ssh forwarding`

Expected: FAIL.

**Step 2: Implement typed config**

Do not open sockets until config and lifecycle are tested.

**Step 3: Commit**

```bash
git add crates/rssh-ssh
git commit -m "feat: model ssh port forwarding"
```

## Phase 8: Release Readiness

### Task 8.1: Add CI Matrix

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Extend CI**

Add Windows, Linux, and macOS jobs once native dependencies compile on all three.

**Step 2: Run local checks**

Run: `cargo fmt --all -- --check`

Run: `cargo test --workspace`

Expected: PASS.

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: test rust workspace across platforms"
```

### Task 8.2: Add Packaging Plan

**Files:**
- Create: `docs/packaging.md`
- Modify: `README.md`

**Step 1: Document packaging targets**

Start with Windows MSI or MSIX, then add macOS app bundle and Linux AppImage or
deb/rpm.

**Step 2: Commit**

```bash
git add docs/packaging.md README.md
git commit -m "docs: add packaging plan"
```

## Completion Gates

- `cargo fmt --all -- --check` passes.
- `cargo test --workspace` passes.
- SSH shell MVP connects to a test server with password and key auth.
- Local PTY MVP works on Windows through ConPTY.
- Renderer prototype opens a native window and draws terminal cells.
- Session profiles persist without storing secrets in plaintext.
- Known host changes are blocking.
- Reference source commits are recorded in `refs/sources.json`.
