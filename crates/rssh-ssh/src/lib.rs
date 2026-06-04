use rssh_core::TerminalSize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshSessionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub initial_size: TerminalSize,
}

impl SshSessionConfig {
    #[must_use]
    pub fn new(
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        initial_size: TerminalSize,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            username: username.into(),
            initial_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use super::SshSessionConfig;

    #[test]
    fn session_config_keeps_terminal_size() {
        let config = SshSessionConfig::new("example.com", 22, "ops", TerminalSize::new(100, 40));

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "ops");
        assert_eq!(config.initial_size, TerminalSize::new(100, 40));
    }
}
