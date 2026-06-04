# MVP 1: Terminal Core

MVP 1 establishes the first usable terminal-state layer for R-SSH. It does not
open windows, connect to SSH, or spawn local shells yet. Its job is to turn a
small terminal byte stream into a styled terminal grid with enough correctness to
support the next PTY and SSH milestones.

## Completed Scope

- `Cell` model with character, foreground color, background color, bold, italic,
  underline, and inverse-video attributes.
- `TerminalGrid` allocation, bounds-checked reads, and bounds-checked writes.
- `Terminal::feed` for printable UTF-8 text.
- Newline and carriage-return handling.
- Basic ESC line movement:
  - index (`IND`, `ESC D`)
  - next line (`NEL`, `ESC E`)
  - reverse index (`RI`, `ESC M`)
- Backspace handling moves the cursor left without erasing content.
- Horizontal tab moves to the next 8-column tab stop.
- Basic main-screen linefeed scrolling when output reaches the bottom row.
- Delayed auto-wrap at the right edge, including bottom-row scroll on the next
  printable character.
- Auto-wrap mode tracking with `DECSET ?7` and `DECRST ?7`; disabled auto-wrap
  keeps the cursor at the right edge and overwrites the last column.
- Basic `DECSTBM` scroll-region handling with `ESC[<top>;<bottom>r`; linefeed
  at the bottom margin scrolls only the configured region, and `ESC[r` restores
  full-screen scrolling.
- Basic `DECOM` origin-mode handling with `DECSET ?6` and `DECRST ?6`; `CUP`
  and `HVP` are relative to the active scroll region while origin mode is
  enabled.
- Basic alternate-screen support for `DECSET ?1049` and `DECRST ?1049`, with a
  clear alternate grid and restoration of the main grid/cursor on exit.
- Basic CSI cursor handling:
  - absolute cursor positioning with `CUP`/`HVP` (`ESC[row;columnH` and
    `ESC[row;columnf`)
  - column/row positioning with `CHA`/`HPA`/`VPA` (`ESC[columnG`,
    ``ESC[column` ``, and `ESC[rowd`)
  - relative cursor movement with `CUU`, `CUD`, `CUF`, `CUB`, `HPR`, and `VPR`
  - line movement with `CNL` and `CPL`
- Cursor save/restore with both `ESC7`/`ESC8` and CSI `s`/`u`.
- Cursor visibility tracking for `DECSET ?25` and `DECRST ?25`.
- G0 character-set switching for DEC Special Graphics (`ESC(0` / `ESC(B`),
  with common box-drawing glyph mapping.
- Incomplete CSI, OSC title, and `ESC7`/`ESC8` sequences are retained across
  `Terminal::feed` calls so PTY chunk boundaries do not leak control bytes into
  the grid.
- Incomplete UTF-8 sequences are retained across `Terminal::feed` calls so PTY
  chunk boundaries do not create replacement characters.
- Basic erase handling:
  - erase in display (`ED`, `ESC[J`)
  - erase in line (`EL`, `ESC[K`)
- Basic character editing within the active row:
  - insert blank characters (`ICH`, `ESC[@`)
  - delete characters (`DCH`, `ESC[P`)
  - erase characters (`ECH`, `ESC[X`)
- Repeat preceding printable character with `REP` (`ESC[<count>b`), including
  repeated DEC Special Graphics line-drawing glyphs.
- Basic line editing within the active scroll region:
  - insert lines (`IL`, `ESC[L`)
  - delete lines (`DL`, `ESC[M`)
- Basic scroll-region scrolling:
  - scroll up (`SU`, `ESC[S`)
  - scroll down (`SD`, `ESC[T`)
- OSC title sequences terminated by BEL or ST are ignored so shell title updates
  do not appear as terminal text.
- Basic SGR handling:
  - reset
  - bold, italic, underline, inverse video
  - 8-color and bright 8-color foreground/background
  - indexed and RGB extended color forms
- CJK wide-character placement across two columns.
- Terminal grid resizing preserves visible top-left cells, clamps cursor state,
  resets the active scroll region, and marks the resized viewport damaged.
- Merged terminal damage regions for changed cells.
- Shared `DamageRegion` in `rssh-core`, re-exported by `rssh-renderer`.

## Explicit Non-Scope

- Full VT/xterm compatibility.
- Scrollback storage and full scroll-region edge-case behavior.
- Full alternate-screen edge-case behavior beyond `?1049`.
- Mouse modes.
- Hyperlinks and OSC clipboard.
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
- ESC `IND`/`NEL`/`RI` movement and scroll-region boundary scrolling
- backspace and horizontal tab control handling
- bottom-row linefeed scrolling
- delayed auto-wrap and bottom-row auto-wrap scrolling
- `?7h/l` auto-wrap mode tracking at the right edge
- `DECSTBM` scroll-region setup, reset, and region-limited linefeed scrolling
- `?6h/l` origin-mode cursor positioning relative to scroll regions
- `?1049` alternate-screen enter/exit with main screen restoration
- CSI cursor positioning, line movement, and relative cursor movement
- `?25h/l` cursor visibility tracking
- `ESC7`/`ESC8` and CSI `s`/`u` cursor save/restore
- DEC Special Graphics line drawing and split `ESC(` sequence handling
- split CSI, OSC title, and `ESC7`/`ESC8` sequences across `feed` calls
- split UTF-8 characters across `feed` calls
- CSI display and line erase handling
- CSI insert/delete/erase character handling
- CSI `REP` repeated printable character handling
- CSI insert/delete line handling with scroll-region limits
- CSI scroll up/down handling with scroll-region limits
- OSC title sequence filtering for BEL and ST terminators
- SGR color/style parsing, including inverse video
- CJK wide-character layout
- terminal grid resize growth/shrink, cursor clamping, and resize damage
- merged damage tracking

## Next Milestone

MVP 2 should connect this terminal core to real byte streams:

1. Define the PTY session trait.
2. Add Windows ConPTY support.
3. Feed local shell output into `Terminal::feed`.
4. Add an SSH shell adapter behind `rssh-ssh`.
