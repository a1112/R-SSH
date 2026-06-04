use base64::{Engine, engine::general_purpose::STANDARD};
use rssh_core::TerminalSize;
use rssh_terminal::Terminal;

pub struct TerminalRuntime {
    terminal: Terminal,
    output_filter: TerminalOutputFilter,
    mode_tracker: TerminalModeTracker,
    clipboard_tracker: TerminalClipboardTracker,
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::new(size),
            output_filter: TerminalOutputFilter::new(size),
            mode_tracker: TerminalModeTracker::default(),
            clipboard_tracker: TerminalClipboardTracker::default(),
        }
    }

    pub fn feed_pty_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.clipboard_tracker.process(bytes);
        self.mode_tracker.process(bytes);
        let output = self.output_filter.process(bytes);

        let mut responses = Vec::new();
        for event in output.events {
            match event {
                FilteredOutputEvent::Display(display) => self.terminal.feed(&display),
                FilteredOutputEvent::Response(response) => {
                    responses.push(
                        self.output_filter
                            .response_bytes(response, self.terminal.cursor()),
                    );
                }
            }
        }

        responses
    }

    pub fn take_clipboard_texts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_tracker.texts)
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
}

struct FilteredOutput {
    events: Vec<FilteredOutputEvent>,
}

enum FilteredOutputEvent {
    Display(Vec<u8>),
    Response(TerminalResponse),
}

impl TerminalOutputFilter {
    const RESPONSES: &'static [TerminalQueryResponse] = &[
        TerminalQueryResponse {
            query: b"\x1b[6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x1b[?6n",
            response: TerminalResponse::CursorPosition { private: true },
        },
        TerminalQueryResponse {
            query: b"\x1b[c",
            response: TerminalResponse::Static(b"\x1b[?1;2c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[>c",
            response: TerminalResponse::Static(b"\x1b[>0;0;0c"),
        },
        TerminalQueryResponse {
            query: b"\x1b[5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x1b[18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[19t",
            response: TerminalResponse::ScreenSize,
        },
    ];

    fn new(size: TerminalSize) -> Self {
        Self {
            pending: Vec::new(),
            size,
        }
    }

    fn resize(&mut self, size: TerminalSize) {
        self.size = size;
    }

    fn process(&mut self, bytes: &[u8]) -> FilteredOutput {
        self.pending.extend_from_slice(bytes);

        let mut events = Vec::new();

        while let Some((index, response)) = self.find_next_response() {
            if index > 0 {
                events.push(FilteredOutputEvent::Display(self.pending[..index].to_vec()));
            }
            events.push(FilteredOutputEvent::Response(response.response));
            self.pending.drain(..index + response.query.len());
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

    fn find_next_response(&self) -> Option<(usize, &'static TerminalQueryResponse)> {
        Self::RESPONSES
            .iter()
            .filter_map(|response| {
                find_subslice(&self.pending, response.query).map(|index| (index, response))
            })
            .min_by_key(|(index, _)| *index)
    }

    fn suffix_len_matching_query_prefix(pending: &[u8]) -> usize {
        Self::RESPONSES
            .iter()
            .map(|response| suffix_prefix_len(pending, response.query))
            .max()
            .unwrap_or(0)
    }

    fn response_bytes(&self, response: TerminalResponse, cursor: (u16, u16)) -> Vec<u8> {
        response.response_bytes(self.size, cursor)
    }
}

struct TerminalQueryResponse {
    query: &'static [u8],
    response: TerminalResponse,
}

#[derive(Clone, Copy)]
enum TerminalResponse {
    Static(&'static [u8]),
    CursorPosition { private: bool },
    TextAreaSize,
    ScreenSize,
}

impl TerminalResponse {
    fn response_bytes(self, size: TerminalSize, cursor: (u16, u16)) -> Vec<u8> {
        match self {
            TerminalResponse::Static(bytes) => bytes.to_vec(),
            TerminalResponse::CursorPosition { private } => {
                let (row, column) = cursor;
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
            TerminalResponse::TextAreaSize => {
                format!("\x1b[8;{};{}t", size.rows, size.columns).into_bytes()
            }
            TerminalResponse::ScreenSize => {
                format!("\x1b[9;{};{}t", size.rows, size.columns).into_bytes()
            }
        }
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
}

impl TerminalClipboardTracker {
    const OSC52_PREFIX: &'static [u8] = b"\x1b]52;";
    const ST_TERMINATOR: &'static [u8] = b"\x1b\\";
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some(start) = find_subslice(&self.pending, Self::OSC52_PREFIX) else {
                self.retain_possible_prefix();
                return;
            };
            if start > 0 {
                self.pending.drain(..start);
            }

            let content_start = Self::OSC52_PREFIX.len();
            let Some(terminator) = find_osc_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            if let Some(text) =
                decode_osc52_clipboard_content(&self.pending[content_start..content_end])
            {
                self.texts.push(text);
            }

            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = suffix_prefix_len(&self.pending, Self::OSC52_PREFIX);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

struct OscTerminator {
    index: usize,
    length: usize,
}

fn find_osc_terminator(bytes: &[u8]) -> Option<OscTerminator> {
    let bel = bytes
        .iter()
        .position(|byte| *byte == b'\x07')
        .map(|index| OscTerminator { index, length: 1 });
    let st =
        find_subslice(bytes, TerminalClipboardTracker::ST_TERMINATOR).map(|index| OscTerminator {
            index,
            length: TerminalClipboardTracker::ST_TERMINATOR.len(),
        });

    match (bel, st) {
        (Some(bel), Some(st)) => Some(if bel.index <= st.index { bel } else { st }),
        (Some(bel), None) => Some(bel),
        (None, Some(st)) => Some(st),
        (None, None) => None,
    }
}

fn decode_osc52_clipboard_content(content: &[u8]) -> Option<String> {
    let separator = content.iter().position(|byte| *byte == b';')?;
    let payload = &content[separator + 1..];
    let decoded = STANDARD.decode(payload).ok()?;

    String::from_utf8(decoded).ok()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MouseProtocolMode {
    #[default]
    X10,
    Sgr,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MouseReportingMode {
    #[default]
    None,
    Normal,
    ButtonEvent,
    AnyEvent,
}

impl MouseReportingMode {
    pub(crate) const fn is_enabled(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MouseInputMode {
    reporting: MouseReportingMode,
    protocol: MouseProtocolMode,
}

impl MouseInputMode {
    pub(crate) const fn new(reporting: MouseReportingMode, protocol: MouseProtocolMode) -> Self {
        Self {
            reporting,
            protocol,
        }
    }

    pub(crate) const fn reporting(self) -> MouseReportingMode {
        self.reporting
    }

    pub(crate) const fn protocol(self) -> MouseProtocolMode {
        self.protocol
    }

    pub(crate) const fn reporting_enabled(self) -> bool {
        self.reporting.is_enabled()
    }
}

#[derive(Default)]
struct TerminalModeTracker {
    pending: Vec<u8>,
    mouse_modes: MouseModes,
    tracked_modes: TrackedTerminalModes,
}

impl TerminalModeTracker {
    const APPLICATION_KEYPAD_PREFIX: &'static [u8] = b"\x1b=";
    const CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x1b[?";
    const NUMERIC_KEYPAD_PREFIX: &'static [u8] = b"\x1b>";

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);

        loop {
            let Some(start) = self.find_next_mode_start() else {
                self.retain_possible_prefix();
                return;
            };
            if start.index > 0 {
                self.pending.drain(..start.index);
            }

            match start.sequence {
                ModeSequence::ApplicationKeypad(enabled) => {
                    self.tracked_modes
                        .set(TrackedTerminalModes::APPLICATION_KEYPAD, enabled);
                    self.pending.drain(..2);
                }
                ModeSequence::CsiPrivateMode => {
                    match Self::parse_private_mode_sequence(&self.pending) {
                        ModeParse::Complete {
                            modes,
                            enabled,
                            consumed,
                        } => {
                            for mode in modes {
                                if self.mouse_modes.set(mode, enabled) {
                                    continue;
                                }
                                match mode {
                                    1 => self.tracked_modes.set(
                                        TrackedTerminalModes::APPLICATION_CURSOR_KEYS,
                                        enabled,
                                    ),
                                    1004 => self
                                        .tracked_modes
                                        .set(TrackedTerminalModes::FOCUS_REPORTING, enabled),
                                    2004 => self
                                        .tracked_modes
                                        .set(TrackedTerminalModes::BRACKETED_PASTE, enabled),
                                    _ => {}
                                }
                            }
                            self.pending.drain(..consumed);
                        }
                        ModeParse::Incomplete => return,
                        ModeParse::Invalid => {
                            self.pending.drain(..1);
                        }
                    }
                }
            }
        }
    }

    fn find_next_mode_start(&self) -> Option<ModeSequenceStart> {
        [
            (Self::CSI_PRIVATE_MODE_PREFIX, ModeSequence::CsiPrivateMode),
            (
                Self::APPLICATION_KEYPAD_PREFIX,
                ModeSequence::ApplicationKeypad(true),
            ),
            (
                Self::NUMERIC_KEYPAD_PREFIX,
                ModeSequence::ApplicationKeypad(false),
            ),
        ]
        .into_iter()
        .filter_map(|(prefix, sequence)| {
            find_subslice(&self.pending, prefix).map(|index| ModeSequenceStart { index, sequence })
        })
        .min_by_key(|start| start.index)
    }

    fn application_cursor_keys(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::APPLICATION_CURSOR_KEYS)
    }

    fn application_keypad(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::APPLICATION_KEYPAD)
    }

    fn focus_reporting(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::FOCUS_REPORTING)
    }

    fn bracketed_paste(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::BRACKETED_PASTE)
    }

    fn mouse_input_mode(&self) -> MouseInputMode {
        self.mouse_modes.input_mode()
    }

    fn parse_private_mode_sequence(bytes: &[u8]) -> ModeParse {
        let mut cursor = Self::CSI_PRIVATE_MODE_PREFIX.len();
        let mut modes = Vec::new();

        loop {
            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            let start = cursor;
            let mut mode = 0u16;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                mode = mode
                    .saturating_mul(10)
                    .saturating_add(u16::from(bytes[cursor] - b'0'));
                cursor += 1;
            }

            if cursor == start {
                return ModeParse::Invalid;
            }
            modes.push(mode);

            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            match bytes[cursor] {
                b';' => cursor += 1,
                b'h' | b'l' => {
                    return ModeParse::Complete {
                        modes,
                        enabled: bytes[cursor] == b'h',
                        consumed: cursor + 1,
                    };
                }
                _ => return ModeParse::Invalid,
            }
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [
            Self::CSI_PRIVATE_MODE_PREFIX,
            Self::APPLICATION_KEYPAD_PREFIX,
            Self::NUMERIC_KEYPAD_PREFIX,
        ]
        .into_iter()
        .map(|prefix| suffix_prefix_len(&self.pending, prefix))
        .max()
        .unwrap_or(0);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct TrackedTerminalModes(u8);

impl TrackedTerminalModes {
    const APPLICATION_CURSOR_KEYS: u8 = 1;
    const APPLICATION_KEYPAD: u8 = 1 << 1;
    const FOCUS_REPORTING: u8 = 1 << 2;
    const BRACKETED_PASTE: u8 = 1 << 3;

    fn set(&mut self, flag: u8, enabled: bool) {
        if enabled {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }

    const fn enabled(self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

#[derive(Default)]
struct MouseModes(u8);

impl MouseModes {
    const NORMAL: u8 = 1;
    const BUTTON_EVENT: u8 = 1 << 1;
    const ANY_EVENT: u8 = 1 << 2;
    const SGR_PROTOCOL: u8 = 1 << 3;

    fn set(&mut self, mode: u16, enabled: bool) -> bool {
        let Some(mask) = Self::mask(mode) else {
            return false;
        };

        if enabled {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }

        true
    }

    fn input_mode(&self) -> MouseInputMode {
        let reporting = if self.0 & Self::ANY_EVENT != 0 {
            MouseReportingMode::AnyEvent
        } else if self.0 & Self::BUTTON_EVENT != 0 {
            MouseReportingMode::ButtonEvent
        } else if self.0 & Self::NORMAL != 0 {
            MouseReportingMode::Normal
        } else {
            MouseReportingMode::None
        };
        let protocol = if self.0 & Self::SGR_PROTOCOL != 0 {
            MouseProtocolMode::Sgr
        } else {
            MouseProtocolMode::X10
        };

        MouseInputMode::new(reporting, protocol)
    }

    const fn mask(mode: u16) -> Option<u8> {
        match mode {
            1000 => Some(Self::NORMAL),
            1002 => Some(Self::BUTTON_EVENT),
            1003 => Some(Self::ANY_EVENT),
            1006 => Some(Self::SGR_PROTOCOL),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct ModeSequenceStart {
    index: usize,
    sequence: ModeSequence,
}

#[derive(Clone, Copy)]
enum ModeSequence {
    CsiPrivateMode,
    ApplicationKeypad(bool),
}

enum ModeParse {
    Complete {
        modes: Vec<u16>,
        enabled: bool,
        consumed: usize,
    },
    Incomplete,
    Invalid,
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use super::{MouseInputMode, MouseProtocolMode, MouseReportingMode, TerminalRuntime};

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
    fn answers_text_area_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[18tafter");

        assert_eq!(responses, vec![b"\x1b[8;43;132t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[18t"));
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
    fn extracts_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;Y29weQ==\x07");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
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
    fn tracks_combined_private_input_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?1;1004;2004h");

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
