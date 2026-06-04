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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRegion {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl DamageRegion {
    #[must_use]
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    #[must_use]
    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }
}

#[cfg(test)]
mod tests {
    use super::{DamageRegion, SessionId, TerminalSize};

    #[test]
    fn exposes_session_id_value() {
        assert_eq!(SessionId::new(42).get(), 42);
    }

    #[test]
    fn computes_terminal_cell_count() {
        assert_eq!(TerminalSize::new(120, 30).cells(), 3600);
    }

    #[test]
    fn zero_width_damage_region_is_empty() {
        assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    }

    #[test]
    fn damage_region_reports_right_edge() {
        assert_eq!(DamageRegion::new(2, 0, 3, 1).right(), 5);
    }
}
