use rssh_core::TerminalSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshConfigError {
    EmptyHost,
    EmptyUsername,
    InvalidPort,
    InvalidTerminalSize,
}

impl std::fmt::Display for SshConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyHost => "SSH host cannot be empty",
            Self::EmptyUsername => "SSH username cannot be empty",
            Self::InvalidPort => "SSH port must be greater than zero",
            Self::InvalidTerminalSize => "SSH terminal size must have non-zero columns and rows",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SshConfigError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshSessionConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub initial_size: TerminalSize,
}

impl SshSessionConfig {
    /// Creates a config without validation for callers that already normalized
    /// trusted fields. Use [`Self::try_new`] for user-facing input.
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

    /// Creates a validated SSH session config from user-facing fields.
    ///
    /// # Errors
    ///
    /// Returns [`SshConfigError`] when host or username is empty, port is zero,
    /// or the requested terminal size has zero columns or rows.
    pub fn try_new(
        host: impl Into<String>,
        port: u16,
        username: impl Into<String>,
        initial_size: TerminalSize,
    ) -> Result<Self, SshConfigError> {
        let host = host.into();
        let username = username.into();
        let host = host.trim();
        let username = username.trim();

        if host.is_empty() {
            return Err(SshConfigError::EmptyHost);
        }
        if username.is_empty() {
            return Err(SshConfigError::EmptyUsername);
        }
        if port == 0 {
            return Err(SshConfigError::InvalidPort);
        }
        if initial_size.columns == 0 || initial_size.rows == 0 {
            return Err(SshConfigError::InvalidTerminalSize);
        }

        Ok(Self::new(host, port, username, initial_size))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshSessionError {
    message: String,
}

impl SshSessionError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SshSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SshSessionError {}

pub trait SshShellSession {
    /// Reads bytes from the remote shell channel into `buffer`.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the adapter cannot read from the active
    /// SSH channel.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SshSessionError>;

    /// Writes bytes to the remote shell channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the adapter cannot write to or flush the
    /// active SSH channel.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, SshSessionError>;

    /// Resizes the remote PTY attached to the shell channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the remote PTY resize request fails.
    fn resize(&mut self, size: TerminalSize) -> Result<(), SshSessionError>;

    /// Sends a keepalive through the underlying SSH connection.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the keepalive request fails.
    fn keepalive(&mut self) -> Result<(), SshSessionError>;

    /// Closes the remote shell session.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the adapter cannot close the SSH channel
    /// cleanly.
    fn close(&mut self) -> Result<(), SshSessionError>;
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use super::{SshSessionConfig, SshShellSession};

    #[test]
    fn session_config_keeps_terminal_size() {
        let config = SshSessionConfig::new("example.com", 22, "ops", TerminalSize::new(100, 40));

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "ops");
        assert_eq!(config.initial_size, TerminalSize::new(100, 40));
    }

    #[test]
    fn session_config_try_new_trims_and_validates_fields() {
        let config =
            SshSessionConfig::try_new(" example.com ", 22, " ops ", TerminalSize::new(100, 40))
                .unwrap();

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 22);
        assert_eq!(config.username, "ops");
        assert_eq!(config.initial_size, TerminalSize::new(100, 40));
    }

    #[test]
    fn session_config_rejects_empty_host() {
        assert_eq!(
            SshSessionConfig::try_new(" ", 22, "ops", TerminalSize::new(100, 40)),
            Err(super::SshConfigError::EmptyHost)
        );
    }

    #[test]
    fn session_config_rejects_empty_username() {
        assert_eq!(
            SshSessionConfig::try_new("example.com", 22, " ", TerminalSize::new(100, 40)),
            Err(super::SshConfigError::EmptyUsername)
        );
    }

    #[test]
    fn session_config_rejects_zero_port() {
        assert_eq!(
            SshSessionConfig::try_new("example.com", 0, "ops", TerminalSize::new(100, 40)),
            Err(super::SshConfigError::InvalidPort)
        );
    }

    #[test]
    fn session_config_rejects_zero_terminal_dimensions() {
        assert_eq!(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(0, 40)),
            Err(super::SshConfigError::InvalidTerminalSize)
        );
        assert_eq!(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(100, 0)),
            Err(super::SshConfigError::InvalidTerminalSize)
        );
    }

    #[test]
    fn ssh_shell_session_trait_models_channel_io_and_resize() {
        let mut session = MockSshSession::default();
        let mut output = [0; 4];

        assert_eq!(session.read(&mut output).unwrap(), 4);
        assert_eq!(&output, b"pong");
        assert_eq!(session.write(b"ping").unwrap(), 4);
        session.resize(TerminalSize::new(120, 30)).unwrap();
        session.keepalive().unwrap();
        session.close().unwrap();

        assert_eq!(session.written, b"ping");
        assert_eq!(session.size, TerminalSize::new(120, 30));
        assert!(session.kept_alive);
        assert!(session.closed);
    }

    struct MockSshSession {
        written: Vec<u8>,
        size: TerminalSize,
        kept_alive: bool,
        closed: bool,
    }

    impl Default for MockSshSession {
        fn default() -> Self {
            Self {
                written: Vec::new(),
                size: TerminalSize::new(80, 24),
                kept_alive: false,
                closed: false,
            }
        }
    }

    impl super::SshShellSession for MockSshSession {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, super::SshSessionError> {
            buffer.copy_from_slice(b"pong");
            Ok(4)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, super::SshSessionError> {
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn resize(&mut self, size: TerminalSize) -> Result<(), super::SshSessionError> {
            self.size = size;
            Ok(())
        }

        fn keepalive(&mut self) -> Result<(), super::SshSessionError> {
            self.kept_alive = true;
            Ok(())
        }

        fn close(&mut self) -> Result<(), super::SshSessionError> {
            self.closed = true;
            Ok(())
        }
    }
}
