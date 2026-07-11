# Built-in Color Scheme Alias Completeness Design

## Goal

Ensure every canonical color-scheme name and alias in the repository's pinned
WezTerm color-scheme data resolves through R-SSH's static built-in palette
lookup.

## Scope

Keep `builtin_color_scheme_toml` as an explicit Rust mapping. Add only the
upstream aliases that resolve to an existing bundled palette. Do not introduce
name normalization, fuzzy matching, runtime downloads, or dynamic Lua palette
construction.

## Design

The parity test reads the pinned WezTerm color-scheme data and checks its
canonical names and aliases against `builtin_color_scheme_toml`. The lookup
continues to return `None` for unknown names. When the test identifies an
unmapped alias, add it as an alternate match arm for the palette it aliases.

This preserves the current deterministic lookup behavior while making the
pinned upstream dataset the completeness contract.

## Verification

Run the focused alias-completeness test, then the `rssh-app` test suite and the
workspace test suite. The final state must have zero upstream canonical names
or aliases missing from the lookup.
