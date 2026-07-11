# Static load_scheme Path Expressions Design

## Goal

Match more of WezTerm's `wezterm.color.load_scheme` argument semantics by
accepting paths that R-SSH can evaluate statically, rather than requiring the
path to appear inline as a quoted or long-bracket literal.

The supported subset includes top-level string variables, variable chains, and
pure static `..` concatenation while preserving Lua assignment-time value
semantics.

## Upstream behavior

Pinned WezTerm registers `load_scheme` as an mlua function whose argument type
is `String`. Lua evaluates the argument expression before the Rust function
receives it, so literals, variables, concatenation, and function results are
all equivalent when they evaluate to a string. The function reads that path,
parses the TOML scheme, and returns `(colors, metadata)`.

R-SSH statically interprets a bounded Lua subset instead of executing arbitrary
configuration code. It already recognizes direct, `require`, module-alias,
static-key, and function-alias forms of `load_scheme`, plus the first colors
result in single- and multiple-variable assignments. The remaining immediate
gap is that every recognized call is normalized and then sent back through a
literal-only path parser.

## Chosen scope

Support path expressions composed only of:

- quoted or long-bracket string literals;
- a top-level variable whose latest binding before evaluation is another
  supported static string expression;
- `..` concatenation of supported static string expressions;
- recursive variable chains up to a fixed depth limit.

This is broader than special-casing one `path_var`, but remains deterministic
and consistent with the repository's static Lua compatibility model.

Keep these forms outside this slice:

- helper calls, conditionals, `os.getenv`, and other runtime computation;
- `wezterm.config_dir`, home-directory, or environment-backed values until the
  static config environment models them explicitly;
- consumption of `metadata.name` or other fields from the second return value;
- general Lua execution or a full AST evaluator.

Existing recognition of the first return value as colors remains unchanged.

## Architecture

### Canonical call parser

Route every recognized call form through one canonical path-call parser. The
outer resolver computes the original call's source offset exactly once, before
any direct/module/function alias is normalized into an owned canonical string.
The canonical parser receives both that string and the original offset.

For a parenthesized call, reuse the balanced argument-list and top-level
argument splitters and require exactly one argument. For Lua's no-parentheses
call syntax, retain only quoted and long-bracket literal sugar; an identifier
cannot legally be used as no-parentheses string-call sugar.

Reject incomplete calls, multiple arguments, and expression continuations
after the call while continuing to accept the statement and table-field
terminators already used by supported configuration forms.

### Static path expression evaluator

Add a path-focused recursive evaluator with a small depth limit. It parses an
exact string literal, splits top-level `..` concatenation, or resolves an exact
identifier through its latest top-level binding before a supplied offset.

Variable lookup must be strict: the final matching top-level declaration or
assignment controls the result even when its right-hand side is dynamic or
otherwise unsupported. In that case static resolution fails rather than
falling back to an older literal binding.

When following a variable binding, recursively evaluate its right-hand side
with the lookup boundary moved back to that assignment's source offset. This
preserves Lua's assignment-time value capture. For example:

```lua
local dir = '/first'
local path = dir .. '/scheme.toml'
dir = '/second'
local colors = wezterm.color.load_scheme(path)
```

must resolve `path` under `/first`, not `/second`. A self-referential update
such as `path = path .. '.toml'` likewise sees the earlier `path` binding.

Top-level scanning continues to exclude helper/function bodies. Rebindings
after the call are outside the call offset and cannot affect the result.

### Shared consumers

The existing `config.colors`, intermediate colors variable, and
`config.color_schemes['Name']` flows already converge on
`lua_wezterm_color_load_scheme_path_from_query_with_static_source`. Extend that
central resolver rather than adding consumer-specific variable handling.

Update the legacy/fallback `config.colors` assignment path to extract the full
right-hand-side expression and invoke the same central resolver. This keeps
returned table initializers and other supported assignment shapes from
retaining a literal-only edge path.

## Data flow

1. A colors consumer finds a direct call or traces a colors variable back to a
   `load_scheme` assignment.
2. The central resolver records the original call offset.
3. Direct receivers or supported aliases normalize to one canonical
   `wezterm.color.load_scheme` call without changing that offset.
4. The call parser extracts exactly one argument expression.
5. The static evaluator resolves literals, bindings, and concatenation using
   assignment-time lookup boundaries.
6. The resulting path enters the existing TOML read/parse and color-override
   path.

No new configuration state or color-application path is introduced.

## Error handling

An unsupported or ambiguous expression returns no static path and therefore no
static color override, matching existing behavior for Lua outside R-SSH's
supported subset. Dynamic shadowing must fail closed rather than use stale
static data. Existing file-read and TOML parse handling remains unchanged.

The recursion limit prevents alias cycles and pathological expression chains.

## Testing

Use TDD with focused parser and integration coverage:

1. A parser test covers exact literals, direct static concatenation, recursive
   variable chains, assignment-time capture, and self-referential extension.
2. A parser test proves a dynamic latest binding, a later uninitialized
   declaration, multiple arguments, and invalid no-parentheses identifier
   syntax are rejected rather than falling back.
3. A `config.colors` integration test uses multiple temporary schemes to prove
   the latest supported path binding before the call wins and a later rebinding
   is ignored.
4. A `config.color_schemes` integration test uses a module/function alias and a
   statically concatenated path variable to prove the second consumer shares
   the resolver.
5. Existing literal, direct/parenthesized `require`, static-key, function-alias,
   result-variable, mutation, and TOML-loading regressions remain green.

Update `docs/research/wezterm-parity-gap.md`, `docs/architecture.md`, and
`docs/mvp-6-app-shell-v1.md` with the same bounded claim: supported static path
expressions resolve at their evaluation points, while dynamic path computation
and metadata consumption remain open.
