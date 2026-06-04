#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

impl TerminalSize {
    #[must_use]
    pub const fn new(columns: u16, rows: u16) -> Self {
        Self { columns, rows }
    }

    #[must_use]
    pub const fn cells(self) -> usize {
        self.columns as usize * self.rows as usize
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionId, TerminalSize};

    #[test]
    fn exposes_session_id_value() {
        assert_eq!(SessionId::new(42).get(), 42);
    }

    #[test]
    fn computes_terminal_cell_count() {
        assert_eq!(TerminalSize::new(120, 30).cells(), 3600);
    }
}
