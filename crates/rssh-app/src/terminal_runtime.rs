use rssh_core::TerminalSize;
use rssh_terminal::Terminal;

pub struct TerminalRuntime {
    terminal: Terminal,
    output_filter: TerminalOutputFilter,
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::new(size),
            output_filter: TerminalOutputFilter::default(),
        }
    }

    pub fn feed_pty_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let output = self.output_filter.process(bytes);
        if !output.display.is_empty() {
            self.terminal.feed(&output.display);
        }
        output.responses
    }

    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
}

#[derive(Default)]
struct TerminalOutputFilter {
    pending: Vec<u8>,
}

struct FilteredOutput {
    display: Vec<u8>,
    responses: Vec<Vec<u8>>,
}

impl TerminalOutputFilter {
    const CURSOR_POSITION_QUERY: &'static [u8] = b"\x1b[6n";
    const CURSOR_POSITION_RESPONSE: &'static [u8] = b"\x1b[1;1R";

    fn process(&mut self, bytes: &[u8]) -> FilteredOutput {
        self.pending.extend_from_slice(bytes);

        let mut display = Vec::new();
        let mut responses = Vec::new();

        while let Some(index) = find_subslice(&self.pending, Self::CURSOR_POSITION_QUERY) {
            display.extend_from_slice(&self.pending[..index]);
            responses.push(Self::CURSOR_POSITION_RESPONSE.to_vec());
            self.pending
                .drain(..index + Self::CURSOR_POSITION_QUERY.len());
        }

        let retained = suffix_prefix_len(&self.pending, Self::CURSOR_POSITION_QUERY);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            display.extend_from_slice(&self.pending[..writable]);
            self.pending.drain(..writable);
        }

        FilteredOutput { display, responses }
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
        assert_eq!(second, vec![b"\x1b[1;1R".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[6n"));
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
