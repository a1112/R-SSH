use rssh_core::TerminalSize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: ' ' }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalGrid {
    size: TerminalSize,
    cells: Vec<Cell>,
}

impl TerminalGrid {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            size,
            cells: vec![Cell::default(); size.cells()],
        }
    }

    #[must_use]
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use super::TerminalGrid;

    #[test]
    fn grid_allocates_one_cell_per_terminal_slot() {
        let grid = TerminalGrid::new(TerminalSize::new(80, 24));

        assert_eq!(grid.size(), TerminalSize::new(80, 24));
        assert_eq!(grid.len(), 1920);
        assert!(!grid.is_empty());
    }
}
