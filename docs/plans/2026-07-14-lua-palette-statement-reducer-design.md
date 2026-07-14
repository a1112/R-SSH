# Lua Palette Statement Reducer Design

## Goal

Replace the overlapping palette-variable scanners with one bounded, source-ordered
statement pipeline. The pipeline must make identity validation and mutation
application agree on exactly what each supported Lua statement means, while
continuing to fail closed for dynamic behavior.

This design completes the built-in scheme-map consumer slice and establishes the
mutation semantics required by the next `wezterm.color.get_default_colors()`
slice, especially preservation of the upstream 16..255 indexed palette.

## Why the current shape must change

The current implementation independently:

- splits top-level statements for binding identity;
- recognizes direct mutations for escape analysis;
- rescans the source through field, palette-slot, indexed-slot, tab-bar, and
  ColorSpec mutation helpers;
- reconstructs synthetic Lua tables and reparses them.

That duplication has produced observable disagreements: function aliases can
escape capture tracking, literal parsers can accept only an expression prefix,
empty composite assignments can be classified as safe but never replayed,
long strings can be confused with long-string index keys, and the repeated
rescans make mutation replay quadratic in the number of statements.

## Considered approaches

### 1. Unified statement events and a sequential reducer (selected)

Lex each logical top-level statement once, classify supported palette effects
into typed events, and use the same event stream for identity proof and mutation
replay. This is a larger refactor, but it removes the source of the review
failures and provides the correct foundation for default palettes.

### 2. Continue extending the existing scanners

Add special cases for function aliases, boolean tails, empty tables, and long
strings. This minimizes the immediate diff but preserves divergent parsers and
the quadratic replay path. Two review cycles have already demonstrated that
this approach does not provide a defensible completion boundary.

### 3. Execute Lua in a restricted runtime

Evaluate the relevant configuration and inspect the resulting tables. This
would improve language fidelity, but it expands the trust and side-effect
surface well beyond the bounded native compatibility layer. It is not suitable
for this slice.

## Architecture

### Logical statement lexer

Introduce a single iterator over logical top-level statements. Each item
contains its exact source range and a lexical view that understands:

- quoted strings and escapes;
- Lua long strings, including contents beginning with `[`;
- bracket indexes whose key is a long string, such as `palette[[[background]]]`;
- balanced table, call, and index delimiters;
- dotted call continuations split across lines;
- comments and labels without losing source offsets.

The lexer must determine long-string versus outer-index syntax from balanced
context and delimiters, not from a one-character lookahead heuristic.

### Typed palette events

Parse each logical statement at most once into one of a bounded set of events:

- `KnownBinding` for table, `load_scheme`, built-in scheme, and later default
  palette sources;
- `UnknownBinding` for an unprovable whole-variable assignment;
- `ScalarReplace` for a complete supported scalar or ColorSpec value;
- `CompositeReplace` for whole indexed, ANSI, brights, tab-bar, and nested item
  tables, including explicit empty-table replacement where the target type can
  represent it;
- `SlotPatch` for indexed, ANSI, and bright entries;
- `NestedPatch` for ColorSpec and tab-bar fields;
- `FunctionDefinition`, `FunctionAlias`, and `FunctionCall` for closure identity;
- `Escape` for aliases, unknown calls, receiver calls, or any other use whose
  effect cannot be proven;
- `Irrelevant` for statements that do not reference the tracked palette.

Events retain the statement source range and all statically resolved keys and
values. Unsupported or partially parsed palette statements do not degrade to
`Irrelevant`; they become `Escape` or cause the candidate source to fail closed.

### Exact expression parsing

Every supported RHS parser returns both its value and consumed range. A
statement is accepted only when comments and whitespace are the only remaining
tokens. This rule applies uniformly to strings, long strings, booleans,
numbers, tables, ColorSpec values, and static aliases.

Consequently, expressions such as `false or true`, a quoted prefix followed by
an unknown call, and a table literal followed by concatenation are rejected
instead of being interpreted as their first token.

### Identity and closure tracking

The identity pass consumes the same event stream used by the reducer. It tracks:

- the latest known palette binding and its mutation lifetime;
- unknown rebindings and escapes;
- functions that capture the active palette variable cell;
- transitive function aliases and redefinitions;
- calls through either original names or aliases.

Function capture is tied to the lexical binding that exists at the definition
site. A function defined before a later `local palette` declaration must not be
treated as capturing that later local. If an alias retains an older closure
after the original function name is redefined, the alias continues to refer to
the older capture.

Any called closure whose mutation cannot be replayed exactly invalidates the
static candidate.

### Sequential mutation reducer

Start from the resolved base palette and apply events once in ascending source
offset order:

- scalar and ColorSpec replacement use last-write-wins semantics;
- whole indexed replacement discards the prior indexed map, including when the
  replacement is empty;
- indexed slot patches modify the current map without removing other entries;
- whole ANSI/brights replacement replaces exactly the corresponding eight
  entries; incomplete arrays fail closed unless the target representation
  explicitly supports an empty value;
- whole tab-bar/item replacement clears fields absent from the replacement;
- nested patches modify the current composite value after any earlier
  replacement;
- later whole replacements discard earlier nested or slot patches.

No synthetic multi-statement Lua table is built. No mutation helper rescans the
source from the beginning. Parsing plus replay is linear in the relevant source
length and event count.

## Error handling and compatibility boundary

The pipeline remains a bounded static interpreter. Dynamic branches, loops,
computed keys, arbitrary calls, result-table aliases that cannot be proven,
and side-effecting argument expressions fail closed.

Existing supported direct tables, `load_scheme`, built-in schemes, whole-map
lookups, static aliases, quoted/long-string keys, indexed/ANSI/brights slots,
tab-bar mutations, and ColorSpec mutations remain supported. A compatibility
test must exist before deleting or replacing any old helper.

## Testing

Use TDD in three layers:

1. Lexer and exact-expression unit tests for ordinary long strings, long-string
   index keys, comments, dotted continuations, boolean tails, and literal tails.
2. Event and reducer tests for function aliases, binding lifetimes, empty and
   non-empty whole replacements, slot patches, nested patches, and both
   directions of whole-versus-nested ordering.
3. End-to-end `WindowApp` tests across table, `load_scheme`, direct built-in,
   whole-map built-in, custom schemes, and `config.colors` consumers.

Required regression groups include all current `scheme_map`, `whole_map`,
`load_scheme`, `palette_mutation`, and `color_spec` tests. Final verification is
the serial `rssh-app` suite, workspace tests, `cargo fmt --all -- --check`, and
`git diff --check`.

## Delivery sequence

Keep the already passing source-order and long-bracket positive tests, add the
reviewer counterexamples as RED tests, introduce the event pipeline beside the
old helpers, migrate one source kind at a time, then delete superseded scanners
only after every compatibility group is green. Update the parity tracker after
the built-in map slice is fully approved; implement `get_default_colors()` on
the same reducer in the following slice.
