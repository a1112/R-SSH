# Functional CI Stabilization Design

## Goal

Make PR #4's CI and functional-test workflows deterministic on GitHub-hosted runners, while retaining privileged macOS coverage as an explicit manual validation path.

## Constraints

- Preserve the closed functional scenario and behavior catalogs.
- Keep evidence generation fail-closed: failed scenarios must not produce passing manifests.
- Do not run untrusted pull-request code on self-hosted runners.
- Do not weaken assertions merely to make platform failures disappear.
- Keep privileged macOS jobs available even though the repository currently has no registered self-hosted runner.

## Design

### PR and manual job topology

Pull requests execute only jobs backed by GitHub-hosted runners. The trusted aggregate validates a hosted PR matrix that includes hosted macOS but excludes the three privileged self-hosted jobs. `workflow_dispatch` retains the privileged native, Tauri, and production-Tauri macOS jobs and a separate full aggregate. This prevents a PR from remaining queued forever when no self-hosted runner exists without deleting the privileged coverage contract.

### Deterministic lifecycle handling

PTY and transport drivers treat normal peer shutdown as a lifecycle outcome rather than an infrastructure error. Reader completion, writer closure, child exit, and master close are ordered explicitly, and tests cover readers that disconnect after delivering all expected bytes. Stress synchronization uses an observable protocol marker and drains output before asserting completion.

### Portable endpoints and platform harnesses

Unix observer endpoints resolve the requested evidence path to a short, owner-only socket under a protected runtime directory, using the same deterministic hash identity as Windows named pipes. X11 and Wayland helpers poll bounded readiness conditions, preserve compositor diagnostics, and make cleanup idempotent. Window discovery follows the launched process tree instead of assuming the top-level process owns the visible window.

### Browser and production smoke compatibility

Clipboard permissions are granted only where Playwright supports them; Firefox and WebKit still exercise clipboard behavior through page-level capability checks and explicit fallback expectations. Production Web smoke resolves its config relative to the `web` package. Production Tauri smoke waits for an observable PTY child and records startup diagnostics before failing.

### Policy and failure reporting

The OSI-approved `0BSD` license is added to the cargo-deny allowlist because it enters through the selected local-socket dependency. Evidence directories are created before execution so failure uploads retain diagnostics. Platform scripts report the actual startup or readiness failure rather than a cleanup error.

## Validation

Each root cause receives a regression test that is observed failing before the implementation change. Focused tests run after each fix, followed by formatting, Clippy, Python policy tests, Web lint/unit/browser enumeration, both matrix validators, the full Rust workspace suite, and staged-diff checks. After push, PR CI and Functional workflows are monitored until they complete or expose a new actionable failure.
