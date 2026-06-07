# Xterm Mouse Protocol Expansion Design

## Goal

Move R-SSH closer to a complete native console by supporting the older xterm
mouse protocol variants still negotiated by terminal applications:

- `1005` UTF-8 extended mouse coordinates
- `1015` urxvt extended mouse coordinates

`1016` SGR-pixels is intentionally out of scope for implementation because it
requires pixel coordinates. The current console and native window input models
only carry terminal cell coordinates.

## Context

The current implementation supports:

- Mouse reporting modes `1000`, `1002`, and `1003`
- Focus reporting `1004`
- SGR mouse protocol `1006`
- Legacy `CSI M` encoding when no extended protocol is active
- DECRQM status replies for the supported mouse modes

This leaves applications that request `1005` or `1015` without a faithful
negotiation path. Reporting support through DECRQM is also incomplete for those
modes.

## Design

Extend `MouseProtocolMode` from a two-state enum to four states:

- `X10`: default legacy `CSI M` coordinate bytes
- `Utf8`: xterm `1005` UTF-8 coordinate bytes
- `Sgr`: xterm `1006` `CSI < Cb ; Cx ; Cy M/m`
- `Urxvt`: urxvt `1015` `CSI Cb ; Cx ; Cy M/m`

`MouseModes` continues to hold the active reporting granularity separately from
the active encoding protocol. The protocol flags may all be tracked for DECRQM,
but the effective input mode exposes one encoder selected in deterministic
preference order:

1. SGR `1006`
2. urxvt `1015`
3. UTF-8 `1005`
4. legacy X10

`1016` remains unknown to DECRQM and ignored for mode tracking until the event
model carries pixel coordinates.

## Encoding Rules

All protocol paths use the existing button/modifier code calculation.

- Legacy X10: preserve current `CSI M` behavior.
- UTF-8 `1005`: encode `code + 32`, `column + 32`, and `row + 32` as UTF-8
  scalar values. Button release keeps the existing X10-style release code.
- SGR `1006`: preserve current `CSI <...M/m` behavior.
- urxvt `1015`: emit `CSI {code + 32};{column};{row}M`. Button release keeps
  the X10-style release code and final `M`.

The console (`local.rs`) and native window (`window.rs`) must use the same
encoder semantics.

## Testing

Add tests before implementation:

- `terminal_modes.rs`:
  - `1005` and `1015` mode tracking updates `MouseInputMode`
  - DECRQM reports enabled/disabled for `1005` and `1015`
  - `1016` remains unknown
  - protocol fallback is deterministic when multiple protocols are enabled
- `local.rs`:
  - crossterm mouse events encode as UTF-8 and urxvt sequences
- `window.rs`:
  - native window mouse events encode as UTF-8 and urxvt sequences
- `terminal_runtime.rs`:
  - runtime DECRQM and `mouse_input_mode()` reflect the shared tracker

Run targeted tests for each module while developing, then run the full workspace
verification before packaging.

## Documentation

Update README and MVP docs to state:

- supported mouse reporting modes: `1000`, `1002`, `1003`
- supported mouse protocols: legacy X10, `1005`, `1006`, `1015`
- unsupported protocol: `1016` because pixel coordinates are not represented
