# WezTerm Built-in Color Scheme Completion Design

## Goal

Complete R-SSH's static WezTerm built-in color-scheme catalog through the end
of the upstream alphabetical list while preserving exact palette behavior and
keeping each review unit small.

## Source of truth

Use `refs/wezterm/docs/colorschemes/data.json` from the pinned local WezTerm
reference as the authoritative source for scheme names, aliases, colors,
metadata, and indexed palette entries. Do not hand-normalize values or merge
distinct upstream schemes merely because their palettes are similar.

## Delivery strategy

First finish the existing uncommitted Ura-through-VibrantInk batch. Then add
the remaining schemes, from Vice Alt through zenwritten_light, in alphabetical
batches of roughly ten schemes. Each batch is an independent commit so that
source-data mistakes are easy to review and bisect.

For every scheme, update all four parity surfaces together:

1. Add the exact name-to-TOML mapping, including only non-conflicting upstream
   aliases.
2. Embed the upstream TOML-equivalent colors and metadata without changing
   R-SSH's existing color resolution semantics.
3. Add a table-driven effective-config test covering foreground, background,
   cursor, selection, representative ANSI entries, and indexed color 16 when
   present.
4. Extend the implemented-scheme inventory in
   `docs/research/wezterm-parity-gap.md`.

## Error handling and compatibility

Unknown scheme names continue to use the existing unresolved-name path. A
missing optional upstream field remains absent rather than being synthesized.
Aliases must not shadow a distinct canonical scheme already present in the
catalog. Existing built-in and user-defined scheme precedence is unchanged.

## Verification

For each batch:

- Run the targeted table-driven color-scheme test.
- Run `cargo fmt --all -- --check`.
- Run `cargo test -p rssh-app` before committing the batch.

After the final Z batch, run `cargo test --workspace` and compare the upstream
name set against R-SSH's mapping table to prove that no remaining canonical
scheme is missing.
