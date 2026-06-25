use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::ZlibDecoder;
use rssh_core::{DamageRegion, TerminalSize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    Cell, Color, CursorShape, CursorStyle, InlineImageFormat, ItermInlineImage, ScrollbackLine,
    SemanticCommandExit, SemanticType, SemanticZone, TerminalGrid, UnderlineStyle,
};

pub const DEFAULT_SCROLLBACK_LIMIT: usize = 3_500;
const DEFAULT_UNICODE_VERSION: u32 = 9;
const UNICODE_PRESENTATION_SELECTOR_VERSION: u32 = 14;
const TEXT_PRESENTATION_SELECTOR: char = '\u{fe0e}';
const EMOJI_PRESENTATION_SELECTOR: char = '\u{fe0f}';
const ANONYMOUS_KITTY_IMAGE_ID: u32 = 0;
const MAX_KITTY_RELATIVE_CHAIN_DEPTH: usize = 8;
const KITTY_UNICODE_PLACEHOLDER: char = '\u{10eeee}';
const KITTY_PLACEHOLDER_DIACRITICS: [char; 256] = [
    '\u{0305}', '\u{030d}', '\u{030e}', '\u{0310}', '\u{0312}', '\u{033d}', '\u{033e}', '\u{033f}',
    '\u{0346}', '\u{034a}', '\u{034b}', '\u{034c}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}',
    '\u{035b}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}',
    '\u{036a}', '\u{036b}', '\u{036c}', '\u{036d}', '\u{036e}', '\u{036f}', '\u{0483}', '\u{0484}',
    '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}',
    '\u{0598}', '\u{0599}', '\u{059c}', '\u{059d}', '\u{059e}', '\u{059f}', '\u{05a0}', '\u{05a1}',
    '\u{05a8}', '\u{05a9}', '\u{05ab}', '\u{05ac}', '\u{05af}', '\u{05c4}', '\u{0610}', '\u{0611}',
    '\u{0612}', '\u{0613}', '\u{0614}', '\u{0615}', '\u{0616}', '\u{0617}', '\u{0657}', '\u{0658}',
    '\u{0659}', '\u{065a}', '\u{065b}', '\u{065d}', '\u{065e}', '\u{06d6}', '\u{06d7}', '\u{06d8}',
    '\u{06d9}', '\u{06da}', '\u{06db}', '\u{06dc}', '\u{06df}', '\u{06e0}', '\u{06e1}', '\u{06e2}',
    '\u{06e4}', '\u{06e7}', '\u{06e8}', '\u{06eb}', '\u{06ec}', '\u{0730}', '\u{0732}', '\u{0733}',
    '\u{0735}', '\u{0736}', '\u{073a}', '\u{073d}', '\u{073f}', '\u{0740}', '\u{0741}', '\u{0743}',
    '\u{0745}', '\u{0747}', '\u{0749}', '\u{074a}', '\u{07eb}', '\u{07ec}', '\u{07ed}', '\u{07ee}',
    '\u{07ef}', '\u{07f0}', '\u{07f1}', '\u{07f3}', '\u{0816}', '\u{0817}', '\u{0818}', '\u{0819}',
    '\u{081b}', '\u{081c}', '\u{081d}', '\u{081e}', '\u{081f}', '\u{0820}', '\u{0821}', '\u{0822}',
    '\u{0823}', '\u{0825}', '\u{0826}', '\u{0827}', '\u{0829}', '\u{082a}', '\u{082b}', '\u{082c}',
    '\u{082d}', '\u{0951}', '\u{0953}', '\u{0954}', '\u{0f82}', '\u{0f83}', '\u{0f86}', '\u{0f87}',
    '\u{135d}', '\u{135e}', '\u{135f}', '\u{17dd}', '\u{193a}', '\u{1a17}', '\u{1a75}', '\u{1a76}',
    '\u{1a77}', '\u{1a78}', '\u{1a79}', '\u{1a7a}', '\u{1a7b}', '\u{1a7c}', '\u{1b6b}', '\u{1b6d}',
    '\u{1b6e}', '\u{1b6f}', '\u{1b70}', '\u{1b71}', '\u{1b72}', '\u{1b73}', '\u{1cd0}', '\u{1cd1}',
    '\u{1cd2}', '\u{1cda}', '\u{1cdb}', '\u{1ce0}', '\u{1dc0}', '\u{1dc1}', '\u{1dc3}', '\u{1dc4}',
    '\u{1dc5}', '\u{1dc6}', '\u{1dc7}', '\u{1dc8}', '\u{1dc9}', '\u{1dcb}', '\u{1dcc}', '\u{1dd1}',
    '\u{1dd2}', '\u{1dd3}', '\u{1dd4}', '\u{1dd5}', '\u{1dd6}', '\u{1dd7}', '\u{1dd8}', '\u{1dd9}',
    '\u{1dda}', '\u{1ddb}', '\u{1ddc}', '\u{1ddd}', '\u{1dde}', '\u{1ddf}', '\u{1de0}', '\u{1de1}',
    '\u{1de2}', '\u{1de3}', '\u{1de4}', '\u{1de5}', '\u{1de6}', '\u{1dfe}', '\u{20d0}', '\u{20d1}',
    '\u{20d4}', '\u{20d5}', '\u{20d6}', '\u{20d7}', '\u{20db}', '\u{20dc}', '\u{20e1}', '\u{20e7}',
    '\u{20e9}', '\u{20f0}', '\u{2cef}', '\u{2cf0}', '\u{2cf1}', '\u{2de0}', '\u{2de1}', '\u{2de2}',
    '\u{2de3}', '\u{2de4}', '\u{2de5}', '\u{2de6}', '\u{2de7}', '\u{2de8}', '\u{2de9}', '\u{2dea}',
    '\u{2deb}', '\u{2dec}', '\u{2ded}', '\u{2dee}', '\u{2def}', '\u{2df0}', '\u{2df1}', '\u{2df2}',
    '\u{2df3}', '\u{2df4}', '\u{2df5}', '\u{2df6}', '\u{2df7}', '\u{2df8}', '\u{2df9}', '\u{2dfa}',
    '\u{2dfb}', '\u{2dfc}', '\u{2dfd}', '\u{2dfe}', '\u{2dff}', '\u{a66f}', '\u{a67c}', '\u{a67d}',
    '\u{a6f0}', '\u{a6f1}', '\u{a8e0}', '\u{a8e1}', '\u{a8e2}', '\u{a8e3}', '\u{a8e4}', '\u{a8e5}',
];
type KittyPlacementKey = (u32, u32);
type KittyPlaceholderRenderKey = (usize, u16, u32, Option<u32>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalUnknownEscapeSequence {
    pub sequence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterSet {
    Ascii,
    DecSpecialGraphics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterWriteMode {
    Replace,
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct TerminalModes {
    cursor_visible: bool,
    cursor_blinking: bool,
    cursor_shape: CursorShape,
    screen_reverse: bool,
    auto_wrap: bool,
    reverse_wrap: bool,
    sixel_display_mode: bool,
    sixel_scrolls_right: bool,
    origin_mode: bool,
    left_right_margin_mode: bool,
    write_mode: CharacterWriteMode,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            cursor_blinking: false,
            cursor_shape: CursorShape::Block,
            screen_reverse: false,
            auto_wrap: true,
            reverse_wrap: false,
            sixel_display_mode: false,
            sixel_scrolls_right: false,
            origin_mode: false,
            left_right_margin_mode: false,
            write_mode: CharacterWriteMode::Replace,
        }
    }
}

impl TerminalModes {
    fn with_cursor_style(cursor_style: CursorStyle) -> Self {
        Self {
            cursor_shape: cursor_style.shape(),
            cursor_blinking: cursor_style.blinking(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
struct PendingKittyGraphics {
    image_format: InlineImageFormat,
    medium: KittyTransmissionMedium,
    compression: Option<char>,
    action: KittyUploadAction,
    image_id: Option<u32>,
    image_number: Option<u32>,
    placement_id: Option<u32>,
    z_index: Option<i32>,
    pixel_width: Option<u32>,
    pixel_height: Option<u32>,
    display_columns: Option<u16>,
    display_rows: Option<u16>,
    no_cursor_movement: bool,
    source_x: Option<u32>,
    source_y: Option<u32>,
    source_width: Option<u32>,
    source_height: Option<u32>,
    target_x: Option<u32>,
    target_y: Option<u32>,
    file_offset: Option<u64>,
    file_size: Option<u64>,
    quiet: Option<u8>,
    virtual_placement: bool,
    encoded_data: String,
}

#[derive(Debug, Clone)]
struct StoredKittyImage {
    image_format: InlineImageFormat,
    pixel_width: Option<u32>,
    pixel_height: Option<u32>,
    display_columns: Option<u16>,
    display_rows: Option<u16>,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
struct KittyVirtualPlacement {
    image_id: u32,
    placement_id: Option<u32>,
    z_index: Option<i32>,
    display_columns: Option<u16>,
    display_rows: Option<u16>,
    source_rect: KittySourceRect,
    target_x: Option<u32>,
    target_y: Option<u32>,
}

#[derive(Debug, Clone)]
struct PendingKittyPlaceholder {
    row: usize,
    column: u16,
    foreground: Color,
    underline_color: Color,
    image_id: Option<u32>,
    placement_id: Option<u32>,
    diacritics: Vec<u32>,
    rendered_row: Option<usize>,
    rendered_column: Option<u16>,
    rendered_image_id: Option<u32>,
    rendered_placement_id: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct LastKittyPlaceholder {
    row: usize,
    column: u16,
    foreground: Color,
    underline_color: Color,
    image_id_high_byte: u32,
    placeholder_row: u32,
    placeholder_column: u32,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedKittyPlaceholder {
    image_id: u32,
    image_id_high_byte: u32,
    placeholder_row: u32,
    placeholder_column: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct KittySourceRect {
    x: Option<u32>,
    y: Option<u32>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default)]
struct KittyPlacementOptions {
    display_columns: Option<u16>,
    display_rows: Option<u16>,
    image_id: Option<u32>,
    placement_id: Option<u32>,
    z_index: Option<i32>,
    parent_placement: Option<KittyPlacementKey>,
    row: Option<usize>,
    column: Option<u16>,
    move_cursor: bool,
    source_rect: KittySourceRect,
    target_x: Option<u32>,
    target_y: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KittyTransmissionMedium {
    Direct,
    File,
    TempFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KittyUploadAction {
    Display,
    Store,
    Query,
}

#[derive(Debug, Clone, Copy, Default)]
struct KittyGraphicsParams {
    action: Option<char>,
    format: Option<u32>,
    medium: Option<char>,
    compression: Option<char>,
    image_id: Option<u32>,
    image_number: Option<u32>,
    placement_id: Option<u32>,
    parent_image_id: Option<u32>,
    parent_placement_id: Option<u32>,
    parent_offset_columns: Option<i32>,
    parent_offset_rows: Option<i32>,
    z_index: Option<i32>,
    delete_target: Option<char>,
    cell_x: Option<u16>,
    cell_y: Option<u16>,
    more_chunks: Option<u8>,
    pixel_width: Option<u32>,
    pixel_height: Option<u32>,
    display_columns: Option<u16>,
    display_rows: Option<u16>,
    no_cursor_movement: bool,
    virtual_placement: bool,
    source_x: Option<u32>,
    source_y: Option<u32>,
    source_width: Option<u32>,
    source_height: Option<u32>,
    target_x: Option<u32>,
    target_y: Option<u32>,
    file_offset: Option<u64>,
    file_size: Option<u64>,
    quiet: Option<u8>,
}

impl PendingKittyGraphics {
    const fn source_rect(&self) -> KittySourceRect {
        KittySourceRect {
            x: self.source_x,
            y: self.source_y,
            width: self.source_width,
            height: self.source_height,
        }
    }
}

impl KittyGraphicsParams {
    const fn source_rect(self) -> KittySourceRect {
        KittySourceRect {
            x: self.source_x,
            y: self.source_y,
            width: self.source_width,
            height: self.source_height,
        }
    }
}

#[derive(Debug, Clone)]
struct TabStops {
    columns: Vec<u16>,
}

impl TabStops {
    fn new(size: TerminalSize) -> Self {
        Self {
            columns: default_tab_stops(size),
        }
    }

    fn resize(&mut self, size: TerminalSize) {
        self.columns.retain(|column| *column < size.columns);
    }

    fn set(&mut self, column: u16, size: TerminalSize) {
        if column >= size.columns || self.columns.binary_search(&column).is_ok() {
            return;
        }

        let index = self.columns.partition_point(|stop| *stop < column);
        self.columns.insert(index, column);
    }

    fn clear(&mut self, column: u16) {
        self.columns.retain(|stop| *stop != column);
    }

    fn clear_all(&mut self) {
        self.columns.clear();
    }

    fn next_after(&self, column: u16, fallback: u16) -> u16 {
        self.columns
            .iter()
            .copied()
            .find(|stop| *stop > column)
            .unwrap_or(fallback)
    }

    fn previous_before(&self, column: u16) -> u16 {
        self.columns
            .iter()
            .rev()
            .copied()
            .find(|stop| *stop < column)
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct Terminal {
    grid: TerminalGrid,
    scrollback: Vec<ScrollbackLine>,
    scrollback_limit: usize,
    title: Option<String>,
    icon_title: Option<String>,
    window_title: Option<String>,
    title_stack: Vec<Option<String>>,
    current_working_dir: Option<String>,
    badge_format: Option<String>,
    user_vars: HashMap<String, String>,
    inline_images: Vec<ItermInlineImage>,
    kitty_graphics_responses: Vec<Vec<u8>>,
    pending_kitty_graphics: Option<PendingKittyGraphics>,
    kitty_images: HashMap<u32, StoredKittyImage>,
    kitty_image_numbers: HashMap<u32, u32>,
    kitty_relative_parents: HashMap<KittyPlacementKey, KittyPlacementKey>,
    kitty_virtual_placements: HashMap<KittyPlacementKey, KittyVirtualPlacement>,
    pending_kitty_placeholder: Option<PendingKittyPlaceholder>,
    last_kitty_placeholder: Option<LastKittyPlaceholder>,
    kitty_placeholder_cells: HashMap<(usize, u16), LastKittyPlaceholder>,
    next_kitty_image_id: u32,
    semantic_prompt_rows: Vec<usize>,
    semantic_command_exits: Vec<SemanticCommandExit>,
    cursor_row: u16,
    cursor_column: u16,
    pending_wrap: bool,
    clear_semantic_type_on_movement: bool,
    pending_utf8: Vec<u8>,
    pending_control: Vec<char>,
    last_printable: Option<char>,
    nfc_last_printable_cell: Option<(u16, u16)>,
    saved_cursor: Option<SavedCursor>,
    main_screen: Option<ScreenState>,
    default_cursor_style: CursorStyle,
    modes: TerminalModes,
    scroll_top: u16,
    scroll_bottom: u16,
    left_margin: u16,
    right_margin: u16,
    character_set: CharacterSet,
    tab_stops: TabStops,
    style: Cell,
    damage: Vec<DamageRegion>,
    bell_count: u64,
    unknown_escape_sequences: Vec<TerminalUnknownEscapeSequence>,
    treat_east_asian_ambiguous_width_as_wide: bool,
    cell_width_overrides: Vec<CellWidthOverride>,
    normalize_output_to_unicode_nfc: bool,
    unicode_version: u32,
    unicode_version_stack: Vec<UnicodeVersionStackEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellWidthOverride {
    pub first: u32,
    pub last: u32,
    pub width: u16,
}

impl CellWidthOverride {
    #[must_use]
    pub const fn new(first: u32, last: u32, width: u16) -> Self {
        Self { first, last, width }
    }

    fn contains(self, ch: char) -> bool {
        let codepoint = ch as u32;
        self.first <= codepoint && codepoint <= self.last
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnicodeVersionStackEntry {
    version: u32,
    label: Option<String>,
}

#[derive(Debug, Clone)]
struct ScreenState {
    grid: TerminalGrid,
    inline_images: Vec<ItermInlineImage>,
    last_kitty_placeholder: Option<LastKittyPlaceholder>,
    kitty_placeholder_cells: HashMap<(usize, u16), LastKittyPlaceholder>,
    cursor_row: u16,
    cursor_column: u16,
    pending_wrap: bool,
    clear_semantic_type_on_movement: bool,
    last_printable: Option<char>,
    nfc_last_printable_cell: Option<(u16, u16)>,
    saved_cursor: Option<SavedCursor>,
    modes: TerminalModes,
    scroll_top: u16,
    scroll_bottom: u16,
    left_margin: u16,
    right_margin: u16,
    character_set: CharacterSet,
    style: Cell,
}

#[derive(Debug, Clone)]
struct SavedCursor {
    row: u16,
    column: u16,
    pending_wrap: bool,
    clear_semantic_type_on_movement: bool,
    origin_mode: bool,
    character_set: CharacterSet,
    style: Cell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FeedAdvance {
    Next(usize),
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SequenceParse<T> {
    Complete(T),
    Cancelled(usize),
    Pending,
}

impl Terminal {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self::new_with_default_cursor_style(size, CursorStyle::default())
    }

    #[must_use]
    pub fn new_with_default_cursor_style(
        size: TerminalSize,
        default_cursor_style: CursorStyle,
    ) -> Self {
        Self {
            grid: TerminalGrid::new(size),
            scrollback: Vec::new(),
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
            title: None,
            icon_title: None,
            window_title: None,
            title_stack: Vec::new(),
            current_working_dir: None,
            badge_format: None,
            user_vars: HashMap::new(),
            inline_images: Vec::new(),
            kitty_graphics_responses: Vec::new(),
            pending_kitty_graphics: None,
            kitty_images: HashMap::new(),
            kitty_image_numbers: HashMap::new(),
            kitty_relative_parents: HashMap::new(),
            kitty_virtual_placements: HashMap::new(),
            pending_kitty_placeholder: None,
            last_kitty_placeholder: None,
            kitty_placeholder_cells: HashMap::new(),
            next_kitty_image_id: 1,
            semantic_prompt_rows: Vec::new(),
            semantic_command_exits: Vec::new(),
            cursor_row: 0,
            cursor_column: 0,
            pending_wrap: false,
            clear_semantic_type_on_movement: false,
            pending_utf8: Vec::new(),
            pending_control: Vec::new(),
            last_printable: None,
            nfc_last_printable_cell: None,
            saved_cursor: None,
            main_screen: None,
            default_cursor_style,
            modes: TerminalModes::with_cursor_style(default_cursor_style),
            scroll_top: 0,
            scroll_bottom: size.rows.saturating_sub(1),
            left_margin: 0,
            right_margin: size.columns.saturating_sub(1),
            character_set: CharacterSet::Ascii,
            tab_stops: TabStops::new(size),
            style: Cell::default(),
            damage: Vec::new(),
            bell_count: 0,
            unknown_escape_sequences: Vec::new(),
            treat_east_asian_ambiguous_width_as_wide: false,
            cell_width_overrides: Vec::new(),
            normalize_output_to_unicode_nfc: false,
            unicode_version: DEFAULT_UNICODE_VERSION,
            unicode_version_stack: Vec::new(),
        }
    }

    pub fn set_unicode_version(&mut self, version: u32) {
        self.unicode_version = version;
        self.unicode_version_stack.clear();
    }

    pub fn set_normalize_output_to_unicode_nfc(&mut self, enabled: bool) {
        self.normalize_output_to_unicode_nfc = enabled;
    }

    pub fn set_treat_east_asian_ambiguous_width_as_wide(&mut self, enabled: bool) {
        self.treat_east_asian_ambiguous_width_as_wide = enabled;
    }

    pub fn set_cell_width_overrides(&mut self, overrides: Vec<CellWidthOverride>) {
        self.cell_width_overrides = overrides;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut input = std::mem::take(&mut self.pending_utf8);
        input.extend_from_slice(bytes);
        let complete_utf8_len = complete_utf8_prefix_len(&input);
        self.pending_utf8
            .extend_from_slice(&input[complete_utf8_len..]);

        let mut chars = std::mem::take(&mut self.pending_control);
        chars.extend(decode_terminal_chars(&input[..complete_utf8_len]));
        let mut index = 0;

        while index < chars.len() {
            if let Some(advance) = self.consume_escape_or_c1_sequence(&chars, index) {
                match advance {
                    FeedAdvance::Next(next_index) => index = next_index,
                    FeedAdvance::Pending => break,
                }
            } else {
                index = self.consume_text_run_or_ascii_control(&chars, index);
            }
        }
    }

    fn consume_escape_or_c1_sequence(
        &mut self,
        chars: &[char],
        index: usize,
    ) -> Option<FeedAdvance> {
        if is_escape_or_c1_sequence_start(chars[index]) {
            self.finish_pending_kitty_placeholder();
            self.nfc_last_printable_cell = None;
        }
        match chars[index] {
            '\u{1b}' => self.consume_escape_sequence(chars, index),
            '\u{9b}' => Some(next_or_pending(self.apply_csi_sequence(chars, index, 1))),
            '\u{9c}' => Some(FeedAdvance::Next(index + 1)),
            '\u{90}' => Some(next_or_pending(self.apply_dcs_sequence(chars, index, 1))),
            '\u{9d}' => Some(next_or_pending(self.skip_c1_osc(chars, index))),
            ch if is_c1_st_control_string(ch) => Some(next_or_pending(
                self.skip_c1_st_control_string(chars, index),
            )),
            '\u{84}' => {
                self.index_down(false);
                Some(FeedAdvance::Next(index + 1))
            }
            '\u{85}' => {
                self.next_line();
                Some(FeedAdvance::Next(index + 1))
            }
            '\u{88}' => {
                self.set_horizontal_tab_stop();
                Some(FeedAdvance::Next(index + 1))
            }
            '\u{8d}' => {
                self.reverse_index();
                Some(FeedAdvance::Next(index + 1))
            }
            _ => None,
        }
    }

    fn consume_escape_sequence(&mut self, chars: &[char], index: usize) -> Option<FeedAdvance> {
        match chars.get(index + 1).copied() {
            Some('[') => Some(next_or_pending(self.apply_csi_sequence(chars, index, 2))),
            Some(']') => Some(next_or_pending(self.skip_osc(chars, index))),
            Some('_') => Some(next_or_pending(self.apply_apc_sequence(chars, index, 2))),
            Some('P') => Some(next_or_pending(self.apply_dcs_sequence(chars, index, 2))),
            Some('\\' | '=' | '>') => Some(FeedAdvance::Next(index + 2)),
            Some('X' | '^') => Some(next_or_pending(self.skip_st_control_string(chars, index))),
            Some('(') => Some(self.consume_g0_character_set_selection(chars, index)),
            Some('#') => Some(self.consume_hash_escape_sequence(chars, index)),
            Some('7') => {
                self.save_cursor();
                Some(FeedAdvance::Next(index + 2))
            }
            Some('8') => {
                self.restore_cursor();
                Some(FeedAdvance::Next(index + 2))
            }
            Some('H') => {
                self.set_horizontal_tab_stop();
                Some(FeedAdvance::Next(index + 2))
            }
            Some('c') => {
                self.reset_terminal();
                Some(FeedAdvance::Next(index + 2))
            }
            Some('D') => {
                self.index_down(false);
                Some(FeedAdvance::Next(index + 2))
            }
            Some('E') => {
                self.next_line();
                Some(FeedAdvance::Next(index + 2))
            }
            Some('M') => {
                self.reverse_index();
                Some(FeedAdvance::Next(index + 2))
            }
            None => {
                self.pending_control.extend_from_slice(&chars[index..]);
                Some(FeedAdvance::Pending)
            }
            Some(command) => {
                self.record_unknown_escape_sequence(format!("ESC {command}"));
                Some(FeedAdvance::Next(index + 2))
            }
        }
    }

    fn consume_hash_escape_sequence(&mut self, chars: &[char], index: usize) -> FeedAdvance {
        match chars.get(index + 2).copied() {
            Some('8') => {
                self.screen_alignment_test();
                FeedAdvance::Next(index + 3)
            }
            Some(_) => FeedAdvance::Next(index + 3),
            None => {
                self.pending_control.extend_from_slice(&chars[index..]);
                FeedAdvance::Pending
            }
        }
    }

    fn consume_g0_character_set_selection(&mut self, chars: &[char], index: usize) -> FeedAdvance {
        if let Some(selector) = chars.get(index + 2).copied() {
            if let Some(character_set) = parse_g0_character_set(selector) {
                self.character_set = character_set;
            }

            FeedAdvance::Next(index + 3)
        } else {
            self.pending_control.extend_from_slice(&chars[index..]);
            FeedAdvance::Pending
        }
    }

    fn consume_text_run_or_ascii_control(&mut self, chars: &[char], index: usize) -> usize {
        let ch = chars[index];
        match ch {
            ch if is_ignored_c0_control(ch) => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                index + 1
            }
            '\u{7}' => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                self.bell_count = self.bell_count.saturating_add(1);
                index + 1
            }
            '\u{8}' => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                self.backspace();
                index + 1
            }
            '\t' => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                self.horizontal_tab();
                index + 1
            }
            '\n' | '\u{b}' | '\u{c}' => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                self.line_feed();
                index + 1
            }
            '\r' => {
                self.finish_pending_kitty_placeholder();
                self.nfc_last_printable_cell = None;
                self.carriage_return();
                index + 1
            }
            ch => {
                if !self.normalize_output_to_unicode_nfc {
                    self.write_char(ch);
                    return index + 1;
                }
                let mut end = index + 1;
                while end < chars.len()
                    && !is_escape_or_c1_sequence_start(chars[end])
                    && !is_ascii_control(chars[end])
                {
                    end += 1;
                }
                let text = chars[index..end].iter().collect::<String>();
                if text.contains(KITTY_UNICODE_PLACEHOLDER) {
                    for ch in text.chars() {
                        self.write_char(ch);
                    }
                } else {
                    let remaining_start = self.normalize_leading_combining_marks(&text);
                    for normalized in text[remaining_start..].nfc() {
                        self.write_char(normalized);
                    }
                }
                end
            }
        }
    }

    fn normalize_leading_combining_marks(&mut self, text: &str) -> usize {
        let leading_end = leading_combining_marks_end(text);
        if leading_end == 0 {
            return 0;
        }

        let Some((row, column)) = self.nfc_last_printable_cell else {
            return 0;
        };
        let Some(previous_cell) = self.grid.get(row, column).cloned() else {
            return 0;
        };
        if previous_cell.ch == KITTY_UNICODE_PLACEHOLDER {
            return 0;
        }

        let normalized = std::iter::once(previous_cell.ch)
            .chain(text[..leading_end].chars())
            .collect::<String>()
            .nfc()
            .collect::<String>();
        let mut normalized_chars = normalized.chars();
        let Some(normalized_ch) = normalized_chars.next() else {
            return 0;
        };
        if normalized_chars.next().is_some() || normalized_ch == previous_cell.ch {
            return 0;
        }

        let previous_width = display_width(
            previous_cell.ch,
            self.treat_east_asian_ambiguous_width_as_wide,
            &self.cell_width_overrides,
        );
        let normalized_width = display_width(
            normalized_ch,
            self.treat_east_asian_ambiguous_width_as_wide,
            &self.cell_width_overrides,
        );
        if previous_width == 0 || previous_width != normalized_width {
            return 0;
        }

        let mut normalized_cell = previous_cell;
        normalized_cell.ch = normalized_ch;
        if self.grid.set(row, column, normalized_cell) {
            self.record_damage(DamageRegion::new(column, row, previous_width, 1));
            self.last_printable = Some(normalized_ch);
            return leading_end;
        }

        0
    }

    fn apply_csi_sequence(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
    ) -> Option<usize> {
        let content_start = index + content_offset;
        match parse_csi(chars, content_start) {
            SequenceParse::Complete((command, sequence_end)) => {
                self.apply_csi(command, &chars[content_start..sequence_end]);
                Some(sequence_end + 1)
            }
            SequenceParse::Cancelled(cancel_index) => Some(cancel_index + 1),
            SequenceParse::Pending => {
                self.pending_control.extend_from_slice(&chars[index..]);
                None
            }
        }
    }

    fn skip_osc(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.apply_osc_sequence(chars, index, 2)
    }

    fn skip_c1_osc(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.apply_osc_sequence(chars, index, 1)
    }

    fn skip_st_control_string(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.skip_control_string(chars, index, 2, parse_st_terminated_control_string)
    }

    fn skip_c1_st_control_string(&mut self, chars: &[char], index: usize) -> Option<usize> {
        self.skip_control_string(chars, index, 1, parse_st_terminated_control_string)
    }

    fn apply_apc_sequence(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
    ) -> Option<usize> {
        let content_start = index + content_offset;
        match parse_st_terminated_control_string(chars, content_start) {
            SequenceParse::Complete(sequence_end) => {
                let content_end = st_content_end(chars, content_start, sequence_end);
                self.apply_apc_content(&chars[content_start..content_end]);
                Some(sequence_end + 1)
            }
            SequenceParse::Cancelled(cancel_index) => Some(cancel_index + 1),
            SequenceParse::Pending => {
                self.pending_control.extend_from_slice(&chars[index..]);
                None
            }
        }
    }

    fn apply_dcs_sequence(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
    ) -> Option<usize> {
        let content_start = index + content_offset;
        match parse_st_terminated_control_string(chars, content_start) {
            SequenceParse::Complete(sequence_end) => {
                let content_end = st_content_end(chars, content_start, sequence_end);
                self.apply_dcs_content(&chars[content_start..content_end]);
                Some(sequence_end + 1)
            }
            SequenceParse::Cancelled(cancel_index) => Some(cancel_index + 1),
            SequenceParse::Pending => {
                self.pending_control.extend_from_slice(&chars[index..]);
                None
            }
        }
    }

    fn apply_osc_sequence(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
    ) -> Option<usize> {
        let content_start = index + content_offset;
        match parse_osc(chars, content_start) {
            SequenceParse::Complete(sequence_end) => {
                let content_end = osc_content_end(chars, content_start, sequence_end);
                self.apply_osc_content(&chars[content_start..content_end]);
                Some(sequence_end + 1)
            }
            SequenceParse::Cancelled(cancel_index) => Some(cancel_index + 1),
            SequenceParse::Pending => {
                self.pending_control.extend_from_slice(&chars[index..]);
                None
            }
        }
    }

    fn apply_osc_content(&mut self, content: &[char]) {
        if let Some((&('L' | 'l'), title)) = content.split_first() {
            let title = title.iter().collect::<String>();
            if content.first() == Some(&'L') {
                self.set_icon_title(title);
            } else {
                self.set_window_title(title);
            }
            return;
        }

        let Some(separator) = content.iter().position(|ch| *ch == ';') else {
            return;
        };

        let command = content[..separator].iter().collect::<String>();
        match command.as_str() {
            "0" => self.set_icon_and_window_title(content[separator + 1..].iter().collect()),
            "1" => self.set_icon_title(content[separator + 1..].iter().collect()),
            "2" => self.set_window_title(content[separator + 1..].iter().collect()),
            "7" => self.current_working_dir = Some(content[separator + 1..].iter().collect()),
            "8" => self.apply_osc8_hyperlink(&content[separator + 1..]),
            "133" => self.apply_osc133_semantic_prompt(&content[separator + 1..]),
            "1337" => self.apply_osc1337_iterm_metadata(&content[separator + 1..]),
            _ => {}
        }
    }

    fn set_icon_and_window_title(&mut self, title: String) {
        self.title = Some(title.clone());
        self.icon_title = Some(title.clone());
        self.window_title = Some(title);
    }

    fn set_icon_title(&mut self, title: String) {
        self.title = Some(title.clone());
        self.icon_title = Some(title);
    }

    fn set_window_title(&mut self, title: String) {
        self.title = Some(title.clone());
        self.window_title = Some(title);
    }

    fn apply_apc_content(&mut self, content: &[char]) {
        let content = content.iter().collect::<String>();
        if let Some(graphics) = content.strip_prefix('G') {
            self.apply_kitty_graphics(graphics);
        }
    }

    fn apply_dcs_content(&mut self, content: &[char]) {
        let content = content.iter().collect::<String>();
        if let Some(sixel_start) = sixel_dcs_marker_index(&content) {
            let options = parse_sixel_dcs_options(&content[..sixel_start]);
            self.apply_sixel_content(options, &content[sixel_start + 1..]);
        }
    }

    fn apply_osc1337_iterm_metadata(&mut self, content: &[char]) {
        let content = content.iter().collect::<String>();
        if let Some(current_dir) = content.strip_prefix("CurrentDir=") {
            self.current_working_dir = Some(current_dir.to_owned());
            return;
        }

        if let Some(user_var) = content.strip_prefix("SetUserVar=") {
            self.apply_osc1337_set_user_var(user_var);
            return;
        }

        if let Some(encoded_badge_format) = content.strip_prefix("SetBadgeFormat=") {
            self.apply_osc1337_set_badge_format(encoded_badge_format);
            return;
        }

        if let Some(file) = content.strip_prefix("File=") {
            self.apply_osc1337_file(file);
            return;
        }

        if let Some(unicode_version) = content.strip_prefix("UnicodeVersion=") {
            self.apply_osc1337_unicode_version(unicode_version);
        }
    }

    fn apply_osc1337_unicode_version(&mut self, value: &str) {
        let trimmed = value.trim();
        if let Some(label) = trimmed.strip_prefix("push") {
            self.unicode_version_stack.push(UnicodeVersionStackEntry {
                version: self.unicode_version,
                label: non_empty_unicode_version_label(label),
            });
            return;
        }

        if let Some(label) = trimmed.strip_prefix("pop") {
            match non_empty_unicode_version_label(label) {
                Some(label) => self.pop_labeled_unicode_version(&label),
                None => {
                    if let Some(entry) = self.unicode_version_stack.pop() {
                        self.unicode_version = entry.version;
                    }
                }
            }
            return;
        }

        if let Ok(version) = trimmed.parse::<u32>() {
            self.unicode_version = version;
        }
    }

    fn pop_labeled_unicode_version(&mut self, label: &str) {
        let Some(index) = self
            .unicode_version_stack
            .iter()
            .rposition(|entry| entry.label.as_deref() == Some(label))
        else {
            return;
        };
        let entry = self.unicode_version_stack[index].clone();
        self.unicode_version_stack.truncate(index);
        self.unicode_version = entry.version;
    }

    fn apply_osc1337_set_user_var(&mut self, user_var: &str) {
        let Some((name, encoded_value)) = user_var.split_once('=') else {
            return;
        };
        let Ok(decoded_value) = STANDARD.decode(encoded_value) else {
            return;
        };
        let Ok(value) = String::from_utf8(decoded_value) else {
            return;
        };

        self.user_vars.insert(name.to_owned(), value);
    }

    fn apply_osc1337_set_badge_format(&mut self, encoded_badge_format: &str) {
        let Ok(decoded_value) = STANDARD.decode(encoded_badge_format) else {
            return;
        };
        let Ok(value) = String::from_utf8(decoded_value) else {
            return;
        };

        self.badge_format = Some(value);
    }

    fn apply_osc1337_file(&mut self, file: &str) {
        let Some((params, encoded_data)) = file.split_once(':') else {
            return;
        };

        let mut inline = false;
        let mut name = None;
        let mut size = None;
        let mut width = None;
        let mut height = None;
        let mut preserve_aspect_ratio = None;

        for param in params.split(';').filter(|param| !param.is_empty()) {
            let Some((key, value)) = param.split_once('=') else {
                continue;
            };

            match key {
                "inline" => inline = value == "1",
                "name" => name = decode_base64_utf8(value),
                "size" => size = value.parse().ok(),
                "width" => width = Some(value.to_owned()),
                "height" => height = Some(value.to_owned()),
                "preserveAspectRatio" => {
                    preserve_aspect_ratio = match value {
                        "0" => Some(false),
                        "1" => Some(true),
                        _ => None,
                    };
                }
                _ => {}
            }
        }

        if !inline {
            return;
        }

        let Ok(data) = STANDARD.decode(encoded_data) else {
            return;
        };

        let row = self.current_history_row();
        let column = self.cursor_column;
        self.record_inline_image_damage(width.as_deref(), height.as_deref());
        self.inline_images.push(ItermInlineImage {
            row,
            column,
            name,
            kitty_image_id: None,
            kitty_placement_id: None,
            kitty_z_index: None,
            size,
            width,
            height,
            preserve_aspect_ratio,
            image_format: InlineImageFormat::Encoded,
            pixel_width: None,
            pixel_height: None,
            source_x: None,
            source_y: None,
            source_width: None,
            source_height: None,
            target_x: None,
            target_y: None,
            data,
        });
    }

    fn apply_kitty_graphics(&mut self, graphics: &str) {
        let (control, encoded_data) = graphics.split_once(';').unwrap_or((graphics, ""));
        let params = parse_kitty_graphics_params(control);
        if params.image_id.is_some() && params.image_number.is_some() {
            self.push_kitty_graphics_error_response(
                params,
                "EINVAL",
                "Image id and image number are mutually exclusive",
            );
            return;
        }

        if params.action == Some('q') && params.more_chunks.is_none() {
            self.apply_kitty_graphics_query(params, encoded_data);
            return;
        }
        if params.action == Some('p') {
            self.apply_kitty_graphics_placement(params);
            return;
        }
        if params.action == Some('d') {
            self.apply_kitty_graphics_delete(params);
            return;
        }

        match (self.pending_kitty_graphics.take(), params.more_chunks) {
            (Some(mut pending), Some(0)) => {
                pending.encoded_data.push_str(encoded_data);
                self.finish_kitty_graphics_upload(&pending);
            }
            (Some(mut pending), Some(1)) => {
                pending.encoded_data.push_str(encoded_data);
                self.pending_kitty_graphics = Some(pending);
            }
            (None, Some(1)) => match start_kitty_graphics_upload(params, encoded_data) {
                Ok(pending) => self.pending_kitty_graphics = Some(pending),
                Err(error) => self.push_kitty_graphics_upload_start_error(params, error),
            },
            (None, Some(0) | None) => match start_kitty_graphics_upload(params, encoded_data) {
                Ok(pending) => self.finish_kitty_graphics_upload(&pending),
                Err(error) => self.push_kitty_graphics_upload_start_error(params, error),
            },
            (Some(_), _) | (None, Some(_)) => {}
        }
    }

    fn apply_kitty_graphics_query(&mut self, params: KittyGraphicsParams, encoded_data: &str) {
        if encoded_data.is_empty() && self.apply_kitty_graphics_stored_image_query(params) {
            return;
        }

        let upload = match start_kitty_graphics_upload(params, encoded_data) {
            Ok(upload) => upload,
            Err(error) => {
                self.push_kitty_graphics_upload_start_error(params, error);
                return;
            }
        };

        let data = match load_kitty_graphics_payload(&upload) {
            Ok(data) => data,
            Err(KittyGraphicsDataError::InvalidBase64) => {
                self.push_kitty_graphics_error_response(params, "EINVAL", "Invalid base64 payload");
                return;
            }
            Err(KittyGraphicsDataError::InvalidFile) => {
                self.push_kitty_graphics_error_response(params, "EINVAL", "Invalid file payload");
                return;
            }
            Err(KittyGraphicsDataError::UnsupportedCompression) => {
                self.push_kitty_graphics_error_response(
                    params,
                    "EINVAL",
                    "Unsupported compression",
                );
                return;
            }
        };
        if !kitty_graphics_payload_is_supported(
            upload.image_format,
            upload.pixel_width,
            upload.pixel_height,
            data.len(),
        ) {
            self.push_kitty_graphics_error_response(params, "EINVAL", "Unsupported image data");
            return;
        }

        self.push_kitty_graphics_ok_response(params);
    }

    fn apply_kitty_graphics_stored_image_query(&mut self, params: KittyGraphicsParams) -> bool {
        if params.image_id.is_none() && params.image_number.is_none() {
            return false;
        }

        let Some(image_id) = self.kitty_image_id_from_params(params) else {
            if let Some(image_number) = params.image_number {
                self.push_kitty_graphics_error_response(
                    params,
                    "ENOENT",
                    &format!("No image with number {image_number}"),
                );
            }
            return true;
        };

        let params = Self::kitty_response_params_with_image_id(params, image_id);
        if self.kitty_images.contains_key(&image_id) {
            self.push_kitty_graphics_ok_response(params);
        } else {
            let subject = params.image_number.map_or_else(
                || format!("id {image_id}"),
                |image_number| format!("number {image_number}"),
            );
            self.push_kitty_graphics_error_response(
                params,
                "ENOENT",
                &format!("No image with {subject}"),
            );
        }
        true
    }

    fn finish_kitty_graphics_upload(&mut self, upload: &PendingKittyGraphics) {
        let data = match load_kitty_graphics_payload(upload) {
            Ok(data) => data,
            Err(error) => {
                if Self::kitty_upload_should_respond(upload) {
                    self.push_kitty_graphics_upload_error_response(upload, error);
                }
                return;
            }
        };

        if !kitty_graphics_payload_is_supported(
            upload.image_format,
            upload.pixel_width,
            upload.pixel_height,
            data.len(),
        ) {
            if Self::kitty_upload_should_respond(upload) {
                self.push_kitty_graphics_error_response(
                    Self::kitty_response_params_from_upload(upload),
                    "EINVAL",
                    "Unsupported image data",
                );
            }
            return;
        }

        if upload.action == KittyUploadAction::Query {
            self.push_kitty_graphics_ok_response(Self::kitty_response_params_from_upload(upload));
            return;
        }

        let image = StoredKittyImage {
            image_format: upload.image_format,
            pixel_width: upload.pixel_width,
            pixel_height: upload.pixel_height,
            display_columns: upload.display_columns,
            display_rows: upload.display_rows,
            data,
        };

        let image_id = upload
            .image_id
            .or_else(|| upload.image_number.map(|_| self.next_kitty_image_id()));
        if let Some(image_id) = upload.image_id {
            self.delete_kitty_placements(false, |image| image.kitty_image_id == Some(image_id));
        }
        if let Some(image_id) = image_id {
            self.kitty_images.insert(image_id, image.clone());
            if let Some(image_number) = upload.image_number {
                self.kitty_image_numbers.insert(image_number, image_id);
            }
            if upload.image_id.is_some() || upload.image_number.is_some() {
                self.push_kitty_graphics_upload_ok_response(upload, image_id);
            }
        }

        if upload.action == KittyUploadAction::Display {
            if upload.virtual_placement {
                if let Some(image_id) = image_id {
                    self.store_kitty_virtual_placement(
                        KittyGraphicsParams {
                            image_id: Some(image_id),
                            image_number: upload.image_number,
                            placement_id: upload.placement_id,
                            z_index: upload.z_index,
                            display_columns: upload.display_columns,
                            display_rows: upload.display_rows,
                            source_x: upload.source_x,
                            source_y: upload.source_y,
                            source_width: upload.source_width,
                            source_height: upload.source_height,
                            target_x: upload.target_x,
                            target_y: upload.target_y,
                            quiet: upload.quiet,
                            virtual_placement: true,
                            ..KittyGraphicsParams::default()
                        },
                        image_id,
                        false,
                    );
                }
                return;
            }
            self.place_kitty_image(
                &image,
                KittyPlacementOptions {
                    display_columns: upload.display_columns,
                    display_rows: upload.display_rows,
                    image_id: Some(image_id.unwrap_or(ANONYMOUS_KITTY_IMAGE_ID)),
                    placement_id: kitty_placement_id(image_id, upload.placement_id),
                    z_index: Some(upload.z_index.unwrap_or(0)),
                    parent_placement: None,
                    row: None,
                    column: None,
                    move_cursor: !upload.no_cursor_movement,
                    source_rect: upload.source_rect(),
                    target_x: upload.target_x,
                    target_y: upload.target_y,
                },
            );
        }
    }

    fn apply_kitty_graphics_placement(&mut self, params: KittyGraphicsParams) {
        let Some(image_id) = self.kitty_image_id_from_params(params) else {
            if let Some(image_number) = params.image_number {
                self.push_kitty_graphics_error_response(
                    params,
                    "ENOENT",
                    &format!("No image with number {image_number}"),
                );
            }
            return;
        };
        if params.parent_image_id.is_some() {
            self.apply_relative_kitty_graphics_placement(params, image_id);
            return;
        }
        if params.virtual_placement {
            self.apply_kitty_virtual_placement(params, image_id);
            return;
        }
        let Some(image) = self.kitty_images.get(&image_id).cloned() else {
            let subject = params.image_number.map_or_else(
                || format!("id {image_id}"),
                |image_number| format!("number {image_number}"),
            );
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ENOENT",
                &format!("No image with {subject}"),
            );
            return;
        };

        self.place_kitty_image(
            &image,
            KittyPlacementOptions {
                display_columns: params.display_columns,
                display_rows: params.display_rows,
                image_id: Some(image_id),
                placement_id: params.placement_id,
                z_index: Some(params.z_index.unwrap_or(0)),
                parent_placement: None,
                row: None,
                column: None,
                move_cursor: !params.no_cursor_movement,
                source_rect: params.source_rect(),
                target_x: params.target_x,
                target_y: params.target_y,
            },
        );
        self.push_kitty_graphics_ok_response(Self::kitty_response_params_with_image_id(
            params, image_id,
        ));
    }

    fn apply_kitty_virtual_placement(&mut self, params: KittyGraphicsParams, image_id: u32) {
        if !self.kitty_images.contains_key(&image_id) {
            let subject = params.image_number.map_or_else(
                || format!("id {image_id}"),
                |image_number| format!("number {image_number}"),
            );
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ENOENT",
                &format!("No image with {subject}"),
            );
            return;
        }

        self.store_kitty_virtual_placement(params, image_id, true);
    }

    fn store_kitty_virtual_placement(
        &mut self,
        params: KittyGraphicsParams,
        image_id: u32,
        respond: bool,
    ) {
        let placement = KittyVirtualPlacement {
            image_id,
            placement_id: params.placement_id,
            z_index: Some(params.z_index.unwrap_or(0)),
            display_columns: params.display_columns,
            display_rows: params.display_rows,
            source_rect: params.source_rect(),
            target_x: params.target_x,
            target_y: params.target_y,
        };
        self.kitty_virtual_placements.insert(
            kitty_virtual_placement_key(image_id, params.placement_id),
            placement,
        );
        if respond {
            self.push_kitty_graphics_ok_response(Self::kitty_response_params_with_image_id(
                params, image_id,
            ));
        }
    }

    fn apply_relative_kitty_graphics_placement(
        &mut self,
        params: KittyGraphicsParams,
        image_id: u32,
    ) {
        if params.virtual_placement {
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "EINVAL",
                "Virtual placements cannot be relative",
            );
            return;
        }
        let Some(parent_image_id) = params.parent_image_id else {
            return;
        };
        let Some(parent_placement_id) = params.parent_placement_id else {
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ENOPARENT",
                &format!("No parent placement with id {parent_image_id}"),
            );
            return;
        };
        let Some((parent_row, parent_column)) =
            self.kitty_relative_parent_origin(parent_image_id, parent_placement_id)
        else {
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ENOPARENT",
                &format!("No parent placement with id {parent_image_id},p={parent_placement_id}"),
            );
            return;
        };
        let row = offset_kitty_history_row(parent_row, params.parent_offset_rows.unwrap_or(0));
        let column = offset_kitty_column(parent_column, params.parent_offset_columns.unwrap_or(0));
        let child_placement = kitty_placement_key(Some(image_id), params.placement_id);
        if child_placement.is_some_and(|child_placement| {
            self.kitty_relative_placement_would_cycle(
                child_placement,
                (parent_image_id, parent_placement_id),
            )
        }) {
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ECYCLE",
                "Relative placement cycle",
            );
            return;
        }
        if self.kitty_relative_parent_depth((parent_image_id, parent_placement_id))
            >= MAX_KITTY_RELATIVE_CHAIN_DEPTH
        {
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ETOODEEP",
                "Relative placement chain too deep",
            );
            return;
        }
        let Some(image) = self.kitty_images.get(&image_id).cloned() else {
            let subject = params.image_number.map_or_else(
                || format!("id {image_id}"),
                |image_number| format!("number {image_number}"),
            );
            self.push_kitty_graphics_error_response(
                Self::kitty_response_params_with_image_id(params, image_id),
                "ENOENT",
                &format!("No image with {subject}"),
            );
            return;
        };
        self.place_kitty_image(
            &image,
            KittyPlacementOptions {
                display_columns: params.display_columns,
                display_rows: params.display_rows,
                image_id: Some(image_id),
                placement_id: params.placement_id,
                z_index: Some(params.z_index.unwrap_or(0)),
                parent_placement: Some((parent_image_id, parent_placement_id)),
                row: Some(row),
                column: Some(column),
                move_cursor: false,
                source_rect: params.source_rect(),
                target_x: params.target_x,
                target_y: params.target_y,
            },
        );
        self.push_kitty_graphics_ok_response(Self::kitty_response_params_with_image_id(
            params, image_id,
        ));
    }

    fn kitty_relative_parent_origin(
        &self,
        parent_image_id: u32,
        parent_placement_id: u32,
    ) -> Option<(usize, u16)> {
        if self
            .kitty_virtual_placements
            .contains_key(&(parent_image_id, parent_placement_id))
        {
            return self.kitty_virtual_parent_origin(parent_image_id, parent_placement_id);
        }

        self.inline_images
            .iter()
            .find(|image| {
                image.kitty_image_id == Some(parent_image_id)
                    && image.kitty_placement_id == Some(parent_placement_id)
            })
            .map(|image| (image.row, image.column))
    }

    fn kitty_virtual_parent_origin(
        &self,
        parent_image_id: u32,
        parent_placement_id: u32,
    ) -> Option<(usize, u16)> {
        let mut min_row = None;
        let mut min_column = None;
        for (&(row, column), &placeholder) in &self.kitty_placeholder_cells {
            let Some((origin_row, origin_column, image_id, placement_id)) =
                self.kitty_placeholder_render_key(row, column, placeholder)
            else {
                continue;
            };
            if image_id == parent_image_id && placement_id == Some(parent_placement_id) {
                min_row = Some(min_row.map_or(origin_row, |row: usize| row.min(origin_row)));
                min_column =
                    Some(min_column.map_or(origin_column, |column: u16| column.min(origin_column)));
            }
        }

        Some((min_row?, min_column?))
    }

    fn kitty_relative_parent_depth(&self, parent: KittyPlacementKey) -> usize {
        let mut depth = 0;
        let mut seen = HashSet::new();
        let mut current = Some(parent);
        while let Some(placement) = current {
            if !seen.insert(placement) {
                return depth;
            }
            current = self.kitty_relative_parents.get(&placement).copied();
            if current.is_some() {
                depth += 1;
            }
        }
        depth
    }

    fn kitty_relative_placement_would_cycle(
        &self,
        child: KittyPlacementKey,
        parent: KittyPlacementKey,
    ) -> bool {
        let mut seen = HashSet::new();
        let mut current = Some(parent);
        while let Some(placement) = current {
            if placement == child {
                return true;
            }
            if !seen.insert(placement) {
                return false;
            }
            current = self.kitty_relative_parents.get(&placement).copied();
        }
        false
    }

    fn apply_kitty_graphics_delete(&mut self, params: KittyGraphicsParams) {
        self.pending_kitty_graphics = None;

        match params.delete_target.unwrap_or('a') {
            'a' | 'A' => {
                let (first_row, last_row) = self.visible_history_rows();
                let columns = self.grid.size().columns;
                self.delete_kitty_physical_placements(params.delete_target == Some('A'), |image| {
                    image.kitty_image_id.is_some()
                        && inline_image_intersects_region(image, first_row, last_row, 0, columns)
                });
            }
            'i' | 'I' => {
                let Some(image_id) = params.image_id else {
                    return;
                };
                self.delete_kitty_placements_by_image_id(
                    image_id,
                    params.placement_id,
                    params.delete_target == Some('I'),
                );
            }
            'n' | 'N' => {
                let Some(image_id) = params
                    .image_number
                    .and_then(|image_number| self.kitty_image_numbers.get(&image_number).copied())
                else {
                    return;
                };
                self.delete_kitty_placements_by_image_id(
                    image_id,
                    params.placement_id,
                    params.delete_target == Some('N'),
                );
            }
            'r' | 'R' => {
                let (Some(first_image_id), Some(last_image_id)) =
                    (params.cell_x.map(u32::from), params.cell_y.map(u32::from))
                else {
                    return;
                };
                self.delete_kitty_placements_by_image_id_range(
                    first_image_id,
                    last_image_id,
                    params.delete_target == Some('R'),
                );
            }
            'c' | 'C' => {
                let row = self.current_history_row();
                let column = self.cursor_column;
                self.delete_kitty_physical_placements(params.delete_target == Some('C'), |image| {
                    kitty_image_intersects_cell(image, row, column)
                });
            }
            'p' | 'P' => {
                let Some((row, column)) = self.kitty_delete_cell(params) else {
                    return;
                };
                self.delete_kitty_physical_placements(params.delete_target == Some('P'), |image| {
                    kitty_image_intersects_cell(image, row, column)
                });
            }
            'x' | 'X' => {
                let Some(column) = params.cell_x.and_then(zero_based_axis) else {
                    return;
                };
                let (first_row, last_row) = self.visible_history_rows();
                self.delete_kitty_physical_placements(params.delete_target == Some('X'), |image| {
                    kitty_image_intersects_column(image, first_row, last_row, column)
                });
            }
            'y' | 'Y' => {
                let Some(row) = params.cell_y.and_then(|row| self.visible_history_row(row)) else {
                    return;
                };
                self.delete_kitty_physical_placements(params.delete_target == Some('Y'), |image| {
                    kitty_image_intersects_row(image, row)
                });
            }
            'z' | 'Z' => {
                let Some(z_index) = params.z_index else {
                    return;
                };
                self.delete_kitty_physical_placements(params.delete_target == Some('Z'), |image| {
                    image.kitty_image_id.is_some() && image.kitty_z_index == Some(z_index)
                });
            }
            'q' | 'Q' => {
                let Some(z_index) = params.z_index else {
                    return;
                };
                let Some((row, column)) = self.kitty_delete_cell(params) else {
                    return;
                };
                self.delete_kitty_physical_placements(params.delete_target == Some('Q'), |image| {
                    image.kitty_z_index == Some(z_index)
                        && kitty_image_intersects_cell(image, row, column)
                });
            }
            _ => (),
        }
    }

    fn delete_kitty_placements_by_image_id(
        &mut self,
        image_id: u32,
        placement_id: Option<u32>,
        remove_unreferenced_data: bool,
    ) {
        self.delete_kitty_virtual_placements_by_image_id(image_id, placement_id);
        if let Some(placement_id) = placement_id {
            self.delete_kitty_placements(false, |image| {
                image.kitty_image_id == Some(image_id)
                    && image.kitty_placement_id == Some(placement_id)
            });
        } else {
            self.delete_kitty_placements(false, |image| image.kitty_image_id == Some(image_id));
        }
        if remove_unreferenced_data {
            self.remove_unreferenced_kitty_images(vec![image_id]);
        }
    }

    fn delete_kitty_virtual_placements_by_image_id(
        &mut self,
        image_id: u32,
        placement_id: Option<u32>,
    ) {
        self.kitty_virtual_placements.retain(|_, placement| {
            placement.image_id != image_id
                || placement_id
                    .is_some_and(|placement_id| placement.placement_id != Some(placement_id))
        });
    }

    fn delete_kitty_placements_by_image_id_range(
        &mut self,
        first_image_id: u32,
        last_image_id: u32,
        remove_unreferenced_data: bool,
    ) {
        if first_image_id > last_image_id {
            return;
        }
        let removed_image_ids = self
            .kitty_images
            .keys()
            .copied()
            .filter(|image_id| *image_id >= first_image_id && *image_id <= last_image_id)
            .collect::<Vec<_>>();
        self.kitty_virtual_placements.retain(|_, placement| {
            placement.image_id < first_image_id || placement.image_id > last_image_id
        });
        self.delete_kitty_placements(false, |image| {
            image
                .kitty_image_id
                .is_some_and(|image_id| image_id >= first_image_id && image_id <= last_image_id)
        });
        if remove_unreferenced_data {
            self.remove_unreferenced_kitty_images(removed_image_ids);
        }
    }

    fn delete_kitty_physical_placements(
        &mut self,
        remove_unreferenced_data: bool,
        mut matches: impl FnMut(&ItermInlineImage) -> bool,
    ) {
        let placeholder_render_keys = self.kitty_placeholder_render_keys();
        self.delete_kitty_placements(remove_unreferenced_data, |image| {
            !kitty_image_matches_placeholder_render(image, &placeholder_render_keys)
                && matches(image)
        });
    }

    fn apply_sixel_content(&mut self, options: SixelOptions, content: &str) {
        let Some(image) = parse_sixel_image(options, content) else {
            return;
        };

        let width = format!("{}px", image.width);
        let height = format!("{}px", image.height);
        let cell_width = inline_image_width_cells(Some(&width));
        let (row, column) = if self.modes.sixel_display_mode {
            (self.visible_history_rows().0, 0)
        } else {
            (self.current_history_row(), self.cursor_column)
        };
        self.record_inline_image_damage_at(row, column, Some(&width), Some(&height));
        self.inline_images.push(ItermInlineImage {
            row,
            column,
            name: None,
            kitty_image_id: None,
            kitty_placement_id: None,
            kitty_z_index: None,
            size: Some(image.data.len()),
            width: Some(width),
            height: Some(height),
            preserve_aspect_ratio: None,
            image_format: InlineImageFormat::Rgba,
            pixel_width: Some(image.width),
            pixel_height: Some(image.height),
            source_x: None,
            source_y: None,
            source_width: None,
            source_height: None,
            target_x: None,
            target_y: None,
            data: image.data,
        });
        if !self.modes.sixel_display_mode {
            self.index_down(false);
            if self.modes.sixel_scrolls_right {
                self.move_cursor_forward(cell_width);
            }
        }
    }

    fn push_kitty_graphics_ok_response(&mut self, params: KittyGraphicsParams) {
        self.push_kitty_graphics_response(params, "OK");
    }

    fn push_kitty_graphics_error_response(
        &mut self,
        params: KittyGraphicsParams,
        code: &str,
        message: &str,
    ) {
        self.push_kitty_graphics_response(params, &format!("{code}:{message}"));
    }

    fn push_kitty_graphics_response(&mut self, params: KittyGraphicsParams, status: &str) {
        if kitty_graphics_response_is_suppressed(params, status) {
            return;
        }

        let mut response = b"\x1b_G".to_vec();
        let mut has_param = false;
        if let Some(image_id) = params.image_id {
            append_kitty_graphics_response_param(&mut response, &mut has_param, b'i', image_id);
        }
        if let Some(image_number) = params.image_number {
            append_kitty_graphics_response_param(&mut response, &mut has_param, b'I', image_number);
        }
        if let Some(placement_id) = params.placement_id {
            append_kitty_graphics_response_param(&mut response, &mut has_param, b'p', placement_id);
        }
        response.push(b';');
        response.extend(
            status
                .bytes()
                .filter(|byte| !matches!(byte, 0x00..=0x1f | 0x7f)),
        );
        response.extend_from_slice(b"\x1b\\");
        self.kitty_graphics_responses.push(response);
    }

    fn delete_kitty_placements(
        &mut self,
        remove_unreferenced_data: bool,
        mut matches: impl FnMut(&ItermInlineImage) -> bool,
    ) {
        let before = self.inline_images.len();
        let mut removed_image_ids = Vec::new();
        let mut relative_removed_image_ids = Vec::new();
        let mut removed_placement_keys = HashSet::new();
        self.inline_images.retain(|image| {
            let remove = matches(image);
            if remove {
                if let Some(image_id) = image.kitty_image_id {
                    removed_image_ids.push(image_id);
                }
                if let Some(placement_key) = kitty_image_placement_key(image) {
                    removed_placement_keys.insert(placement_key);
                }
            }
            !remove
        });
        self.delete_relative_kitty_children(
            &mut removed_image_ids,
            &mut relative_removed_image_ids,
            &mut removed_placement_keys,
        );
        if self.inline_images.len() != before {
            if remove_unreferenced_data {
                self.remove_unreferenced_kitty_images(removed_image_ids);
            }
            self.remove_unreferenced_kitty_images(relative_removed_image_ids);
            let size = self.grid.size();
            self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
        }
        self.delete_orphan_kitty_relative_children();
    }

    fn delete_relative_kitty_children(
        &mut self,
        removed_image_ids: &mut Vec<u32>,
        relative_removed_image_ids: &mut Vec<u32>,
        removed_placement_keys: &mut HashSet<KittyPlacementKey>,
    ) {
        loop {
            let relative_parents = &self.kitty_relative_parents;
            let parent_keys = removed_placement_keys.clone();
            let mut removed_this_pass = Vec::new();
            self.inline_images.retain(|image| {
                let Some(placement_key) = kitty_image_placement_key(image) else {
                    return true;
                };
                let remove = relative_parents
                    .get(&placement_key)
                    .is_some_and(|parent_key| parent_keys.contains(parent_key));
                if remove {
                    removed_this_pass.push(placement_key);
                    if let Some(image_id) = image.kitty_image_id {
                        removed_image_ids.push(image_id);
                        relative_removed_image_ids.push(image_id);
                    }
                }
                !remove
            });
            if removed_this_pass.is_empty() {
                return;
            }
            removed_placement_keys.extend(removed_this_pass);
        }
    }

    fn delete_orphan_kitty_relative_children(&mut self) {
        loop {
            let live_placements = self
                .inline_images
                .iter()
                .filter_map(kitty_image_placement_key)
                .collect::<HashSet<_>>();
            let orphan_keys = self
                .kitty_relative_parents
                .iter()
                .filter_map(|(child, parent)| {
                    (!live_placements.contains(parent) || !live_placements.contains(child))
                        .then_some(*child)
                })
                .collect::<HashSet<_>>();
            if orphan_keys.is_empty() {
                self.kitty_relative_parents.retain(|child, parent| {
                    live_placements.contains(child) && live_placements.contains(parent)
                });
                return;
            }

            let mut removed_image_ids = Vec::new();
            for orphan_key in &orphan_keys {
                self.kitty_relative_parents.remove(orphan_key);
            }
            self.inline_images.retain(|image| {
                let remove = kitty_image_placement_key(image)
                    .is_some_and(|placement_key| orphan_keys.contains(&placement_key));
                if remove {
                    if let Some(image_id) = image.kitty_image_id {
                        removed_image_ids.push(image_id);
                    }
                }
                !remove
            });
            self.remove_unreferenced_kitty_images(removed_image_ids);
        }
    }

    fn remove_unreferenced_kitty_images(&mut self, image_ids: Vec<u32>) {
        for image_id in image_ids {
            if !self
                .inline_images
                .iter()
                .any(|image| image.kitty_image_id == Some(image_id))
                && !self
                    .kitty_virtual_placements
                    .values()
                    .any(|placement| placement.image_id == image_id)
            {
                self.kitty_images.remove(&image_id);
                self.kitty_image_numbers
                    .retain(|_, mapped_image_id| *mapped_image_id != image_id);
            }
        }
    }

    fn kitty_image_id_from_params(&self, params: KittyGraphicsParams) -> Option<u32> {
        params.image_id.or_else(|| {
            params
                .image_number
                .and_then(|image_number| self.kitty_image_numbers.get(&image_number).copied())
        })
    }

    fn kitty_response_params_with_image_id(
        params: KittyGraphicsParams,
        image_id: u32,
    ) -> KittyGraphicsParams {
        KittyGraphicsParams {
            image_id: Some(image_id),
            ..params
        }
    }

    fn push_kitty_graphics_upload_ok_response(
        &mut self,
        upload: &PendingKittyGraphics,
        image_id: u32,
    ) {
        self.push_kitty_graphics_ok_response(KittyGraphicsParams {
            image_id: Some(image_id),
            image_number: upload.image_number,
            placement_id: upload.placement_id,
            quiet: upload.quiet,
            ..KittyGraphicsParams::default()
        });
    }

    fn push_kitty_graphics_upload_error_response(
        &mut self,
        upload: &PendingKittyGraphics,
        error: KittyGraphicsDataError,
    ) {
        match error {
            KittyGraphicsDataError::InvalidBase64 => self.push_kitty_graphics_error_response(
                Self::kitty_response_params_from_upload(upload),
                "EINVAL",
                "Invalid base64 payload",
            ),
            KittyGraphicsDataError::InvalidFile => self.push_kitty_graphics_error_response(
                Self::kitty_response_params_from_upload(upload),
                "EINVAL",
                "Invalid file payload",
            ),
            KittyGraphicsDataError::UnsupportedCompression => self
                .push_kitty_graphics_error_response(
                    Self::kitty_response_params_from_upload(upload),
                    "EINVAL",
                    "Unsupported compression",
                ),
        }
    }

    fn push_kitty_graphics_upload_start_error(
        &mut self,
        params: KittyGraphicsParams,
        error: KittyGraphicsStartError,
    ) {
        if !Self::kitty_params_should_respond(params) {
            return;
        }
        self.push_kitty_graphics_error_response(params, "EINVAL", error.message());
    }

    fn kitty_upload_should_respond(upload: &PendingKittyGraphics) -> bool {
        upload.action == KittyUploadAction::Query
            || Self::kitty_params_should_respond(Self::kitty_response_params_from_upload(upload))
    }

    fn kitty_params_should_respond(params: KittyGraphicsParams) -> bool {
        params.action == Some('q') || params.image_id.is_some() || params.image_number.is_some()
    }

    fn kitty_response_params_from_upload(upload: &PendingKittyGraphics) -> KittyGraphicsParams {
        KittyGraphicsParams {
            image_id: upload.image_id,
            image_number: upload.image_number,
            placement_id: upload.placement_id,
            quiet: upload.quiet,
            ..KittyGraphicsParams::default()
        }
    }

    fn next_kitty_image_id(&mut self) -> u32 {
        let mut image_id = self.next_kitty_image_id.max(1);
        while image_id == ANONYMOUS_KITTY_IMAGE_ID || self.kitty_images.contains_key(&image_id) {
            image_id = image_id.saturating_add(1).max(1);
        }
        self.next_kitty_image_id = image_id.saturating_add(1).max(1);
        image_id
    }

    fn kitty_delete_cell(&self, params: KittyGraphicsParams) -> Option<(usize, u16)> {
        let row = self.visible_history_row(params.cell_y?)?;
        let column = zero_based_axis(params.cell_x?)?;
        Some((row, column))
    }

    fn visible_history_row(&self, one_based_row: u16) -> Option<usize> {
        let row = zero_based_axis(one_based_row)?;
        (row < self.grid.size().rows)
            .then(|| self.scrollback.len().saturating_add(usize::from(row)))
    }

    fn visible_history_rows(&self) -> (usize, usize) {
        let first = self.scrollback.len();
        let last = first.saturating_add(usize::from(self.grid.size().rows));
        (first, last)
    }

    fn place_kitty_image(&mut self, image: &StoredKittyImage, options: KittyPlacementOptions) {
        let (width, height) = kitty_display_dimensions(image, options);
        let placement_columns = inline_image_width_cells(width.as_deref()).max(1);
        let placement_rows = inline_image_height_cells(height.as_deref()).max(1);
        let row = options.row.unwrap_or_else(|| self.current_history_row());
        let column = options.column.unwrap_or(self.cursor_column);

        let placement_key = kitty_placement_key(options.image_id, options.placement_id);
        let previous_origin = placement_key.and_then(|placement_key| {
            self.inline_images
                .iter()
                .find(|image| kitty_image_placement_key(image) == Some(placement_key))
                .map(|image| (image.row, image.column))
        });
        if let Some((image_id, placement_id)) = placement_key {
            let before = self.inline_images.len();
            self.inline_images.retain(|image| {
                image.kitty_image_id != Some(image_id)
                    || image.kitty_placement_id != Some(placement_id)
            });
            if self.inline_images.len() != before {
                let size = self.grid.size();
                self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
            }
            self.kitty_relative_parents
                .remove(&(image_id, placement_id));
        }

        self.record_inline_image_damage_at(row, column, width.as_deref(), height.as_deref());
        self.inline_images.push(ItermInlineImage {
            row,
            column,
            name: None,
            kitty_image_id: options.image_id,
            kitty_placement_id: options.placement_id,
            kitty_z_index: options.z_index,
            size: Some(image.data.len()),
            width,
            height,
            preserve_aspect_ratio: None,
            image_format: image.image_format,
            pixel_width: image.pixel_width,
            pixel_height: image.pixel_height,
            source_x: options.source_rect.x,
            source_y: options.source_rect.y,
            source_width: options.source_rect.width,
            source_height: options.source_rect.height,
            target_x: options.target_x,
            target_y: options.target_y,
            data: image.data.clone(),
        });
        if let Some(placement_key) = placement_key {
            if let Some(parent_placement) = options.parent_placement {
                self.kitty_relative_parents
                    .insert(placement_key, parent_placement);
            }
            if let Some((old_row, old_column)) = previous_origin {
                self.move_relative_kitty_descendants(
                    placement_key,
                    old_row,
                    old_column,
                    row,
                    column,
                );
            }
        }

        if options.move_cursor {
            self.move_kitty_cursor_after_placement(placement_columns, placement_rows);
        }
    }

    fn move_relative_kitty_descendants(
        &mut self,
        parent_key: KittyPlacementKey,
        old_row: usize,
        old_column: u16,
        new_row: usize,
        new_column: u16,
    ) {
        if old_row == new_row && old_column == new_column {
            return;
        }

        let mut stack = vec![parent_key];
        let mut seen = HashSet::new();
        while let Some(parent_key) = stack.pop() {
            if !seen.insert(parent_key) {
                continue;
            }
            let child_keys = self
                .kitty_relative_parents
                .iter()
                .filter_map(|(child_key, mapped_parent)| {
                    (*mapped_parent == parent_key).then_some(*child_key)
                })
                .collect::<Vec<_>>();
            for child_key in child_keys {
                if let Some(image) = self.inline_images.iter_mut().find(|image| {
                    image.kitty_image_id == Some(child_key.0)
                        && image.kitty_placement_id == Some(child_key.1)
                }) {
                    image.row = move_kitty_history_row(image.row, old_row, new_row);
                    image.column = move_kitty_column(image.column, old_column, new_column);
                }
                stack.push(child_key);
            }
        }
    }

    fn move_kitty_cursor_after_placement(&mut self, columns: u16, rows: u16) {
        self.move_cursor_forward(columns);
        self.move_cursor_down(rows);
    }

    fn record_inline_image_damage(&mut self, width: Option<&str>, height: Option<&str>) {
        self.record_inline_image_damage_at(
            self.current_history_row(),
            self.cursor_column,
            width,
            height,
        );
    }

    fn record_inline_image_damage_at(
        &mut self,
        row: usize,
        column: u16,
        width: Option<&str>,
        height: Option<&str>,
    ) {
        let size = self.grid.size();
        let Some(visible_row) = row
            .checked_sub(self.scrollback.len())
            .and_then(|row| u16::try_from(row).ok())
        else {
            return;
        };
        if visible_row >= size.rows || column >= size.columns {
            return;
        }

        let width = inline_image_width_cells(width)
            .min(size.columns.saturating_sub(column))
            .max(1);
        let height = inline_image_height_cells(height)
            .min(size.rows.saturating_sub(visible_row))
            .max(1);
        self.record_damage(DamageRegion::new(column, visible_row, width, height));
    }

    fn apply_osc133_semantic_prompt(&mut self, content: &[char]) {
        let content = content.iter().collect::<String>();
        let (marker, params) = content.split_once(';').unwrap_or((&content, ""));

        match marker {
            "A" | "N" => {
                self.fresh_line();
                self.style.semantic_type = SemanticType::Prompt;
                self.clear_semantic_type_on_movement = false;
                self.record_semantic_prompt_row();
            }
            "P" => {
                self.style.semantic_type = SemanticType::Prompt;
                self.clear_semantic_type_on_movement = false;
                self.record_semantic_prompt_row();
            }
            "B" => {
                self.style.semantic_type = SemanticType::Input;
                self.clear_semantic_type_on_movement = false;
            }
            "I" => {
                self.style.semantic_type = SemanticType::Input;
                self.clear_semantic_type_on_movement = true;
            }
            "C" => {
                self.style.semantic_type = SemanticType::Output;
                self.clear_semantic_type_on_movement = false;
            }
            "D" => self.record_semantic_command_exit(params),
            _ => {}
        }
    }

    fn fresh_line(&mut self) {
        if self.cursor_column != 0 {
            self.newline();
        }
    }

    fn clear_semantic_type_due_to_movement(&mut self) {
        if self.clear_semantic_type_on_movement {
            self.clear_semantic_type_on_movement = false;
            self.style.semantic_type = SemanticType::default();
        }
    }

    fn record_semantic_prompt_row(&mut self) {
        let row = self.current_history_row();
        if self.semantic_prompt_rows.last().copied() == Some(row) {
            return;
        }

        match self.semantic_prompt_rows.binary_search(&row) {
            Ok(_) => {}
            Err(index) => self.semantic_prompt_rows.insert(index, row),
        }
    }

    fn record_semantic_command_exit(&mut self, params: &str) {
        let mut exit_code = None;
        let mut aid = None;

        for (index, part) in params
            .split(';')
            .filter(|part| !part.is_empty())
            .enumerate()
        {
            if let Some(value) = part.strip_prefix("aid=") {
                aid = Some(value.to_owned());
            } else if index == 0 {
                exit_code = part.parse::<i32>().ok();
            }
        }

        self.semantic_command_exits.push(SemanticCommandExit {
            row: self.current_history_row(),
            exit_code,
            aid,
        });
    }

    fn current_history_row(&self) -> usize {
        self.scrollback
            .len()
            .saturating_add(usize::from(self.cursor_row))
    }

    fn apply_osc8_hyperlink(&mut self, content: &[char]) {
        let Some(separator) = content.iter().position(|ch| *ch == ';') else {
            return;
        };

        let uri = content[separator + 1..].iter().collect::<String>();
        if uri.is_empty() {
            self.style.hyperlink = None;
        } else {
            self.style.hyperlink = Some(uri);
        }
    }

    fn skip_control_string(
        &mut self,
        chars: &[char],
        index: usize,
        content_offset: usize,
        parse: fn(&[char], usize) -> SequenceParse<usize>,
    ) -> Option<usize> {
        match parse(chars, index + content_offset) {
            SequenceParse::Complete(sequence_end) => Some(sequence_end + 1),
            SequenceParse::Cancelled(cancel_index) => Some(cancel_index + 1),
            SequenceParse::Pending => {
                self.pending_control.extend_from_slice(&chars[index..]);
                None
            }
        }
    }

    #[must_use]
    pub const fn grid(&self) -> &TerminalGrid {
        &self.grid
    }

    #[must_use]
    pub fn scrollback(&self) -> &[ScrollbackLine] {
        &self.scrollback
    }

    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.scrollback_limit = limit;
        self.trim_scrollback_to_limit();
    }

    #[must_use]
    pub fn semantic_prompt_rows(&self) -> &[usize] {
        &self.semantic_prompt_rows
    }

    #[must_use]
    pub fn semantic_command_exits(&self) -> &[SemanticCommandExit] {
        &self.semantic_command_exits
    }

    #[must_use]
    pub fn semantic_zones(&self) -> Vec<SemanticZone> {
        let mut zones = Vec::new();
        let mut current_zone = None;

        for (row, line) in self.scrollback.iter().enumerate() {
            append_semantic_zones_for_row(row, line.cells(), &mut current_zone, &mut zones);
        }

        let history_len = self.scrollback.len();
        for row in 0..self.grid.size().rows {
            let cells = (0..self.grid.size().columns)
                .map(|column| self.grid.get(row, column).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            append_semantic_zones_for_row(
                history_len + usize::from(row),
                &cells,
                &mut current_zone,
                &mut zones,
            );
        }

        if let Some(zone) = current_zone {
            zones.push(zone);
        }

        zones
    }

    #[must_use]
    pub fn semantic_zone_at(&self, x: usize, y: usize) -> Option<SemanticZone> {
        self.semantic_zones()
            .into_iter()
            .find(|zone| zone.contains(x, y))
    }

    #[must_use]
    pub fn text_from_semantic_zone(&self, zone: SemanticZone) -> Option<String> {
        self.text_from_region(zone.start_x, zone.start_y, zone.end_x, zone.end_y)
    }

    #[must_use]
    pub fn text_from_region(
        &self,
        start_x: usize,
        start_y: usize,
        end_x: usize,
        end_y: usize,
    ) -> Option<String> {
        if start_y > end_y || (start_y == end_y && start_x > end_x) {
            return None;
        }

        let mut lines = Vec::new();
        let mut logical_line = String::new();
        for row in start_y..=end_y {
            let cells = self.cells_for_history_row(row)?;
            if cells.is_empty() {
                if row != start_y && !self.history_row_is_wrapped(row)? {
                    trim_trailing_spaces(&mut logical_line);
                    lines.push(std::mem::take(&mut logical_line));
                }
                continue;
            }

            let first_column = if row == start_y { start_x } else { 0 };
            let last_column = if row == end_y {
                end_x.min(cells.len().saturating_sub(1))
            } else {
                cells.len().saturating_sub(1)
            };

            if first_column > last_column || first_column >= cells.len() {
                if row != start_y && !self.history_row_is_wrapped(row)? {
                    trim_trailing_spaces(&mut logical_line);
                    lines.push(std::mem::take(&mut logical_line));
                }
                continue;
            }

            if row != start_y && !self.history_row_is_wrapped(row)? {
                trim_trailing_spaces(&mut logical_line);
                lines.push(std::mem::take(&mut logical_line));
            }

            let line = cells[first_column..=last_column]
                .iter()
                .map(|cell| cell.ch)
                .collect::<String>();
            logical_line.push_str(&line);
        }

        trim_trailing_spaces(&mut logical_line);
        lines.push(logical_line);
        Some(lines.join("\n"))
    }

    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    #[must_use]
    pub fn icon_title(&self) -> Option<&str> {
        self.icon_title.as_deref()
    }

    #[must_use]
    pub fn window_title(&self) -> Option<&str> {
        self.window_title.as_deref()
    }

    #[must_use]
    pub fn current_working_dir(&self) -> Option<&str> {
        self.current_working_dir.as_deref()
    }

    #[must_use]
    pub fn badge_format(&self) -> Option<&str> {
        self.badge_format.as_deref()
    }

    #[must_use]
    pub fn user_vars(&self) -> &HashMap<String, String> {
        &self.user_vars
    }

    #[must_use]
    pub const fn unicode_version(&self) -> u32 {
        self.unicode_version
    }

    #[must_use]
    pub fn inline_images(&self) -> &[ItermInlineImage] {
        &self.inline_images
    }

    pub fn take_kitty_graphics_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.kitty_graphics_responses)
    }

    #[must_use]
    pub const fn cursor(&self) -> (u16, u16) {
        (self.cursor_row, self.cursor_column)
    }

    fn cells_for_history_row(&self, row: usize) -> Option<Vec<Cell>> {
        if let Some(line) = self.scrollback.get(row) {
            return Some(line.cells().to_vec());
        }

        let grid_row = row.checked_sub(self.scrollback.len())?;
        let grid_row = u16::try_from(grid_row).ok()?;
        if grid_row >= self.grid.size().rows {
            return None;
        }

        Some(
            (0..self.grid.size().columns)
                .map(|column| self.grid.get(grid_row, column).cloned().unwrap_or_default())
                .collect(),
        )
    }

    fn history_row_is_wrapped(&self, row: usize) -> Option<bool> {
        if let Some(line) = self.scrollback.get(row) {
            return Some(line.is_wrapped());
        }

        let grid_row = row.checked_sub(self.scrollback.len())?;
        let grid_row = u16::try_from(grid_row).ok()?;
        if grid_row >= self.grid.size().rows {
            return None;
        }

        Some(self.grid.row_wrapped(grid_row))
    }

    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.modes.cursor_visible
    }

    #[must_use]
    pub const fn cursor_blinking(&self) -> bool {
        self.modes.cursor_blinking
    }

    #[must_use]
    pub const fn cursor_shape(&self) -> CursorShape {
        self.modes.cursor_shape
    }

    pub fn set_default_cursor_style(&mut self, default_cursor_style: CursorStyle) {
        self.default_cursor_style = default_cursor_style;
        self.apply_default_cursor_style();
    }

    #[must_use]
    pub const fn screen_reverse_video(&self) -> bool {
        self.modes.screen_reverse
    }

    #[must_use]
    pub const fn alternate_screen_active(&self) -> bool {
        self.main_screen.is_some()
    }

    #[must_use]
    pub const fn active_style(&self) -> &Cell {
        &self.style
    }

    #[must_use]
    pub const fn scroll_region(&self) -> (u16, u16) {
        (self.scroll_top, self.scroll_bottom)
    }

    #[must_use]
    pub const fn left_right_margins(&self) -> (u16, u16) {
        (self.left_margin, self.right_margin)
    }

    pub fn take_bell_count(&mut self) -> u64 {
        std::mem::take(&mut self.bell_count)
    }

    pub fn take_unknown_escape_sequences(&mut self) -> Vec<TerminalUnknownEscapeSequence> {
        std::mem::take(&mut self.unknown_escape_sequences)
    }

    fn record_unknown_escape_sequence(&mut self, sequence: String) {
        self.unknown_escape_sequences
            .push(TerminalUnknownEscapeSequence { sequence });
    }

    pub fn erase_scrollback_and_viewport(&mut self) {
        let size = self.grid.size();
        self.scrollback.clear();
        self.title_stack.clear();
        self.inline_images.clear();
        self.pending_kitty_graphics = None;
        self.kitty_images.clear();
        self.kitty_image_numbers.clear();
        self.kitty_relative_parents.clear();
        self.kitty_virtual_placements.clear();
        self.pending_kitty_placeholder = None;
        self.last_kitty_placeholder = None;
        self.kitty_placeholder_cells.clear();
        self.next_kitty_image_id = 1;
        self.semantic_prompt_rows.clear();
        self.semantic_command_exits.clear();
        if size.rows == 0 || size.columns == 0 {
            self.cursor_row = 0;
            self.cursor_column = 0;
            self.pending_wrap = false;
            self.clear_semantic_type_on_movement = false;
            return;
        }

        let cursor_row = self.cursor_row.min(size.rows.saturating_sub(1));
        let cursor_line = (0..size.columns)
            .map(|column| {
                self.grid
                    .get(cursor_row, column)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let blank = self.blank_cell();

        for row in 0..size.rows {
            for column in 0..size.columns {
                self.grid.set(row, column, blank.clone());
            }
            self.grid.set_row_wrapped(row, false);
        }

        for (column, cell) in cursor_line.into_iter().enumerate() {
            let column = u16::try_from(column).unwrap_or(u16::MAX);
            self.grid.set(0, column, cell);
        }

        self.cursor_row = 0;
        self.pending_wrap = false;
        self.clear_semantic_type_on_movement = false;
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.grid.resize(size);
        self.tab_stops.resize(size);

        if let Some(screen) = self.main_screen.as_mut() {
            screen.grid.resize(size);
            clamp_screen_state(screen, size);
        }

        self.clamp_to_size();
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.left_margin = 0;
        self.right_margin = size.columns.saturating_sub(1);
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    pub fn take_damage(&mut self) -> Vec<DamageRegion> {
        std::mem::take(&mut self.damage)
    }

    fn reset_terminal(&mut self) {
        let size = self.grid.size();
        self.grid = TerminalGrid::new(size);
        self.scrollback.clear();
        self.inline_images.clear();
        self.pending_kitty_graphics = None;
        self.kitty_images.clear();
        self.kitty_image_numbers.clear();
        self.kitty_relative_parents.clear();
        self.kitty_virtual_placements.clear();
        self.pending_kitty_placeholder = None;
        self.last_kitty_placeholder = None;
        self.kitty_placeholder_cells.clear();
        self.next_kitty_image_id = 1;
        self.semantic_prompt_rows.clear();
        self.semantic_command_exits.clear();
        self.cursor_row = 0;
        self.cursor_column = 0;
        self.pending_wrap = false;
        self.clear_semantic_type_on_movement = false;
        self.last_printable = None;
        self.nfc_last_printable_cell = None;
        self.saved_cursor = None;
        self.main_screen = None;
        self.modes = TerminalModes::with_cursor_style(self.default_cursor_style);
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.left_margin = 0;
        self.right_margin = size.columns.saturating_sub(1);
        self.character_set = CharacterSet::Ascii;
        self.tab_stops = TabStops::new(size);
        self.style = Cell::default();
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn soft_reset_terminal(&mut self) {
        self.set_alternate_screen(false);
        let size = self.grid.size();
        self.modes.write_mode = CharacterWriteMode::Replace;
        self.modes.origin_mode = false;
        self.modes.auto_wrap = true;
        self.modes.left_right_margin_mode = false;
        self.modes.reverse_wrap = false;
        self.modes.screen_reverse = false;
        self.modes.cursor_visible = true;
        self.style = Cell::default();
        self.clear_kitty_graphics();
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.left_margin = 0;
        self.right_margin = self.grid.size().columns.saturating_sub(1);
        self.character_set = CharacterSet::Ascii;
        self.saved_cursor = None;
        self.pending_wrap = false;
        self.clear_semantic_type_on_movement = false;
        self.last_printable = None;
        self.nfc_last_printable_cell = None;
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn clear_kitty_graphics(&mut self) {
        self.inline_images
            .retain(|image| image.kitty_image_id.is_none());
        self.pending_kitty_graphics = None;
        self.kitty_images.clear();
        self.kitty_image_numbers.clear();
        self.kitty_relative_parents.clear();
        self.kitty_virtual_placements.clear();
        self.pending_kitty_placeholder = None;
        self.last_kitty_placeholder = None;
        self.kitty_placeholder_cells.clear();
    }

    fn screen_alignment_test(&mut self) {
        let size = self.grid.size();
        self.modes.origin_mode = false;
        self.scroll_top = 0;
        self.scroll_bottom = size.rows.saturating_sub(1);
        self.left_margin = 0;
        self.right_margin = size.columns.saturating_sub(1);
        self.cursor_row = 0;
        self.cursor_column = 0;
        self.pending_wrap = false;
        self.clear_semantic_type_on_movement = false;

        if size.rows == 0 || size.columns == 0 {
            return;
        }

        let cell = Cell {
            ch: 'E',
            ..Cell::default()
        };
        for row in 0..size.rows {
            for column in 0..size.columns {
                self.grid.set(row, column, cell.clone());
            }
            self.grid.set_row_wrapped(row, false);
        }

        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn newline(&mut self) {
        self.carriage_return();
        self.index_down(false);
    }

    fn line_feed(&mut self) {
        self.index_down(false);
    }

    fn wrapped_newline(&mut self) {
        self.carriage_return();
        self.index_down(true);
    }

    fn next_line(&mut self) {
        self.carriage_return();
        self.index_down(false);
    }

    fn index_down(&mut self, wrapped: bool) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let scroll_bottom = self.scroll_bottom.min(rows - 1);
        if self.cursor_row == scroll_bottom {
            self.scroll_up_region(self.scroll_top, scroll_bottom);
            self.cursor_row = scroll_bottom;
        } else if self.cursor_row + 1 < rows {
            self.cursor_row += 1;
        }
        self.grid.set_row_wrapped(self.cursor_row, wrapped);
        self.clear_semantic_type_due_to_movement();
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let scroll_top = self.scroll_top.min(rows - 1);
        let scroll_bottom = self.scroll_bottom.min(rows - 1);
        if self.cursor_row == scroll_top {
            self.scroll_down_region(scroll_top, scroll_bottom, 1);
            self.cursor_row = scroll_top;
        } else {
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
        self.clear_semantic_type_due_to_movement();
    }

    fn backspace(&mut self) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if size.columns == 0 || size.rows == 0 {
            return;
        }

        let left_boundary = if self.modes.left_right_margin_mode
            && self.cursor_column >= self.left_margin
            && self.cursor_column <= self.right_margin
        {
            self.left_margin
        } else {
            0
        };
        if self.cursor_column > left_boundary {
            self.cursor_column -= 1;
            return;
        }

        if self.modes.reverse_wrap && self.modes.auto_wrap {
            self.cursor_row = if self.cursor_row == 0 {
                size.rows - 1
            } else {
                self.cursor_row - 1
            };
            self.cursor_column = if self.modes.left_right_margin_mode {
                self.right_margin.min(size.columns - 1)
            } else {
                size.columns - 1
            };
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_column =
            if self.modes.left_right_margin_mode && self.cursor_column >= self.left_margin {
                self.left_margin
            } else {
                0
            };
        self.pending_wrap = false;
    }

    fn horizontal_tab(&mut self) {
        self.move_forward_tabs(1);
    }

    fn set_horizontal_tab_stop(&mut self) {
        self.tab_stops.set(self.cursor_column, self.grid.size());
    }

    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => self.tab_stops.clear(self.cursor_column),
            3 => self.tab_stops.clear_all(),
            _ => {}
        }
    }

    fn move_forward_tabs(&mut self, count: u16) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        let fallback = columns - 1;
        for _ in 0..count {
            let next = self.tab_stops.next_after(self.cursor_column, fallback);
            if next == self.cursor_column {
                break;
            }
            self.cursor_column = next;
        }
    }

    fn move_backward_tabs(&mut self, count: u16) {
        self.pending_wrap = false;
        if self.grid.size().columns == 0 {
            return;
        }

        for _ in 0..count {
            let previous = self.tab_stops.previous_before(self.cursor_column);
            if previous == self.cursor_column {
                break;
            }
            self.cursor_column = previous;
        }
    }

    fn scroll_up_region(&mut self, top: u16, bottom: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        let top = top.min(size.rows - 1);
        let bottom = bottom.min(size.rows - 1);
        if top >= bottom {
            return;
        }

        let records_scrollback = self.should_record_scrollback_for_scroll(top, bottom);
        if records_scrollback {
            self.record_scrollback_line(top);
        } else {
            self.scroll_inline_images_up_region(top, bottom, 1);
            self.scroll_kitty_placeholder_cells_up_region(top, bottom, 1);
        }

        for row in top.saturating_add(1)..=bottom {
            for column in 0..size.columns {
                let cell = self.grid.get(row, column).cloned().unwrap_or_default();
                self.grid.set(row - 1, column, cell);
            }
            self.grid.copy_row_wrapped(row, row - 1);
        }

        for column in 0..size.columns {
            self.grid.set(bottom, column, self.blank_cell());
        }
        self.grid.set_row_wrapped(bottom, false);

        self.record_damage(DamageRegion::new(0, top, size.columns, bottom - top + 1));
    }

    fn should_record_scrollback_for_scroll(&self, top: u16, bottom: u16) -> bool {
        let size = self.grid.size();
        self.main_screen.is_none()
            && top == 0
            && bottom == size.rows.saturating_sub(1)
            && size.columns > 0
    }

    fn record_scrollback_line(&mut self, row: u16) {
        let size = self.grid.size();
        let cells = (0..size.columns)
            .map(|column| self.grid.get(row, column).cloned().unwrap_or_default())
            .collect();
        let wrapped = self.grid.row_wrapped(row);

        self.scrollback
            .push(ScrollbackLine::from_cells_wrapped(cells, wrapped));
        self.trim_scrollback_to_limit();
    }

    fn trim_scrollback_to_limit(&mut self) {
        if self.scrollback.len() > self.scrollback_limit {
            let overflow = self.scrollback.len() - self.scrollback_limit;
            self.scrollback.drain(..overflow);
            self.semantic_prompt_rows = self
                .semantic_prompt_rows
                .iter()
                .filter_map(|row| row.checked_sub(overflow))
                .collect();
            self.semantic_command_exits = self
                .semantic_command_exits
                .iter()
                .filter_map(|command| {
                    command
                        .row
                        .checked_sub(overflow)
                        .map(|row| SemanticCommandExit {
                            row,
                            exit_code: command.exit_code,
                            aid: command.aid.clone(),
                        })
                })
                .collect();
            self.rebase_inline_images_after_history_prune(overflow);
        }
    }

    fn write_char(&mut self, ch: char) {
        let ch = self.map_graphic_character(ch);
        if self.apply_unicode_presentation_selector(ch) {
            return;
        }
        if self.apply_kitty_placeholder_diacritic(ch) {
            return;
        }
        self.finish_pending_kitty_placeholder();
        let width = display_width(
            ch,
            self.treat_east_asian_ambiguous_width_as_wide,
            &self.cell_width_overrides,
        );
        if width == 0 {
            return;
        }

        if self.pending_wrap && self.modes.auto_wrap {
            self.wrapped_newline();
        } else if self.pending_wrap {
            self.pending_wrap = false;
        }

        if self.cursor_column.saturating_add(width) > self.grid.size().columns
            && self.modes.auto_wrap
        {
            self.wrapped_newline();
        }

        if self.cursor_row >= self.grid.size().rows
            || self.cursor_column >= self.grid.size().columns
            || self.grid.size().columns == 0
        {
            return;
        }

        let available_width = self.grid.size().columns - self.cursor_column;
        let write_width = width.min(available_width);
        if self.modes.write_mode == CharacterWriteMode::Insert {
            self.insert_blank_characters(write_width);
        }

        let column = self.cursor_column;
        let row = self.cursor_row;
        let history_row = self.scrollback.len().saturating_add(usize::from(row));
        let mut cell = self.style.clone();
        cell.ch = ch;

        if self.grid.set(row, column, cell) {
            self.clear_kitty_placeholder_cells(history_row, column, write_width);
            if write_width > 1 {
                let mut continuation = self.style.clone();
                continuation.ch = ' ';
                for offset in 1..write_width {
                    self.grid.set(row, column + offset, continuation.clone());
                }
            }

            self.record_damage(DamageRegion::new(column, row, write_width, 1));
            self.advance_cursor(write_width);
            self.last_printable = Some(ch);
            self.nfc_last_printable_cell = Some((row, column));
            if ch == KITTY_UNICODE_PLACEHOLDER {
                self.pending_kitty_placeholder = Some(PendingKittyPlaceholder {
                    row: history_row,
                    column,
                    foreground: self.style.foreground,
                    underline_color: self.style.underline_color,
                    image_id: kitty_placeholder_image_id(self.style.foreground),
                    placement_id: kitty_placeholder_placement_id(self.style.underline_color),
                    diacritics: Vec::new(),
                    rendered_row: None,
                    rendered_column: None,
                    rendered_image_id: None,
                    rendered_placement_id: None,
                });
            } else {
                self.last_kitty_placeholder = None;
            }
        }
    }

    fn apply_unicode_presentation_selector(&mut self, selector: char) -> bool {
        if !is_unicode_presentation_selector(selector) {
            return false;
        }

        if self.unicode_version < UNICODE_PRESENTATION_SELECTOR_VERSION {
            return true;
        }

        let Some((row, column)) = self.nfc_last_printable_cell else {
            return true;
        };
        let Some(previous_cell) = self.grid.get(row, column).cloned() else {
            return true;
        };
        if previous_cell.ch == KITTY_UNICODE_PLACEHOLDER {
            return true;
        }

        let previous_width = display_width(
            previous_cell.ch,
            self.treat_east_asian_ambiguous_width_as_wide,
            &self.cell_width_overrides,
        );
        let sequence_width = presentation_sequence_width(
            previous_cell.ch,
            selector,
            self.treat_east_asian_ambiguous_width_as_wide,
            &self.cell_width_overrides,
        );
        if previous_width == 0 || previous_width == sequence_width {
            return true;
        }

        let expected_cursor = column.saturating_add(previous_width);
        if self.cursor_row != row || self.cursor_column != expected_cursor || self.pending_wrap {
            return true;
        }

        let available_width = self.grid.size().columns.saturating_sub(column);
        if sequence_width == 0 || sequence_width > available_width {
            return true;
        }

        if sequence_width > previous_width {
            let mut continuation = previous_cell;
            continuation.ch = ' ';
            for offset in previous_width..sequence_width {
                self.grid.set(row, column + offset, continuation.clone());
            }
        } else {
            for offset in sequence_width..previous_width {
                self.grid.set(row, column + offset, self.blank_cell());
            }
        }

        self.record_damage(DamageRegion::new(
            column,
            row,
            previous_width.max(sequence_width),
            1,
        ));
        self.cursor_column = column;
        self.pending_wrap = false;
        self.advance_cursor(sequence_width);
        true
    }

    fn clear_kitty_placeholder_cells(&mut self, row: usize, column: u16, width: u16) {
        for offset in 0..width {
            let column = column.saturating_add(offset);
            if let Some(placeholder) = self.kitty_placeholder_cells.remove(&(row, column)) {
                if let Some((render_row, render_column, image_id, placement_id)) =
                    self.kitty_placeholder_render_key(row, column, placeholder)
                {
                    self.remove_kitty_placeholder_render(
                        render_row,
                        render_column,
                        image_id,
                        placement_id,
                    );
                }
            }
        }
        if self.last_kitty_placeholder.is_some_and(|placeholder| {
            placeholder.row == row
                && placeholder.column >= column
                && placeholder.column < column.saturating_add(width)
        }) {
            self.last_kitty_placeholder = None;
        }
    }

    fn apply_kitty_placeholder_diacritic(&mut self, ch: char) -> bool {
        let Some(value) = kitty_placeholder_diacritic_value(ch) else {
            return false;
        };
        let Some(mut pending) = self.pending_kitty_placeholder.take() else {
            return false;
        };

        pending.diacritics.push(value);
        self.place_kitty_virtual_placeholder(&mut pending);
        self.pending_kitty_placeholder = Some(pending);
        true
    }

    fn finish_pending_kitty_placeholder(&mut self) {
        if let Some(mut pending) = self.pending_kitty_placeholder.take() {
            self.place_kitty_virtual_placeholder(&mut pending);
        }
    }

    fn place_kitty_virtual_placeholder(&mut self, pending: &mut PendingKittyPlaceholder) {
        let Some(resolved) = self.resolve_kitty_placeholder(pending) else {
            return;
        };
        let Some((origin_row, origin_column)) = kitty_placeholder_origin(
            pending,
            resolved.placeholder_row,
            resolved.placeholder_column,
        ) else {
            self.last_kitty_placeholder = None;
            return;
        };
        let Some(placement) = self
            .kitty_virtual_placement_for_placeholder(resolved.image_id, pending.placement_id)
            .cloned()
        else {
            self.last_kitty_placeholder = None;
            return;
        };
        let Some(image) = self.kitty_images.get(&placement.image_id).cloned() else {
            self.last_kitty_placeholder = None;
            return;
        };

        if let (Some(rendered_row), Some(rendered_column), Some(rendered_image_id)) = (
            pending.rendered_row,
            pending.rendered_column,
            pending.rendered_image_id,
        ) {
            self.remove_kitty_placeholder_render(
                rendered_row,
                rendered_column,
                rendered_image_id,
                pending.rendered_placement_id,
            );
        }
        self.remove_kitty_placeholder_render(
            origin_row,
            origin_column,
            placement.image_id,
            placement.placement_id,
        );

        self.place_kitty_image(
            &image,
            KittyPlacementOptions {
                display_columns: placement.display_columns,
                display_rows: placement.display_rows,
                image_id: Some(placement.image_id),
                placement_id: placement.placement_id,
                z_index: placement.z_index,
                parent_placement: None,
                row: Some(origin_row),
                column: Some(origin_column),
                move_cursor: false,
                source_rect: placement.source_rect,
                target_x: placement.target_x,
                target_y: placement.target_y,
            },
        );
        pending.rendered_row = Some(origin_row);
        pending.rendered_column = Some(origin_column);
        pending.rendered_image_id = Some(placement.image_id);
        pending.rendered_placement_id = placement.placement_id;
        let placeholder = LastKittyPlaceholder {
            row: pending.row,
            column: pending.column,
            foreground: pending.foreground,
            underline_color: pending.underline_color,
            image_id_high_byte: resolved.image_id_high_byte,
            placeholder_row: resolved.placeholder_row,
            placeholder_column: resolved.placeholder_column,
        };
        self.kitty_placeholder_cells
            .insert((pending.row, pending.column), placeholder);
        self.last_kitty_placeholder = Some(placeholder);
    }

    fn kitty_placeholder_render_keys(&self) -> HashSet<KittyPlaceholderRenderKey> {
        self.kitty_placeholder_cells
            .iter()
            .filter_map(|(&(row, column), &placeholder)| {
                self.kitty_placeholder_render_key(row, column, placeholder)
            })
            .collect()
    }

    fn kitty_placeholder_render_key(
        &self,
        row: usize,
        column: u16,
        placeholder: LastKittyPlaceholder,
    ) -> Option<KittyPlaceholderRenderKey> {
        let low_bytes = kitty_placeholder_image_id(placeholder.foreground)? & 0x00ff_ffff;
        let image_id = low_bytes | (placeholder.image_id_high_byte << 24);
        let placement_id = kitty_placeholder_placement_id(placeholder.underline_color);
        let (origin_row, origin_column) = kitty_placeholder_origin_from_cell(
            row,
            column,
            placeholder.placeholder_row,
            placeholder.placeholder_column,
        )?;
        let placement = self.kitty_virtual_placement_for_placeholder(image_id, placement_id)?;
        Some((
            origin_row,
            origin_column,
            placement.image_id,
            placement.placement_id,
        ))
    }

    fn resolve_kitty_placeholder(
        &self,
        pending: &PendingKittyPlaceholder,
    ) -> Option<ResolvedKittyPlaceholder> {
        let low_bytes = pending.image_id? & 0x00ff_ffff;
        let left = self.left_kitty_placeholder_for(pending);
        let (placeholder_row, placeholder_column, image_id_high_byte) =
            match pending.diacritics.as_slice() {
                [] => {
                    let left = left?;
                    (
                        left.placeholder_row,
                        left.placeholder_column.checked_add(1)?,
                        left.image_id_high_byte,
                    )
                }
                [value] => {
                    if let Some(left) = left {
                        if left
                            .placeholder_column
                            .checked_add(1)
                            .is_some_and(|column| column == *value)
                        {
                            (left.placeholder_row, *value, left.image_id_high_byte)
                        } else {
                            (*value, 0, 0)
                        }
                    } else {
                        (*value, 0, 0)
                    }
                }
                [row, column] => {
                    let image_id_high_byte = left
                        .filter(|left| {
                            left.placeholder_row == *row
                                && left
                                    .placeholder_column
                                    .checked_add(1)
                                    .is_some_and(|left_column| left_column == *column)
                        })
                        .map_or(0, |left| left.image_id_high_byte);
                    (*row, *column, image_id_high_byte)
                }
                [row, column, image_id_high_byte, ..] => (*row, *column, *image_id_high_byte),
            };
        Some(ResolvedKittyPlaceholder {
            image_id: low_bytes | (image_id_high_byte << 24),
            image_id_high_byte,
            placeholder_row,
            placeholder_column,
        })
    }

    fn left_kitty_placeholder_for(
        &self,
        pending: &PendingKittyPlaceholder,
    ) -> Option<LastKittyPlaceholder> {
        let left_column = pending.column.checked_sub(1)?;
        let left = self
            .kitty_placeholder_cells
            .get(&(pending.row, left_column))
            .copied()
            .or(self.last_kitty_placeholder)?;
        if left.foreground == pending.foreground
            && left.underline_color == pending.underline_color
            && left.row == pending.row
            && left
                .column
                .checked_add(1)
                .is_some_and(|column| column == pending.column)
        {
            Some(left)
        } else {
            None
        }
    }

    fn remove_kitty_placeholder_render(
        &mut self,
        row: usize,
        column: u16,
        image_id: u32,
        placement_id: Option<u32>,
    ) {
        self.inline_images.retain(|image| {
            image.row != row
                || image.column != column
                || image.kitty_image_id != Some(image_id)
                || image.kitty_placement_id != placement_id
        });
    }

    fn kitty_virtual_placement_for_placeholder(
        &self,
        image_id: u32,
        placement_id: Option<u32>,
    ) -> Option<&KittyVirtualPlacement> {
        if let Some(placement_id) = placement_id {
            return self.kitty_virtual_placements.get(&(image_id, placement_id));
        }

        self.kitty_virtual_placements
            .get(&kitty_virtual_placement_key(image_id, None))
            .or_else(|| {
                self.kitty_virtual_placements
                    .values()
                    .find(|placement| placement.image_id == image_id)
            })
    }

    fn advance_cursor(&mut self, width: u16) {
        let next_column = self.cursor_column.saturating_add(width);
        if next_column >= self.grid.size().columns {
            self.cursor_column = self.grid.size().columns.saturating_sub(1);
            self.pending_wrap = self.modes.auto_wrap;
        } else {
            self.cursor_column = next_column;
            self.pending_wrap = false;
        }
    }

    fn apply_csi(&mut self, command: char, params: &[char]) {
        match command {
            '@' => self.insert_blank_characters(csi_count(params)),
            'A' => self.move_cursor_up(csi_count(params)),
            'B' | 'e' => self.move_cursor_down(csi_count(params)),
            'C' | 'a' => self.move_cursor_forward(csi_count(params)),
            'D' => self.move_cursor_back(csi_count(params)),
            'E' => self.move_cursor_next_line(csi_count(params)),
            'F' => self.move_cursor_previous_line(csi_count(params)),
            'G' | '`' => self.position_cursor_column(params),
            'H' | 'f' => self.position_cursor(params),
            'I' => self.move_forward_tabs(csi_count(params)),
            'J' => self.erase_display(csi_or_private_mode(params), csi_is_private(params)),
            'K' => self.erase_line(csi_or_private_mode(params), csi_is_private(params)),
            'L' => self.insert_lines(csi_count(params)),
            'M' => self.delete_lines(csi_count(params)),
            'P' => self.delete_characters(csi_count(params)),
            'S' => self.scroll_up(csi_count(params)),
            'T' => self.scroll_down(csi_count(params)),
            'X' => self.erase_characters(csi_count(params)),
            'Z' => self.move_backward_tabs(csi_count(params)),
            'b' => self.repeat_previous_character(csi_count(params)),
            'd' => self.position_cursor_row(params),
            'g' => self.clear_tab_stop(csi_mode(params)),
            'm' => self.apply_sgr(params),
            'q' if params.ends_with(&['"']) => self.set_character_protection(params),
            'q' => self.set_cursor_shape(params),
            'p' if params == ['!'] => self.soft_reset_terminal(),
            'r' => self.set_scroll_region(params),
            't' => self.apply_window_manipulation(params),
            'h' => self.set_mode(params, true),
            'l' => self.set_mode(params, false),
            's' if self.modes.left_right_margin_mode => self.set_left_right_margins(params),
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            _ => self.record_unknown_escape_sequence(format!(
                "CSI {}{command}",
                params.iter().collect::<String>()
            )),
        }
    }

    fn apply_window_manipulation(&mut self, params: &[char]) {
        let values = parse_csi_params(params);
        let Some(action) = values.first().copied() else {
            return;
        };
        let target = values.get(1).copied().unwrap_or(0);
        if target > 2 {
            return;
        }

        match action {
            22 => self.title_stack.push(self.title.clone()),
            23 => {
                if let Some(title) = self.title_stack.pop() {
                    self.title = title;
                }
            }
            _ => {}
        }
    }

    fn set_mode(&mut self, params: &[char], enabled: bool) {
        if params.first() == Some(&'?') {
            self.set_private_mode(params, enabled);
        } else {
            self.set_standard_mode(params, enabled);
        }
    }

    fn set_standard_mode(&mut self, params: &[char], enabled: bool) {
        for value in parse_csi_params(params) {
            if value == 4 {
                self.set_insert_mode(enabled);
            }
        }
    }

    fn set_private_mode(&mut self, params: &[char], enabled: bool) {
        let Some(values) = parse_private_csi_params(params) else {
            return;
        };

        for value in values {
            match value {
                5 => self.set_screen_reverse_video(enabled),
                6 => self.set_origin_mode(enabled),
                7 => self.set_auto_wrap(enabled),
                12 => self.modes.cursor_blinking = enabled,
                25 => self.modes.cursor_visible = enabled,
                45 => self.modes.reverse_wrap = enabled,
                69 => self.set_left_right_margin_mode(enabled),
                80 => self.modes.sixel_display_mode = enabled,
                8452 => self.modes.sixel_scrolls_right = enabled,
                47 | 1047 | 1049 => self.set_alternate_screen(enabled),
                1048 => {
                    if enabled {
                        self.save_cursor();
                    } else {
                        self.restore_cursor();
                    }
                }
                _ => {}
            }
        }
    }

    fn set_insert_mode(&mut self, enabled: bool) {
        self.modes.write_mode = if enabled {
            CharacterWriteMode::Insert
        } else {
            CharacterWriteMode::Replace
        };
    }

    fn set_screen_reverse_video(&mut self, enabled: bool) {
        if self.modes.screen_reverse == enabled {
            return;
        }
        self.modes.screen_reverse = enabled;
        let size = self.grid.size();
        self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
    }

    fn set_cursor_shape(&mut self, params: &[char]) {
        let Some(params) = params.strip_suffix(&[' ']) else {
            return;
        };
        let value = parse_csi_params(params).first().copied().unwrap_or(0);
        if value == 0 {
            self.apply_default_cursor_style();
            return;
        }
        self.modes.cursor_shape = match value {
            1 | 2 => CursorShape::Block,
            3 | 4 => CursorShape::Underline,
            5 | 6 => CursorShape::Bar,
            _ => self.modes.cursor_shape,
        };
        self.modes.cursor_blinking = match value {
            1 | 3 | 5 => true,
            0 | 2 | 4 | 6 => false,
            _ => self.modes.cursor_blinking,
        };
    }

    fn apply_default_cursor_style(&mut self) {
        self.modes.cursor_shape = self.default_cursor_style.shape();
        self.modes.cursor_blinking = self.default_cursor_style.blinking();
    }

    fn set_character_protection(&mut self, params: &[char]) {
        let Some(params) = params.strip_suffix(&['"']) else {
            return;
        };
        let value = parse_csi_params(params).first().copied().unwrap_or(0);
        self.style.protected = value == 1;
    }

    fn set_origin_mode(&mut self, enabled: bool) {
        self.modes.origin_mode = enabled;
        self.cursor_home();
    }

    fn set_auto_wrap(&mut self, enabled: bool) {
        self.modes.auto_wrap = enabled;
        if !enabled {
            self.pending_wrap = false;
        }
    }

    fn set_alternate_screen(&mut self, enabled: bool) {
        if enabled {
            if self.main_screen.is_some() {
                return;
            }

            let size = self.grid.size();
            self.main_screen = Some(self.screen_state());
            self.grid = TerminalGrid::new(size);
            self.inline_images.clear();
            self.last_kitty_placeholder = None;
            self.kitty_placeholder_cells.clear();
            self.cursor_row = 0;
            self.cursor_column = 0;
            self.pending_wrap = false;
            self.last_printable = None;
            self.nfc_last_printable_cell = None;
            self.saved_cursor = None;
            self.modes.cursor_visible = true;
            self.modes.origin_mode = false;
            self.modes.left_right_margin_mode = false;
            self.scroll_top = 0;
            self.scroll_bottom = size.rows.saturating_sub(1);
            self.left_margin = 0;
            self.right_margin = size.columns.saturating_sub(1);
            self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
        } else if let Some(screen) = self.main_screen.take() {
            self.restore_screen_state(screen);
            self.delete_orphan_kitty_relative_children();
            let size = self.grid.size();
            self.record_damage(DamageRegion::new(0, 0, size.columns, size.rows));
        }
    }

    fn screen_state(&self) -> ScreenState {
        ScreenState {
            grid: self.grid.clone(),
            inline_images: self.inline_images.clone(),
            last_kitty_placeholder: self.last_kitty_placeholder,
            kitty_placeholder_cells: self.kitty_placeholder_cells.clone(),
            cursor_row: self.cursor_row,
            cursor_column: self.cursor_column,
            pending_wrap: self.pending_wrap,
            clear_semantic_type_on_movement: self.clear_semantic_type_on_movement,
            last_printable: self.last_printable,
            nfc_last_printable_cell: self.nfc_last_printable_cell,
            saved_cursor: self.saved_cursor.clone(),
            modes: self.modes,
            scroll_top: self.scroll_top,
            scroll_bottom: self.scroll_bottom,
            left_margin: self.left_margin,
            right_margin: self.right_margin,
            character_set: self.character_set,
            style: self.style.clone(),
        }
    }

    fn restore_screen_state(&mut self, screen: ScreenState) {
        self.grid = screen.grid;
        self.inline_images = screen.inline_images;
        self.last_kitty_placeholder = screen.last_kitty_placeholder;
        self.kitty_placeholder_cells = screen.kitty_placeholder_cells;
        self.cursor_row = screen.cursor_row;
        self.cursor_column = screen.cursor_column;
        self.pending_wrap = screen.pending_wrap;
        self.clear_semantic_type_on_movement = screen.clear_semantic_type_on_movement;
        self.last_printable = screen.last_printable;
        self.nfc_last_printable_cell = screen.nfc_last_printable_cell;
        self.saved_cursor = screen.saved_cursor;
        self.modes = screen.modes;
        self.scroll_top = screen.scroll_top;
        self.scroll_bottom = screen.scroll_bottom;
        self.left_margin = screen.left_margin;
        self.right_margin = screen.right_margin;
        self.character_set = screen.character_set;
        self.style = screen.style;
        self.clamp_to_size();
    }

    fn clamp_to_size(&mut self) {
        let size = self.grid.size();
        self.cursor_row = clamp_axis(self.cursor_row, size.rows);
        self.cursor_column = clamp_axis(self.cursor_column, size.columns);
        self.scroll_top = clamp_axis(self.scroll_top, size.rows);
        self.scroll_bottom = clamp_axis(self.scroll_bottom, size.rows);
        self.left_margin = clamp_axis(self.left_margin, size.columns);
        self.right_margin = clamp_axis(self.right_margin, size.columns);
        if self.scroll_top >= self.scroll_bottom {
            self.scroll_top = 0;
            self.scroll_bottom = size.rows.saturating_sub(1);
        }
        if self.left_margin >= self.right_margin {
            self.left_margin = 0;
            self.right_margin = size.columns.saturating_sub(1);
        }
        if size.columns == 0 || size.rows == 0 {
            self.pending_wrap = false;
        }
    }

    fn cursor_home(&mut self) {
        self.pending_wrap = false;
        self.cursor_column = if self.modes.origin_mode && self.modes.left_right_margin_mode {
            self.left_margin
        } else {
            0
        };
        self.cursor_row = if self.modes.origin_mode {
            self.scroll_top.min(self.grid.size().rows.saturating_sub(1))
        } else {
            0
        };
        self.clear_semantic_type_due_to_movement();
    }

    fn set_left_right_margin_mode(&mut self, enabled: bool) {
        self.modes.left_right_margin_mode = enabled;
        if !enabled {
            self.left_margin = 0;
            self.right_margin = self.grid.size().columns.saturating_sub(1);
        }
    }

    fn set_scroll_region(&mut self, params: &[char]) {
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let values = parse_csi_params(params);
        let top = param_or_one(values.first().copied()).saturating_sub(1);
        let bottom = values
            .get(1)
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(rows)
            .saturating_sub(1)
            .min(rows - 1);

        if top >= bottom {
            return;
        }

        self.scroll_top = top;
        self.scroll_bottom = bottom;
        self.cursor_home();
    }

    fn set_left_right_margins(&mut self, params: &[char]) {
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        let values = parse_csi_params(params);
        let left = param_or_one(values.first().copied()).saturating_sub(1);
        let right = values
            .get(1)
            .copied()
            .filter(|value| *value != 0)
            .unwrap_or(columns)
            .saturating_sub(1)
            .min(columns - 1);

        if left >= right {
            return;
        }

        self.left_margin = left;
        self.right_margin = right;
        self.cursor_home();
    }

    fn save_cursor(&mut self) {
        self.saved_cursor = Some(SavedCursor {
            row: self.cursor_row,
            column: self.cursor_column,
            pending_wrap: self.pending_wrap,
            clear_semantic_type_on_movement: self.clear_semantic_type_on_movement,
            origin_mode: self.modes.origin_mode,
            character_set: self.character_set,
            style: self.style.clone(),
        });
    }

    fn restore_cursor(&mut self) {
        let Some(saved) = self.saved_cursor.clone() else {
            return;
        };

        self.modes.origin_mode = saved.origin_mode;
        self.character_set = saved.character_set;
        self.style = saved.style;

        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            self.cursor_row = 0;
            self.cursor_column = 0;
            self.pending_wrap = false;
            return;
        }

        self.cursor_row = saved.row.min(size.rows - 1);
        self.cursor_column = saved.column.min(size.columns - 1);
        self.pending_wrap = saved.pending_wrap;
        self.clear_semantic_type_on_movement = saved.clear_semantic_type_on_movement;
    }

    fn move_cursor_up(&mut self, count: u16) {
        self.pending_wrap = false;
        let previous_row = self.cursor_row;
        self.cursor_row = self.cursor_row.saturating_sub(count);
        if self.cursor_row != previous_row {
            self.clear_semantic_type_due_to_movement();
        }
    }

    fn move_cursor_down(&mut self, count: u16) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let previous_row = self.cursor_row;
        self.cursor_row = self.cursor_row.saturating_add(count).min(rows - 1);
        if self.cursor_row != previous_row {
            self.clear_semantic_type_due_to_movement();
        }
    }

    fn move_cursor_forward(&mut self, count: u16) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        self.cursor_column = self.cursor_column.saturating_add(count).min(columns - 1);
    }

    fn move_cursor_back(&mut self, count: u16) {
        self.pending_wrap = false;
        self.cursor_column = self.cursor_column.saturating_sub(count);
    }

    fn move_cursor_next_line(&mut self, count: u16) {
        self.move_cursor_down(count);
        self.cursor_column = 0;
    }

    fn move_cursor_previous_line(&mut self, count: u16) {
        self.move_cursor_up(count);
        self.cursor_column = 0;
    }

    fn position_cursor(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let values = parse_csi_params(params);
        let rows = self.grid.size().rows;
        let columns = self.grid.size().columns;

        if rows == 0 || columns == 0 {
            return;
        }

        let row = self.cursor_row_from_position_param(param_or_one(values.first().copied()));
        let column = self.cursor_column_from_position_param(param_or_one(values.get(1).copied()));

        let previous_row = self.cursor_row;
        self.cursor_row = row;
        self.cursor_column = column;
        if self.cursor_row != previous_row {
            self.clear_semantic_type_due_to_movement();
        }
    }

    fn cursor_row_from_position_param(&self, param: u16) -> u16 {
        let row = param.saturating_sub(1);
        let rows = self.grid.size().rows;
        if rows == 0 {
            return 0;
        }

        if self.modes.origin_mode {
            let top = self.scroll_top.min(rows - 1);
            let bottom = self.scroll_bottom.min(rows - 1);
            top.saturating_add(row).min(bottom)
        } else {
            row.min(rows - 1)
        }
    }

    fn cursor_column_from_position_param(&self, param: u16) -> u16 {
        let column = param.saturating_sub(1);
        let columns = self.grid.size().columns;
        if columns == 0 {
            return 0;
        }

        if self.modes.origin_mode && self.modes.left_right_margin_mode {
            self.left_margin
                .saturating_add(column)
                .min(self.right_margin)
        } else {
            column.min(columns - 1)
        }
    }

    fn position_cursor_column(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let columns = self.grid.size().columns;
        if columns == 0 {
            return;
        }

        let column = csi_count(params).saturating_sub(1);
        self.cursor_column = column.min(columns - 1);
    }

    fn position_cursor_row(&mut self, params: &[char]) {
        self.pending_wrap = false;
        let rows = self.grid.size().rows;
        if rows == 0 {
            return;
        }

        let row = csi_count(params).saturating_sub(1);
        let previous_row = self.cursor_row;
        self.cursor_row = row.min(rows - 1);
        if self.cursor_row != previous_row {
            self.clear_semantic_type_due_to_movement();
        }
    }

    fn erase_display(&mut self, mode: u16, selective: bool) {
        if mode == 3 {
            let removed_rows = self.scrollback.len();
            self.scrollback.clear();
            self.semantic_prompt_rows.clear();
            self.semantic_command_exits.clear();
            self.rebase_inline_images_after_history_prune(removed_rows);
            return;
        }

        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return;
        }

        match mode {
            0 => {
                self.clear_cells(self.cursor_row, self.cursor_column, size.columns, selective);
                for row in self.cursor_row.saturating_add(1)..size.rows {
                    self.clear_cells(row, 0, size.columns, selective);
                }
            }
            1 => {
                for row in 0..self.cursor_row {
                    self.clear_cells(row, 0, size.columns, selective);
                }
                self.clear_cells(
                    self.cursor_row,
                    0,
                    self.cursor_column.saturating_add(1).min(size.columns),
                    selective,
                );
            }
            2 => {
                for row in 0..size.rows {
                    self.clear_cells(row, 0, size.columns, selective);
                }
                if !selective {
                    self.delete_visible_inline_images();
                }
            }
            _ => {}
        }
    }

    fn rebase_inline_images_after_history_prune(&mut self, removed_rows: usize) {
        if removed_rows == 0 {
            return;
        }
        self.inline_images = self
            .inline_images
            .iter()
            .filter_map(|image| {
                let mut image = image.clone();
                image.row = image.row.checked_sub(removed_rows)?;
                Some(image)
            })
            .collect();
        self.rebase_kitty_placeholder_cells_after_history_prune(removed_rows);
        self.delete_orphan_kitty_relative_children();
    }

    fn rebase_kitty_placeholder_cells_after_history_prune(&mut self, removed_rows: usize) {
        self.kitty_placeholder_cells = self
            .kitty_placeholder_cells
            .drain()
            .filter_map(|((row, column), mut placeholder)| {
                let row = row.checked_sub(removed_rows)?;
                placeholder.row = row;
                Some(((row, column), placeholder))
            })
            .collect();
        self.last_kitty_placeholder = self.last_kitty_placeholder.and_then(|mut placeholder| {
            placeholder.row = placeholder.row.checked_sub(removed_rows)?;
            Some(placeholder)
        });
    }

    fn delete_visible_inline_images(&mut self) {
        let first_row = self.scrollback.len();
        let last_row = first_row.saturating_add(usize::from(self.grid.size().rows));
        let columns = self.grid.size().columns;
        self.inline_images.retain(|image| {
            !inline_image_intersects_region(image, first_row, last_row, 0, columns)
        });
        self.delete_orphan_kitty_relative_children();
    }

    fn erase_line(&mut self, mode: u16, selective: bool) {
        let columns = self.grid.size().columns;
        if self.cursor_row >= self.grid.size().rows || columns == 0 {
            return;
        }

        match mode {
            0 => self.clear_cells(self.cursor_row, self.cursor_column, columns, selective),
            1 => self.clear_cells(
                self.cursor_row,
                0,
                self.cursor_column.saturating_add(1).min(columns),
                selective,
            ),
            2 => self.clear_cells(self.cursor_row, 0, columns, selective),
            _ => {}
        }
    }

    fn insert_lines(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range_from_cursor() else {
            return;
        };

        self.scroll_down_region(top, bottom, count);
    }

    fn delete_lines(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range_from_cursor() else {
            return;
        };

        self.scroll_up_region_by(top, bottom, count);
    }

    fn scroll_up(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range() else {
            return;
        };

        self.scroll_up_region_by(top, bottom, count);
    }

    fn scroll_down(&mut self, count: u16) {
        self.pending_wrap = false;
        let Some((top, bottom)) = self.active_scroll_range() else {
            return;
        };

        self.scroll_down_region(top, bottom, count);
    }

    fn active_scroll_range(&self) -> Option<(u16, u16)> {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return None;
        }

        let scroll_top = self.scroll_top.min(size.rows - 1);
        let scroll_bottom = self.scroll_bottom.min(size.rows - 1);
        if scroll_top > scroll_bottom {
            return None;
        }

        Some((scroll_top, scroll_bottom))
    }

    fn active_scroll_range_from_cursor(&self) -> Option<(u16, u16)> {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 {
            return None;
        }

        let scroll_top = self.scroll_top.min(size.rows - 1);
        let scroll_bottom = self.scroll_bottom.min(size.rows - 1);
        if self.cursor_row < scroll_top || self.cursor_row > scroll_bottom {
            return None;
        }

        Some((self.cursor_row, scroll_bottom))
    }

    fn scroll_down_region(&mut self, top: u16, bottom: u16, count: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 || top > bottom || count == 0 {
            return;
        }

        let height = bottom - top + 1;
        let count = count.min(height);
        self.scroll_inline_images_down_region(top, bottom, count);
        self.scroll_kitty_placeholder_cells_down_region(top, bottom, count);

        if count < height {
            let shift_bottom = bottom - count;
            for row in (top..=shift_bottom).rev() {
                for column in 0..size.columns {
                    let cell = self.grid.get(row, column).cloned().unwrap_or_default();
                    self.grid.set(row + count, column, cell);
                }
                self.grid.copy_row_wrapped(row, row + count);
            }
        }

        for row in top..top + count {
            for column in 0..size.columns {
                self.grid.set(row, column, self.blank_cell());
            }
            self.grid.set_row_wrapped(row, false);
        }

        self.record_damage(DamageRegion::new(0, top, size.columns, height));
    }

    fn scroll_inline_images_down_region(&mut self, top: u16, bottom: u16, count: u16) {
        if count == 0 || top > bottom {
            return;
        }

        let first_row = self.scrollback.len().saturating_add(usize::from(top));
        let last_row = self
            .scrollback
            .len()
            .saturating_add(usize::from(bottom))
            .saturating_add(1);
        let count = usize::from(count);
        let mut shifted_images = Vec::with_capacity(self.inline_images.len());

        for mut image in self.inline_images.drain(..) {
            let (image_top, image_bottom) = kitty_image_row_range(&image);
            if image_bottom <= first_row || image_top >= last_row {
                shifted_images.push(image);
                continue;
            }

            if image_top < first_row || image_bottom > last_row {
                continue;
            }

            let Some(new_row) = image.row.checked_add(count) else {
                continue;
            };
            let image_height = image_bottom.saturating_sub(image_top);
            if new_row < first_row || new_row.saturating_add(image_height) > last_row {
                continue;
            }

            image.row = new_row;
            shifted_images.push(image);
        }

        self.inline_images = shifted_images;
        self.delete_orphan_kitty_relative_children();
    }

    fn scroll_kitty_placeholder_cells_down_region(&mut self, top: u16, bottom: u16, count: u16) {
        if count == 0 || top > bottom {
            return;
        }

        let first_row = self.scrollback.len().saturating_add(usize::from(top));
        let last_row = self
            .scrollback
            .len()
            .saturating_add(usize::from(bottom))
            .saturating_add(1);
        let count = usize::from(count);
        let mut shifted = HashMap::new();

        for ((row, column), mut placeholder) in self.kitty_placeholder_cells.drain() {
            if row < first_row || row >= last_row {
                shifted.insert((row, column), placeholder);
                continue;
            }
            let Some(new_row) = row.checked_add(count) else {
                continue;
            };
            if new_row >= last_row {
                continue;
            }
            placeholder.row = new_row;
            shifted.insert((new_row, column), placeholder);
        }

        self.kitty_placeholder_cells = shifted;
        self.last_kitty_placeholder = None;
    }

    fn scroll_inline_images_up_region(&mut self, top: u16, bottom: u16, count: u16) {
        if count == 0 || top > bottom {
            return;
        }

        let first_row = self.scrollback.len().saturating_add(usize::from(top));
        let last_row = self
            .scrollback
            .len()
            .saturating_add(usize::from(bottom))
            .saturating_add(1);
        let count = usize::from(count);
        let mut shifted_images = Vec::with_capacity(self.inline_images.len());

        for mut image in self.inline_images.drain(..) {
            let (image_top, image_bottom) = kitty_image_row_range(&image);
            if image_bottom <= first_row || image_top >= last_row {
                shifted_images.push(image);
                continue;
            }

            if image_top < first_row || image_bottom > last_row {
                continue;
            }

            let Some(new_row) = image.row.checked_sub(count) else {
                continue;
            };
            let image_height = image_bottom.saturating_sub(image_top);
            if new_row < first_row || new_row.saturating_add(image_height) > last_row {
                continue;
            }

            image.row = new_row;
            shifted_images.push(image);
        }

        self.inline_images = shifted_images;
        self.delete_orphan_kitty_relative_children();
    }

    fn scroll_kitty_placeholder_cells_up_region(&mut self, top: u16, bottom: u16, count: u16) {
        if count == 0 || top > bottom {
            return;
        }

        let first_row = self.scrollback.len().saturating_add(usize::from(top));
        let last_row = self
            .scrollback
            .len()
            .saturating_add(usize::from(bottom))
            .saturating_add(1);
        let count = usize::from(count);
        let mut shifted = HashMap::new();

        for ((row, column), mut placeholder) in self.kitty_placeholder_cells.drain() {
            if row < first_row || row >= last_row {
                shifted.insert((row, column), placeholder);
                continue;
            }
            let Some(new_row) = row.checked_sub(count) else {
                continue;
            };
            if new_row < first_row {
                continue;
            }
            placeholder.row = new_row;
            shifted.insert((new_row, column), placeholder);
        }

        self.kitty_placeholder_cells = shifted;
        self.last_kitty_placeholder = None;
    }

    fn scroll_up_region_by(&mut self, top: u16, bottom: u16, count: u16) {
        let size = self.grid.size();
        if size.rows == 0 || size.columns == 0 || top > bottom || count == 0 {
            return;
        }

        let height = bottom - top + 1;
        let count = count.min(height);
        self.scroll_inline_images_up_region(top, bottom, count);
        self.scroll_kitty_placeholder_cells_up_region(top, bottom, count);

        if count < height {
            let shift_bottom = bottom - count;
            for row in top..=shift_bottom {
                for column in 0..size.columns {
                    let cell = self
                        .grid
                        .get(row + count, column)
                        .cloned()
                        .unwrap_or_default();
                    self.grid.set(row, column, cell);
                }
                self.grid.copy_row_wrapped(row + count, row);
            }
        }

        let blank_start = if count == height {
            top
        } else {
            bottom - count + 1
        };
        for row in blank_start..=bottom {
            for column in 0..size.columns {
                self.grid.set(row, column, self.blank_cell());
            }
            self.grid.set_row_wrapped(row, false);
        }

        self.record_damage(DamageRegion::new(0, top, size.columns, height));
    }

    fn insert_blank_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        let shift_end = size.columns - count;
        for column in (self.cursor_column..shift_end).rev() {
            let cell = self
                .grid
                .get(self.cursor_row, column)
                .cloned()
                .unwrap_or_default();
            self.grid.set(self.cursor_row, column + count, cell);
        }

        for column in self.cursor_column..self.cursor_column + count {
            self.grid.set(self.cursor_row, column, self.blank_cell());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            size.columns - self.cursor_column,
            1,
        ));
    }

    fn delete_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        let shift_end = size.columns - count;
        for column in self.cursor_column..shift_end {
            let cell = self
                .grid
                .get(self.cursor_row, column + count)
                .cloned()
                .unwrap_or_default();
            self.grid.set(self.cursor_row, column, cell);
        }

        for column in shift_end..size.columns {
            self.grid.set(self.cursor_row, column, self.blank_cell());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            size.columns - self.cursor_column,
            1,
        ));
    }

    fn erase_characters(&mut self, count: u16) {
        self.pending_wrap = false;
        let size = self.grid.size();
        if self.cursor_row >= size.rows || self.cursor_column >= size.columns || count == 0 {
            return;
        }

        let count = count.min(size.columns - self.cursor_column);
        for column in self.cursor_column..self.cursor_column + count {
            self.grid.set(self.cursor_row, column, self.blank_cell());
        }

        self.record_damage(DamageRegion::new(
            self.cursor_column,
            self.cursor_row,
            count,
            1,
        ));
    }

    fn repeat_previous_character(&mut self, count: u16) {
        let Some(ch) = self.last_printable else {
            return;
        };

        for _ in 0..count {
            self.write_char(ch);
        }
    }

    fn clear_cells(&mut self, row: u16, start_column: u16, end_column: u16, selective: bool) {
        let columns = self.grid.size().columns;
        if row >= self.grid.size().rows || start_column >= columns {
            return;
        }

        let end_column = end_column.min(columns);
        if start_column >= end_column {
            return;
        }

        let source_row = self.scrollback.len().saturating_add(usize::from(row));
        if !selective {
            self.clear_kitty_placeholder_cells(source_row, start_column, end_column - start_column);
        }
        for column in start_column..end_column {
            if selective
                && self
                    .grid
                    .get(row, column)
                    .is_some_and(|cell| cell.protected)
            {
                continue;
            }
            if selective {
                self.clear_kitty_placeholder_cells(source_row, column, 1);
            }
            self.grid.set(row, column, self.blank_cell());
        }
        if start_column == 0 && end_column >= columns {
            self.grid.set_row_wrapped(row, false);
        }

        self.record_damage(DamageRegion::new(
            start_column,
            row,
            end_column - start_column,
            1,
        ));
    }

    fn blank_cell(&self) -> Cell {
        let mut cell = self.style.clone();
        cell.ch = ' ';
        cell
    }

    fn apply_sgr(&mut self, params: &[char]) {
        let values = parse_sgr_params(params);
        let mut index = 0;

        while index < values.len() {
            match values[index] {
                SgrParameter::UnderlineStyle(style) => self.apply_underline_style(style),
                SgrParameter::Code(code) => match code {
                    0 => self.reset_style(),
                    1 => self.style.bold = true,
                    2 => self.style.faint = true,
                    3 => self.style.italic = true,
                    4 => self.apply_underline_style(UnderlineStyle::Single),
                    5 => {
                        self.style.blink = true;
                        self.style.rapid_blink = false;
                    }
                    6 => {
                        self.style.blink = true;
                        self.style.rapid_blink = true;
                    }
                    7 => self.style.inverse = true,
                    8 => self.style.conceal = true,
                    9 => self.style.strikethrough = true,
                    22 => {
                        self.style.bold = false;
                        self.style.faint = false;
                    }
                    21 => self.apply_underline_style(UnderlineStyle::Double),
                    23 => self.style.italic = false,
                    24 => self.apply_underline_style(UnderlineStyle::None),
                    25 => {
                        self.style.blink = false;
                        self.style.rapid_blink = false;
                    }
                    27 => self.style.inverse = false,
                    28 => self.style.conceal = false,
                    29 => self.style.strikethrough = false,
                    30..=37 => {
                        self.style.foreground = Color::Indexed(saturating_u8(code - 30));
                    }
                    39 => self.style.foreground = Color::Default,
                    40..=47 => {
                        self.style.background = Color::Indexed(saturating_u8(code - 40));
                    }
                    49 => self.style.background = Color::Default,
                    53 => self.style.overline = true,
                    55 => self.style.overline = false,
                    73 => self.style.vertical_align = crate::VerticalAlign::Superscript,
                    74 => self.style.vertical_align = crate::VerticalAlign::Subscript,
                    75 => self.style.vertical_align = crate::VerticalAlign::Baseline,
                    59 => self.style.underline_color = Color::Default,
                    90..=97 => {
                        self.style.foreground = Color::Indexed(saturating_u8(code - 90 + 8));
                    }
                    100..=107 => {
                        self.style.background = Color::Indexed(saturating_u8(code - 100 + 8));
                    }
                    38 | 48 | 58 => {
                        if let Some((color, consumed)) = parse_extended_color(&values[index + 1..])
                        {
                            match code {
                                38 => self.style.foreground = color,
                                48 => self.style.background = color,
                                58 => self.style.underline_color = color,
                                _ => {}
                            }
                            index += consumed;
                        }
                    }
                    _ => {}
                },
            }

            index += 1;
        }
    }

    fn apply_underline_style(&mut self, style: UnderlineStyle) {
        self.style.underline_style = style;
        self.style.underline = matches!(
            style,
            UnderlineStyle::Single
                | UnderlineStyle::Curly
                | UnderlineStyle::Dotted
                | UnderlineStyle::Dashed
        );
        self.style.double_underline = style == UnderlineStyle::Double;
    }

    fn reset_style(&mut self) {
        let hyperlink = self.style.hyperlink.take();
        let semantic_type = self.style.semantic_type;
        self.style = Cell::default();
        self.style.hyperlink = hyperlink;
        self.style.semantic_type = semantic_type;
    }

    fn record_damage(&mut self, region: DamageRegion) {
        if region.is_empty() {
            return;
        }

        if let Some(last) = self.damage.last_mut() {
            let adjacent =
                last.y == region.y && last.height == region.height && last.right() == region.x;

            if adjacent {
                last.width = last.width.saturating_add(region.width);
                return;
            }
        }

        self.damage.push(region);
    }

    fn map_graphic_character(&self, ch: char) -> char {
        match self.character_set {
            CharacterSet::Ascii => ch,
            CharacterSet::DecSpecialGraphics => map_dec_special_graphics(ch),
        }
    }
}

fn parse_g0_character_set(selector: char) -> Option<CharacterSet> {
    match selector {
        'B' => Some(CharacterSet::Ascii),
        '0' => Some(CharacterSet::DecSpecialGraphics),
        _ => None,
    }
}

fn map_dec_special_graphics(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        '~' => '·',
        _ => ch,
    }
}

fn default_tab_stops(size: TerminalSize) -> Vec<u16> {
    let mut stops = Vec::new();
    let mut column = 8;
    while column < size.columns {
        stops.push(column);
        column = column.saturating_add(8);
    }
    stops
}

fn parse_csi(chars: &[char], mut index: usize) -> SequenceParse<(char, usize)> {
    while index < chars.len() {
        let ch = chars[index];
        if is_cancel_control(ch) {
            return SequenceParse::Cancelled(index);
        }
        if ('@'..='~').contains(&ch) {
            return SequenceParse::Complete((ch, index));
        }
        index += 1;
    }

    SequenceParse::Pending
}

fn clamp_screen_state(screen: &mut ScreenState, size: TerminalSize) {
    screen.cursor_row = clamp_axis(screen.cursor_row, size.rows);
    screen.cursor_column = clamp_axis(screen.cursor_column, size.columns);
    screen.scroll_top = clamp_axis(screen.scroll_top, size.rows);
    screen.scroll_bottom = clamp_axis(screen.scroll_bottom, size.rows);
    if screen.scroll_top >= screen.scroll_bottom {
        screen.scroll_top = 0;
        screen.scroll_bottom = size.rows.saturating_sub(1);
    }
    if size.columns == 0 || size.rows == 0 {
        screen.pending_wrap = false;
    }
}

fn clamp_axis(value: u16, limit: u16) -> u16 {
    value.min(limit.saturating_sub(1))
}

fn parse_osc(chars: &[char], mut index: usize) -> SequenceParse<usize> {
    while index < chars.len() {
        match chars[index] {
            '\u{7}' | '\u{9c}' => return SequenceParse::Complete(index),
            '\u{1b}' if chars.get(index + 1) == Some(&'\\') => {
                return SequenceParse::Complete(index + 1);
            }
            ch if is_cancel_control(ch) => return SequenceParse::Cancelled(index),
            _ => index += 1,
        }
    }

    SequenceParse::Pending
}

fn osc_content_end(chars: &[char], content_start: usize, sequence_end: usize) -> usize {
    st_content_end(chars, content_start, sequence_end)
}

fn st_content_end(chars: &[char], content_start: usize, sequence_end: usize) -> usize {
    if sequence_end > content_start
        && chars.get(sequence_end) == Some(&'\\')
        && chars.get(sequence_end - 1) == Some(&'\u{1b}')
    {
        sequence_end - 1
    } else {
        sequence_end
    }
}

fn parse_st_terminated_control_string(chars: &[char], mut index: usize) -> SequenceParse<usize> {
    while index < chars.len() {
        match chars[index] {
            '\u{9c}' => return SequenceParse::Complete(index),
            '\u{1b}' if chars.get(index + 1) == Some(&'\\') => {
                return SequenceParse::Complete(index + 1);
            }
            ch if is_cancel_control(ch) => return SequenceParse::Cancelled(index),
            _ => index += 1,
        }
    }

    SequenceParse::Pending
}

fn is_c1_st_control_string(ch: char) -> bool {
    matches!(ch, '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}')
}

fn is_escape_or_c1_sequence_start(ch: char) -> bool {
    matches!(
        ch,
        '\u{1b}'
            | '\u{84}'
            | '\u{85}'
            | '\u{88}'
            | '\u{8d}'
            | '\u{90}'
            | '\u{9b}'
            | '\u{9c}'
            | '\u{9d}'
    ) || is_c1_st_control_string(ch)
}

fn is_cancel_control(ch: char) -> bool {
    matches!(ch, '\u{18}' | '\u{1a}')
}

fn is_ascii_control(ch: char) -> bool {
    matches!(ch, '\0'..='\u{1f}' | '\u{7f}')
}

fn leading_combining_marks_end(text: &str) -> usize {
    let mut end = 0;
    for (index, ch) in text.char_indices() {
        if canonical_combining_class(ch) == 0 {
            break;
        }
        end = index + ch.len_utf8();
    }
    end
}

fn is_ignored_c0_control(ch: char) -> bool {
    matches!(ch, '\0'..='\u{6}' | '\u{e}'..='\u{1a}' | '\u{1c}'..='\u{1f}')
}

fn decode_base64_utf8(value: &str) -> Option<String> {
    let decoded = STANDARD.decode(value).ok()?;
    String::from_utf8(decoded).ok()
}

#[derive(Debug, Clone)]
struct SixelImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SixelBackground {
    Opaque,
    Transparent,
}

#[derive(Debug, Clone, Copy)]
struct SixelOptions {
    background: SixelBackground,
    pixel_height_scale: u32,
}

impl Default for SixelOptions {
    fn default() -> Self {
        Self {
            background: SixelBackground::Opaque,
            pixel_height_scale: 2,
        }
    }
}

struct SixelCanvas {
    pixels: HashMap<(u32, u32), [u8; 4]>,
    palette: HashMap<u16, [u8; 4]>,
    current_color: [u8; 4],
    background: SixelBackground,
    x: u32,
    y: u32,
    max_x: u32,
    max_y: u32,
    declared_width: Option<u32>,
    declared_height: Option<u32>,
    pixel_height_scale: u32,
}

const VT340_DEFAULT_SIXEL_PALETTE: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [51, 51, 204, 255],
    [204, 33, 33, 255],
    [51, 204, 51, 255],
    [204, 51, 204, 255],
    [51, 204, 204, 255],
    [204, 204, 51, 255],
    [135, 135, 135, 255],
    [66, 66, 66, 255],
    [84, 84, 153, 255],
    [153, 66, 66, 255],
    [84, 153, 84, 255],
    [153, 84, 153, 255],
    [84, 153, 153, 255],
    [153, 153, 84, 255],
    [204, 204, 204, 255],
];

impl SixelCanvas {
    fn new(options: SixelOptions) -> Self {
        let palette = VT340_DEFAULT_SIXEL_PALETTE
            .iter()
            .copied()
            .enumerate()
            .map(|(index, color)| {
                (
                    u16::try_from(index).expect("default palette index fits u16"),
                    color,
                )
            })
            .collect();

        Self {
            pixels: HashMap::new(),
            palette,
            current_color: VT340_DEFAULT_SIXEL_PALETTE[0],
            background: options.background,
            x: 0,
            y: 0,
            max_x: 0,
            max_y: 0,
            declared_width: None,
            declared_height: None,
            pixel_height_scale: options.pixel_height_scale,
        }
    }

    fn write_bits(&mut self, bits: u8, repeat: u32) {
        for _ in 0..repeat.max(1) {
            for bit in 0..6 {
                if bits & (1 << bit) != 0 {
                    let y = self.y.saturating_add(bit);
                    self.pixels.insert((self.x, y), self.current_color);
                    self.max_x = self.max_x.max(self.x);
                    self.max_y = self.max_y.max(y);
                }
            }
            self.x = self.x.saturating_add(1);
        }
    }

    fn carriage_return(&mut self) {
        self.x = 0;
    }

    fn newline(&mut self) {
        self.x = 0;
        self.y = self.y.saturating_add(6);
    }

    fn select_color(&mut self, index: u16) {
        if let Some(color) = self.palette.get(&index).copied() {
            self.current_color = color;
        }
    }

    fn define_rgb_color(&mut self, index: u16, red: u16, green: u16, blue: u16) {
        let color = [
            sixel_percent_to_u8(red),
            sixel_percent_to_u8(green),
            sixel_percent_to_u8(blue),
            255,
        ];
        self.palette.insert(index, color);
        self.current_color = color;
    }

    fn define_hls_color(&mut self, index: u16, hue: u16, lightness: u16, saturation: u16) {
        let color = sixel_hls_to_rgba(hue, lightness, saturation);
        self.palette.insert(index, color);
        self.current_color = color;
    }

    fn set_raster_attributes(
        &mut self,
        aspect_numerator: u32,
        aspect_denominator: u32,
        width: u32,
        height: u32,
    ) {
        self.pixel_height_scale = sixel_pixel_height_scale(aspect_numerator, aspect_denominator);
        if width > 0 {
            self.declared_width = Some(width);
        }
        if height > 0 {
            self.declared_height = Some(height);
        }
    }

    fn into_image(self) -> Option<SixelImage> {
        if self.pixels.is_empty() && self.background == SixelBackground::Transparent {
            return None;
        }

        let (drawn_width, drawn_height) = if self.pixels.is_empty() {
            (0, 0)
        } else {
            (self.max_x.checked_add(1)?, self.max_y.checked_add(1)?)
        };
        let width = self
            .declared_width
            .map_or(drawn_width, |declared| declared.max(drawn_width));
        let logical_height = self
            .declared_height
            .map_or(drawn_height, |declared| declared.max(drawn_height));
        let height = logical_height.checked_mul(self.pixel_height_scale)?;
        if width == 0 || logical_height == 0 || height == 0 {
            return None;
        }
        let len = usize::try_from(width)
            .ok()?
            .checked_mul(usize::try_from(height).ok()?)?
            .checked_mul(4)?;
        let background_color = match self.background {
            SixelBackground::Opaque => self.palette.get(&0).copied().unwrap_or([0, 0, 0, 255]),
            SixelBackground::Transparent => [0, 0, 0, 0],
        };
        let mut data = vec![0; len];
        for pixel in data.chunks_exact_mut(4) {
            pixel.copy_from_slice(&background_color);
        }

        for ((x, y), color) in self.pixels {
            if x >= width || y >= logical_height {
                continue;
            }
            let physical_y = y.checked_mul(self.pixel_height_scale)?;
            for row_offset in 0..self.pixel_height_scale {
                let row = physical_y.checked_add(row_offset)?;
                if row >= height {
                    continue;
                }
                let index = usize::try_from((row * width + x) * 4).ok()?;
                data.get_mut(index..index + 4)?.copy_from_slice(&color);
            }
        }

        Some(SixelImage {
            width,
            height,
            data,
        })
    }
}

fn parse_sixel_dcs_options(params: &str) -> SixelOptions {
    let params = parse_sixel_numeric_params(params);
    let pixel_height_scale = sixel_dcs_macro_pixel_height_scale(params.first().copied());
    let background = match params.get(1).copied() {
        Some(1) => SixelBackground::Transparent,
        _ => SixelBackground::Opaque,
    };

    SixelOptions {
        background,
        pixel_height_scale,
    }
}

fn sixel_dcs_marker_index(content: &str) -> Option<usize> {
    let marker = content.find('q')?;
    let params = &content[..marker];
    if !params.chars().all(|ch| ch.is_ascii_digit() || ch == ';') {
        return None;
    }

    (parse_sixel_numeric_params(params).first().copied() != Some(1000)).then_some(marker)
}

fn parse_sixel_image(options: SixelOptions, content: &str) -> Option<SixelImage> {
    let mut canvas = SixelCanvas::new(options);
    let mut chars = content.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '$' => canvas.carriage_return(),
            '-' => canvas.newline(),
            '!' => {
                let repeat = parse_sixel_number(&mut chars).unwrap_or(1);
                if let Some(bits) = chars.next().and_then(sixel_bits) {
                    canvas.write_bits(bits, repeat);
                }
            }
            '#' => parse_sixel_color_introducer(&mut chars, &mut canvas),
            '"' => parse_sixel_raster_attributes(&mut chars, &mut canvas),
            ch => {
                if let Some(bits) = sixel_bits(ch) {
                    canvas.write_bits(bits, 1);
                }
            }
        }
    }

    canvas.into_image()
}

fn parse_sixel_color_introducer(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    canvas: &mut SixelCanvas,
) {
    let params = parse_sixel_parameter_list(chars);
    let Some(index) = params.first().copied() else {
        return;
    };

    match params.get(1).copied() {
        Some(1) if params.len() >= 5 => {
            canvas.define_hls_color(index, params[2], params[3], params[4]);
        }
        Some(2) if params.len() >= 5 => {
            canvas.define_rgb_color(index, params[2], params[3], params[4]);
        }
        _ => canvas.select_color(index),
    }
}

fn parse_sixel_raster_attributes(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    canvas: &mut SixelCanvas,
) {
    let params = parse_sixel_parameter_list(chars);
    if params.len() >= 4 {
        canvas.set_raster_attributes(
            u32::from(params[0]),
            u32::from(params[1]),
            u32::from(params[2]),
            u32::from(params[3]),
        );
    }
}

fn parse_sixel_parameter_list(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Vec<u16> {
    let mut raw = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() || ch == ';' {
            raw.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    parse_sixel_numeric_params(&raw)
}

fn parse_sixel_numeric_params(raw: &str) -> Vec<u16> {
    raw.split(';')
        .map(|value| {
            if value.is_empty() {
                0
            } else {
                value.parse::<u16>().unwrap_or(0)
            }
        })
        .collect()
}

fn parse_sixel_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<u32> {
    let mut raw = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            raw.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    raw.parse::<u32>().ok()
}

fn sixel_bits(ch: char) -> Option<u8> {
    let code = u32::from(ch);
    (0x3f..=0x7e)
        .contains(&code)
        .then(|| u8::try_from(code - 0x3f).ok())
        .flatten()
}

fn sixel_percent_to_u8(value: u16) -> u8 {
    let value = u32::from(value.min(100));
    u8::try_from((value * 255 + 50) / 100).unwrap_or(255)
}

fn sixel_pixel_height_scale(numerator: u32, denominator: u32) -> u32 {
    if numerator == 0 || denominator == 0 {
        return 1;
    }

    ((numerator + denominator / 2) / denominator).max(1)
}

fn sixel_dcs_macro_pixel_height_scale(value: Option<u16>) -> u32 {
    match value.unwrap_or(0) {
        2 => 5,
        3 | 4 => 3,
        7..=9 => 1,
        _ => 2,
    }
}

fn sixel_hls_to_rgba(hue: u16, lightness: u16, saturation: u16) -> [u8; 4] {
    let hue = u16::try_from((u32::from(hue) + 240) % 360)
        .map(f32::from)
        .expect("normalized sixel HLS hue fits u16")
        / 360.0;
    let lightness = f32::from(lightness.min(100)) / 100.0;
    let saturation = f32::from(saturation.min(100)) / 100.0;

    let (red, green, blue) = if saturation == 0.0 {
        (lightness, lightness, lightness)
    } else {
        let q = if lightness < 0.5 {
            lightness * (1.0 + saturation)
        } else {
            lightness + saturation - lightness * saturation
        };
        let p = 2.0 * lightness - q;
        (
            hue_to_rgb(p, q, hue + 1.0 / 3.0),
            hue_to_rgb(p, q, hue),
            hue_to_rgb(p, q, hue - 1.0 / 3.0),
        )
    };

    [
        unit_float_to_u8(red),
        unit_float_to_u8(green),
        unit_float_to_u8(blue),
        255,
    ]
}

fn hue_to_rgb(p: f32, q: f32, mut hue: f32) -> f32 {
    if hue < 0.0 {
        hue += 1.0;
    }
    if hue > 1.0 {
        hue -= 1.0;
    }

    if hue < 1.0 / 6.0 {
        p + (q - p) * 6.0 * hue
    } else if hue < 1.0 / 2.0 {
        q
    } else if hue < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - hue) * 6.0
    } else {
        p
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn unit_float_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn start_kitty_graphics_upload(
    params: KittyGraphicsParams,
    encoded_data: &str,
) -> Result<PendingKittyGraphics, KittyGraphicsStartError> {
    let action = match params.action {
        Some('T') => KittyUploadAction::Display,
        Some('t') | None => KittyUploadAction::Store,
        Some('q') => KittyUploadAction::Query,
        _ => return Err(KittyGraphicsStartError::Action),
    };

    let medium = match params.medium.unwrap_or('d') {
        'd' => KittyTransmissionMedium::Direct,
        'f' => KittyTransmissionMedium::File,
        't' => KittyTransmissionMedium::TempFile,
        _ => return Err(KittyGraphicsStartError::TransmissionMedium),
    };
    if params
        .compression
        .is_some_and(|compression| compression != 'z')
    {
        return Err(KittyGraphicsStartError::Compression);
    }

    let image_format = match params.format.unwrap_or(32) {
        24 => InlineImageFormat::Rgb,
        32 => InlineImageFormat::Rgba,
        100 => InlineImageFormat::Encoded,
        _ => return Err(KittyGraphicsStartError::ImageFormat),
    };

    Ok(PendingKittyGraphics {
        image_format,
        medium,
        compression: params.compression,
        action,
        image_id: params.image_id,
        image_number: params.image_number,
        placement_id: params.placement_id,
        z_index: params.z_index,
        pixel_width: params.pixel_width,
        pixel_height: params.pixel_height,
        display_columns: params.display_columns,
        display_rows: params.display_rows,
        no_cursor_movement: params.no_cursor_movement,
        source_x: params.source_x,
        source_y: params.source_y,
        source_width: params.source_width,
        source_height: params.source_height,
        target_x: params.target_x,
        target_y: params.target_y,
        file_offset: params.file_offset,
        file_size: params.file_size,
        quiet: params.quiet,
        virtual_placement: params.virtual_placement,
        encoded_data: encoded_data.to_owned(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KittyGraphicsStartError {
    Action,
    TransmissionMedium,
    Compression,
    ImageFormat,
}

impl KittyGraphicsStartError {
    const fn message(self) -> &'static str {
        match self {
            Self::Action => "Unsupported action",
            Self::TransmissionMedium => "Unsupported transmission medium",
            Self::Compression => "Unsupported compression",
            Self::ImageFormat => "Unsupported image format",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KittyGraphicsDataError {
    InvalidBase64,
    InvalidFile,
    UnsupportedCompression,
}

fn load_kitty_graphics_payload(
    upload: &PendingKittyGraphics,
) -> Result<Vec<u8>, KittyGraphicsDataError> {
    let data = match upload.medium {
        KittyTransmissionMedium::Direct => STANDARD
            .decode(&upload.encoded_data)
            .map_err(|_| KittyGraphicsDataError::InvalidBase64)?,
        KittyTransmissionMedium::File => read_kitty_graphics_file_payload(
            &upload.encoded_data,
            upload.file_offset,
            upload.file_size,
            false,
        )?,
        KittyTransmissionMedium::TempFile => read_kitty_graphics_file_payload(
            &upload.encoded_data,
            upload.file_offset,
            upload.file_size,
            true,
        )?,
    };

    decode_kitty_graphics_payload(upload.compression, data)
        .ok_or(KittyGraphicsDataError::UnsupportedCompression)
}

fn read_kitty_graphics_file_payload(
    encoded_path: &str,
    offset: Option<u64>,
    size: Option<u64>,
    delete_after_read: bool,
) -> Result<Vec<u8>, KittyGraphicsDataError> {
    let path = STANDARD
        .decode(encoded_path)
        .map_err(|_| KittyGraphicsDataError::InvalidBase64)?;
    let path = String::from_utf8(path).map_err(|_| KittyGraphicsDataError::InvalidFile)?;
    let path = PathBuf::from(path);

    let metadata = fs::metadata(&path).map_err(|_| KittyGraphicsDataError::InvalidFile)?;
    if !metadata.is_file() {
        return Err(KittyGraphicsDataError::InvalidFile);
    }

    let data = {
        let mut file = fs::File::open(&path).map_err(|_| KittyGraphicsDataError::InvalidFile)?;
        if let Some(offset) = offset {
            file.seek(SeekFrom::Start(offset))
                .map_err(|_| KittyGraphicsDataError::InvalidFile)?;
        }

        let mut data = Vec::new();
        if let Some(size) = size {
            let mut limited = file.take(size);
            limited
                .read_to_end(&mut data)
                .map_err(|_| KittyGraphicsDataError::InvalidFile)?;
        } else {
            file.read_to_end(&mut data)
                .map_err(|_| KittyGraphicsDataError::InvalidFile)?;
        }
        data
    };

    if delete_after_read && kitty_temp_file_can_be_deleted(&path) {
        let _ = fs::remove_file(&path);
    }

    Ok(data)
}

fn kitty_temp_file_can_be_deleted(path: &Path) -> bool {
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    if !path
        .as_os_str()
        .to_string_lossy()
        .contains("tty-graphics-protocol")
    {
        return false;
    }

    kitty_temp_directories()
        .into_iter()
        .any(|temp_dir| fs::canonicalize(temp_dir).is_ok_and(|temp_dir| path.starts_with(temp_dir)))
}

fn kitty_temp_directories() -> Vec<PathBuf> {
    #[cfg(unix)]
    {
        let mut dirs = vec![std::env::temp_dir()];
        dirs.push(PathBuf::from("/tmp"));
        dirs.push(PathBuf::from("/var/tmp"));
        dirs.push(PathBuf::from("/dev/shm"));
        dirs
    }

    #[cfg(not(unix))]
    {
        vec![std::env::temp_dir()]
    }
}

fn append_kitty_graphics_response_param(
    response: &mut Vec<u8>,
    has_param: &mut bool,
    name: u8,
    value: u32,
) {
    if *has_param {
        response.push(b',');
    }
    response.push(name);
    response.push(b'=');
    response.extend_from_slice(value.to_string().as_bytes());
    *has_param = true;
}

fn kitty_graphics_response_is_suppressed(params: KittyGraphicsParams, status: &str) -> bool {
    match params.quiet {
        Some(1) => status == "OK",
        Some(2) => status != "OK",
        _ => false,
    }
}

fn kitty_placement_id(image_id: Option<u32>, placement_id: Option<u32>) -> Option<u32> {
    image_id.and(placement_id)
}

fn kitty_placement_key(
    image_id: Option<u32>,
    placement_id: Option<u32>,
) -> Option<KittyPlacementKey> {
    Some((image_id?, placement_id?))
}

fn kitty_virtual_placement_key(image_id: u32, placement_id: Option<u32>) -> KittyPlacementKey {
    (image_id, placement_id.unwrap_or(0))
}

fn kitty_image_placement_key(image: &ItermInlineImage) -> Option<KittyPlacementKey> {
    kitty_placement_key(image.kitty_image_id, image.kitty_placement_id)
}

fn kitty_display_dimensions(
    image: &StoredKittyImage,
    options: KittyPlacementOptions,
) -> (Option<String>, Option<String>) {
    let columns = options.display_columns.or(image.display_columns);
    let rows = options.display_rows.or(image.display_rows);
    let aspect = kitty_source_aspect_pixels(image, options.source_rect);

    let derived_rows = match (columns, rows, aspect) {
        (Some(columns), None, Some((source_width, source_height))) => {
            derive_kitty_display_axis(columns, source_height, source_width)
        }
        _ => None,
    };
    let derived_columns = match (columns, rows, aspect) {
        (None, Some(rows), Some((source_width, source_height))) => {
            derive_kitty_display_axis(rows, source_width, source_height)
        }
        _ => None,
    };

    let width = columns
        .or(derived_columns)
        .map(|columns| columns.to_string())
        .or_else(|| image.pixel_width.map(|width| format!("{width}px")));
    let height = rows
        .or(derived_rows)
        .map(|rows| rows.to_string())
        .or_else(|| image.pixel_height.map(|height| format!("{height}px")));

    (width, height)
}

fn kitty_source_aspect_pixels(
    image: &StoredKittyImage,
    source_rect: KittySourceRect,
) -> Option<(u32, u32)> {
    let pixel_width = image.pixel_width?;
    let pixel_height = image.pixel_height?;
    let source_x = source_rect.x.unwrap_or(0);
    let source_y = source_rect.y.unwrap_or(0);
    if source_x >= pixel_width || source_y >= pixel_height {
        return None;
    }

    let width = source_rect.width.unwrap_or(pixel_width - source_x);
    let height = source_rect.height.unwrap_or(pixel_height - source_y);
    Some((
        width.min(pixel_width - source_x),
        height.min(pixel_height - source_y),
    ))
    .filter(|(width, height)| *width > 0 && *height > 0)
}

fn derive_kitty_display_axis(
    known_cells: u16,
    numerator_pixels: u32,
    denominator_pixels: u32,
) -> Option<u16> {
    if denominator_pixels == 0 {
        return None;
    }
    let derived = u64::from(known_cells)
        .saturating_mul(u64::from(numerator_pixels))
        .saturating_add(u64::from(denominator_pixels) - 1)
        / u64::from(denominator_pixels);
    Some(u16::try_from(derived.max(1)).unwrap_or(u16::MAX))
}

fn kitty_image_matches_placeholder_render(
    image: &ItermInlineImage,
    placeholder_render_keys: &HashSet<KittyPlaceholderRenderKey>,
) -> bool {
    image.kitty_image_id.is_some_and(|image_id| {
        placeholder_render_keys.contains(&(
            image.row,
            image.column,
            image_id,
            image.kitty_placement_id,
        ))
    })
}

fn kitty_placeholder_image_id(color: Color) -> Option<u32> {
    kitty_placeholder_color_value(color).filter(|image_id| *image_id != ANONYMOUS_KITTY_IMAGE_ID)
}

fn kitty_placeholder_placement_id(color: Color) -> Option<u32> {
    kitty_placeholder_color_value(color).filter(|placement_id| *placement_id != 0)
}

fn kitty_placeholder_origin(
    pending: &PendingKittyPlaceholder,
    placeholder_row: u32,
    placeholder_column: u32,
) -> Option<(usize, u16)> {
    kitty_placeholder_origin_from_cell(
        pending.row,
        pending.column,
        placeholder_row,
        placeholder_column,
    )
}

fn kitty_placeholder_origin_from_cell(
    row: usize,
    column: u16,
    placeholder_row: u32,
    placeholder_column: u32,
) -> Option<(usize, u16)> {
    let row_offset = usize::try_from(placeholder_row).ok()?;
    let column_offset = u16::try_from(placeholder_column).ok()?;
    Some((
        row.checked_sub(row_offset)?,
        column.checked_sub(column_offset)?,
    ))
}

fn kitty_placeholder_color_value(color: Color) -> Option<u32> {
    match color {
        Color::Default => None,
        Color::Indexed(value) => Some(u32::from(value)),
        Color::Rgb(red, green, blue) | Color::Rgba(red, green, blue, _) => {
            Some((u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue))
        }
    }
}

fn kitty_placeholder_diacritic_value(ch: char) -> Option<u32> {
    KITTY_PLACEHOLDER_DIACRITICS
        .iter()
        .position(|diacritic| *diacritic == ch)
        .and_then(|index| u32::try_from(index).ok())
}

fn offset_kitty_history_row(row: usize, offset: i32) -> usize {
    let magnitude = usize::try_from(offset.unsigned_abs()).unwrap_or(usize::MAX);
    if offset >= 0 {
        row.saturating_add(magnitude)
    } else {
        row.saturating_sub(magnitude)
    }
}

fn offset_kitty_column(column: u16, offset: i32) -> u16 {
    let column = offset_kitty_history_row(usize::from(column), offset);
    u16::try_from(column).unwrap_or(u16::MAX)
}

fn move_kitty_history_row(row: usize, old_origin: usize, new_origin: usize) -> usize {
    if new_origin >= old_origin {
        row.saturating_add(new_origin - old_origin)
    } else {
        row.saturating_sub(old_origin - new_origin)
    }
}

fn move_kitty_column(column: u16, old_origin: u16, new_origin: u16) -> u16 {
    if new_origin >= old_origin {
        column.saturating_add(new_origin - old_origin)
    } else {
        column.saturating_sub(old_origin - new_origin)
    }
}

fn kitty_image_intersects_cell(image: &ItermInlineImage, row: usize, column: u16) -> bool {
    if image.kitty_image_id.is_none() {
        return false;
    }

    let (left, right) = kitty_image_column_range(image);
    let (top, bottom) = kitty_image_row_range(image);

    row >= top && row < bottom && column >= left && column < right
}

fn kitty_image_intersects_column(
    image: &ItermInlineImage,
    first_row: usize,
    last_row: usize,
    column: u16,
) -> bool {
    if image.kitty_image_id.is_none() {
        return false;
    }

    let (left, right) = kitty_image_column_range(image);
    let (top, bottom) = kitty_image_row_range(image);

    column >= left && column < right && top < last_row && bottom > first_row
}

fn kitty_image_intersects_row(image: &ItermInlineImage, row: usize) -> bool {
    if image.kitty_image_id.is_none() {
        return false;
    }

    let (top, bottom) = kitty_image_row_range(image);
    row >= top && row < bottom
}

fn inline_image_intersects_region(
    image: &ItermInlineImage,
    first_row: usize,
    last_row: usize,
    first_column: u16,
    last_column: u16,
) -> bool {
    let (left, right) = kitty_image_column_range(image);
    let (top, bottom) = kitty_image_row_range(image);

    top < last_row && bottom > first_row && left < last_column && right > first_column
}

fn kitty_image_column_range(image: &ItermInlineImage) -> (u16, u16) {
    let width = inline_image_width_cells(image.width.as_deref()).max(1);
    (
        image.column,
        image
            .column
            .saturating_add(width)
            .max(image.column.saturating_add(1)),
    )
}

fn kitty_image_row_range(image: &ItermInlineImage) -> (usize, usize) {
    let height = usize::from(inline_image_height_cells(image.height.as_deref()).max(1));
    (
        image.row,
        image
            .row
            .saturating_add(height)
            .max(image.row.saturating_add(1)),
    )
}

fn zero_based_axis(value: u16) -> Option<u16> {
    value.checked_sub(1)
}

fn parse_kitty_graphics_params(control: &str) -> KittyGraphicsParams {
    let mut params = KittyGraphicsParams::default();

    for param in control.split(',').filter(|param| !param.is_empty()) {
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };

        match key {
            "a" => params.action = parse_single_char(value),
            "d" => params.delete_target = parse_single_char(value),
            "f" => params.format = value.parse::<u32>().ok(),
            "i" => params.image_id = parse_positive_u32(value),
            "I" => params.image_number = parse_positive_u32(value),
            "p" => params.placement_id = parse_positive_u32(value),
            "P" => params.parent_image_id = parse_positive_u32(value),
            "Q" => params.parent_placement_id = parse_positive_u32(value),
            "t" => params.medium = parse_single_char(value),
            "o" => params.compression = parse_single_char(value),
            "m" => params.more_chunks = value.parse::<u8>().ok(),
            "s" => params.pixel_width = parse_positive_u32(value),
            "v" => params.pixel_height = parse_positive_u32(value),
            "c" => params.display_columns = parse_positive_u16(value),
            "r" => params.display_rows = parse_positive_u16(value),
            "C" => params.no_cursor_movement = value == "1",
            "U" => params.virtual_placement = value == "1",
            "w" => params.source_width = parse_positive_u32(value),
            "h" => params.source_height = parse_positive_u32(value),
            "X" => params.target_x = value.parse::<u32>().ok(),
            "Y" => params.target_y = value.parse::<u32>().ok(),
            "H" => params.parent_offset_columns = value.parse::<i32>().ok(),
            "V" => params.parent_offset_rows = value.parse::<i32>().ok(),
            "x" => {
                params.cell_x = parse_positive_u16(value);
                params.source_x = value.parse::<u32>().ok();
            }
            "y" => {
                params.cell_y = parse_positive_u16(value);
                params.source_y = value.parse::<u32>().ok();
            }
            "z" => params.z_index = value.parse::<i32>().ok(),
            "O" => params.file_offset = value.parse::<u64>().ok(),
            "S" => params.file_size = value.parse::<u64>().ok(),
            "q" => params.quiet = value.parse::<u8>().ok(),
            _ => {}
        }
    }

    params
}

fn parse_single_char(value: &str) -> Option<char> {
    let mut chars = value.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

fn decode_kitty_graphics_payload(compression: Option<char>, data: Vec<u8>) -> Option<Vec<u8>> {
    match compression {
        None => Some(data),
        Some('z') => {
            let mut decoder = ZlibDecoder::new(data.as_slice());
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).ok()?;
            Some(decompressed)
        }
        Some(_) => None,
    }
}

fn kitty_graphics_payload_is_supported(
    image_format: InlineImageFormat,
    pixel_width: Option<u32>,
    pixel_height: Option<u32>,
    payload_len: usize,
) -> bool {
    let bytes_per_pixel = match image_format {
        InlineImageFormat::Encoded => return true,
        InlineImageFormat::Rgb => 3,
        InlineImageFormat::Rgba => 4,
    };
    let Some(pixel_width) = pixel_width else {
        return false;
    };
    let Some(pixel_height) = pixel_height else {
        return false;
    };
    let Some(expected_len) = usize::try_from(pixel_width)
        .ok()
        .and_then(|width| {
            usize::try_from(pixel_height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
    else {
        return false;
    };

    payload_len == expected_len
}

fn parse_positive_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|value| *value > 0)
}

fn parse_positive_u16(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|value| *value > 0)
}

const DEFAULT_INLINE_IMAGE_CELL_WIDTH_PIXELS: u16 = 8;
const DEFAULT_INLINE_IMAGE_CELL_HEIGHT_PIXELS: u16 = 16;

fn inline_image_width_cells(value: Option<&str>) -> u16 {
    inline_image_axis_cells(value, DEFAULT_INLINE_IMAGE_CELL_WIDTH_PIXELS)
}

fn inline_image_height_cells(value: Option<&str>) -> u16 {
    inline_image_axis_cells(value, DEFAULT_INLINE_IMAGE_CELL_HEIGHT_PIXELS)
}

fn inline_image_axis_cells(value: Option<&str>, cell_pixels: u16) -> u16 {
    let Some(value) = value else {
        return 1;
    };
    if let Some(pixels) = value.strip_suffix("px").and_then(parse_positive_u32) {
        let cell_pixels = u32::from(cell_pixels.max(1));
        let cells = pixels.saturating_add(cell_pixels - 1) / cell_pixels;
        return u16::try_from(cells).unwrap_or(u16::MAX).max(1);
    }
    value
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or(1)
}

fn next_or_pending(next_index: Option<usize>) -> FeedAdvance {
    match next_index {
        Some(next_index) => FeedAdvance::Next(next_index),
        None => FeedAdvance::Pending,
    }
}

fn csi_count(params: &[char]) -> u16 {
    param_or_one(parse_csi_params(params).first().copied())
}

fn csi_mode(params: &[char]) -> u16 {
    parse_csi_params(params).first().copied().unwrap_or(0)
}

fn csi_or_private_mode(params: &[char]) -> u16 {
    parse_private_csi_params(params)
        .unwrap_or_else(|| parse_csi_params(params))
        .first()
        .copied()
        .unwrap_or(0)
}

fn csi_is_private(params: &[char]) -> bool {
    matches!(params, ['?', ..])
}

fn param_or_one(value: Option<u16>) -> u16 {
    match value {
        Some(0) | None => 1,
        Some(value) => value,
    }
}

fn append_semantic_zones_for_row(
    row: usize,
    cells: &[Cell],
    current_zone: &mut Option<SemanticZone>,
    zones: &mut Vec<SemanticZone>,
) {
    let Some(first_non_blank) = cells.iter().position(|cell| cell.ch != ' ') else {
        return;
    };
    let Some(last_non_blank) = cells.iter().rposition(|cell| cell.ch != ' ') else {
        return;
    };

    let mut start = first_non_blank;
    while start <= last_non_blank {
        let semantic_type = cells[start].semantic_type;
        let mut end = start;
        while end < last_non_blank && cells[end + 1].semantic_type == semantic_type {
            end += 1;
        }

        if cells[start..=end].iter().any(|cell| cell.ch != ' ') {
            append_semantic_zone(
                SemanticZone::new(row, start, row, end, semantic_type),
                current_zone,
                zones,
            );
        }

        start = end + 1;
    }
}

fn append_semantic_zone(
    zone: SemanticZone,
    current_zone: &mut Option<SemanticZone>,
    zones: &mut Vec<SemanticZone>,
) {
    if let Some(current) = current_zone.as_mut() {
        if current.semantic_type == zone.semantic_type {
            current.end_y = zone.end_y;
            current.end_x = zone.end_x;
            return;
        }
    }

    if let Some(current) = current_zone.replace(zone) {
        zones.push(current);
    }
}

fn trim_trailing_spaces(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
}

fn parse_csi_params(params: &[char]) -> Vec<u16> {
    let raw = params.iter().collect::<String>();
    raw.split(';')
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                part.parse::<u16>().unwrap_or(0)
            }
        })
        .collect()
}

fn parse_private_csi_params(params: &[char]) -> Option<Vec<u16>> {
    let ['?', rest @ ..] = params else {
        return None;
    };

    Some(parse_csi_params(rest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SgrParameter {
    Code(u16),
    UnderlineStyle(UnderlineStyle),
}

fn parse_sgr_params(params: &[char]) -> Vec<SgrParameter> {
    if params.is_empty() {
        return vec![SgrParameter::Code(0)];
    }

    let raw = params.iter().collect::<String>();
    let mut parsed = Vec::new();

    for group in raw.split(';') {
        if group.is_empty() {
            parsed.push(SgrParameter::Code(0));
            continue;
        }

        let values = group
            .split(':')
            .map(parse_sgr_numeric_part)
            .collect::<Vec<_>>();

        if group.contains(':') && values.first() == Some(&4) {
            let style = values.get(1).map_or(UnderlineStyle::None, |value| {
                underline_style_from_sgr(*value)
            });
            parsed.push(SgrParameter::UnderlineStyle(style));
        } else {
            parsed.extend(values.into_iter().map(SgrParameter::Code));
        }
    }

    parsed
}

fn parse_sgr_numeric_part(part: &str) -> u16 {
    if part.is_empty() {
        0
    } else {
        part.parse::<u16>().unwrap_or(0)
    }
}

const fn underline_style_from_sgr(style: u16) -> UnderlineStyle {
    match style {
        0 => UnderlineStyle::None,
        2 => UnderlineStyle::Double,
        3 => UnderlineStyle::Curly,
        4 => UnderlineStyle::Dotted,
        5 => UnderlineStyle::Dashed,
        _ => UnderlineStyle::Single,
    }
}

fn parse_extended_color(values: &[SgrParameter]) -> Option<(Color, usize)> {
    use SgrParameter::Code;

    match values {
        [Code(5), Code(index), ..] => Some((Color::Indexed(saturating_u8(*index)), 2)),
        [Code(2), Code(0), Code(red), Code(green), Code(blue), ..] => Some((
            Color::Rgb(
                saturating_u8(*red),
                saturating_u8(*green),
                saturating_u8(*blue),
            ),
            5,
        )),
        [Code(2), Code(red), Code(green), Code(blue), ..] => Some((
            Color::Rgb(
                saturating_u8(*red),
                saturating_u8(*green),
                saturating_u8(*blue),
            ),
            4,
        )),
        [
            Code(6),
            Code(0),
            Code(red),
            Code(green),
            Code(blue),
            Code(alpha),
            ..,
        ] => Some((
            Color::Rgba(
                saturating_u8(*red),
                saturating_u8(*green),
                saturating_u8(*blue),
                saturating_u8(*alpha),
            ),
            6,
        )),
        [Code(6), Code(red), Code(green), Code(blue), Code(alpha), ..] => Some((
            Color::Rgba(
                saturating_u8(*red),
                saturating_u8(*green),
                saturating_u8(*blue),
                saturating_u8(*alpha),
            ),
            5,
        )),
        _ => None,
    }
}

fn complete_utf8_prefix_len(bytes: &[u8]) -> usize {
    let Some(mut start) = bytes.len().checked_sub(1) else {
        return 0;
    };

    while start > 0 && is_utf8_continuation(bytes[start]) {
        start -= 1;
    }

    let first = bytes[start];
    let Some(expected_len) = utf8_sequence_len(first) else {
        return bytes.len();
    };

    let available_len = bytes.len() - start;
    if available_len >= expected_len {
        return bytes.len();
    }

    if bytes[start + 1..]
        .iter()
        .all(|byte| is_utf8_continuation(*byte))
    {
        start
    } else {
        bytes.len()
    }
}

fn decode_terminal_chars(bytes: &[u8]) -> Vec<char> {
    let mut chars = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if let Some(ch) = raw_c1_control(bytes[index]) {
            chars.push(ch);
            index += 1;
            continue;
        }

        match std::str::from_utf8(&bytes[index..]) {
            Ok(text) => {
                chars.extend(text.chars());
                break;
            }
            Err(error) if error.valid_up_to() > 0 => {
                let valid_end = index + error.valid_up_to();
                let text = std::str::from_utf8(&bytes[index..valid_end]).unwrap_or("");
                chars.extend(text.chars());
                index = valid_end;
            }
            Err(error) => {
                chars.push('\u{fffd}');
                index += error.error_len().unwrap_or(1);
            }
        }
    }

    chars
}

fn raw_c1_control(byte: u8) -> Option<char> {
    if (0x80..=0x9f).contains(&byte) {
        char::from_u32(u32::from(byte))
    } else {
        None
    }
}

fn utf8_sequence_len(byte: u8) -> Option<usize> {
    match byte {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

fn saturating_u8(value: u16) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

fn display_width(
    ch: char,
    treat_east_asian_ambiguous_width_as_wide: bool,
    cell_width_overrides: &[CellWidthOverride],
) -> u16 {
    if let Some(override_width) = cell_width_overrides
        .iter()
        .find(|override_width| override_width.contains(ch))
        .map(|override_width| override_width.width)
    {
        return override_width;
    }

    let width = if treat_east_asian_ambiguous_width_as_wide {
        UnicodeWidthChar::width_cjk(ch)
    } else {
        UnicodeWidthChar::width(ch)
    };
    match width {
        Some(0) => 0,
        Some(width) => u16_display_width(width),
        None => 1,
    }
}

fn presentation_sequence_width(
    ch: char,
    selector: char,
    treat_east_asian_ambiguous_width_as_wide: bool,
    cell_width_overrides: &[CellWidthOverride],
) -> u16 {
    if let Some(override_width) = cell_width_overrides
        .iter()
        .find(|override_width| override_width.contains(ch))
        .map(|override_width| override_width.width)
    {
        return override_width;
    }

    let mut sequence = String::with_capacity(ch.len_utf8() + selector.len_utf8());
    sequence.push(ch);
    sequence.push(selector);
    let width = if treat_east_asian_ambiguous_width_as_wide {
        sequence.as_str().width_cjk()
    } else {
        sequence.as_str().width()
    };
    u16_display_width(width)
}

fn u16_display_width(width: usize) -> u16 {
    if width > usize::from(u16::MAX) {
        u16::MAX
    } else {
        u16::try_from(width).unwrap_or(1)
    }
}

fn is_unicode_presentation_selector(ch: char) -> bool {
    matches!(ch, TEXT_PRESENTATION_SELECTOR | EMOJI_PRESENTATION_SELECTOR)
}

fn non_empty_unicode_version_label(label: &str) -> Option<String> {
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_owned())
}
