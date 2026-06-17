# MVP 3: Native Window Renderer

MVP 3 adds the first native window rendering path. It is still a demo renderer,
not a full terminal compositor, but it proves the next product-critical chain:
terminal grid -> renderer cells -> RGBA framebuffer -> native `winit` window.

## Completed Scope

- `rssh-renderer` converts `rssh-terminal::TerminalGrid` or
  `rssh-terminal::Terminal` into a `TerminalRenderSnapshot`.
- `TerminalRenderSnapshot` keeps row, column, character, foreground,
  background, and basic style flags including inverse video for visible cells.
  When terminal screen reverse-video mode is active, snapshots XOR the global
  inverse state across the full visible viewport, including otherwise blank
  cells.
- `TerminalRenderSnapshot` carries visible cursor row/column, shape, and blink
  state when built from a `Terminal`.
- `TerminalRenderSnapshot` carries iTerm2/WezTerm inline image metadata and
  decoded payload bytes from `OSC 1337;File=...`, plus the supported Kitty
  direct/local-file image subset, translating retained-history image rows into
  live or scrollback viewport rows and preserving image items when pane
  snapshots are overlaid.
- `PixelRenderer` draws snapshot cells into an RGBA framebuffer.
- `PixelRenderer` decodes PNG, JPEG, and GIF iTerm2/WezTerm inline image
  payloads from `OSC 1337;File=...`, including delay-aware animated GIF frame
  selection by elapsed render time, draws them into the RGBA framebuffer,
  supports cell and `px` dimensions, and includes them in damage-region redraws
  when damage intersects any covered image cell.
- `PixelRenderer` draws the supported Kitty direct/local-file image subset:
  single-block and chunked, uncompressed and `o=z` zlib-compressed raw `f=24`
  RGB, raw `f=32` RGBA, and encoded `f=100` payloads through the existing image
  decoder path, including regular-file `t=f` simple-file payloads with optional
  `O`/`S` file slicing and temporary-file `t=t` payloads with guarded
  `tty-graphics-protocol` temp-file deletion. It uses cell or pixel dimensions
  from the terminal snapshot and clips source pixels with basic `x`/`y`/`w`/`h`
  source rectangles, then applies Kitty `X`/`Y` target pixel offsets relative
  to the placement cell.
- `PixelRenderer` draws minimal Kitty stored-image placements produced by
  `a=t,i=<id>` plus `a=p,i=<id>` because those placements are normalized into
  the same render snapshot image items as direct displays. Placement ids are
  preserved in snapshots so `(image id, placement id)` replacement/deletion is
  reflected before drawing, and placement source rectangles crop the stored
  source image before scaling. Stored placements also apply Kitty `X`/`Y`
  target pixel offsets relative to the placement cell.
- `PixelRenderer` applies Kitty z-index layer ordering for supported inline
  images: negative z-index images render between cell backgrounds and text,
  negative z-index values below `i32::MIN / 2` render below non-default cell
  backgrounds, while zero and positive z-index images render above text in
  ascending z-index order, using Kitty image id as the tie-breaker when
  overlapping Kitty images have the same z-index.
- `PixelRenderer` omits Kitty placements removed by supported `a=d` delete
  actions, including image-id, placement-id, cursor-cell, explicit-cell,
  visible-column, visible-row, z-index, and cell-plus-z-index deletes, because
  deletion updates the terminal snapshot before rendering.
- `PixelRenderer` draws basic Sixel DCS `q` images because the terminal core
  normalizes supported Sixel VT340 default palette entries, RGB plus DEC HLS
  hue color definitions, DCS macro pixel aspect, DECGRA aspect override,
  raster-size, repeat, and newline bitmap payloads into raw RGBA inline image
  snapshot items. Default and `?80l` output starts at the text cursor and
  advances below the image while preserving the left-edge column; `?8452h`
  moves the post-Sixel cursor to the right edge. DECSDM `?80h` output starts
  at the active graphics-page origin while preserving the text cursor, and
  WezTerm's tmux-control `DCS 1000 q` is ignored rather than emitted as a
  Sixel snapshot item.
- `PixelRenderer` maps the xterm 256-color indexed palette, including the
  6x6x6 color cube and grayscale ramp.
- `PixelRenderer` draws bold text with an extra bitmap stroke, italic text with
  a slanted bitmap glyph pass, and faint text with a dimmed foreground color.
- `PixelRenderer` can render hidden blink phases by suppressing foreground
  pixels for blinking cells while preserving cell backgrounds.
- `PixelRenderer` draws underlined and double-underlined text with the cell
  underline color when set, and draws strikethrough and overlined text using
  the cell foreground color.
- `PixelRenderer` draws colon-separated underline styles, including curly,
  dotted, and dashed underline strokes, using the cell underline color when set.
- `PixelRenderer` hides concealed text foreground pixels while preserving cell
  background rendering.
- `PixelRenderer` draws block, underline, and bar cursors for visible cursor
  snapshots, supports configurable px, DPI-scaled pt, percent-of-default, and
  cell-fraction thickness for underline/bar cursor glyphs, can force
  reverse-video cursor fills from the cursor cell's effective foreground color,
  and supports both hidden-phase and interpolated-opacity blinking cursors.
- The renderer uses `font8x8` for a minimal built-in glyph path.
- `rssh-app` starts a native `winit` window by default.
- `pixels` presents the renderer framebuffer through a GPU-backed window
  surface.
- `rssh-app window --frames N` renders N frames and exits, which gives the
  native window path an automated smoke check.
- `rssh-app local` remains available for the console-hosted local PTY path.

## Run

Open the native renderer demo:

```powershell
cargo run -p rssh-app
```

Equivalent explicit command:

```powershell
cargo run -p rssh-app -- window
```

Automated one-frame window smoke:

```powershell
cargo run -p rssh-app -- window --frames 1
```

## Verification

Default checks:

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Native window smoke:

```powershell
cargo run -p rssh-app -- window --frames 1
```

Renderer-specific tests cover:

- terminal grid to render snapshot conversion
- terminal cursor position, shape, and blink state to render snapshot conversion
- iTerm2/WezTerm inline image metadata/payload propagation into render
  snapshots, including scrollback viewports and overlaid pane snapshots
- render snapshot cell color mapping used by inactive-pane styling
- PNG, JPEG, GIF first-frame, selected animated GIF frame, and delay-aware
  elapsed-time animated GIF frame drawing for iTerm2/WezTerm inline image
  payloads, Kitty direct, local-file `t=f`/`t=t`, stored-placement,
  zlib-compressed, and chunked RGB image payload drawing, Kitty source-rectangle
  cropping, Kitty target pixel offsets, Kitty z-index layer ordering, Kitty
  image, placement, cell, row, column, and z-index deletion, basic Sixel bitmap
  drawing, pixel-sized image bounds, and damage-region redraws for
  image-covered cells
- preservation of cell position and style metadata, including faint, italic,
  blinking, double-underlined, underline-styled, underline-colored, concealed,
  and overlined cells
- blink-phase hiding for blinking text plus hidden/interpolated-opacity
  blinking cursors
- glyph foreground pixels drawn into an RGBA target
- bold terminal text drawn with additional foreground pixels
- italic terminal text drawn with shifted foreground pixels
- faint terminal text drawn with dimmed foreground pixels
- blinking terminal text hidden during an explicit hidden blink phase
- concealed terminal text rendered as background-only cells
- underlined, underline-styled, underline-colored, double-underlined,
  strikethrough, and overlined terminal text drawn into an RGBA target
- xterm 256-color indexed foreground output from terminal bytes to RGBA pixels
- inverse-video foreground/background swapping
- full-viewport screen reverse-video snapshots for `DECSET ?5`
- block, underline, and bar cursor pixels drawn into an RGBA target, including
  px, DPI-scaled pt, percent-of-default, and cell-fraction thickness overrides for
  underline/bar cursor glyphs

## Explicit Non-Scope

- GPU text shaping with `cosmic-text`.
- Glyph atlas caching.
- Scrollback rendering.
- Selection and mouse interaction.
- High-quality image resampling, full Kitty image protocol drawing beyond
  direct/local-file payload transfer/display, remaining Kitty placement controls
  beyond source rectangles and target offsets, and full Sixel protocol coverage
  beyond the basic bitmap subset.
- Live PTY streaming into the native window.
- SSH session rendering.
- Terminal grid resizing from window size.

## Next Milestone

MVP 4 should connect the native window to a live PTY-backed terminal session:

1. Move PTY output feeding from the console path into a shared session runtime.
2. Feed PTY bytes into `rssh-terminal` continuously.
3. Rebuild render snapshots from terminal damage.
4. Send keyboard input from `winit` events to the active PTY writer.
