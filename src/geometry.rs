#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
    pub width: u16,
    pub height: u16,
}

impl Viewport {
    pub fn fit(term_cols: u16, term_rows: u16, width: u16, height: u16, aspect: f32) -> Self {
        let available_cols = term_cols.max(1);
        let available_rows = term_rows.saturating_sub(2).max(1);
        let ratio = f32::from(width) / f32::from(height);
        let (cols, rows) = if f32::from(available_cols) * aspect / f32::from(available_rows) > ratio
        {
            (
                (f32::from(available_rows) * ratio / aspect)
                    .floor()
                    .max(1.0) as u16,
                available_rows,
            )
        } else {
            (
                available_cols,
                (f32::from(available_cols) * aspect / ratio)
                    .floor()
                    .max(1.0) as u16,
            )
        };
        Self {
            x: term_cols.saturating_sub(cols) / 2,
            y: if term_rows > 2 {
                1 + (available_rows - rows) / 2
            } else {
                0
            },
            cols,
            rows,
            width,
            height,
        }
    }

    pub fn point(&self, col: u16, row: u16) -> Option<(u32, u32)> {
        let x = col.checked_sub(self.x)?;
        let y = row.checked_sub(self.y)?;
        if x >= self.cols || y >= self.rows {
            return None;
        }
        Some((
            ((2 * u32::from(x) + 1) * u32::from(self.width) / (2 * u32::from(self.cols)))
                .min(u32::from(self.width) - 1),
            ((2 * u32::from(y) + 1) * u32::from(self.height) / (2 * u32::from(self.rows)))
                .min(u32::from(self.height) - 1),
        ))
    }
}
