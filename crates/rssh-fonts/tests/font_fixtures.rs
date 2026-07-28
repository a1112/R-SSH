use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

use sha2::{Digest, Sha256};
use ttf_parser::{Face, Tag, name_id};

const MANIFEST_HEADER: &str = "role\tfile\tlicense\tlicense_file\tcodepoints\tsequences\tgsub_features\tcolor\tsource\tversion\tsubset_command";

#[derive(Debug)]
struct Fixture<'a> {
    role: &'a str,
    file: &'a str,
    license: &'a str,
    license_file: &'a str,
    codepoints: &'a str,
    sequences: &'a str,
    gsub_features: &'a str,
    color: &'a str,
    source: &'a str,
    version: &'a str,
    subset_command: &'a str,
}

#[test]
fn shaping_font_fixtures_are_licensed_pinned_and_cover_the_manifest() {
    let root = fixture_root();
    assert!(
        root.is_dir(),
        "missing fixture directory: {}",
        root.display()
    );

    let readme = read_utf8(&root.join("README.md"));
    let manifest = read_utf8(&root.join("MANIFEST.tsv"));
    let checksums = parse_checksums(&root.join("SHA256SUMS"));
    let fixtures = parse_manifest(&manifest);

    assert!(
        !fixtures.is_empty(),
        "font fixture manifest must contain at least one fixture"
    );
    let fixture_roles = fixtures
        .iter()
        .map(|fixture| fixture.role)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        fixture_roles.len(),
        fixtures.len(),
        "MANIFEST.tsv contains duplicate fixture roles"
    );
    assert_eq!(
        fixture_roles,
        BTreeSet::from([
            "arabic",
            "cjk",
            "color-emoji",
            "devanagari",
            "hebrew",
            "latin-ligature",
            "symbols-text",
        ]),
        "fixture roles must cover every Task 12 shaping scenario"
    );

    let fixture_files = fixture_font_files(&root);
    let manifest_files = fixtures
        .iter()
        .map(|fixture| fixture.file.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        manifest_files.len(),
        fixtures.len(),
        "MANIFEST.tsv contains duplicate fixture paths"
    );
    assert_eq!(
        fixture_files, manifest_files,
        "every font fixture must appear exactly once in MANIFEST.tsv"
    );
    assert_eq!(
        checksums.keys().cloned().collect::<BTreeSet<_>>(),
        manifest_files,
        "SHA256SUMS must list every and only fixture font"
    );

    for fixture in fixtures {
        let bytes = validate_fixture_metadata(&root, &readme, &checksums, &fixture);
        validate_font_coverage(&fixture, &bytes);
    }

    let total_size = manifest_files
        .iter()
        .map(|file| {
            fs::metadata(root.join(file))
                .unwrap_or_else(|error| panic!("failed to stat {file}: {error}"))
                .len()
        })
        .sum::<u64>();
    assert!(
        total_size <= 2 * 1024 * 1024,
        "font fixtures exceed the 2 MiB repository budget: {total_size} bytes"
    );
}

fn validate_fixture_metadata(
    root: &Path,
    readme: &str,
    checksums: &BTreeMap<String, String>,
    fixture: &Fixture<'_>,
) -> Vec<u8> {
    assert_portable_relative_path(fixture.file);
    assert_portable_relative_path(fixture.license_file);
    assert_eq!(
        fixture.license, "OFL-1.1",
        "{} uses a license outside the fixture allow-list",
        fixture.file
    );
    assert!(
        matches!(
            fixture.license_file,
            "LICENSES/Noto-OFL-1.1.txt" | "LICENSES/Noto-Emoji-OFL-1.1.txt"
        ),
        "{} must retain its exact upstream OFL-1.1 text",
        fixture.file
    );

    let normalized_license = read_utf8(&root.join(fixture.license_file)).to_ascii_lowercase();
    assert!(
        normalized_license.contains("sil open font license")
            && normalized_license.contains("version 1.1"),
        "{} does not contain the retained OFL-1.1 text",
        fixture.license_file
    );
    assert!(
        official_immutable_source(fixture.source),
        "{} must have an immutable official raw GitHub source URL",
        fixture.file
    );
    assert!(
        !fixture.version.trim().is_empty(),
        "{} must record its upstream font version",
        fixture.file
    );
    assert!(
        fixture.subset_command.starts_with("pyftsubset ")
            && fixture.subset_command.contains("--no-recalc-timestamp")
            && fixture.subset_command.contains("--canonical-order"),
        "{} must record a reproducible pyftsubset command",
        fixture.file
    );
    assert!(
        readme.contains(fixture.file)
            && readme.contains(fixture.source)
            && readme.contains(fixture.version)
            && readme.contains(fixture.subset_command),
        "README.md must document file, source, version, and subset command for {}",
        fixture.file
    );

    let bytes = fs::read(root.join(fixture.file))
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", fixture.file));
    let maximum_size = if fixture.role == "color-emoji" {
        1_500 * 1024
    } else {
        256 * 1024
    };
    assert!(
        bytes.len() <= maximum_size,
        "{} exceeds its minimal fixture size budget of {maximum_size} bytes",
        fixture.file
    );
    assert_eq!(
        checksums[fixture.file],
        sha256_hex(&bytes),
        "SHA-256 mismatch for {}",
        fixture.file
    );
    bytes
}

fn validate_font_coverage(fixture: &Fixture<'_>, bytes: &[u8]) {
    let face = Face::parse(bytes, 0)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error:?}", fixture.file));
    assert!(
        face.names()
            .into_iter()
            .filter(|name| name.name_id == name_id::VERSION)
            .filter_map(|name| name.to_string())
            .any(|version| version.contains(fixture.version)),
        "{} does not embed documented version {}",
        fixture.file,
        fixture.version
    );
    let expected_color = fixture.color.parse::<bool>().unwrap_or_else(|error| {
        panic!(
            "{} has invalid color flag {:?}: {error}",
            fixture.file, fixture.color
        )
    });
    for codepoint in comma_separated(fixture.codepoints) {
        let (value, character) = parse_codepoint(fixture.file, codepoint);
        assert!(
            face.glyph_index(character).is_some(),
            "{} is missing documented glyph U+{value:04X}",
            fixture.file
        );
    }
    validate_sequences(fixture, &face, expected_color);
    validate_gsub_features(fixture, &face);

    let has_color = face.tables().colr.is_some()
        || face.tables().cbdt.is_some()
        || face.tables().sbix.is_some()
        || face.tables().svg.is_some();
    assert_eq!(
        has_color, expected_color,
        "{} color-table coverage differs from MANIFEST.tsv",
        fixture.file
    );
}

fn validate_sequences(fixture: &Fixture<'_>, face: &Face<'_>, expected_color: bool) {
    for sequence in comma_separated(fixture.sequences) {
        let codepoints = sequence
            .split('+')
            .map(|codepoint| parse_codepoint(fixture.file, codepoint))
            .collect::<Vec<_>>();
        assert!(
            codepoints.len() >= 2,
            "{} has a non-sequence coverage entry {sequence:?}",
            fixture.file
        );
        for (index, (value, character)) in codepoints.iter().copied().enumerate() {
            if matches!(value, 0xFE0E | 0xFE0F) {
                validate_variation_selector(
                    fixture,
                    face,
                    expected_color,
                    sequence,
                    &codepoints,
                    index,
                    character,
                );
            } else {
                assert!(
                    face.glyph_index(character).is_some(),
                    "{} is missing U+{value:04X} from sequence {sequence:?}",
                    fixture.file
                );
            }
        }
    }
}

fn validate_variation_selector(
    fixture: &Fixture<'_>,
    face: &Face<'_>,
    expected_color: bool,
    sequence: &str,
    codepoints: &[(u32, char)],
    index: usize,
    selector: char,
) {
    let (value, _) = codepoints[index];
    let (_, base) = codepoints[index.checked_sub(1).unwrap_or_else(|| {
        panic!(
            "{} sequence {sequence:?} starts with a variation selector",
            fixture.file
        )
    })];
    if value == 0xFE0E {
        assert!(
            !expected_color && face.glyph_index(base).is_some(),
            "{} lacks a text-presentation base in {sequence:?}",
            fixture.file
        );
    } else {
        assert!(
            face.glyph_variation_index(base, selector).is_some(),
            "{} lacks documented variation sequence in {sequence:?}",
            fixture.file
        );
    }
}

fn validate_gsub_features(fixture: &Fixture<'_>, face: &Face<'_>) {
    let features = face.tables().gsub.map(|gsub| {
        gsub.features
            .into_iter()
            .map(|feature| feature.tag)
            .collect::<BTreeSet<_>>()
    });
    for feature in comma_separated(fixture.gsub_features) {
        let tag_bytes: [u8; 4] = feature.as_bytes().try_into().unwrap_or_else(|_| {
            panic!(
                "{} has non-four-byte GSUB feature {feature:?}",
                fixture.file
            )
        });
        assert!(
            features
                .as_ref()
                .is_some_and(|features| features.contains(&Tag::from_bytes(&tag_bytes))),
            "{} is missing documented GSUB feature {feature}",
            fixture.file
        );
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/fixtures/fonts")
}

fn read_utf8(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn parse_manifest(contents: &str) -> Vec<Fixture<'_>> {
    let mut lines = contents.lines();
    assert_eq!(
        lines.next(),
        Some(MANIFEST_HEADER),
        "MANIFEST.tsv header changed without updating its integrity test"
    );
    lines
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            assert_eq!(
                columns.len(),
                11,
                "manifest row must contain exactly eleven tab-separated columns: {line:?}"
            );
            Fixture {
                role: columns[0],
                file: columns[1],
                license: columns[2],
                license_file: columns[3],
                codepoints: columns[4],
                sequences: columns[5],
                gsub_features: columns[6],
                color: columns[7],
                source: columns[8],
                version: columns[9],
                subset_command: columns[10],
            }
        })
        .collect()
}

fn parse_checksums(path: &Path) -> BTreeMap<String, String> {
    let mut checksums = BTreeMap::new();
    for line in read_utf8(path)
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let (checksum, file) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("invalid SHA256SUMS row: {line:?}"));
        assert!(
            checksum.len() == 64
                && checksum
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "invalid lowercase SHA-256 in row: {line:?}"
        );
        assert_portable_relative_path(file);
        assert!(
            checksums
                .insert(file.to_owned(), checksum.to_owned())
                .is_none(),
            "SHA256SUMS contains duplicate fixture path {file:?}"
        );
    }
    checksums
}

fn fixture_font_files(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root)
        .unwrap_or_else(|error| panic!("failed to list {}: {error}", root.display()))
        .map(|entry| entry.expect("fixture directory entry must be readable"))
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?;
            matches!(extension.to_ascii_lowercase().as_str(), "ttf" | "otf").then(|| {
                path.file_name()
                    .expect("font fixture must have a file name")
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect()
}

fn comma_separated(value: &str) -> impl Iterator<Item = &str> {
    value.split(',').filter(|item| !item.is_empty())
}

fn parse_codepoint(file: &str, codepoint: &str) -> (u32, char) {
    let value = u32::from_str_radix(codepoint, 16)
        .unwrap_or_else(|error| panic!("{file} has invalid codepoint {codepoint}: {error}"));
    let character = char::from_u32(value)
        .unwrap_or_else(|| panic!("{file} has non-scalar codepoint {codepoint}"));
    (value, character)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
            output
        })
}

fn assert_portable_relative_path(value: &str) {
    let path = Path::new(value);
    assert!(
        !value.contains('\\')
            && !path.is_absolute()
            && path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "fixture path must be a portable, normalized relative path: {value:?}"
    );
}

fn immutable_github_source(source: &str) -> bool {
    source.split('/').any(|segment| {
        segment.len() == 40
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn official_immutable_source(source: &str) -> bool {
    (source.starts_with("https://raw.githubusercontent.com/google/fonts/")
        || source.starts_with("https://raw.githubusercontent.com/googlefonts/noto-emoji/"))
        && immutable_github_source(source)
}
