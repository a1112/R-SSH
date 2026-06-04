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
}

#[cfg(test)]
mod tests {
    use super::DamageRegion;

    #[test]
    fn zero_width_region_is_empty() {
        assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    }
}
