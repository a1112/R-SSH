use rssh_core::TerminalSize;
use rssh_terminal::Terminal;

pub struct TerminalRuntime {
    terminal: Terminal,
    output_filter: TerminalOutputFilter,
    mode_tracker: TerminalModeTracker,
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::new(size),
            output_filter: TerminalOutputFilter::new(size),
            mode_tracker: TerminalModeTracker::default(),
        }
    }

    pub fn feed_pty_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
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
struct TerminalModeTracker {
    pending: Vec<u8>,
    application_cursor_keys: bool,
}

impl TerminalModeTracker {
    const CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x1b[?";

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);

        loop {
            let Some(start) = find_subslice(&self.pending, Self::CSI_PRIVATE_MODE_PREFIX) else {
                self.retain_possible_prefix();
                return;
            };
            if start > 0 {
                self.pending.drain(..start);
            }

            match Self::parse_private_mode_sequence(&self.pending) {
                ModeParse::Complete {
                    modes,
                    enabled,
                    consumed,
                } => {
                    if modes.contains(&1) {
                        self.application_cursor_keys = enabled;
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

    fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
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
        let retained = suffix_prefix_len(&self.pending, Self::CSI_PRIVATE_MODE_PREFIX);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
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
