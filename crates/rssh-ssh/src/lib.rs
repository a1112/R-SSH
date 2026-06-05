use std::path::PathBuf;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshAuthError {
    EmptyPassword,
    EmptyPrivateKeyPath,
}

impl std::fmt::Display for SshAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyPassword => "SSH password cannot be empty",
            Self::EmptyPrivateKeyPath => "SSH private key path cannot be empty",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SshAuthError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshAuthMethod {
    PasswordPrompt,
    Password {
        password: String,
    },
    PrivateKey {
        path: PathBuf,
        passphrase: Option<String>,
    },
    Agent,
}

impl SshAuthMethod {
    #[must_use]
    pub const fn password_prompt() -> Self {
        Self::PasswordPrompt
    }

    /// Creates password authentication data.
    ///
    /// # Errors
    ///
    /// Returns [`SshAuthError::EmptyPassword`] when the password is empty or
    /// only whitespace.
    pub fn password(password: impl Into<String>) -> Result<Self, SshAuthError> {
        let password = password.into();
        if password.trim().is_empty() {
            return Err(SshAuthError::EmptyPassword);
        }

        Ok(Self::Password { password })
    }

    /// Creates private-key authentication data.
    ///
    /// # Errors
    ///
    /// Returns [`SshAuthError::EmptyPrivateKeyPath`] when `path` is empty.
    pub fn private_key(
        path: impl Into<PathBuf>,
        passphrase: Option<impl Into<String>>,
    ) -> Result<Self, SshAuthError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(SshAuthError::EmptyPrivateKeyPath);
        }

        Ok(Self::PrivateKey {
            path,
            passphrase: passphrase.map(Into::into),
        })
    }
}

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
pub struct SshConnectRequest {
    pub config: SshSessionConfig,
    pub auth: SshAuthMethod,
}

impl SshConnectRequest {
    #[must_use]
    pub const fn new(config: SshSessionConfig, auth: SshAuthMethod) -> Self {
        Self { config, auth }
    }

    /// Creates a connection request with password authentication.
    ///
    /// # Errors
    ///
    /// Returns [`SshAuthError::EmptyPassword`] when the password is empty or
    /// only whitespace.
    pub fn password(
        config: SshSessionConfig,
        password: impl Into<String>,
    ) -> Result<Self, SshAuthError> {
        Ok(Self::new(config, SshAuthMethod::password(password)?))
    }

    /// Creates a connection request that should prompt for a password through
    /// the active terminal or a future secure prompt.
    #[must_use]
    pub const fn password_prompt(config: SshSessionConfig) -> Self {
        Self::new(config, SshAuthMethod::PasswordPrompt)
    }

    /// Creates a connection request with private-key authentication.
    ///
    /// # Errors
    ///
    /// Returns [`SshAuthError::EmptyPrivateKeyPath`] when `path` is empty.
    pub fn private_key(
        config: SshSessionConfig,
        path: impl Into<PathBuf>,
        passphrase: Option<impl Into<String>>,
    ) -> Result<Self, SshAuthError> {
        Ok(Self::new(
            config,
            SshAuthMethod::private_key(path, passphrase)?,
        ))
    }

    #[must_use]
    pub const fn agent(config: SshSessionConfig) -> Self {
        Self::new(config, SshAuthMethod::Agent)
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

pub trait SshShellConnector {
    /// Connects to an SSH server and starts a shell session.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when connecting, authenticating, requesting
    /// the remote PTY, or starting the shell fails.
    fn connect(
        &mut self,
        request: SshConnectRequest,
    ) -> Result<Box<dyn SshShellSession>, SshSessionError>;
}

pub trait SshShellSession: Send {
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
    use std::path::PathBuf;

    use rssh_core::TerminalSize;

    use super::{
        SshAuthError, SshAuthMethod, SshConnectRequest, SshSessionConfig, SshShellConnector,
        SshShellSession,
    };

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

    #[test]
    fn connect_request_accepts_password_auth() {
        let config = valid_config();
        let request = SshConnectRequest::password(config.clone(), "secret").unwrap();

        assert_eq!(request.config, config);
        assert_eq!(
            request.auth,
            SshAuthMethod::Password {
                password: "secret".to_owned()
            }
        );
    }

    #[test]
    fn connect_request_rejects_empty_password() {
        let error = SshConnectRequest::password(valid_config(), " ").unwrap_err();

        assert_eq!(error, SshAuthError::EmptyPassword);
    }

    #[test]
    fn connect_request_accepts_password_prompt_auth() {
        let config = valid_config();
        let request = SshConnectRequest::password_prompt(config.clone());

        assert_eq!(request.config, config);
        assert_eq!(request.auth, SshAuthMethod::PasswordPrompt);
    }

    #[test]
    fn connect_request_accepts_private_key_auth() {
        let config = valid_config();
        let request = SshConnectRequest::private_key(
            config.clone(),
            PathBuf::from("C:/Users/ops/.ssh/id_ed25519"),
            Some("secret"),
        )
        .unwrap();

        assert_eq!(request.config, config);
        assert_eq!(
            request.auth,
            SshAuthMethod::PrivateKey {
                path: PathBuf::from("C:/Users/ops/.ssh/id_ed25519"),
                passphrase: Some("secret".to_owned())
            }
        );
    }

    #[test]
    fn connect_request_rejects_empty_private_key_path() {
        let error = SshConnectRequest::private_key(valid_config(), PathBuf::new(), None::<String>)
            .unwrap_err();

        assert_eq!(error, SshAuthError::EmptyPrivateKeyPath);
    }

    #[test]
    fn connect_request_accepts_agent_auth() {
        let config = valid_config();
        let request = SshConnectRequest::agent(config.clone());

        assert_eq!(request.config, config);
        assert_eq!(request.auth, SshAuthMethod::Agent);
    }

    #[test]
    fn ssh_shell_connector_trait_creates_shell_session_from_request() {
        let request = SshConnectRequest::agent(valid_config());
        let mut connector = MockSshConnector::default();

        let mut session = connector.connect(request.clone()).unwrap();
        let mut output = [0; 4];

        assert_eq!(session.read(&mut output).unwrap(), 4);
        assert_eq!(&output, b"pong");
        assert_eq!(session.write(b"ping").unwrap(), 4);
        assert_eq!(connector.last_request, Some(request));
    }

    fn valid_config() -> SshSessionConfig {
        SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(100, 40)).unwrap()
    }

    #[derive(Default)]
    struct MockSshConnector {
        last_request: Option<SshConnectRequest>,
    }

    impl SshShellConnector for MockSshConnector {
        fn connect(
            &mut self,
            request: SshConnectRequest,
        ) -> Result<Box<dyn SshShellSession>, super::SshSessionError> {
            self.last_request = Some(request);
            Ok(Box::new(MockSshSession::default()))
        }
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
