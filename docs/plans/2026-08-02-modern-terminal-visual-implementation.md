# Modern Terminal Visuals Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ship a modern dark default appearance using Cascadia Mono, a balanced deep-blue ANSI palette, concept-scale 17px/10x21 terminal density, comfortable padding, and a clearly layered tab bar without weakening Unicode or WezTerm configuration compatibility.

**Architecture:** Keep all appearance changes in the existing native-window defaults and GPU font bootstrap so user configuration retains precedence. Platform font loading remains best-effort with embedded fixtures as the final fallback; terminal grid geometry and GPU raster size change together to preserve row and column alignment. The modern default density is intentionally larger than the legacy compatibility fixtures, while explicit WezTerm font/grid/padding values continue to win. DPI is applied exactly once: logical terminal metrics become physical cells, the GPU atlas rasterizes at the same effective scale, and monitor transitions resize the physical window while preserving terminal rows and columns.

**Tech Stack:** Rust 2024, winit, wgpu, glyphon, rssh-fonts/cosmic-text, native window screenshot E2E, built-in ImageGen for the concept mockup.

---

### Task 1: Generate and approve the visual concept

**Files:**
- Create: `docs/assets/rssh-modern-terminal-concept.png`
- Reference: `docs/plans/2026-08-02-modern-terminal-visual-design.md`

**Step 1: Generate the concept image**

Use the built-in ImageGen tool with this project-bound prompt:

```text
Use case: ui-mockup
Asset type: R-SSH terminal visual concept
Primary request: a polished modern dark terminal window matching a practical Windows desktop application
Scene/backdrop: isolated application window, no desktop wallpaper
Subject: one R-SSH terminal window with a compact dark tab bar, PowerShell prompt, Git status, Chinese text, ANSI colors, and a short AI CLI response
Style/medium: high-fidelity desktop UI mockup, implementation-realistic, no glassmorphism
Composition/framing: 16:10 landscape, full window visible, comfortable 8px content padding
Color palette: background #0B1220, foreground #D8E2F0, tab bar #080D18, active tab #172033, cyan accent #38BDF8, cursor #67E8F9
Text (verbatim): "R-SSH", "main", "你好，终端", "cargo test --workspace", "All checks passed"
Constraints: Cascadia Mono-like typography, crisp grid alignment, restrained shadows, legible ANSI red green yellow blue cyan magenta
Avoid: neon cyberpunk, translucent background, fake IDE panels, distorted text, watermark
```

**Step 2: Inspect and persist the selected output**

Copy the selected output into `docs/assets/rssh-modern-terminal-concept.png` and inspect it at original resolution. Verify the terminal window is fully visible, the palette relationships match the design, and the image does not imply unsupported UI.

**Step 3: Commit**

```powershell
git add -- docs/assets/rssh-modern-terminal-concept.png
git commit -m "docs: add modern terminal visual concept"
```

### Task 2: Lock the new defaults with failing tests

**Files:**
- Modify: `crates/rssh-app/src/window.rs:132600-135900`
- Modify: `crates/rssh-app/src/window_gpu.rs:580-630`

**Step 1: Write the palette and padding tests**

Add tests that construct `NativeWindowApp::new_for_test()` and assert:

```rust
assert_eq!(app.native_resolved_palette().foreground, Color::Rgb(0xd8, 0xe2, 0xf0));
assert_eq!(app.native_resolved_palette().background, Color::Rgb(0x0b, 0x12, 0x20));
assert_eq!(app.native_resolved_palette().cursor_bg, Color::Rgb(0x67, 0xe8, 0xf9));
assert_eq!(app.window_padding, NativeWindowPadding {
    left: NativeWindowPaddingDimension::Pixels(8),
    right: NativeWindowPaddingDimension::Pixels(8),
    top: NativeWindowPaddingDimension::Pixels(6),
    bottom: NativeWindowPaddingDimension::Pixels(6),
});
```

Assert all 16 ANSI slots against the approved palette and assert that configured color/padding overrides still win.

**Step 2: Write the font selection test**

On Windows, load `bundled_emergency_font_catalog()` and shape `"R-SSH 你好 😀"`. Assert the Latin clusters select `Cascadia Mono`, the Chinese clusters are non-tofu, and all clusters retain the expected logical cell spans.

**Step 3: Run tests to verify failure**

```powershell
$env:TEMP='E:\temp\rssh-visual'; $env:TMP='E:\temp\rssh-visual'; $env:CARGO_TARGET_DIR='E:\project\R-SSH\target\production-parity'
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default -- --nocapture
```

Expected: FAIL because the current defaults remain `#0c0c0c`, `#e5e5e5`, zero padding, and Noto Sans.

**Step 4: Commit the failing tests**

```powershell
git add -- crates/rssh-app/src/window.rs crates/rssh-app/src/window_gpu.rs
git commit -m "test: specify modern terminal visual defaults"
```

### Task 3: Make Cascadia Mono the platform-preferred terminal font

**Files:**
- Modify: `crates/rssh-app/src/window_gpu.rs:445-580`

**Step 1: Add platform candidates**

Add Windows candidates before the current script fallbacks:

```rust
("CascadiaMono.system.ttf", r"C:\Windows\Fonts\CascadiaMono.ttf"),
("CascadiaCode.system.ttf", r"C:\Windows\Fonts\CascadiaCode.ttf"),
("SourceCodePro.system.ttf", r"C:\Windows\Fonts\SourceCodePro-Regular.ttf"),
("Consolas.system.ttf", r"C:\Windows\Fonts\consola.ttf"),
```

Add macOS Menlo/Monaco and Linux Noto Sans Mono/DejaVu Sans Mono candidates using best-effort paths. Missing or invalid files must remain non-fatal.

**Step 2: Change the font configuration**

Use `Cascadia Mono` as the primary family, followed by `Cascadia Code`, `Source Code Pro`, `Consolas`, platform mono families, `Noto Sans`, then the existing script fallbacks. Set the initial raster font size to `15.0`.

**Step 3: Run the focused tests**

```powershell
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default_font -- --nocapture
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app emergency_font_catalog_covers_common_cli_ui_scripts -- --nocapture
```

Expected: PASS, with no tofu for common scripts.

**Step 4: Commit**

```powershell
git add -- crates/rssh-app/src/window_gpu.rs
git commit -m "feat: prefer modern monospace terminal fonts"
```

### Task 4: Apply the modern default palette

**Files:**
- Modify: `crates/rssh-app/src/window.rs:195-235`
- Modify: `crates/rssh-app/src/window.rs:84570-84620`
- Test: `crates/rssh-app/src/window.rs:132600-135900`

**Step 1: Replace the terminal defaults**

Set foreground/background/cursor and ANSI constants to:

```text
foreground #D8E2F0   background #0B1220   cursor #67E8F9
black      #111827   red        #F87171   green  #4ADE80   yellow #FBBF24
blue       #60A5FA   magenta    #C084FC   cyan   #22D3EE   white  #CBD5E1
bright blk #64748B   bright red #FB7185   bright grn #86EFAC bright ylw #FDE047
bright blu #93C5FD   bright mag #D8B4FE   bright cyn #67E8F9 bright wht #F8FAFC
```

Use a dark cursor foreground and a blue-gray selection background. Keep `native_wezterm_default_colors_palette()` unchanged because it represents the pinned upstream API, not the R-SSH application default.

**Step 2: Make configured schemes retain precedence**

Run existing color-scheme tests and ensure Lua/TOML overrides replace every new default exactly as before.

**Step 3: Run tests**

```powershell
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default_palette -- --nocapture
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app color_scheme -- --nocapture
```

Expected: PASS.

**Step 4: Commit**

```powershell
git add -- crates/rssh-app/src/window.rs
git commit -m "feat: add modern dark default palette"
```

### Task 5: Align terminal grid geometry and padding

**Files:**
- Modify: `crates/rssh-app/src/window.rs:80-140`
- Modify: `crates/rssh-app/src/window.rs:220-235`
- Test: `crates/rssh-app/src/window.rs:137900-140100`

**Step 1: Write the geometry regression tests**

Assert the base grid uses 9x18 physical pixels, initial frame dimensions derive from those constants, the PTY row/column count remains unchanged, and configured padding is removed before pixel-to-cell conversion.

**Step 2: Run tests to verify failure**

```powershell
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default_geometry -- --nocapture
```

Expected: FAIL against the current 8x16 zero-padding defaults.

**Step 3: Implement the geometry defaults**

Add modern-only geometry defaults of `10x21` cells and `14/14/10/10` pixels of padding, and use them for the modern production window. Keep the legacy `CELL_WIDTH`/`CELL_HEIGHT` and padding constants unchanged for compatibility fixtures and explicit legacy input; update only tests whose expectations intentionally derive from the modern defaults.

**Step 4: Run geometry and native window tests**

```powershell
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default_geometry -- --nocapture
cargo +1.89.0 test --locked -p rssh-app --test native_window_e2e -- --nocapture
```

Expected: PASS.

**Step 5: Commit**

```powershell
git add -- crates/rssh-app/src/window.rs
git commit -m "feat: improve default terminal spacing"
```

### Task 6: Style the default tab bar and render a real comparison

**Files:**
- Modify: `crates/rssh-app/src/window.rs:3150-3225`
- Modify: `crates/rssh-app/src/window.rs:84580-84620`
- Create: `.tmp/rssh-modern-terminal-real.png` (verification artifact, do not commit)

**Step 1: Add failing tab-bar tests**

Assert the native app default resolves:

```text
tab background #080D18
active tab background #172033, foreground #F8FAFC, bold
inactive tab background #101827, foreground #8492A6
hover background #1E293B, foreground #D8E2F0
new-tab background #080D18, foreground #38BDF8
```

Assert explicit WezTerm `colors.tab_bar` entries still override each value.

**Step 2: Implement default tab colors**

Introduce named default constants and initialize `NativeWindowApp` with them. Do not change `NativeTabBarItemColors::default()` because that type also represents “no override” in parsers.

**Step 3: Run focused tests**

```powershell
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app modern_default_tab_bar -- --nocapture
cargo +1.89.0 test --locked -p rssh-app --bin rssh-app tab_bar -- --nocapture
```

Expected: PASS.

**Step 4: Build and capture the real window**

```powershell
$env:TEMP='E:\temp\rssh-visual'; $env:TMP='E:\temp\rssh-visual'; $env:CARGO_TARGET_DIR='E:\project\R-SSH\target\production-parity'
cargo +1.89.0 build --locked --release -p rssh-app
```

Launch the release window with the Unicode/ANSI fixture used in prior visual testing and capture `.tmp/rssh-modern-terminal-real.png`. Compare it with the concept image for hierarchy, spacing, contrast, baseline, wide-character alignment, and clipping.

**Step 5: Commit**

```powershell
git add -- crates/rssh-app/src/window.rs
git commit -m "feat: style the default terminal tab bar"
```

### Task 7: Run the compatibility and regression gates

**Files:**
- Modify if needed: `crates/rssh-app/src/window.rs`
- Modify if needed: `crates/rssh-app/src/window_gpu.rs`
- Test: `crates/rssh-renderer/tests/gpu_text.rs`

**Step 1: Run focused rendering tests**

```powershell
cargo +1.89.0 test --locked -p rssh-renderer --test gpu_text -- --nocapture
cargo +1.89.0 test --locked -p rssh-app --test native_window_e2e -- --nocapture
```

Expected: GPU text 18/18 and native window E2E 5/5 pass, or higher if new tests are added.

**Step 2: Repeat real visual compatibility captures**

Capture and inspect OpenCode, Claude Code, Codex, Git color log, npm help, 35-line output, and multilingual Unicode at 100%, 125%, and 150% Windows scaling when available. No capture may show tofu, row overlap, baseline drift, or compressed tab glyphs.

**Step 3: Run the full gate**

```powershell
cargo +1.89.0 test --locked --workspace --all-targets
cargo +1.89.0 fmt --all -- --check
git diff --check
```

Expected: exit code 0 and zero failed tests.

**Step 4: Run release diagnostics**

```powershell
E:\project\R-SSH\target\production-parity\release\rssh-app.exe doctor --json
E:\project\R-SSH\target\production-parity\release\rssh-app.exe self-test --json
```

Expected: both reports contain `"ok":true`.

**Step 5: Commit final verification adjustments**

```powershell
git add -- crates/rssh-app/src/window.rs crates/rssh-app/src/window_gpu.rs crates/rssh-renderer/tests/gpu_text.rs
git commit -m "test: verify modern terminal visuals"
```
