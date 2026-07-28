use rssh_core::TerminalSize;

use crate::{Cell, SequenceNo};

/// One visible terminal row and all metadata that must travel with it.
#[derive(Debug, Clone)]
pub struct GridRow {
    cells: Vec<Cell>,
    reflow_overflow: Vec<Cell>,
    wrapped: bool,
    last_change_seqno: SequenceNo,
}

impl GridRow {
    fn blank(columns: u16, cell: &Cell, seqno: SequenceNo) -> Self {
        Self {
            cells: vec![cell.clone(); usize::from(columns)],
            reflow_overflow: Vec::new(),
            wrapped: false,
            last_change_seqno: seqno,
        }
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[must_use]
    pub const fn is_wrapped(&self) -> bool {
        self.wrapped
    }

    #[must_use]
    pub const fn last_change_seqno(&self) -> SequenceNo {
        self.last_change_seqno
    }

    pub(crate) fn into_parts(self) -> (Vec<Cell>, Vec<Cell>, bool, SequenceNo) {
        (
            self.cells,
            self.reflow_overflow,
            self.wrapped,
            self.last_change_seqno,
        )
    }

    fn resize_columns(&mut self, columns: u16, preserve_row_metadata: bool, seqno: SequenceNo) {
        self.cells.resize(usize::from(columns), Cell::default());
        if !preserve_row_metadata {
            self.reflow_overflow.clear();
            self.last_change_seqno = seqno;
        }
    }
}

/// Visible terminal storage addressed by logical row and column.
#[derive(Debug, Clone)]
pub struct TerminalGrid {
    size: TerminalSize,
    rows: Vec<GridRow>,
}

impl TerminalGrid {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self::new_with_seqno(size, 1)
    }

    #[must_use]
    pub(crate) fn new_with_seqno(size: TerminalSize, seqno: SequenceNo) -> Self {
        let blank = Cell::default();
        Self {
            size,
            rows: (0..size.rows)
                .map(|_| GridRow::blank(size.columns, &blank, seqno))
                .collect(),
        }
    }

    #[must_use]
    pub const fn size(&self) -> TerminalSize {
        self.size
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.size.cells()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size.cells() == 0
    }

    #[must_use]
    pub fn get(&self, row: u16, column: u16) -> Option<&Cell> {
        self.rows
            .get(usize::from(row))
            .and_then(|row| row.cells.get(usize::from(column)))
    }

    pub fn set(&mut self, row: u16, column: u16, cell: Cell) -> bool {
        let Some(row) = self.rows.get_mut(usize::from(row)) else {
            return false;
        };
        let Some(slot) = row.cells.get_mut(usize::from(column)) else {
            return false;
        };

        *slot = cell;
        row.reflow_overflow.clear();
        true
    }

    #[must_use]
    pub(crate) fn row_wrapped(&self, row: u16) -> bool {
        self.rows
            .get(usize::from(row))
            .is_some_and(GridRow::is_wrapped)
    }

    pub(crate) fn set_row_wrapped(&mut self, row: u16, wrapped: bool) {
        if let Some(row) = self.rows.get_mut(usize::from(row)) {
            row.wrapped = wrapped;
        }
    }

    #[must_use]
    pub(crate) fn row_last_change_seqno(&self, row: u16) -> Option<SequenceNo> {
        self.rows
            .get(usize::from(row))
            .map(GridRow::last_change_seqno)
    }

    pub(crate) fn set_row_last_change_seqno(&mut self, row: u16, seqno: SequenceNo) -> bool {
        let Some(row) = self.rows.get_mut(usize::from(row)) else {
            return false;
        };
        row.last_change_seqno = seqno;
        true
    }

    #[must_use]
    pub(crate) fn cells_with_reflow_overflow(&self, row: u16) -> Vec<Cell> {
        let Some(row) = self.rows.get(usize::from(row)) else {
            return Vec::new();
        };
        let mut cells = row.cells.clone();
        cells.extend(row.reflow_overflow.iter().cloned());
        cells
    }

    pub(crate) fn set_reflow_overflow(&mut self, row: u16, overflow: Vec<Cell>) {
        if let Some(row) = self.rows.get_mut(usize::from(row)) {
            row.reflow_overflow = overflow;
        }
    }

    pub fn resize(&mut self, size: TerminalSize) {
        let new_row_seqno = self
            .rows
            .iter()
            .map(GridRow::last_change_seqno)
            .max()
            .unwrap_or(1);
        self.resize_with_seqno(size, new_row_seqno);
    }

    pub(crate) fn resize_with_seqno(&mut self, size: TerminalSize, new_row_seqno: SequenceNo) {
        let old_size = self.size;
        let old_rows = std::mem::take(&mut self.rows);
        let preserve_row_metadata = old_size.columns == size.columns;
        let blank = Cell::default();
        let mut old_rows = old_rows.into_iter();

        self.rows = (0..size.rows)
            .map(|_| {
                if let Some(mut row) = old_rows.next() {
                    row.resize_columns(size.columns, preserve_row_metadata, new_row_seqno);
                    row
                } else {
                    GridRow::blank(size.columns, &blank, new_row_seqno)
                }
            })
            .collect();
        self.size = size;
    }

    pub(crate) fn scroll_up_rows(
        &mut self,
        top: u16,
        bottom: u16,
        count: u16,
        blank: &Cell,
        seqno: SequenceNo,
    ) -> Vec<GridRow> {
        let columns = self.size.columns;
        let Some(rows) = self.row_region_mut(top, bottom) else {
            return Vec::new();
        };
        let count = usize::from(count).min(rows.len());
        if count == 0 {
            return Vec::new();
        }

        rows.rotate_left(count);
        let first_exiting = rows.len() - count;
        rows[first_exiting..]
            .iter_mut()
            .map(|row| std::mem::replace(row, GridRow::blank(columns, blank, seqno)))
            .collect()
    }

    pub(crate) fn scroll_down_rows(
        &mut self,
        top: u16,
        bottom: u16,
        count: u16,
        blank: &Cell,
        seqno: SequenceNo,
    ) {
        let columns = self.size.columns;
        let Some(rows) = self.row_region_mut(top, bottom) else {
            return;
        };
        let count = usize::from(count).min(rows.len());
        if count == 0 {
            return;
        }

        rows.rotate_right(count);
        for row in &mut rows[..count] {
            *row = GridRow::blank(columns, blank, seqno);
        }
    }

    fn row_region_mut(&mut self, top: u16, bottom: u16) -> Option<&mut [GridRow]> {
        if top > bottom || bottom >= self.size.rows {
            return None;
        }
        self.rows.get_mut(usize::from(top)..=usize::from(bottom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_rotation_moves_complete_rows_without_copying_owned_cells() {
        let mut grid = TerminalGrid::new_with_seqno(TerminalSize::new(1, 3), 1);
        let hyperlink = "https://example.test/row-owned-allocation".repeat(4);
        assert!(grid.set(
            0,
            0,
            Cell {
                ch: 'x',
                hyperlink: Some(hyperlink),
                ..Cell::default()
            }
        ));
        grid.set_row_wrapped(0, true);
        grid.set_reflow_overflow(
            0,
            vec![Cell {
                ch: '界',
                ..Cell::default()
            }],
        );
        assert!(grid.set_row_last_change_seqno(0, 99));
        let allocation = grid
            .get(0, 0)
            .and_then(|cell| cell.hyperlink.as_ref())
            .map(|hyperlink| hyperlink.as_ptr())
            .unwrap();

        let exiting = grid.scroll_up_rows(0, 2, 1, &Cell::default(), 100);

        assert_eq!(exiting.len(), 1);
        assert_eq!(exiting[0].cells()[0].ch, 'x');
        assert!(exiting[0].is_wrapped());
        assert_eq!(exiting[0].last_change_seqno(), 99);
        assert_eq!(
            exiting[0].cells()[0].hyperlink.as_ref().unwrap().as_ptr(),
            allocation
        );
        assert_eq!(exiting[0].reflow_overflow[0].ch, '界');
    }
}
