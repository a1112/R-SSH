# WezTerm Built-in Color Scheme Completion Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Finish the static WezTerm built-in color-scheme catalog from Ura through zenwritten_light with exact upstream palette resolution and proof that no canonical upstream scheme is missing.

**Architecture:** Keep the existing name-to-embedded-TOML lookup in `window.rs`. Treat the pinned WezTerm `data.json` as authoritative, add schemes in small alphabetical batches, and test each batch through `NativeWindowApp::native_effective_config` rather than testing raw strings.

**Tech Stack:** Rust, Cargo tests, TOML parsing already present in `rssh-app`, pinned WezTerm JSON reference.

---

### Task 1: Finish the existing Ura-through-VibrantInk batch

**Files:**
- Modify: `crates/rssh-app/src/window.rs:20432`
- Modify: `crates/rssh-app/src/window.rs:58264`
- Test: `crates/rssh-app/src/window.rs:146057`
- Modify: `docs/research/wezterm-parity-gap.md:3875`

**Step 1: Repair the failing test fixture**

Rename the table test to end in `_twilight_light_to_vibrantink_builtin_color_schemes`. Correct its tuple values to match the established field types: `selection_fg` is `Option<Option<Color>>`, while `cursor_bg` is `Color`. In particular, use the Ura-through-VibrantInk source values from `refs/wezterm/docs/colorschemes/data.json`, including Urple (Gogh)'s cursor background `#877a9b`.

**Step 2: Run the targeted test to verify the existing implementation**

Run:

```powershell
cargo test -p rssh-app window_app_loads_wezterm_lua_twilight_light_to_vibrantink_builtin_color_schemes -- --nocapture
```

Expected: PASS for all Ura-through-VibrantInk cases.

**Step 3: Check formatting and the application test suite**

Run:

```powershell
cargo fmt --all -- --check
cargo test -p rssh-app
```

Expected: both commands exit 0.

**Step 4: Commit the batch**

```powershell
git add crates/rssh-app/src/window.rs docs/research/wezterm-parity-gap.md
git commit -m "feat: load ura vibrant ink schemes"
```

### Task 2: Add Vice Alt through Vs Code Light+

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Write the failing effective-config table test**

Add `window_app_loads_wezterm_lua_vice_alt_to_vs_code_light_builtin_color_schemes` with cases for: Vice Alt, Vice Dark, vimbones, Violet Dark, Violet Light, VisiBlue, VisiBone, Visibone Alt. 2, Vs Code Dark+, and Vs Code Light+. Assert foreground, background, cursor, selection, ANSI 0/1/2/3/8/15, and indexed 16 when present.

**Step 2: Run it and confirm the catalog entries are missing**

```powershell
cargo test -p rssh-app window_app_loads_wezterm_lua_vice_alt_to_vs_code_light_builtin_color_schemes -- --nocapture
```

Expected: FAIL because the new names do not yet resolve to upstream palettes.

**Step 3: Add the exact upstream mapping and embedded TOML**

Copy the ten canonical records from `refs/wezterm/docs/colorschemes/data.json` into the existing match table and constant section. Preserve optional fields, indexed colors, metadata, and only non-conflicting aliases.

**Step 4: Update the parity inventory and verify**

Add the ten canonical names to `docs/research/wezterm-parity-gap.md`, then run the targeted test, `cargo fmt --all -- --check`, and `cargo test -p rssh-app`.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs docs/research/wezterm-parity-gap.md
git commit -m "feat: load vice violet vscode schemes"
```

### Task 3: Add vulcan through WildCherry

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Write a failing table test**

Add `window_app_loads_wezterm_lua_vulcan_to_wildcherry_builtin_color_schemes` for: vulcan, VWbug, Warm Neon, WarmNeon, Website, Wez, Wez (Gogh), Whimsy, Wild Cherry, and WildCherry.

**Step 2: Run the targeted test**

Expected: FAIL before mappings are added.

**Step 3: Add exact upstream mappings and TOML records**

Preserve both canonical schemes where a Gogh and non-Gogh variant exist. Add aliases only when they do not hide a distinct canonical record.

**Step 4: Update docs and verify**

Run the targeted test, format check, and full `rssh-app` tests. Expected: all pass.

**Step 5: Commit**

```powershell
git add crates/rssh-app/src/window.rs docs/research/wezterm-parity-gap.md
git commit -m "feat: load vulcan wild cherry schemes"
```

### Task 4: Add wilmersdorf through Wombat

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Write a failing table test**

Add `window_app_loads_wezterm_lua_wilmersdorf_to_wombat_builtin_color_schemes` for: wilmersdorf, Windows 10, Windows 10 Light, Windows 95, Windows 95 Light, Windows High Contrast, Windows High Contrast Light, Windows NT, Windows NT Light, and Wombat.

**Step 2: Run the targeted test**

Expected: FAIL before implementation.

**Step 3: Add upstream mappings and TOML**

Preserve the distinct light/dark base16 records and all indexed palette entries.

**Step 4: Update docs, verify, and commit**

Run the targeted test, format check, and `cargo test -p rssh-app`; then commit as `feat: load wilmersdorf windows wombat schemes`.

### Task 5: Add Wombat (Gogh) through zenbones

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Write a failing table test**

Add `window_app_loads_wezterm_lua_wombat_gogh_to_zenbones_builtin_color_schemes` for: Wombat (Gogh), Woodland, Wryan, Wryan (Gogh), Wzoreck, X::DotShare, X::Erosion, XCode Dusk, Yousai, and zenbones.

**Step 2: Run the targeted test**

Expected: FAIL before implementation.

**Step 3: Add exact mappings and TOML**

Keep punctuation and case exact for `X::` names and lowercase `zenbones`.

**Step 4: Update docs, verify, and commit**

Run the targeted test, format check, and `cargo test -p rssh-app`; then commit as `feat: load wombat zenbones schemes`.

### Task 6: Add zenbones_dark through zenwritten_light

**Files:**
- Modify: `crates/rssh-app/src/window.rs`
- Test: `crates/rssh-app/src/window.rs`
- Modify: `docs/research/wezterm-parity-gap.md`

**Step 1: Write a failing table test**

Add `window_app_loads_wezterm_lua_zenbones_dark_to_zenwritten_light_builtin_color_schemes` for: zenbones_dark, Zenburn, Zenburn (base16), Zenburn (Gogh), zenburn (terminal.sexy), zenburned, zenwritten_dark, and zenwritten_light.

**Step 2: Run the targeted test**

Expected: FAIL before implementation.

**Step 3: Add exact mappings and TOML**

Retain all case-sensitive canonical names because the five Zenburn-family names are distinct upstream records.

**Step 4: Update docs, verify, and commit**

Run the targeted test, format check, and `cargo test -p rssh-app`; then commit as `feat: load remaining zen schemes`.

### Task 7: Prove catalog completeness

**Files:**
- Modify if needed: `crates/rssh-app/src/window.rs`
- Modify if needed: `docs/research/wezterm-parity-gap.md`

**Step 1: Compare canonical name sets**

Extract every `metadata.name` from the pinned upstream JSON and every canonical string arm from `builtin_color_scheme_toml`. Account separately for aliases and the seven R-SSH `Builtin *` schemes. Expected: no upstream canonical name is absent.

**Step 2: Repair any discrepancies with a failing regression case first**

If the comparison finds a missing or shadowed record, add it to the appropriate table test before changing the mapping.

**Step 3: Run final verification**

```powershell
cargo fmt --all -- --check
cargo test --workspace
```

Expected: both commands exit 0.

**Step 4: Record final evidence**

Update `docs/research/wezterm-parity-gap.md` to state that the pinned canonical built-in color-scheme catalog is complete, without claiming broader WezTerm feature parity.

**Step 5: Commit only if the audit required changes**

Use `fix: complete WezTerm color scheme catalog` for catalog corrections or `docs: record WezTerm color catalog parity` for documentation-only evidence.
