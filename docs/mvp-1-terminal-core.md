# MVP 1: Terminal Core

MVP 1 establishes the first usable terminal-state layer for R-SSH. It does not
open windows, connect to SSH, or spawn local shells yet. Its job is to turn a
small terminal byte stream into a styled terminal grid with enough correctness to
support the next PTY and SSH milestones.

## Completed Scope

- `Cell` model with character, foreground color, background color, bold, italic,
  and underline attributes.
- `TerminalGrid` allocation, bounds-checked reads, and bounds-checked writes.
- `Terminal::feed` for printable UTF-8 text.
- Newline and carriage-return handling.
- Backspace handling moves the cursor left without erasing content.
- Horizontal tab moves to the next 8-column tab stop.
- Basic main-screen linefeed scrolling when output reaches the bottom row.
- Delayed auto-wrap at the right edge, including bottom-row scroll on the next
  printable character.
- Basic CSI cursor handling:
  - absolute cursor positioning with `CUP`/`HVP` (`ESC[row;columnH` and
    `ESC[row;columnf`)
  - relative cursor movement with `CUU`, `CUD`, `CUF`, and `CUB`
- Cursor save/restore with both `ESC7`/`ESC8` and CSI `s`/`u`.
- Basic erase handling:
  - erase in display (`ED`, `ESC[J`)
  - erase in line (`EL`, `ESC[K`)
- OSC title sequences terminated by BEL or ST are ignored so shell title updates
  do not appear as terminal text.
- Basic SGR handling:
  - reset
  - bold, italic, underline
  - 8-color and bright 8-color foreground/background
  - indexed and RGB extended color forms
- CJK wide-character placement across two columns.
- Merged terminal damage regions for changed cells.
- Shared `DamageRegion` in `rssh-core`, re-exported by `rssh-renderer`.

## Explicit Non-Scope

- Full VT/xterm compatibility.
- Scrollback and configurable scroll-region behavior.
- Alternate screen.
- Mouse modes.
- Hyperlinks and OSC clipboard.
- Streaming partial UTF-8 handling across separate `feed` calls.
- GPU rendering.
- Local PTY and SSH channel I/O.

## Verification

Run:

```powershell
cargo fmt --all -- --check
cargo test --workspace
```

The terminal-specific MVP 1 checks live in `crates/rssh-terminal/src/lib.rs` and
cover:

- styled default cells
- grid get/set bounds behavior
- plain text parsing
- newline parsing
- backspace and horizontal tab control handling
- bottom-row linefeed scrolling
- delayed auto-wrap and bottom-row auto-wrap scrolling
- CSI cursor positioning and relative cursor movement
- `ESC7`/`ESC8` and CSI `s`/`u` cursor save/restore
- CSI display and line erase handling
- OSC title sequence filtering for BEL and ST terminators
- SGR color/style parsing
- CJK wide-character layout
- merged damage tracking

## Next Milestone

MVP 2 should connect this terminal core to real byte streams:

1. Define the PTY session trait.
2. Add Windows ConPTY support.
3. Feed local shell output into `Terminal::feed`.
4. Add an SSH shell adapter behind `rssh-ssh`.
