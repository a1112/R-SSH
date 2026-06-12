# MVP 1: Terminal Core

MVP 1 establishes the first usable terminal-state layer for R-SSH. It does not
open windows, connect to SSH, or spawn local shells yet. Its job is to turn a
small terminal byte stream into a styled terminal grid with enough correctness to
support the next PTY and SSH milestones.

## Completed Scope

- `Cell` model with character, foreground color, background color, underline
  color, underline style, bold, faint, italic, blink, underline, double
  underline, conceal, strikethrough, overline, and inverse-video attributes.
- `TerminalGrid` allocation, bounds-checked reads, and bounds-checked writes.
- `Terminal::feed` for printable UTF-8 text.
- Newline and carriage-return handling.
- C0 `NUL` is ignored without advancing the cursor; `BEL` records a pending
  bell event without writing a cell or moving the cursor; `VT` and `FF` follow
  the existing newline behavior.
- Basic ESC line movement:
  - index (`IND`, `ESC D`)
  - next line (`NEL`, `ESC E`)
  - reverse index (`RI`, `ESC M`)
- C1 byte-form `IND` (`0x84`), `NEL` (`0x85`), and `RI` (`0x8D`) map to the
  same line movement behavior.
- Backspace handling moves the cursor left without erasing content.
- Horizontal tab moves to the next 8-column tab stop.
- Custom horizontal tab stops:
  - set tab stop (`HTS`, `ESC H`)
  - clear current/all tab stops (`TBC`, `ESC[g` / `ESC[3g`)
  - cursor forward/backward tabulation (`CHT`/`CBT`, `ESC[I` / `ESC[Z`)
- C1 byte-form `HTS` (`0x88`) sets the active column as a tab stop.
- Full terminal reset with `RIS` (`ESC c`), restoring the grid, cursor, modes,
  style, scroll region, character set, and tab stops to defaults.
- Soft terminal reset with `DECSTR` (`CSI ! p`), restoring insert/replace mode,
  origin mode, scroll region, G0 character set, and saved-cursor state without
  clearing visible cells or scrollback.
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
- Basic `DECLRMM`/`DECSLRM` left-right margin handling with `DECSET ?69` and
  `CSI <left>;<right>s`; carriage return, backspace, and origin-mode `CUP`/`HVP`
  honor the active left margin.
- Basic alternate-screen support for `DECSET`/`DECRST` `?47`, `?1047`, and
  `?1049`, with a clear alternate grid and restoration of the main grid/cursor
  on exit.
- Private cursor save/restore mode `?1048h/l`.
- Basic CSI cursor handling:
  - absolute cursor positioning with `CUP`/`HVP` (`ESC[row;columnH` and
    `ESC[row;columnf`)
  - column/row positioning with `CHA`/`HPA`/`VPA` (`ESC[columnG`,
    ``ESC[column` ``, and `ESC[rowd`)
  - relative cursor movement with `CUU`, `CUD`, `CUF`, `CUB`, `HPR`, and `VPR`
  - line movement with `CNL` and `CPL`
- C1 byte-form CSI (`0x9B`) is accepted alongside the 7-bit `ESC[` form.
- Cursor save/restore with both `ESC7`/`ESC8` and CSI `s`/`u`, including
  cursor position, pending wrap state, SGR style, G0 character set, and origin
  mode.
- Cursor visibility tracking for `DECSET ?25` and `DECRST ?25`, plus cursor
  blinking tracking for `DECSET ?12` and `DECRST ?12`.
- Cursor shape and blinking tracking for `DECSCUSR` (`CSI Ps SP q`) with block,
  underline, and bar shapes.
- Xterm title-stack window operations save and restore the tracked terminal
  title through `CSI 22;0;0t` and `CSI 23;0;0t`.
- G0 character-set switching for DEC Special Graphics (`ESC(0` / `ESC(B`),
  with common box-drawing glyph mapping.
- Incomplete CSI, OSC title, DCS/SOS/PM/APC, and `ESC7`/`ESC8` sequences are
  retained across `Terminal::feed` calls so PTY chunk boundaries do not leak
  control bytes into the grid.
- `CAN` and `SUB` cancel in-progress CSI, OSC, and ST-terminated control strings
  so subsequent printable text is parsed normally.
- Incomplete UTF-8 sequences are retained across `Terminal::feed` calls so PTY
  chunk boundaries do not create replacement characters.
- Basic erase handling:
  - erase in display (`ED`, `ESC[J`)
  - erase in line (`EL`, `ESC[K`)
- Background color erase behavior uses the active SGR background for blank
  cells created by display/line/character erase, character insertion/deletion,
  and scrolling.
- xterm erase-saved-lines handling with `CSI 3 J`, clearing scrollback without
  erasing the visible grid.
- Basic character editing within the active row:
  - insert blank characters (`ICH`, `ESC[@`)
  - delete characters (`DCH`, `ESC[P`)
  - erase characters (`ECH`, `ESC[X`)
- Insert/replace mode (`IRM`, `ESC[4h` / `ESC[4l`) so printable characters can
  shift existing row content before being written.
- Repeat preceding printable character with `REP` (`ESC[<count>b`), including
  repeated DEC Special Graphics line-drawing glyphs.
- Basic line editing within the active scroll region:
  - insert lines (`IL`, `ESC[L`)
  - delete lines (`DL`, `ESC[M`)
- Basic scroll-region scrolling:
  - scroll up (`SU`, `ESC[S`)
  - scroll down (`SD`, `ESC[T`)
- OSC title sequences terminated by BEL or ST update terminal title metadata
  without appearing as terminal text.
- OSC 8 hyperlink sequences terminated by ST update cell hyperlink metadata
  without appearing as terminal text; equivalent C1 OSC/ST forms are handled
  too. SGR reset preserves the active hyperlink until an empty OSC 8 URI clears
  it.
- DCS, SOS, PM, and non-Kitty APC control strings terminated by ST are ignored
  so unsupported terminal capability probes do not appear as terminal text.
- C1 byte-form OSC (`0x9D`) and C1 ST (`0x9C`) are recognized for OSC and
  ST-terminated control strings.
- iTerm2/WezTerm `OSC 1337;File=...:<base64>` inline image metadata is parsed
  without appearing as terminal text; base64 `name` and payload data, cursor
  row/column, size, width, height, and preserve-aspect-ratio fields are retained
  in retained-history coordinates for renderer snapshots.
- Kitty Graphics Protocol APC `ESC_G...` inline images are parsed for the
  direct `a=T` path with default/direct transfer medium plus the regular-file
  `t=f` simple-file and temporary-file `t=t` media, uncompressed or `o=z`
  zlib-compressed payloads, single-block and `m=1`/`m=0` chunked payloads,
  `f=24` RGB, `f=32` RGBA, and `f=100` encoded payloads. Local-file payloads
  decode the base64 path, read only regular files, and honor optional `O`
  offset and `S` size limits; temporary-file payloads delete the file after
  reading only when the canonical path is under a known temp directory and
  contains `tty-graphics-protocol`. Raw RGB/RGBA payloads require `s`/`v` pixel
  dimensions, optional `c`/`r` display cell dimensions are retained, basic
  `x`/`y`/`w`/`h` source rectangles are retained for renderer cropping,
  `X`/`Y` target pixel offsets are retained for renderer placement, and image
  bytes are recorded in retained-history coordinates for renderer snapshots.
  Basic direct `a=q` support queries validate supported direct/local file
  payloads and queue Kitty `OK`/`EINVAL` responses for PTY writeback, honoring
  Kitty `q=1` OK-response suppression and `q=2` error-response suppression.
- Kitty Graphics Protocol stored-image flow is supported for the direct
  `a=t,i=<id>` transmit path, terminal-assigned image numbers through
  `a=t,I=<number>` with `i`/`I` OK responses, and minimal placement at the
  current cursor via either `a=p,i=<id>` or `a=p,I=<number>`, reusing stored
  pixel data and default display dimensions unless the placement supplies
  `c`/`r`. Non-zero image ids may also carry `p=<placement-id>`; repeated
  `(image id, placement id)` pairs replace the previous visible placement.
  Placements may also supply basic `x`/`y`/`w`/`h` source rectangles for
  renderer cropping and `X`/`Y` target pixel offsets for renderer placement.
  Direct and stored placements advance the cursor by the placement cell
  rectangle, while `C=1` suppresses that cursor movement. Stored-image
  placements with ids queue Kitty `OK` responses when the image exists and
  `ENOENT` responses when the referenced image id or image number is missing,
  and stored-image existence queries with `a=q,i=<id>` or `a=q,I=<number>`
  return `OK`/`ENOENT`, honoring Kitty `q=1`/`q=2` response suppression.
  Commands that specify both `i=<id>` and `I=<number>` are rejected with
  `EINVAL`.
- Kitty Graphics Protocol delete flow is supported for `a=d` to remove visible
  Kitty placements and `a=d,d=i,i=<id>` / `a=d,d=I,i=<id>` to remove placements
  for a specific image id, plus `a=d,d=n,I=<number>` /
  `a=d,d=N,I=<number>` to remove placements for the latest image assigned to
  an image number, and `a=d,d=r,x=<first>,y=<last>` /
  `a=d,d=R,x=<first>,y=<last>` to remove placements in an image-id range,
  including `p=<placement-id>` pair deletion for id/number targets. Uppercase
  delete targets `I`, `N`, and `R` also drop unreferenced stored image data.
  The position-oriented delete subset is also supported for `d=c/C` at the
  current cursor, `d=p/P,x=<col>,y=<row>` at an explicit cell, and
  `d=x/X,x=<col>` / `d=y/Y,y=<row>` for visible columns or rows. Z-index
  delete matching is supported for `d=z/Z,z=<index>` and
  `d=q/Q,x=<col>,y=<row>,z=<index>`. Uppercase forms drop
  stored image data once no visible placement still references that image id.
  Terminal erase operations also maintain image placement state: `CSI 2J`
  removes visible inline-image placements without dropping stored Kitty image
  data, while `CSI 3J` removes scrollback inline images and rebases retained
  visible image rows after scrollback is cleared. Alternate-screen `?1049`
  switching snapshots main-screen inline-image placements, hides them while the
  alternate screen is active, and discards alternate-screen placements on exit.
- Basic Sixel DCS `q` image payloads are parsed into retained inline image
  metadata for renderer snapshots, covering RGB percentage and HLS color
  definitions, color selection, raster-attribute `Ph`/`Pv` pixel dimensions,
  sixel data bytes, repeat introducers, carriage return, and sixel newline.
- Basic SGR handling:
  - reset
  - bold, faint, italic, blink, underline, double underline, conceal,
    strikethrough, overline, inverse video
  - colon-separated underline style forms `4:0` through `4:5`, including reset,
    single, double, curly, dotted, and dashed underline styles
  - 8-color and bright 8-color foreground/background
  - indexed and RGB extended color forms for foreground, background, and
    underline color, including semicolon and xterm colon-separated SGR
    parameters
- CJK wide-character placement across two columns.
- Terminal grid resizing preserves visible top-left cells, clamps cursor state,
  resets the active scroll region, and marks the resized viewport damaged.
- Merged terminal damage regions for changed cells.
- Shared `DamageRegion` in `rssh-core`, re-exported by `rssh-renderer`.
- Bounded main-screen scrollback storage records lines that leave the top of the
  full primary screen during normal upward scrolling. Local scroll regions and
  alternate-screen output do not pollute the primary scrollback.

## Explicit Non-Scope

- Full VT/xterm compatibility.
- Interactive scrollback UI/search and full scroll-region edge-case behavior.
- Full alternate-screen edge-case behavior beyond `?1049`.
- Mouse modes.
- Hyperlink activation UI and OSC clipboard handling inside terminal core.
- Full Kitty graphics protocol coverage beyond the direct and local-file
  `t=f`/`t=t` subset, including shared-memory transfers, richer placement
  controls, broader query-response variants, and animation.
- Full Sixel protocol coverage beyond the basic DCS `q`
  color/raster-size/repeat/newline bitmap subset.
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
- C0 `NUL` filtering, `BEL` event tracking, and `VT`/`FF` newline handling
- ESC `IND`/`NEL`/`RI` movement and scroll-region boundary scrolling
- C1 byte-form `IND`/`NEL`/`RI` line movement
- backspace and horizontal tab control handling
- custom tab stop setting, clearing, C1 `HTS`, and CSI forward/backward tab
  movement
- `RIS` full terminal reset for grid, cursor, modes, style, character set, and
  tab stops
- `DECSTR` soft terminal reset for insert/replace mode, origin mode, scroll
  region, G0 character set, and saved-cursor state without clearing cells
- bottom-row linefeed scrolling
- delayed auto-wrap and bottom-row auto-wrap scrolling
- `?7h/l` auto-wrap mode tracking at the right edge
- `DECSTBM` scroll-region setup, reset, and region-limited linefeed scrolling
- `?6h/l` origin-mode cursor positioning relative to scroll regions
- `?47`, `?1047`, and `?1049` alternate-screen enter/exit with main screen
  restoration
- `?1049` alternate-screen inline-image isolation and main-screen image
  restoration
- `?1048h/l` private cursor save/restore without switching screens
- CSI cursor positioning, line movement, and relative cursor movement
- C1 byte-form CSI parsing and split-sequence buffering
- `?25h/l` cursor visibility tracking and `?12h/l` cursor blinking tracking
- `DECSCUSR` cursor shape/blinking tracking for block, underline, and bar
  cursors
- `ESC7`/`ESC8` and CSI `s`/`u` cursor save/restore, including style,
  character set, and origin-mode restoration
- DEC Special Graphics line drawing and split `ESC(` sequence handling
- split CSI, OSC title, DCS/SOS/PM/APC, and `ESC7`/`ESC8` sequences across
  `feed` calls
- `CAN`/`SUB` cancellation for CSI, OSC, and ST-terminated control strings
- split UTF-8 characters across `feed` calls
- CSI display and line erase handling
- background color erase for display/line/character erase, insert/delete
  character blanks, and newly exposed scroll rows
- CSI `3J` scrollback clearing without visible grid erasure
- CSI insert/delete/erase character handling
- CSI `IRM` insert/replace mode for printable character writes
- CSI `REP` repeated printable character handling
- CSI insert/delete line handling with scroll-region limits
- CSI `S`/`T` scrolling moves inline image placements with their text rows and
  drops placements scrolled out of the affected region
- CSI scroll up/down handling with scroll-region limits
- OSC title metadata tracking and text filtering for BEL and ST terminators
- OSC 8 hyperlink metadata tracking, including C1 OSC/ST forms, and SGR reset
  preservation
- DCS/SOS/PM/APC control-string filtering with split-sequence buffering
- C1 byte-form OSC/ST control-string filtering
- iTerm2/WezTerm `OSC 1337;File` inline image metadata and payload capture
- Kitty direct `a=T` RGB inline image metadata, zlib decompression, and chunked
  payload capture
- Kitty regular-file `t=f` simple-file RGB payload capture with optional `O`/`S`
  file slicing
- Kitty temporary-file `t=t` RGB payload capture with optional `O`/`S` slicing
  and guarded `tty-graphics-protocol` temp-file deletion
- Kitty stored `a=t` image metadata and minimal `a=p,i=<id>` placement capture,
  including `p=<placement-id>` replacement
- Kitty `I=<number>` image-number uploads with generated image-id responses,
  placement and deletion by image number, and `i`/`I` mutual-exclusion errors
- Kitty direct and stored-placement `x`/`y`/`w`/`h` source rectangle metadata
  propagation for renderer cropping
- Kitty direct and stored-placement `X`/`Y` target pixel offset metadata
  propagation for renderer placement
- Kitty direct and stored-placement cursor movement, including `C=1`
  no-cursor-movement suppression
- Kitty direct `a=q` support query responses plus stored-image query and
  stored-placement `OK`/`ENOENT` responses for PTY writeback
- Kitty Graphics Protocol `q=1` OK-response and `q=2` error-response
  suppression
- Kitty `a=d` visible-placement deletion, image-id deletion, image-number
  deletion, image-id range deletion, plus `(image id, placement id)` pair
  deletion
- Kitty `a=d` cursor-cell, explicit-cell, visible-column, and visible-row
  placement deletion
- Kitty `a=d` z-index placement deletion and cell-plus-z-index placement
  deletion
- Terminal erase display `CSI 2J`/`CSI 3J` inline-image deletion and retained
  visible image row rebasing
- Basic Sixel DCS `q` bitmap capture with RGB and HLS palette color
  definitions, raster-attribute `Ph`/`Pv` pixel dimensions, repeat introducers,
  carriage returns, and sixel newlines
- SGR color/style parsing, including inverse video, faint, blink, double
  underline, colon-separated underline styles, underline color, conceal,
  strikethrough, overline, and colon-separated extended color parameters
- CJK wide-character layout
- terminal grid resize growth/shrink, cursor clamping, and resize damage
- merged damage tracking
- bounded main-screen scrollback capture, excluding local scroll regions and
  alternate-screen output

## Next Milestone

MVP 2 should connect this terminal core to real byte streams:

1. Define the PTY session trait.
2. Add Windows ConPTY support.
3. Feed local shell output into `Terminal::feed`.
4. Add an SSH shell adapter behind `rssh-ssh`.
