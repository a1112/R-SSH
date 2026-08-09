# `ttf-parser` migration checkpoint

RustSec advisory `RUSTSEC-2026-0192` marks `ttf-parser` unmaintained and
recommends `skrifa`. R-SSH depends on `ttf-parser` directly in `rssh-fonts` and
transitively through `fontdb`, `cosmic-text`, `owned_ttf_parser`, and the
vendored `glyphon` stack. Replacing only the direct call sites would therefore
leave the same parser in the executable.

Before 2026-09-30, capture the existing font fixture matrix, variable-font and
color-glyph behavior, malformed-font rejection, fallback coverage ordering,
and raster output tolerances. Then evaluate a coordinated `skrifa` migration
for both first-party and upstream font-stack call sites. The temporary
`cargo-deny` exception is limited to this unmaintained notice; any vulnerability
advisory remains release-blocking.
