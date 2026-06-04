#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyBackend {
    WindowsConpty,
    UnixPty,
}

impl PtyBackend {
    #[must_use]
    pub fn current_platform() -> Self {
        if cfg!(windows) {
            Self::WindowsConpty
        } else {
            Self::UnixPty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PtyBackend;

    #[test]
    fn selects_a_platform_backend() {
        let backend = PtyBackend::current_platform();

        assert!(matches!(
            backend,
            PtyBackend::WindowsConpty | PtyBackend::UnixPty
        ));
    }
}
