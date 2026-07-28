# Deterministic shaping font fixtures

These fonts are deliberately small, repository-owned test inputs for
`rssh-fonts`. Tests must not substitute fonts installed on the host. Task 11
creates only the crate shell and fixture integrity test; Task 12 extends that
crate with the catalog and shaping implementation.

The five planned shaping categories are represented by Latin, CJK, Arabic,
Devanagari, and color emoji subsets. Hebrew is intentionally included because
Task 12 also requires deterministic Hebrew/Arabic bidi tests. Noto Sans
Symbols 2 supplies monochrome U+2708/U+2764 bases for VS15 tests, while the
color fixture supplies the corresponding VS16 forms.

The pinned Noto Sans Symbols 2 source does not map U+263A, so that codepoint is
intentionally covered only by Noto Color Emoji. The monochrome subset contains
the U+2708/U+2764 bases that the VS15 tests need.

All sources are pinned to immutable official GitHub commits. The files here
were generated with Python 3.12 and the fonttools version pinned in
`FONTTOOLS_VERSION`. `MANIFEST.tsv` is the machine-readable coverage contract,
and `SHA256SUMS` authenticates the exact committed subsets. Sequence and GSUB
entries verify that each font contains the ingredients needed by Task 12; real
shaping, bidi, and fallback behavior remains a Task 12 responsibility.

## Source downloads

Download each linked source without changing its destination name under a
temporary `upstream/` directory:

- `NotoSans.ttf`, version `2.015`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosans/NotoSans%5Bwdth,wght%5D.ttf
- `NotoSansSC.ttf`, version `2.004-H2`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanssc/NotoSansSC%5Bwght%5D.ttf
- `NotoSansArabic.ttf`, version `2.012`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosansarabic/NotoSansArabic%5Bwdth,wght%5D.ttf
- `NotoSansDevanagari.ttf`, version `2.006`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosansdevanagari/NotoSansDevanagari%5Bwdth,wght%5D.ttf
- `NotoSansHebrew.ttf`, version `3.001`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanshebrew/NotoSansHebrew%5Bwdth,wght%5D.ttf
- `NotoSansSymbols2.ttf`, version `2.008`: https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanssymbols2/NotoSansSymbols2-Regular.ttf
- `NotoColorEmoji.ttf`, version `2.051`: https://raw.githubusercontent.com/googlefonts/noto-emoji/8998f5dd683424a73e2314a8c1f1e359c19e8742/fonts/NotoColorEmoji.ttf

## Exact upstream licenses

Each subset retains the license from its own upstream directory. The integrity
test fixes the role-to-license mapping, raw URL, and SHA-256; sharing the Latin
license text with other Noto families is forbidden because copyright notices
and reserved font names differ. In particular, the CJK license retains Adobe's
Reserved Font Name `Source`, while the generated subset's name IDs 1, 4, and 6
must not use that reserved name.

- `latin-ligature` → `LICENSES/NotoSans-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosans/OFL.txt; SHA-256 `cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a`
- `cjk` → `LICENSES/NotoSansSC-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanssc/OFL.txt; SHA-256 `1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9`
- `arabic` → `LICENSES/NotoSansArabic-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosansarabic/OFL.txt; SHA-256 `07fc70bfeb985cc1a87a8587d0a0c80bab11c86c9dc3fd95b6f0cb332f983e96`
- `devanagari` → `LICENSES/NotoSansDevanagari-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosansdevanagari/OFL.txt; SHA-256 `a216f6f8d85c7228093e0ee5e258d9d377e6671f68acb4db1930b29583d0f331`
- `hebrew` → `LICENSES/NotoSansHebrew-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanshebrew/OFL.txt; SHA-256 `9b9fe028b5ba74d231659a1bbaf0ed09b11e759d1ca6a070999e16d151616b47`
- `symbols-text` → `LICENSES/NotoSansSymbols2-OFL-1.1.txt`; https://raw.githubusercontent.com/google/fonts/7ff85c87f93ea6cca5f41c69f2e4edcb90240f26/ofl/notosanssymbols2/OFL.txt; SHA-256 `b118dd41337806a5d4797052c77caf3bd096aed783e5eb21b4d11154351e1ac0`
- `color-emoji` → `LICENSES/NotoColorEmoji-OFL-1.1.txt`; https://raw.githubusercontent.com/googlefonts/noto-emoji/8998f5dd683424a73e2314a8c1f1e359c19e8742/fonts/LICENSE; SHA-256 `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2`

## Rebuild commands

Create an environment with the exact version from `FONTTOOLS_VERSION`, then
run the portable rebuild/check driver from the repository root:

```text
python tests/fixtures/fonts/rebuild_check.py --upstream upstream
```

The driver rebuilds into a temporary directory and fails if any SHA differs.
Pass `--write` only when intentionally replacing the committed fixtures.
These are the exact commands consumed from `MANIFEST.tsv`:

```text
pyftsubset upstream/NotoSans.ttf --output-file=tests/fixtures/fonts/NotoSans-Latin.fixture.ttf --unicodes=U+0020-007E,U+FFFD --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoSansSC.ttf --output-file=tests/fixtures/fonts/NotoSansSC-CJK.fixture.ttf --unicodes=U+0020,U+4E2D,U+6587,U+FFFD --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoSansArabic.ttf --output-file=tests/fixtures/fonts/NotoSansArabic.fixture.ttf --unicodes=U+0020,U+0627,U+0633,U+0644,U+0645,U+0651,U+FFFD --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoSansDevanagari.ttf --output-file=tests/fixtures/fonts/NotoSansDevanagari.fixture.ttf --unicodes=U+0020,U+0915,U+0937,U+093F,U+094D,U+FFFD --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoSansHebrew.ttf --output-file=tests/fixtures/fonts/NotoSansHebrew.fixture.ttf --unicodes=U+0020,U+05D0-05D2,U+FFFD --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoSansSymbols2.ttf --output-file=tests/fixtures/fonts/NotoSansSymbols2.fixture.ttf --unicodes=U+2708,U+2764 --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-hinting --no-recalc-timestamp --canonical-order
pyftsubset upstream/NotoColorEmoji.ttf --output-file=tests/fixtures/fonts/NotoColorEmoji.fixture.ttf --unicodes=U+0031,U+200D,U+20E3,U+263A,U+2708,U+2764,U+FE0F,U+1F1F8,U+1F1FA,U+1F3FD,U+1F44D,U+1F466-1F469,U+1F600 --layout-features=* --glyph-names --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline --recommended-glyphs --name-IDs=* --name-legacy --name-languages=* --no-recalc-timestamp --canonical-order
```

After rebuilding, update `SHA256SUMS` only after reviewing the source commit,
embedded version, retained license, cmap/variation coverage, GSUB features, and
color tables. The Rust integrity test enforces those properties and portable
relative paths.
