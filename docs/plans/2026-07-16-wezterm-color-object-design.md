# WezTerm Color Object Design

## Goal

Add bounded-static support for the complete `ColorWrap` API exposed by
WezTerm commit `093bf6b`. A color returned by `wezterm.color.parse(...)` or
`wezterm.color.from_hsla(...)` must be usable through static variables, method
chains, multi-result assignments, comparisons, and every existing R-SSH color
consumer without claiming arbitrary Lua execution.

## Upstream contract

At commit `093bf6b`, `lua-api-crates/color-funcs/src/lib.rs` exposes a
`ColorWrap(RgbaColor)` userdata. It provides:

- constructors: `wezterm.color.parse(string)` and
  `wezterm.color.from_hsla(h, s, l, a)`;
- color-returning methods: `complement`, `complement_ryb`, `saturate`,
  `desaturate`, `saturate_fixed`, `desaturate_fixed`, `lighten`, `darken`,
  `lighten_fixed`, `darken_fixed`, `adjust_hue_fixed`, and
  `adjust_hue_fixed_ryb`;
- multi-color methods: `triad` and `square`;
- component methods: `srgba_u8`, `linear_rgba`, `hsla`, and `laba`;
- numeric comparison methods: `contrast_ratio` and `delta_e`;
- `__tostring` and `__eq`.

The implementation delegates its color math to `wezterm-color-types` version
`0.3.0`. R-SSH will use that same crate and version rather than reproducing the
HSL, RYB, linear RGB, Lab, contrast, and CIEDE2000 algorithms.

## Approaches considered

### Typed bounded-static evaluator using `wezterm-color-types` (selected)

Introduce a small evaluator whose values distinguish colors, numbers,
booleans, strings, and ordered tuples. It evaluates only the constructors,
methods, variables, aliases, and operators needed by the upstream Color API.
Adapters expose those typed results to the existing color, number, boolean,
string, and multi-assignment parsers.

This preserves upstream calculations and gives all consumers one identity and
evaluation model.

### Copy the upstream formulas into `window.rs`

This avoids a dependency but duplicates HSL/RYB conversion, gamma correction,
Lab conversion, contrast, and DeltaE behavior. It would be easy for rounding or
future maintenance to drift from the pinned upstream implementation.

### Extend only `lua_color_from_query`

This is the smallest change and could support color-returning method chains,
but it cannot correctly model `triad`, `square`, component tuples, numeric
comparisons, equality, or `tostring`. It would not complete the approved Color
object boundary.

## Value representation and precision

The evaluator uses an internal value similar to:

```rust
enum NativeStaticLuaColorValue {
    Color(wezterm_color_types::SrgbaTuple),
    Number(f64),
    Integer(u8),
    Bool(bool),
    String(String),
    Tuple(Vec<NativeStaticLuaColorValue>),
}
```

Color method chains retain `SrgbaTuple` throughout evaluation. Conversion to
R-SSH's `rssh_terminal::Color` happens only at a consumer boundary:

- opaque consumers drop alpha in the existing way;
- selection/background consumers retain the existing u8 alpha semantics;
- `srgba_u8` and `tostring` use upstream truncation and formatting;
- floating component and comparison results remain numeric.

This avoids cumulative u8 quantization between chained methods.

## Expression grammar

The evaluator accepts a deliberately bounded expression grammar:

1. exact constructor calls:
   - `wezterm.color.parse(<static string>)`;
   - `wezterm.color.from_hsla(<four finite static numbers>)`;
2. the same statically proven module/color-namespace/static-key aliases used by
   the existing color helper resolvers;
3. direct constructor aliases such as
   `local parse = wezterm.color.parse; parse('red')`;
4. top-level color variables resolved at their original binding offset;
5. exact colon-method calls on a proven color expression, including chained
   calls;
6. exact equality/inequality between proven color expressions;
7. `tostring(<proven color expression>)`;
8. top-level single- and multi-target assignments whose right-hand side is a
   proven scalar or tuple result.

Method lookup extraction, bound-method escape, arbitrary dot-call emulation,
table packing/unpacking of tuple results, loops, branches, runtime-generated
arguments, metatable mutation, and general Lua evaluation remain outside this
slice. Color methods use the upstream userdata's documented colon-call shape.

## Arity and result rules

Each method has an exact signature:

- no arguments: `complement`, `complement_ryb`, `triad`, `square`,
  `srgba_u8`, `linear_rgba`, `hsla`, and `laba`;
- one finite numeric argument: all saturation, lightness, and hue methods;
- one proven color argument: `contrast_ratio` and `delta_e`.

`triad` returns two colors and `square` returns three. The component methods
return four values. Multi-target assignment follows Lua's positional behavior:
missing targets are ignored and missing values become unprovable for this
bounded evaluator rather than being invented. Scalar consumers use only scalar
results; they do not silently take the first element of a tuple.

## Static identity and ordering

Constructor aliases, color variables, and tuple component variables resolve at
the expression's original source offset. Rebinding after a call does not change
the captured value. A fresh known color binding resets prior identity, while
unknown rebinding, multi-target ambiguity, dynamic mutation, closure escape, or
an identity read that cannot be proven causes the relevant expression to fail
closed.

The resolver keeps the existing recursion limit and non-function-block rules.
It reuses the shared WezTerm receiver/static-key machinery so API mutation or
shadowing invalidates constructor resolution consistently with
`get_default_colors`, `get_builtin_schemes`, and `load_scheme`.

## Consumer integration

The evaluator is inserted beneath the shared static-source adapters rather than
special-cased in individual fields:

- `lua_color_from_query_with_static_source` accepts scalar Color results;
- opaque and selection adapters preserve their current alpha behavior;
- numeric static parsing accepts scalar Number/Integer results;
- boolean static parsing accepts color equality results;
- string/static `tostring` paths accept the upstream Color string;
- multi-target binding resolution exposes tuple elements to later supported
  expressions.

As a result, Color expressions work in all existing color locations:
`config.colors`, named color schemes and their mutations, window/background
gradients, background layers, window frame and integrated title colors,
ColorSpec fields, visual bell and selector colors, and any later consumer that
already uses the shared adapters.

## Error handling

The evaluator returns `None` when proof fails. It rejects:

- wrong constructor or method arity;
- non-finite numeric values;
- expression tails after a complete result;
- dynamic arguments, keys, receivers, or rebinding;
- method calls on non-colors;
- tuple use in scalar contexts;
- scalar use where an unresolved tuple element is required;
- cycles or recursion beyond the shared limit;
- mutation or shadowing of the relevant WezTerm API.

Existing parsers keep their current fallback behavior, but a syntactically
recognized unprovable Color expression must not be reinterpreted as a literal
color or unrelated static value.

## Testing

Tests are anchored to the pinned upstream crate behavior.

1. A method matrix locks all 20 methods, constructors, alpha handling, string
   conversion, equality, and representative exact/epsilon results.
2. Resolver tests cover module, namespace, static-key, constructor, and color
   variable aliases plus multi-target assignments and chained methods.
3. Negative tests cover wrong arity, dynamic values, rebinds, escapes,
   expression tails, tuple/scalar mismatches, API mutation, and cycles.
4. End-to-end tests feed transformed colors into direct `config.colors`, named
   custom schemes, mutations, gradients/background layers, window-frame and
   ColorSpec consumers.
5. Numeric, boolean, tuple, and `tostring` tests prove non-color results are not
   merely parsed but can flow through existing supported static contexts.
6. Final gates are focused Color/helper regressions,
   `cargo test -p rssh-app`, `cargo test --workspace`,
   `cargo fmt --all -- --check`, and `git diff --check`.

## Completion boundary

This slice completes the bounded-static API surface of the pinned
`ColorWrap`. It does not claim a general Lua VM, arbitrary userdata behavior,
dynamic method extraction, tuple table expansion, runtime control flow, or
image-derived color analysis.
