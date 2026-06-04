pub use rssh_core::DamageRegion;

#[cfg(test)]
mod tests {
    use super::DamageRegion;

    #[test]
    fn zero_width_region_is_empty() {
        assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    }
}
