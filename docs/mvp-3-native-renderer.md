# MVP 3: Native Window Renderer

MVP 3 adds the first native window rendering path. It is still a demo renderer,
not a full terminal compositor, but it proves the next product-critical chain:
terminal grid -> renderer cells -> RGBA framebuffer -> native `winit` window.

## Completed Scope

- `rssh-renderer` converts `rssh-terminal::TerminalGrid` or
  `rssh-terminal::Terminal` into a `TerminalRenderSnapshot`.
- `TerminalRenderSnapshot` keeps row, column, character, foreground,
  background, and basic style flags including inverse video for visible cells.
- `TerminalRenderSnapshot` carries visible cursor row/column and shape when
  built from a `Terminal`.
- `PixelRenderer` draws snapshot cells into an RGBA framebuffer.
- `PixelRenderer` maps the xterm 256-color indexed palette, including the
  6x6x6 color cube and grayscale ramp.
- `PixelRenderer` draws bold text with an extra bitmap stroke.
- `PixelRenderer` draws underlined and strikethrough text using the cell
  foreground color.
- `PixelRenderer` draws block, underline, and bar cursors for visible cursor
  snapshots.
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
- terminal cursor position and shape to render snapshot conversion
- preservation of cell position and style metadata
- glyph foreground pixels drawn into an RGBA target
- bold terminal text drawn with additional foreground pixels
- underlined and strikethrough terminal text drawn into an RGBA target
- xterm 256-color indexed foreground output from terminal bytes to RGBA pixels
- inverse-video foreground/background swapping
- block, underline, and bar cursor pixels drawn into an RGBA target

## Explicit Non-Scope

- GPU text shaping with `cosmic-text`.
- Glyph atlas caching.
- Scrollback rendering.
- Selection and mouse interaction.
- Live PTY streaming into the native window.
- SSH session rendering.
- Terminal grid resizing from window size.

## Next Milestone

MVP 4 should connect the native window to a live PTY-backed terminal session:

1. Move PTY output feeding from the console path into a shared session runtime.
2. Feed PTY bytes into `rssh-terminal` continuously.
3. Rebuild render snapshots from terminal damage.
4. Send keyboard input from `winit` events to the active PTY writer.
