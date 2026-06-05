use base64::{Engine, engine::general_purpose::STANDARD};
use rssh_core::TerminalSize;
use rssh_terminal::Terminal;

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
        let mut bells = 0_u64;
        for event in output.events {
            match event {
                FilteredOutputEvent::Display(display) => {
                    self.terminal.feed(&display);
                    bells = bells.saturating_add(self.terminal.take_bell_count());
                    display_bytes.extend(self.visible_output_filter.process(&display));
                }
                FilteredOutputEvent::Response(response) => {
                    responses.push(
                        self.output_filter
                            .response_bytes(response, self.terminal.cursor()),
                    );
                }
            }
        }

        TerminalRuntimeOutput {
            responses,
            display: display_bytes,
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
            query: b"\x1b[5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x9b5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
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
    queries: Vec<String>,
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
            match parse_osc52_clipboard_content(&self.pending[content_start..content_end]) {
                Some(ClipboardSequence::Write(text)) => self.texts.push(text),
                Some(ClipboardSequence::Query(selection)) => self.queries.push(selection),
                None => {}
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
    fn answers_screen_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[19tafter");

        assert_eq!(responses, vec![b"\x1b[9;43;132t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[19t"));
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
    fn extracts_osc52_clipboard_queries_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;?\x07");

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
