# Stage 5: Startup, Lazy Resources, and Composition

## Goal

Make `rssh-app` a deliberately small production GUI composition root while
preserving the existing CPU-first/GPU-later startup path and all native SSH UX.
The Stage 5 release candidate must keep Windows first-present p95 at or below
500 ms and move empty-window and one-SSH-session memory toward the 45 MiB and
60 MiB gates without redefining the Stage 0 metrics.

## Startup contract

The product startup path is divided into four observable layers:

| Layer | Trigger | Allowed work | Work that must remain absent |
| --- | --- | --- | --- |
| A: first-present critical | process start through the first successful CPU present | minimal CLI/profile projection, event loop, window, bootstrap surface and bootstrap snapshot | Tokio runtime, network connection, known-hosts I/O, SFTP/SCP, full fallback font catalog, image decoder initialization, diagnostics |
| B: post-present idle | first-present completion | full configuration, watcher, deferred WGPU completion, bounded fallback-font discovery | secrets, network connection, transfer protocols |
| C: first native SSH connection | SSH pane starts after first present | one shared bounded Tokio runtime, DNS/TCP, host-key verification, authentication and channel startup | SFTP/SCP/forwarding unless explicitly requested |
| D: feature on demand | explicit command or content | transfers, forwarding, optional image formats and diagnostics | unrelated optional resources |

`first_present` remains the timing boundary. Memory collection remains a
separate marker and must not delay that boundary.

## Shared native SSH runtime

`rssh-ssh` will expose a cloneable runtime handle and a lazy runtime owner.
The production configuration is initially:

- two Tokio worker threads;
- eight blocking threads maximum;
- `rssh-io` thread names;
- I/O and time drivers enabled;
- one runtime per `rssh-app` composition, reused by every native GUI pane.

`RusshChannelOpener` accepts an injected handle. Existing CLI and library
callers retain a compatibility fallback so this change does not alter public
connection behavior. The GUI connector obtains the handle only after the
first-present boundary and before its first native connection attempt. Runtime
construction failure becomes the existing pane-level `Failed` state and never
terminates the event loop.

Tests must prove that the owner is uninitialized before first access, concurrent
access returns the same handle, different openers reuse that handle, the worker
and blocking limits are explicit, and cancellation/shutdown behavior remains
unchanged.

## Product feature and entrypoint matrix

Stage 5 introduces these product features:

- `native-gui`: winit, bootstrap presentation, renderer and window composition;
- `ssh`: native SSH shell support and its runtime adapter;
- `local-pty`: local PTY shell support and its runtime adapter;
- `image-basic`: PNG/JPEG decoding;
- `image-gif`: animated GIF decoding;
- `image-legacy`: DDS/Farbfeld/ICO/PNM/TGA/TIFF decoding;
- `transfer-tools`: SFTP/SCP and forwarding CLI paths;
- `diagnostic-tools`: bench, doctor, self-test and diagnostic GUI paths;
- `production-gui`: `native-gui + ssh + local-pty + image-basic`.

During the migration, the developer default retains the complete command
surface so existing tests and downstream invocations do not silently lose
commands. Release packaging explicitly builds the `production-gui` profile;
diagnostic and transfer executables are built as separate artifacts/jobs. A
machine-readable contract test prevents a packaged GUI build from enabling
`diagnostic-tools`, `transfer-tools`, GIF or legacy image formats.

The current `rssh-app` executable name and supported GUI invocations remain
stable. Unsupported commands in a deliberately reduced build fail with a clear
feature-specific error instead of disappearing from help or panicking.

## Implementation batches

### Batch 1: contracts and shared lazy runtime

1. Add failing unit tests for lazy initialization, runtime identity and bounded
   configuration.
2. Add the runtime handle/owner to `rssh-ssh`.
3. Inject the shared handle into GUI `RusshChannelOpener` instances after the
   first-present boundary.
4. Run native SSH loopback, host-key, auth, reconnect, cancellation and Clippy
   suites.

### Batch 2: decoder and product feature boundaries

1. Split `rterm-render-cpu` image decoder features into basic, GIF and legacy
   groups with basic as the production default.
2. Add `rssh-app` product features and make optional dependencies follow them.
3. Gate command dispatch and entrypoint-only modules without changing the full
   developer build behavior.
4. Add `cargo tree` and compile-contract tests for the minimal production GUI.

### Batch 3: packaging and startup enforcement

1. Change release packaging to build the explicit production GUI feature set.
2. Keep deterministic reduced-feature builds in shared PR CI and absolute
   startup/memory gates only on the fixed Windows runner.
3. Add a startup contract asserting that runtime/font/image/transfer resources
   are absent before `first_present`.
4. Run 40 process-cold startup samples plus empty-window and one-SSH memory
   candidates on the fixed runner.

## Required verification

- `cargo test -p rssh-ssh --all-targets --locked`
- `cargo test -p rssh-app --all-targets --locked`
- `cargo test --workspace --all-targets --locked`
- reduced production GUI build and dependency-tree contract
- native SSH loopback: password, agent, encrypted and unencrypted key
- host-key unknown/changed decisions, retry and cancellation
- CPU-to-GPU and forced-GPU-failure native window E2E
- package smoke, format and Clippy with warnings denied
- fixed Windows runner: 40 process-cold runs, p95 first present at most 500 ms

## Rollback rules

Each batch is a separate commit. The GUI can fall back to its current
per-connection runtime by reverting Batch 1, and packaging can fall back to the
full developer feature set by reverting Batch 3. Any SSH UX regression, package
smoke failure, or first-present regression pauses Stage 5; thresholds and metric
definitions are not changed to make the result pass.
