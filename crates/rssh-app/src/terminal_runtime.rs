use base64::{Engine, engine::general_purpose::STANDARD};
use rssh_core::{DamageRegion, TerminalSize};
use rssh_terminal::{Cell, Color, CursorShape, Terminal};

use crate::{
    terminal_modes::{MouseInputMode, TerminalModeTracker},
    visible_output::TerminalVisibleOutputFilter,
};

pub struct TerminalRuntime {
    terminal: Terminal,
    output_filter: TerminalOutputFilter,
    visible_output_filter: TerminalVisibleOutputFilter,
    mode_tracker: TerminalModeTracker,
    clipboard_tracker: TerminalClipboardTracker,
}

pub(crate) struct TerminalRuntimeOutput {
    pub(crate) responses: Vec<Vec<u8>>,
    pub(crate) display: Vec<u8>,
    pub(crate) damage: Vec<DamageRegion>,
    pub(crate) bells: u64,
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::new(size),
            output_filter: TerminalOutputFilter::new(size),
            visible_output_filter: TerminalVisibleOutputFilter::default(),
            mode_tracker: TerminalModeTracker::default(),
            clipboard_tracker: TerminalClipboardTracker::default(),
        }
    }

    #[cfg(test)]
    pub fn feed_pty_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.feed_pty_output_with_display(bytes).responses
    }

    pub(crate) fn feed_pty_output_with_display(&mut self, bytes: &[u8]) -> TerminalRuntimeOutput {
        self.clipboard_tracker.process(bytes);
        self.mode_tracker.process_without_emitting(bytes);
        let output = self.output_filter.process(bytes);

        let mut responses = Vec::new();
        let mut display_bytes = Vec::new();
        let mut damage = Vec::new();
        let mut bells = 0_u64;
        for event in output.events {
            match event {
                FilteredOutputEvent::Display(display) => {
                    self.terminal.feed(&display);
                    damage.extend(self.terminal.take_damage());
                    bells = bells.saturating_add(self.terminal.take_bell_count());
                    display_bytes.extend(self.visible_output_filter.process(&display));
                }
                FilteredOutputEvent::Response(response) => {
                    responses.push(self.output_filter.response_bytes(
                        response,
                        &self.terminal,
                        &self.mode_tracker,
                    ));
                }
            }
        }

        TerminalRuntimeOutput {
            responses,
            display: display_bytes,
            damage,
            bells,
        }
    }

    pub fn take_clipboard_texts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_tracker.texts)
    }

    pub fn take_clipboard_queries(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_tracker.queries)
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.terminal.resize(size);
        self.output_filter.resize(size);
    }

    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    #[must_use]
    pub fn application_cursor_keys(&self) -> bool {
        self.mode_tracker.application_cursor_keys()
    }

    #[must_use]
    pub fn focus_reporting(&self) -> bool {
        self.mode_tracker.focus_reporting()
    }

    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        self.mode_tracker.bracketed_paste()
    }

    #[must_use]
    pub fn application_keypad(&self) -> bool {
        self.mode_tracker.application_keypad()
    }

    #[must_use]
    pub fn mouse_input_mode(&self) -> MouseInputMode {
        self.mode_tracker.mouse_input_mode()
    }
}

struct TerminalOutputFilter {
    pending: Vec<u8>,
    size: TerminalSize,
    color_state: TerminalColorState,
}

struct FilteredOutput {
    events: Vec<FilteredOutputEvent>,
}

enum FilteredOutputEvent {
    Display(Vec<u8>),
    Response(TerminalResponse),
}

impl TerminalOutputFilter {
    const CELL_HEIGHT_PIXELS: u16 = 16;
    const CELL_WIDTH_PIXELS: u16 = 8;
    const RESPONSES: &'static [TerminalQueryResponse] = &[
        TerminalQueryResponse {
            query: b"\x1b[6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x9b6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x1b[?6n",
            response: TerminalResponse::CursorPosition { private: true },
        },
        TerminalQueryResponse {
            query: b"\x9b?6n",
            response: TerminalResponse::CursorPosition { private: true },
        },
        TerminalQueryResponse {
            query: b"\x1b[c",
            response: TerminalResponse::Static(b"\x1b[?1;2c"),
        },
        TerminalQueryResponse {
            query: b"\x9bc",
            response: TerminalResponse::Static(b"\x1b[?1;2c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[>c",
            response: TerminalResponse::Static(b"\x1b[>0;0;0c"),
        },
        TerminalQueryResponse {
            query: b"\x9b>c",
            response: TerminalResponse::Static(b"\x1b[>0;0;0c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x1b[>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x1b[5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x9b5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x1b[11t",
            response: TerminalResponse::WindowState,
        },
        TerminalQueryResponse {
            query: b"\x9b11t",
            response: TerminalResponse::WindowState,
        },
        TerminalQueryResponse {
            query: b"\x1b[14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x9b14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[13t",
            response: TerminalResponse::WindowPosition,
        },
        TerminalQueryResponse {
            query: b"\x9b13t",
            response: TerminalResponse::WindowPosition,
        },
        TerminalQueryResponse {
            query: b"\x1b[15t",
            response: TerminalResponse::ScreenPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x9b15t",
            response: TerminalResponse::ScreenPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x9b16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x9b18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[19t",
            response: TerminalResponse::ScreenSize,
        },
        TerminalQueryResponse {
            query: b"\x9b19t",
            response: TerminalResponse::ScreenSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[20t",
            response: TerminalResponse::IconLabel,
        },
        TerminalQueryResponse {
            query: b"\x9b20t",
            response: TerminalResponse::IconLabel,
        },
        TerminalQueryResponse {
            query: b"\x1b[21t",
            response: TerminalResponse::WindowTitle,
        },
        TerminalQueryResponse {
            query: b"\x9b21t",
            response: TerminalResponse::WindowTitle,
        },
    ];

    fn new(size: TerminalSize) -> Self {
        Self {
            pending: Vec::new(),
            size,
            color_state: TerminalColorState::default(),
        }
    }

    fn resize(&mut self, size: TerminalSize) {
        self.size = size;
    }

    fn process(&mut self, bytes: &[u8]) -> FilteredOutput {
        self.color_state.process(bytes);
        self.pending.extend_from_slice(bytes);

        let mut events = Vec::new();

        while let Some((index, response)) = self.find_next_response() {
            if index > 0 {
                events.push(FilteredOutputEvent::Display(self.pending[..index].to_vec()));
            }
            events.push(FilteredOutputEvent::Response(response.response));
            self.pending.drain(..index + response.consumed);
        }

        let retained = Self::suffix_len_matching_query_prefix(&self.pending);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            events.push(FilteredOutputEvent::Display(
                self.pending[..writable].to_vec(),
            ));
            self.pending.drain(..writable);
        }

        FilteredOutput { events }
    }

    fn find_next_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        let static_response = Self::RESPONSES
            .iter()
            .filter_map(|response| {
                find_subslice(&self.pending, response.query).map(|index| {
                    (
                        index,
                        MatchedTerminalResponse {
                            consumed: response.query.len(),
                            response: response.response.clone(),
                        },
                    )
                })
            })
            .min_by_key(|(index, _)| *index);
        let mode_response = find_private_mode_status_query(&self.pending).map(
            |PrivateModeStatusQuery {
                 index,
                 consumed,
                 mode,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::PrivateModeStatus(mode),
                    },
                )
            },
        );
        let osc_color_response = find_osc_color_query(&self.pending).map(
            |OscColorQuery {
                 index,
                 consumed,
                 query,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::OscColor(query),
                    },
                )
            },
        );
        let decrqss_response = find_decrqss_query(&self.pending).map(
            |DecrqssQuery {
                 index,
                 consumed,
                 response,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::Decrqss(response),
                    },
                )
            },
        );
        let xtgettcap_response = find_xtgettcap_query(&self.pending, self.size).map(
            |XtGetTcapQuery {
                 index,
                 consumed,
                 response,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::XtGetTcap(response),
                    },
                )
            },
        );

        static_response
            .into_iter()
            .chain(mode_response)
            .chain(osc_color_response)
            .chain(decrqss_response)
            .chain(xtgettcap_response)
            .filter(|(index, _)| !is_inside_osc_or_st_control_string(&self.pending, *index))
            .min_by_key(|(index, _)| *index)
    }

    fn suffix_len_matching_query_prefix(pending: &[u8]) -> usize {
        let static_query_suffix = Self::RESPONSES
            .iter()
            .map(|response| suffix_prefix_len(pending, response.query))
            .max()
            .unwrap_or(0);
        static_query_suffix
            .max(private_mode_status_query_suffix_len(pending))
            .max(osc_color_query_suffix_len(pending))
            .max(decrqss_query_suffix_len(pending))
            .max(xtgettcap_query_suffix_len(pending))
            .max(incomplete_osc_control_sequence_suffix_len(pending))
            .max(incomplete_st_control_sequence_suffix_len(pending))
    }

    fn response_bytes(
        &self,
        response: TerminalResponse,
        terminal: &Terminal,
        modes: &TerminalModeTracker,
    ) -> Vec<u8> {
        response.response_bytes(self.size, terminal, modes, &self.color_state)
    }
}

struct TerminalQueryResponse {
    query: &'static [u8],
    response: TerminalResponse,
}

struct MatchedTerminalResponse {
    consumed: usize,
    response: TerminalResponse,
}

#[derive(Clone)]
enum TerminalResponse {
    Static(&'static [u8]),
    CursorPosition { private: bool },
    WindowState,
    WindowPixelSize,
    WindowPosition,
    ScreenPixelSize,
    CharacterCellSize,
    TextAreaSize,
    ScreenSize,
    IconLabel,
    WindowTitle,
    PrivateModeStatus(u16),
    OscColor(OscColorResponse),
    Decrqss(DecrqssResponse),
    XtGetTcap(XtGetTcapResponse),
    XtVersion,
}

impl TerminalResponse {
    fn response_bytes(
        self,
        size: TerminalSize,
        terminal: &Terminal,
        modes: &TerminalModeTracker,
        color_state: &TerminalColorState,
    ) -> Vec<u8> {
        match self {
            TerminalResponse::Static(bytes) => bytes.to_vec(),
            TerminalResponse::CursorPosition { private } => {
                let (row, column) = terminal.cursor();
                if private {
                    format!(
                        "\x1b[?{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                } else {
                    format!(
                        "\x1b[{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                }
            }
            TerminalResponse::WindowState => b"\x1b[1t".to_vec(),
            TerminalResponse::WindowPixelSize => format!(
                "\x1b[4;{};{}t",
                u32::from(size.rows) * u32::from(TerminalOutputFilter::CELL_HEIGHT_PIXELS),
                u32::from(size.columns) * u32::from(TerminalOutputFilter::CELL_WIDTH_PIXELS)
            )
            .into_bytes(),
            TerminalResponse::WindowPosition => b"\x1b[3;0;0t".to_vec(),
            TerminalResponse::ScreenPixelSize => format!(
                "\x1b[5;{};{}t",
                u32::from(size.rows) * u32::from(TerminalOutputFilter::CELL_HEIGHT_PIXELS),
                u32::from(size.columns) * u32::from(TerminalOutputFilter::CELL_WIDTH_PIXELS)
            )
            .into_bytes(),
            TerminalResponse::CharacterCellSize => format!(
                "\x1b[6;{};{}t",
                TerminalOutputFilter::CELL_HEIGHT_PIXELS,
                TerminalOutputFilter::CELL_WIDTH_PIXELS
            )
            .into_bytes(),
            TerminalResponse::TextAreaSize => {
                format!("\x1b[8;{};{}t", size.rows, size.columns).into_bytes()
            }
            TerminalResponse::ScreenSize => {
                format!("\x1b[9;{};{}t", size.rows, size.columns).into_bytes()
            }
            TerminalResponse::IconLabel => osc_title_response(b'L', terminal.title()),
            TerminalResponse::WindowTitle => osc_title_response(b'l', terminal.title()),
            TerminalResponse::PrivateModeStatus(mode) => {
                format!("\x1b[?{};{}$y", mode, modes.private_mode_report_value(mode)).into_bytes()
            }
            TerminalResponse::OscColor(query) => color_state.response(query),
            TerminalResponse::Decrqss(query) => query.response(terminal),
            TerminalResponse::XtGetTcap(query) => query.response(),
            TerminalResponse::XtVersion => xtversion_response(),
        }
    }
}

fn xtversion_response() -> Vec<u8> {
    format!("\x1bP>|R-SSH {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes()
}

fn osc_title_response(kind: u8, title: Option<&str>) -> Vec<u8> {
    let mut response = Vec::from([0x1b, b']', kind]);
    response.extend(
        title
            .unwrap_or_default()
            .bytes()
            .filter(|byte| !matches!(byte, 0x00..=0x1f | 0x7f)),
    );
    response.extend_from_slice(b"\x1b\\");
    response
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_inside_osc_or_st_control_string(bytes: &[u8], index: usize) -> bool {
    is_inside_control_string(bytes, index, find_next_osc_start, find_osc_color_terminator)
        || is_inside_control_string(
            bytes,
            index,
            find_next_st_control_string_start,
            find_xtgettcap_terminator,
        )
}

fn is_inside_control_string(
    bytes: &[u8],
    index: usize,
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<OscColorTerminator>,
) -> bool {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_start, prefix_len)) = find_next_start(&bytes[offset..]) else {
            return false;
        };
        let start = offset + relative_start;
        if start >= index {
            return false;
        }

        let content_start = start + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return true;
        };
        let end = content_start + terminator.index + terminator.length;
        if index < end {
            return true;
        }
        offset = end;
    }

    false
}

fn incomplete_osc_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_control_sequence_start(bytes, find_next_osc_start, find_osc_color_terminator)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_prefix_len(bytes, b"\x1b]"))
}

fn incomplete_st_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_control_sequence_start(
        bytes,
        find_next_st_control_string_start,
        find_xtgettcap_terminator,
    )
    .map_or(0, |start| bytes.len() - start)
    .max(
        [
            b"\x1bP".as_slice(),
            b"\x1bX".as_slice(),
            b"\x1b^".as_slice(),
            b"\x1b_".as_slice(),
        ]
        .into_iter()
        .map(|prefix| suffix_prefix_len(bytes, prefix))
        .max()
        .unwrap_or(0),
    )
}

fn find_incomplete_control_sequence_start(
    bytes: &[u8],
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<OscColorTerminator>,
) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_index, prefix_len)) = find_next_start(&bytes[offset..]) else {
            break;
        };
        let index = offset + relative_index;
        let content_start = index + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return Some(index);
        };
        offset = content_start + terminator.index + terminator.length;
    }

    None
}

struct DecrqssQuery {
    index: usize,
    consumed: usize,
    response: DecrqssResponse,
}

#[derive(Clone)]
struct DecrqssResponse {
    kind: Option<DecrqssKind>,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum DecrqssKind {
    Sgr,
    CursorShape,
    ScrollRegion,
}

impl DecrqssResponse {
    fn response(&self, terminal: &Terminal) -> Vec<u8> {
        let mut response = if let Some(kind) = self.kind {
            let mut bytes = b"\x1bP1$r".to_vec();
            match kind {
                DecrqssKind::Sgr => append_sgr_state(terminal.active_style(), &mut bytes),
                DecrqssKind::CursorShape => {
                    append_cursor_shape_state(terminal.cursor_shape(), &mut bytes);
                }
                DecrqssKind::ScrollRegion => {
                    append_scroll_region_state(terminal.scroll_region(), &mut bytes);
                }
            }
            bytes
        } else {
            b"\x1bP0$r".to_vec()
        };
        response.extend_from_slice(self.terminator.bytes());
        response
    }
}

fn find_decrqss_query(bytes: &[u8]) -> Option<DecrqssQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1bP".as_slice(), 2), (b"\x90".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_decrqss_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &DecrqssQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_decrqss_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<DecrqssQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"$q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];

    Some(DecrqssQuery {
        index,
        consumed: prefix_len + b"$q".len() + terminator.index + terminator.length,
        response: DecrqssResponse {
            kind: parse_decrqss_kind(content),
            terminator: terminator.response_terminator,
        },
    })
}

fn parse_decrqss_kind(content: &[u8]) -> Option<DecrqssKind> {
    match content {
        b"m" => Some(DecrqssKind::Sgr),
        b" q" => Some(DecrqssKind::CursorShape),
        b"r" => Some(DecrqssKind::ScrollRegion),
        _ => None,
    }
}

fn append_sgr_state(style: &Cell, bytes: &mut Vec<u8>) {
    let mut params = Vec::new();
    if style.bold {
        params.push("1".to_owned());
    }
    if style.italic {
        params.push("3".to_owned());
    }
    if style.underline {
        params.push("4".to_owned());
    }
    if style.inverse {
        params.push("7".to_owned());
    }
    append_color_sgr(38, style.foreground, &mut params);
    append_color_sgr(48, style.background, &mut params);

    if params.is_empty() {
        bytes.push(b'0');
    } else {
        bytes.extend_from_slice(params.join(";").as_bytes());
    }
    bytes.push(b'm');
}

fn append_color_sgr(prefix: u8, color: Color, params: &mut Vec<String>) {
    match color {
        Color::Default => {}
        Color::Indexed(index) => {
            params.push(prefix.to_string());
            params.push("5".to_owned());
            params.push(index.to_string());
        }
        Color::Rgb(red, green, blue) => {
            params.push(prefix.to_string());
            params.push("2".to_owned());
            params.push(red.to_string());
            params.push(green.to_string());
            params.push(blue.to_string());
        }
    }
}

fn append_cursor_shape_state(shape: CursorShape, bytes: &mut Vec<u8>) {
    let value = match shape {
        CursorShape::Block => 2,
        CursorShape::Underline => 3,
        CursorShape::Bar => 5,
    };
    bytes.extend_from_slice(value.to_string().as_bytes());
    bytes.extend_from_slice(b" q");
}

fn append_scroll_region_state((top, bottom): (u16, u16), bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(top.saturating_add(1).to_string().as_bytes());
    bytes.push(b';');
    bytes.extend_from_slice(bottom.saturating_add(1).to_string().as_bytes());
    bytes.push(b'r');
}

fn decrqss_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_decrqss_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_decrqss_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes) || b"\x90".starts_with(bytes);
    };
    if !b"$q".starts_with(rest) && !rest.starts_with(b"$q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"$q") {
        return [b"m".as_slice(), b" q".as_slice(), b"r".as_slice()]
            .into_iter()
            .any(|target| target.starts_with(body));
    }
    true
}

struct XtGetTcapQuery {
    index: usize,
    consumed: usize,
    response: XtGetTcapResponse,
}

#[derive(Clone)]
struct XtGetTcapResponse {
    entries: Vec<XtGetTcapEntry>,
    terminator: OscResponseTerminator,
}

#[derive(Clone)]
struct XtGetTcapEntry {
    name_hex: Vec<u8>,
    value_hex: Vec<u8>,
}

impl XtGetTcapResponse {
    fn response(&self) -> Vec<u8> {
        let mut response = if self.entries.is_empty() {
            b"\x1bP0+r".to_vec()
        } else {
            let mut bytes = b"\x1bP1+r".to_vec();
            for (index, entry) in self.entries.iter().enumerate() {
                if index > 0 {
                    bytes.push(b';');
                }
                bytes.extend_from_slice(&entry.name_hex);
                bytes.push(b'=');
                bytes.extend_from_slice(&entry.value_hex);
            }
            bytes
        };
        response.extend_from_slice(self.terminator.bytes());
        response
    }
}

fn find_xtgettcap_query(bytes: &[u8], size: TerminalSize) -> Option<XtGetTcapQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1bP".as_slice(), 2), (b"\x90".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_xtgettcap_query(bytes, index, prefix_len, size) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &XtGetTcapQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_xtgettcap_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
    size: TerminalSize,
) -> Option<XtGetTcapQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"+q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];
    let entries = content
        .split(|byte| *byte == b';')
        .filter_map(|entry| parse_xtgettcap_entry(entry, size))
        .collect();

    Some(XtGetTcapQuery {
        index,
        consumed: prefix_len + b"+q".len() + terminator.index + terminator.length,
        response: XtGetTcapResponse {
            entries,
            terminator: terminator.response_terminator,
        },
    })
}

fn find_xtgettcap_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });

    [st, c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_xtgettcap_entry(name_hex: &[u8], size: TerminalSize) -> Option<XtGetTcapEntry> {
    let name = decode_ascii_hex(name_hex)?;
    let value_hex = xtgettcap_value_hex(&name, size)?;
    Some(XtGetTcapEntry {
        name_hex: name_hex.to_vec(),
        value_hex,
    })
}

fn xtgettcap_value_hex(name: &[u8], size: TerminalSize) -> Option<Vec<u8>> {
    match name {
        b"Co" | b"colors" => Some(b"323536".to_vec()),
        b"TN" => Some(b"787465726d2d323536636f6c6f72".to_vec()),
        b"RGB" => Some(b"524742".to_vec()),
        b"Ms" => Some(b"1b5d35323b25703125733b257032257307".to_vec()),
        b"co" => Some(decimal_value_hex(size.columns)),
        b"li" => Some(decimal_value_hex(size.rows)),
        _ => None,
    }
}

fn decimal_value_hex(value: u16) -> Vec<u8> {
    encode_ascii_hex(value.to_string().as_bytes())
}

fn encode_ascii_hex(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)]);
        encoded.push(HEX[usize::from(byte & 0x0f)]);
    }
    encoded
}

fn decode_ascii_hex(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(parse_hex_digit(pair[0])? * 16 + parse_hex_digit(pair[1])?))
        .collect()
}

fn xtgettcap_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_xtgettcap_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_xtgettcap_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes) || b"\x90".starts_with(bytes);
    };
    if !b"+q".starts_with(rest) && !rest.starts_with(b"+q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"+q") {
        return body
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b';');
    }
    true
}

struct OscColorQuery {
    index: usize,
    consumed: usize,
    query: OscColorResponse,
}

#[derive(Clone, Copy)]
struct OscColorResponse {
    kind: OscColorKind,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum OscColorKind {
    DefaultForeground,
    DefaultBackground,
    Palette(u8),
}

#[derive(Clone, Copy)]
enum OscResponseTerminator {
    Bel,
    St,
    C1St,
}

struct OscColorTerminator {
    index: usize,
    length: usize,
    response_terminator: OscResponseTerminator,
}

fn find_osc_color_query(bytes: &[u8]) -> Option<OscColorQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [(b"\x1b]".as_slice(), 2), (b"\x9d".as_slice(), 1)] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_osc_color_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &OscColorQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_osc_color_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<OscColorQuery> {
    let content_start = index + prefix_len;
    let terminator = find_osc_color_terminator(&bytes[content_start..])?;
    let content_end = content_start + terminator.index;
    let kind = parse_osc_color_query_content(&bytes[content_start..content_end])?;

    Some(OscColorQuery {
        index,
        consumed: content_end + terminator.length - index,
        query: OscColorResponse {
            kind,
            terminator: terminator.response_terminator,
        },
    })
}

fn find_osc_color_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let bel = bytes
        .iter()
        .position(|byte| *byte == b'\x07')
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::Bel,
        });
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });

    [bel, st, c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_osc_color_query_content(content: &[u8]) -> Option<OscColorKind> {
    match content {
        b"10;?" => Some(OscColorKind::DefaultForeground),
        b"11;?" => Some(OscColorKind::DefaultBackground),
        _ => parse_palette_color_query(content),
    }
}

fn parse_palette_color_query(content: &[u8]) -> Option<OscColorKind> {
    let rest = content.strip_prefix(b"4;")?;
    let separator = rest.iter().position(|byte| *byte == b';')?;
    if &rest[separator + 1..] != b"?" {
        return None;
    }
    let index = parse_u8_decimal(&rest[..separator])?;
    Some(OscColorKind::Palette(index))
}

fn parse_u8_decimal(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0u16;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(u16::from(*byte - b'0'));
    }
    u8::try_from(value).ok()
}

fn osc_color_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_osc_color_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_osc_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b]")
        .or_else(|| bytes.strip_prefix(b"\x9d"))
    else {
        return b"\x1b]".starts_with(bytes) || b"\x9d".starts_with(bytes);
    };

    b"10;?".starts_with(rest) || b"11;?".starts_with(rest) || is_palette_color_query_prefix(rest)
}

fn is_palette_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"4") else {
        return bytes.is_empty();
    };
    let Some(rest) = rest.strip_prefix(b";") else {
        return rest.is_empty();
    };
    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b";" || tail == b";?"
}

struct TerminalColorState {
    foreground: [u8; 3],
    background: [u8; 3],
    palette_overrides: Vec<(u8, [u8; 3])>,
    pending: Vec<u8>,
}

impl Default for TerminalColorState {
    fn default() -> Self {
        Self {
            foreground: DEFAULT_FOREGROUND,
            background: DEFAULT_BACKGROUND,
            palette_overrides: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl TerminalColorState {
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some((index, prefix_len)) = find_next_osc_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if is_inside_osc_or_st_control_string(&self.pending, index) {
                self.pending.drain(..index.saturating_add(1));
                continue;
            }
            if index > 0 {
                self.pending.drain(..index);
            }

            let content_start = prefix_len;
            let Some(terminator) = find_osc_color_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            if let Some(change) = parse_osc_color_change(&self.pending[content_start..content_end])
            {
                self.apply(change);
            }
            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn response(&self, query: OscColorResponse) -> Vec<u8> {
        let mut response = match query.kind {
            OscColorKind::DefaultForeground => {
                format!("\x1b]10;{}", rgb_response(self.foreground)).into_bytes()
            }
            OscColorKind::DefaultBackground => {
                format!("\x1b]11;{}", rgb_response(self.background)).into_bytes()
            }
            OscColorKind::Palette(index) => format!(
                "\x1b]4;{};{}",
                index,
                rgb_response(self.palette_color(index))
            )
            .into_bytes(),
        };
        response.extend_from_slice(query.terminator.bytes());
        response
    }

    fn apply(&mut self, change: OscColorChange) {
        match change {
            OscColorChange::DefaultForeground(color) => self.foreground = color,
            OscColorChange::DefaultBackground(color) => self.background = color,
            OscColorChange::Palette(index, color) => {
                if let Some((_, existing)) = self
                    .palette_overrides
                    .iter_mut()
                    .find(|(palette_index, _)| *palette_index == index)
                {
                    *existing = color;
                } else {
                    self.palette_overrides.push((index, color));
                }
            }
        }
    }

    fn palette_color(&self, index: u8) -> [u8; 3] {
        self.palette_overrides
            .iter()
            .find_map(|(palette_index, color)| (*palette_index == index).then_some(*color))
            .unwrap_or_else(|| indexed_color(index))
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [b"\x1b]".as_slice(), b"\x9d".as_slice()]
            .into_iter()
            .map(|prefix| suffix_prefix_len(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Clone, Copy)]
enum OscColorChange {
    DefaultForeground([u8; 3]),
    DefaultBackground([u8; 3]),
    Palette(u8, [u8; 3]),
}

fn find_next_osc_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [(b"\x1b]".as_slice(), 2), (b"\x9d".as_slice(), 1)]
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            find_subslice(bytes, prefix).map(|index| (index, prefix_len))
        })
        .min_by_key(|(index, _)| *index)
}

fn find_next_st_control_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [
        (b"\x1bP".as_slice(), 2),
        (b"\x1bX".as_slice(), 2),
        (b"\x1b^".as_slice(), 2),
        (b"\x1b_".as_slice(), 2),
        (b"\x90".as_slice(), 1),
        (b"\x98".as_slice(), 1),
        (b"\x9e".as_slice(), 1),
        (b"\x9f".as_slice(), 1),
    ]
    .into_iter()
    .filter_map(|(prefix, prefix_len)| {
        find_subslice(bytes, prefix).map(|index| (index, prefix_len))
    })
    .min_by_key(|(index, _)| *index)
}

fn parse_osc_color_change(content: &[u8]) -> Option<OscColorChange> {
    if let Some(color) = content.strip_prefix(b"10;").and_then(parse_rgb_color_spec) {
        return Some(OscColorChange::DefaultForeground(color));
    }
    if let Some(color) = content.strip_prefix(b"11;").and_then(parse_rgb_color_spec) {
        return Some(OscColorChange::DefaultBackground(color));
    }
    parse_palette_color_change(content)
}

fn parse_palette_color_change(content: &[u8]) -> Option<OscColorChange> {
    let rest = content.strip_prefix(b"4;")?;
    let separator = rest.iter().position(|byte| *byte == b';')?;
    let index = parse_u8_decimal(&rest[..separator])?;
    let color = parse_rgb_color_spec(&rest[separator + 1..])?;
    Some(OscColorChange::Palette(index, color))
}

fn parse_rgb_color_spec(value: &[u8]) -> Option<[u8; 3]> {
    let rest = value.strip_prefix(b"rgb:")?;
    let mut components = rest.split(|byte| *byte == b'/');
    let red = parse_rgb_component(components.next()?)?;
    let green = parse_rgb_component(components.next()?)?;
    let blue = parse_rgb_component(components.next()?)?;
    components.next().is_none().then_some([red, green, blue])
}

fn parse_rgb_component(component: &[u8]) -> Option<u8> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| value * 17),
        2..=4 => parse_hex_byte(&component[..2]),
        _ => None,
    }
}

fn parse_hex_byte(bytes: &[u8]) -> Option<u8> {
    Some(parse_hex_digit(bytes[0])? * 16 + parse_hex_digit(bytes[1])?)
}

fn parse_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl OscResponseTerminator {
    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Bel => b"\x07",
            Self::St => b"\x1b\\",
            Self::C1St => b"\x9c",
        }
    }
}

const DEFAULT_FOREGROUND: [u8; 3] = [229, 229, 229];
const DEFAULT_BACKGROUND: [u8; 3] = [12, 12, 12];

fn rgb_response(color: [u8; 3]) -> String {
    format!(
        "rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}",
        color[0], color[1], color[2]
    )
}

fn indexed_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    if let Some(color) = ANSI.get(usize::from(index)) {
        return *color;
    }

    if (16..=231).contains(&index) {
        let cube_index = index - 16;
        return [
            xterm_color_cube_intensity(cube_index / 36),
            xterm_color_cube_intensity((cube_index / 6) % 6),
            xterm_color_cube_intensity(cube_index % 6),
        ];
    }

    let level = 8 + (index - 232) * 10;
    [level, level, level]
}

const fn xterm_color_cube_intensity(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

struct PrivateModeStatusQuery {
    index: usize,
    consumed: usize,
    mode: u16,
}

fn find_private_mode_status_query(bytes: &[u8]) -> Option<PrivateModeStatusQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [
        (b"\x1b[?".as_slice(), b"\x1b[?".len()),
        (b"\x9b?".as_slice(), b"\x9b?".len()),
    ] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_private_mode_status_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &PrivateModeStatusQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_private_mode_status_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<PrivateModeStatusQuery> {
    let mut cursor = index + prefix_len;
    let start = cursor;
    let mut mode = 0u16;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        mode = mode
            .saturating_mul(10)
            .saturating_add(u16::from(bytes[cursor] - b'0'));
        cursor += 1;
    }
    if cursor == start || bytes.get(cursor..cursor + 2) != Some(b"$p") {
        return None;
    }
    Some(PrivateModeStatusQuery {
        index,
        consumed: cursor + 2 - index,
        mode,
    })
}

fn private_mode_status_query_suffix_len(bytes: &[u8]) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| is_private_mode_status_query_prefix(&bytes[bytes.len() - length..]))
        .unwrap_or(0)
}

fn is_private_mode_status_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[?")
        .or_else(|| bytes.strip_prefix(b"\x9b?"))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x1b[?".starts_with(bytes)
            || b"\x9b?".starts_with(bytes);
    };

    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b"$"
}

fn suffix_prefix_len(bytes: &[u8], prefix: &[u8]) -> usize {
    let max_len = bytes.len().min(prefix.len().saturating_sub(1));

    (1..=max_len)
        .rev()
        .find(|&length| bytes[bytes.len() - length..] == prefix[..length])
        .unwrap_or(0)
}

#[derive(Default)]
struct TerminalClipboardTracker {
    pending: Vec<u8>,
    texts: Vec<String>,
    queries: Vec<String>,
}

impl TerminalClipboardTracker {
    const OSC52_PREFIXES: &'static [&'static [u8]] = &[b"\x1b]52;", b"\x9d52;"];
    const ST_TERMINATOR: &'static [u8] = b"\x1b\\";
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some((start, prefix_len)) = find_next_osc52_clipboard_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if is_inside_osc_or_st_control_string(&self.pending, start) {
                self.pending.drain(..start.saturating_add(1));
                continue;
            }
            if start > 0 {
                self.pending.drain(..start);
            }

            let content_start = prefix_len;
            let Some(terminator) = find_osc_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            match parse_osc52_clipboard_content(&self.pending[content_start..content_end]) {
                Some(ClipboardSequence::Write(text)) => self.texts.push(text),
                Some(ClipboardSequence::Query(selection)) => self.queries.push(selection),
                None => {}
            }

            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = Self::OSC52_PREFIXES
            .iter()
            .map(|prefix| suffix_prefix_len(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

fn find_next_osc52_clipboard_start(bytes: &[u8]) -> Option<(usize, usize)> {
    TerminalClipboardTracker::OSC52_PREFIXES
        .iter()
        .filter_map(|prefix| find_subslice(bytes, prefix).map(|index| (index, prefix.len())))
        .min_by_key(|(index, _)| *index)
}

struct OscTerminator {
    index: usize,
    length: usize,
}

fn find_osc_terminator(bytes: &[u8]) -> Option<OscTerminator> {
    [
        bytes
            .iter()
            .position(|byte| *byte == b'\x07')
            .map(|index| OscTerminator { index, length: 1 }),
        find_subslice(bytes, TerminalClipboardTracker::ST_TERMINATOR).map(|index| OscTerminator {
            index,
            length: TerminalClipboardTracker::ST_TERMINATOR.len(),
        }),
        bytes
            .iter()
            .position(|byte| *byte == 0x9c)
            .map(|index| OscTerminator { index, length: 1 }),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|terminator| terminator.index)
}

enum ClipboardSequence {
    Write(String),
    Query(String),
}

fn parse_osc52_clipboard_content(content: &[u8]) -> Option<ClipboardSequence> {
    let separator = content.iter().position(|byte| *byte == b';')?;
    let selection = String::from_utf8(content[..separator].to_vec()).ok()?;
    let payload = &content[separator + 1..];
    if payload == b"?" {
        return Some(ClipboardSequence::Query(selection));
    }

    let decoded = STANDARD.decode(payload).ok()?;
    let text = String::from_utf8(decoded).ok()?;

    Some(ClipboardSequence::Write(text))
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use crate::terminal_modes::{MouseInputMode, MouseProtocolMode, MouseReportingMode};

    use super::TerminalRuntime;

    #[test]
    fn feeds_plain_pty_output_into_terminal_grid() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let responses = runtime.feed_pty_output(b"abc");

        assert!(responses.is_empty());
        assert_eq!(runtime.terminal().grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(runtime.terminal().grid().get(0, 1).unwrap().ch, 'b');
        assert_eq!(runtime.terminal().grid().get(0, 2).unwrap().ch, 'c');
    }

    #[test]
    fn reports_bell_events_without_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let output = runtime.feed_pty_output_with_display(b"ab\x07cd\x07");

        assert!(output.responses.is_empty());
        assert_eq!(output.bells, 2);
        assert_eq!(output.display, b"abcd");
        assert_eq!(terminal_text(&runtime), "abcd                ");
    }

    #[test]
    fn reports_damage_regions_from_terminal_feed() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let output = runtime.feed_pty_output_with_display(b"abc");

        assert_eq!(
            output.damage,
            vec![rssh_core::DamageRegion::new(0, 0, 3, 1)]
        );
    }

    #[test]
    fn omits_osc_title_from_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b]0;ops\x07after");

        assert!(output.responses.is_empty());
        assert_eq!(runtime.terminal().title(), Some("ops"));
        assert_eq!(output.display, b"beforeafter");
        assert!(terminal_text(&runtime).contains("beforeafter"));
    }

    #[test]
    fn omits_split_osc_title_from_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1b]0;op");
        let second = runtime.feed_pty_output_with_display(b"s\x07after");

        assert_eq!(first.display, b"before");
        assert_eq!(second.display, b"after");
        assert_eq!(runtime.terminal().title(), Some("ops"));
        assert!(terminal_text(&runtime).contains("beforeafter"));
    }

    #[test]
    fn tracks_c1_osc8_hyperlinks_without_displaying_control_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output =
            runtime.feed_pty_output_with_display(b"a\x9d8;;https://example.com\x9cbc\x9d8;;\x9cd");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"abcd");
        assert_eq!(
            runtime
                .terminal()
                .grid()
                .get(0, 1)
                .unwrap()
                .hyperlink
                .as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            runtime
                .terminal()
                .grid()
                .get(0, 2)
                .unwrap()
                .hyperlink
                .as_deref(),
            Some("https://example.com")
        );
        assert_eq!(runtime.terminal().grid().get(0, 0).unwrap().hyperlink, None);
        assert_eq!(runtime.terminal().grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn ignores_queries_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b]0;title \x1b[6n\x07after");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn holds_split_queries_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1b]0;title \x1b[");
        let second = runtime.feed_pty_output_with_display(b"6n\x07after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert!(second.responses.is_empty());
        assert_eq!(second.display, b"after");
    }

    #[test]
    fn holds_split_queries_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1bPpayload \x1b[");
        let second = runtime.feed_pty_output_with_display(b"6n\x1b\\after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert!(second.responses.is_empty());
        assert_eq!(second.display, b"after");
    }

    #[test]
    fn answers_cursor_position_query_without_feeding_it_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output(b"before\x1b[");
        let second = runtime.feed_pty_output(b"6nafter");

        assert!(first.is_empty());
        assert_eq!(second, vec![b"\x1b[1;7R".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[6n"));
    }

    #[test]
    fn answers_cursor_position_query_with_current_cursor() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"abc\x1b[6n");

        assert_eq!(responses, vec![b"\x1b[1;4R".to_vec()]);
        assert!(terminal_text(&runtime).contains("abc"));
    }

    #[test]
    fn answers_c1_cursor_position_query_without_feeding_it_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"abc\x9b6n");

        assert_eq!(responses, vec![b"\x1b[1;4R".to_vec()]);
        assert!(terminal_text(&runtime).contains("abc"));
        assert!(!terminal_text(&runtime).contains("6n"));
    }

    #[test]
    fn answers_private_cursor_position_query_with_current_cursor() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"abc\x1b[?6n");

        assert_eq!(responses, vec![b"\x1b[?1;4R".to_vec()]);
        assert!(terminal_text(&runtime).contains("abc"));
    }

    #[test]
    fn answers_device_and_status_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"a\x1b[c b\x1b[>c c\x1b[5n d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;2c".to_vec(),
                b"\x1b[>0;0;0c".to_vec(),
                b"\x1b[0n".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("[>c"));
        assert!(!text.contains("[5n"));
    }

    #[test]
    fn answers_c1_device_and_status_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"a\x9bc b\x9b>c c\x9b5n d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;2c".to_vec(),
                b"\x1b[>0;0;0c".to_vec(),
                b"\x1b[0n".to_vec()
            ]
        );
        assert!(terminal_text(&runtime).contains("a b c d"));
    }

    #[test]
    fn answers_text_area_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[18tafter");

        assert_eq!(responses, vec![b"\x1b[8;43;132t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[18t"));
    }

    #[test]
    fn answers_window_pixel_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[14tafter");

        assert_eq!(responses, vec![b"\x1b[4;688;1056t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[14t"));
    }

    #[test]
    fn answers_window_position_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[13tafter");

        assert_eq!(responses, vec![b"\x1b[3;0;0t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[13t"));
    }

    #[test]
    fn answers_screen_pixel_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[15tafter");

        assert_eq!(responses, vec![b"\x1b[5;688;1056t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[15t"));
    }

    #[test]
    fn answers_character_cell_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[16tafter");

        assert_eq!(responses, vec![b"\x1b[6;16;8t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[16t"));
    }

    #[test]
    fn answers_screen_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[19tafter");

        assert_eq!(responses, vec![b"\x1b[9;43;132t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[19t"));
    }

    #[test]
    fn answers_window_state_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[11tafter");

        assert_eq!(responses, vec![b"\x1b[1t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[11t"));
    }

    #[test]
    fn answers_window_title_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses =
            runtime.feed_pty_output(b"\x1b]0;ops\x07before\x1b[20t middle\x1b[21tafter");

        assert_eq!(
            responses,
            vec![b"\x1b]Lops\x1b\\".to_vec(), b"\x1b]lops\x1b\\".to_vec()]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before middleafter"));
        assert!(!text.contains("[20t"));
        assert!(!text.contains("[21t"));
    }

    #[test]
    fn answers_c1_terminal_size_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let text_area = runtime.feed_pty_output(b"\x9b18t");
        let screen = runtime.feed_pty_output(b"\x9b19t");

        assert_eq!(text_area, vec![b"\x1b[8;43;132t".to_vec()]);
        assert_eq!(screen, vec![b"\x1b[9;43;132t".to_vec()]);
    }

    #[test]
    fn answers_c1_window_pixel_and_cell_size_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let window_pixels = runtime.feed_pty_output(b"\x9b14t");
        let cell_pixels = runtime.feed_pty_output(b"\x9b16t");

        assert_eq!(window_pixels, vec![b"\x1b[4;688;1056t".to_vec()]);
        assert_eq!(cell_pixels, vec![b"\x1b[6;16;8t".to_vec()]);
    }

    #[test]
    fn answers_c1_window_position_and_screen_pixel_size_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let window_position = runtime.feed_pty_output(b"\x9b13t");
        let screen_pixels = runtime.feed_pty_output(b"\x9b15t");

        assert_eq!(window_position, vec![b"\x1b[3;0;0t".to_vec()]);
        assert_eq!(screen_pixels, vec![b"\x1b[5;688;1056t".to_vec()]);
    }

    #[test]
    fn answers_c1_window_state_and_title_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        runtime.feed_pty_output(b"\x1b]0;ops\x07");
        let state = runtime.feed_pty_output(b"\x9b11t");
        let icon = runtime.feed_pty_output(b"\x9b20t");
        let title = runtime.feed_pty_output(b"\x9b21t");

        assert_eq!(state, vec![b"\x1b[1t".to_vec()]);
        assert_eq!(icon, vec![b"\x1b]Lops\x1b\\".to_vec()]);
        assert_eq!(title, vec![b"\x1b]lops\x1b\\".to_vec()]);
    }

    #[test]
    fn answers_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime
            .feed_pty_output(b"before\x1b[?1h\x1b[?1$p middle\x1b[?1004$p after\x1b[?9999$p");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;1$y".to_vec(),
                b"\x1b[?1004;2$y".to_vec(),
                b"\x1b[?9999;0$y".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle after"));
        assert!(!text.contains("$p"));
    }

    #[test]
    fn answers_osc_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]10;?\x07 middle\x1b]11;?\x1b\\ after\x1b]4;1;?\x07done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec(),
                b"\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\".to_vec(),
                b"\x1b]4;1;rgb:cdcd/3131/3131\x07".to_vec()
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle afterdone"));
        assert!(!text.contains("10;?"));
        assert!(!text.contains("11;?"));
        assert!(!text.contains("4;1;?"));
    }

    #[test]
    fn answers_c1_osc_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9d4;196;?\x9c");

        assert_eq!(
            responses,
            vec![b"\x1b]4;196;rgb:ffff/0000/0000\x9c".to_vec()]
        );
    }

    #[test]
    fn answers_osc_color_queries_after_color_changes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]10;rgb:11/22/33\x07 middle\x1b]10;?\x07 after\x1b]4;1;rgb:01/02/03\x1b\\ done\x1b]4;1;?\x1b\\",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:1111/2222/3333\x07".to_vec(),
                b"\x1b]4;1;rgb:0101/0202/0303\x1b\\".to_vec()
            ]
        );
        assert_eq!(output.display, b"before middle after done");
    }

    #[test]
    fn ignores_osc_color_changes_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1bPpayload \x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec()]
        );
        assert_eq!(output.display, b" after");
    }

    #[test]
    fn ignores_split_osc_color_changes_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let first = runtime.feed_pty_output_with_display(b"\x1bPpayload ");
        let second =
            runtime.feed_pty_output_with_display(b"\x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07");

        assert!(first.responses.is_empty());
        assert!(first.display.is_empty());
        assert_eq!(
            second.responses,
            vec![b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec()]
        );
        assert_eq!(second.display, b" after");
    }

    #[test]
    fn answers_xtgettcap_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1bP+q436f\x1b\\ middle\x90+q544e;524742\x9c after\x1bP+q626164\x1b\\done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1+r436f=323536\x1b\\".to_vec(),
                b"\x1bP1+r544e=787465726d2d323536636f6c6f72;524742=524742\x9c".to_vec(),
                b"\x1bP0+r\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle afterdone"));
        assert!(!text.contains("+q"));
    }

    #[test]
    fn answers_xtgettcap_size_queries_from_current_terminal_size() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let output = runtime.feed_pty_output_with_display(b"before\x1bP+q636f;6c69\x1b\\after");

        assert_eq!(
            output.responses,
            vec![b"\x1bP1+r636f=313332;6c69=3433\x1b\\".to_vec()]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_decrqss_state_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b[1;4;38;5;196;48;2;1;2;3m\x1bP$qm\x1b\\ middle\x1b[5 q\x90$q q\x9c after\x1b[2;5r\x1bP$qr\x1b\\done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1$r1;4;38;5;196;48;2;1;2;3m\x1b\\".to_vec(),
                b"\x1bP1$r5 q\x9c".to_vec(),
                b"\x1bP1$r2;5r\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");
        assert!(!String::from_utf8_lossy(&output.display).contains("$q"));
    }

    #[test]
    fn answers_xtversion_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"before\x1b[>q middle\x1b[>0q after\x9b>q done");

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle after done");
    }

    #[test]
    fn answers_c1_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b?1000;1006h\x1b[?2004h");
        let normal_mouse = runtime.feed_pty_output(b"\x9b?1000$p");
        let sgr_mouse = runtime.feed_pty_output(b"\x9b?1006$p");
        let bracketed_paste = runtime.feed_pty_output(b"\x9b?2004$p");

        assert_eq!(normal_mouse, vec![b"\x1b[?1000;1$y".to_vec()]);
        assert_eq!(sgr_mouse, vec![b"\x1b[?1006;1$y".to_vec()]);
        assert_eq!(bracketed_paste, vec![b"\x1b[?2004;1$y".to_vec()]);
    }

    #[test]
    fn tracks_application_cursor_key_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.application_cursor_keys());

        runtime.feed_pty_output(b"\x1b[?1h");
        assert!(runtime.application_cursor_keys());

        runtime.feed_pty_output(b"\x1b[?1l");
        assert!(!runtime.application_cursor_keys());
    }

    #[test]
    fn tracks_split_application_cursor_key_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?");
        assert!(!runtime.application_cursor_keys());

        runtime.feed_pty_output(b"1h");
        assert!(runtime.application_cursor_keys());
    }

    #[test]
    fn tracks_focus_reporting_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.focus_reporting());

        runtime.feed_pty_output(b"\x1b[?1004h");
        assert!(runtime.focus_reporting());

        runtime.feed_pty_output(b"\x1b[?1004l");
        assert!(!runtime.focus_reporting());
    }

    #[test]
    fn tracks_bracketed_paste_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.bracketed_paste());

        runtime.feed_pty_output(b"\x1b[?2004h");
        assert!(runtime.bracketed_paste());

        runtime.feed_pty_output(b"\x1b[?2004l");
        assert!(!runtime.bracketed_paste());
    }

    #[test]
    fn ignores_private_input_modes_inside_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]0;title \x1b[?1004h\x07\x1bPpayload \x1b[?2004h\x1b\\");

        assert!(!runtime.focus_reporting());
        assert!(!runtime.bracketed_paste());
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?1004$p\x1b[?2004$p"),
            vec![b"\x1b[?1004;2$y".to_vec(), b"\x1b[?2004;2$y".to_vec()]
        );
    }

    #[test]
    fn extracts_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;Y29weQ==\x07");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_c1_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;Y29weQ==\x9c");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn ignores_osc52_clipboard_text_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]0;title \x1b]52;c;Y29weQ==\x07");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn ignores_osc52_clipboard_text_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1bPpayload \x1b]52;c;Y29weQ==\x1b\\");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn ignores_split_osc52_clipboard_text_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1bPpayload ");
        runtime.feed_pty_output(b"\x1b]52;c;Y29weQ==\x1b\\");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn extracts_split_osc52_clipboard_text_with_st_terminator() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;Y2");
        assert!(runtime.take_clipboard_texts().is_empty());

        runtime.feed_pty_output(b"9weQ==\x1b\\");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
    }

    #[test]
    fn extracts_split_c1_osc52_clipboard_text() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;Y2");
        assert!(runtime.take_clipboard_texts().is_empty());

        runtime.feed_pty_output(b"9weQ==\x9c");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
    }

    #[test]
    fn extracts_osc52_clipboard_queries_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;?\x07");

        assert_eq!(runtime.take_clipboard_queries(), vec!["c".to_owned()]);
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn extracts_c1_osc52_clipboard_queries_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;?\x9c");

        assert_eq!(runtime.take_clipboard_queries(), vec!["c".to_owned()]);
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn tracks_combined_private_input_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?1;1004;2004h");

        assert!(runtime.application_cursor_keys());
        assert!(runtime.focus_reporting());
        assert!(runtime.bracketed_paste());
    }

    #[test]
    fn tracks_c1_private_input_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9b?1;1004;2004h");

        assert!(runtime.application_cursor_keys());
        assert!(runtime.focus_reporting());
        assert!(runtime.bracketed_paste());
    }

    #[test]
    fn tracks_mouse_reporting_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert_eq!(runtime.mouse_input_mode(), MouseInputMode::default());

        runtime.feed_pty_output(b"\x1b[?1000;1006h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr)
        );

        runtime.feed_pty_output(b"\x1b[?1002h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr)
        );

        runtime.feed_pty_output(b"\x1b[?1006l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::X10)
        );

        runtime.feed_pty_output(b"\x1b[?1002;1000l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::None, MouseProtocolMode::X10)
        );
    }

    #[test]
    fn tracks_application_keypad_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.application_keypad());

        runtime.feed_pty_output(b"\x1b=");
        assert!(runtime.application_keypad());

        runtime.feed_pty_output(b"\x1b>");
        assert!(!runtime.application_keypad());
    }

    #[test]
    fn tracks_split_application_keypad_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b");
        assert!(!runtime.application_keypad());

        runtime.feed_pty_output(b"=");
        assert!(runtime.application_keypad());
    }

    #[test]
    fn resize_updates_terminal_grid_and_size_query_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(4, 2));
        runtime.feed_pty_output(b"abcd\nef");

        runtime.resize(TerminalSize::new(6, 3));
        let responses = runtime.feed_pty_output(b"\x1b[18t");

        assert_eq!(runtime.terminal().grid().size(), TerminalSize::new(6, 3));
        assert_eq!(responses, vec![b"\x1b[8;3;6t".to_vec()]);
    }

    #[test]
    fn answers_split_device_attribute_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output(b"before\x1b[");
        let second = runtime.feed_pty_output(b">cafter");

        assert!(first.is_empty());
        assert_eq!(second, vec![b"\x1b[>0;0;0c".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[>c"));
    }

    fn terminal_text(runtime: &TerminalRuntime) -> String {
        let grid = runtime.terminal().grid();
        let size = grid.size();
        let mut text = String::new();

        for row in 0..size.rows {
            for column in 0..size.columns {
                text.push(grid.get(row, column).unwrap().ch);
            }
        }

        text
    }
}
