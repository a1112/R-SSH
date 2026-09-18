# Stage 7 Product GUI Probe Design

## Goal

Measure the exact packaged `production-gui` executable for the Stage 7
empty-window, SSH1, and GPU-steady residence cohorts. The existing benchmark
launcher currently starts the private `diagnostic-gui` command, which is
disabled in the production feature set and therefore cannot certify the
product binary.

The probe must not add a public R-SSH CLI option, enable `diagnostic-gui` in a
production build, or let the diagnostic executable satisfy product evidence.

## Architecture

Add a private product-probe mode to `rssh-bench-launcher`. In this mode the
launcher starts the normal `ssh --gui --renderer auto` product entry point and
passes a versioned probe descriptor through a private environment variable.
The descriptor contains only the run ID, scenario, and bounded hold duration.
The existing isolated SSH fixture continues to pass its generated password
through the existing secret environment channel.

`run_ssh_gui` validates the descriptor before creating the event loop. When it
is present, it installs the existing diagnostic marker/hold controller around
the normal product pane. Empty-window suppresses transport startup; SSH1 uses
the normal native SSH pane and consumes the fixture secret only when the
masked password prompt is active. The product CLI, renderer selection, pane
runtime, and packaged executable remain unchanged.

The product-probe descriptor is an internal test protocol, not a trust
boundary. It is fail-closed: unknown fields, invalid run IDs, unsupported
scenarios, or out-of-range hold durations abort before a window is created.
No password, key material, host path, or runner identity is placed in the
descriptor or marker stream.

## Data Flow

1. The Stage 7 coordinator invokes `run-stage0-diagnostics.ps1` with its new
   product-probe switch and the explicit packaged product executable.
2. Stage 0 invokes `rssh-bench-launcher --product-gui` without building.
3. The launcher creates the SSH1 loopback fixture when required, sets the
   versioned descriptor, and starts normal `ssh --gui` arguments.
4. `run_ssh_gui` emits the existing process/window/GPU/scenario markers from
   the actual product path.
5. The launcher waits for the owner-ready marker, samples the product process,
   requests bounded shutdown, and preserves the existing
   `rssh.diagnostics/v2` record.
6. The Stage 7 coordinator retains ten raw samples per process and aggregates
   process medians exactly as frozen by the split contract.

## Failure Handling

- A production probe that does not reach a complete GPU frame fails rather
  than falling back to diagnostic evidence.
- SSH1 must reach a connected frame; prompt, authentication, host-key, or
  transport failure remains visible in the launcher result and fails the gate.
- Product-probe mode accepts only renderer `auto` and rejects diagnostic GPU,
  font, and attribution overrides.
- The descriptor and fixture secret are removed from the child environment
  after each bounded launch by normal child-process lifetime; neither is
  copied into JSON evidence.
- Existing diagnostic font and attribution runs continue to use
  `diagnostic-gui` and the separately hashed diagnostic executable.

## Verification

Unit tests cover product-mode argument parsing, incompatible option rejection,
descriptor validation, and normal product CLI construction for empty-window
and SSH1. Contract tests require Stage 0 and both product coordinators to select
product mode for all residence measurements while startup continues to use
`--benchmark-startup` directly.

A bounded Windows smoke test uses a production-only build and one launcher
process to prove the previous `diagnostic-tools` error is gone. The protected
5+30 hardware cohorts remain the only source of Stage 7 GO evidence.
