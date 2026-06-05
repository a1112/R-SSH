use std::path::PathBuf;

use rssh_core::TerminalSize;

mod russh_client;

pub use russh_client::{
    RusshAuthOutcome, RusshAuthPlan, RusshAuthRequest, RusshChannelOpener, RusshChannelStartupPlan,
    RusshChannelStartupRequest, RusshClientHandler, RusshConnectPlan, RusshHostKeyPolicy,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshStartupError {
    EmptyCommand,
}

impl std::fmt::Display for SshStartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyCommand => "SSH remote command cannot be empty",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SshStartupError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshSessionStartup {
    Shell,
    Command(Vec<String>),
    NoShell,
}

impl SshSessionStartup {
    /// Creates a remote command startup request.
    ///
    /// # Errors
    ///
    /// Returns [`SshStartupError::EmptyCommand`] when no command tokens are
    /// provided or every token is empty.
    pub fn command(command: impl IntoIterator<Item = String>) -> Result<Self, SshStartupError> {
        let command = command.into_iter().collect::<Vec<_>>();
        if command.iter().all(|argument| argument.trim().is_empty()) {
            return Err(SshStartupError::EmptyCommand);
        }

        Ok(Self::Command(command))
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
    pub startup: SshSessionStartup,
}

impl SshConnectRequest {
    #[must_use]
    pub fn new(config: SshSessionConfig, auth: SshAuthMethod) -> Self {
        Self {
            config,
            auth,
            startup: SshSessionStartup::Shell,
        }
    }

    #[must_use]
    pub fn with_startup(mut self, startup: SshSessionStartup) -> Self {
        self.startup = startup;
        self
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
    pub fn password_prompt(config: SshSessionConfig) -> Self {
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
    pub fn agent(config: SshSessionConfig) -> Self {
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
    /// Connects to an SSH server and starts the requested shell, command, or
    /// no-shell session.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when connecting, authenticating, requesting
    /// the remote PTY, or starting the requested channel mode fails.
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

pub trait SshChannel: Send {
    /// Reads bytes from an established remote shell channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the underlying SSH backend cannot read
    /// from the channel.
    fn read_channel(&mut self, buffer: &mut [u8]) -> Result<usize, SshSessionError>;

    /// Writes bytes to an established remote shell channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the underlying SSH backend cannot write
    /// to the channel.
    fn write_channel(&mut self, bytes: &[u8]) -> Result<usize, SshSessionError>;

    /// Resizes the remote PTY bound to the channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the backend rejects the PTY resize.
    fn resize_pty(&mut self, size: TerminalSize) -> Result<(), SshSessionError>;

    /// Sends a keepalive request through the SSH connection.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the backend cannot send the keepalive.
    fn send_keepalive(&mut self) -> Result<(), SshSessionError>;

    /// Closes the remote shell channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the backend cannot close the channel.
    fn close_channel(&mut self) -> Result<(), SshSessionError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshChannelOpenPlan {
    pub pty_size: Option<TerminalSize>,
    pub startup: SshSessionStartup,
}

impl SshChannelOpenPlan {
    #[must_use]
    pub fn from_request(request: &SshConnectRequest) -> Self {
        let pty_size = match request.startup {
            SshSessionStartup::Shell | SshSessionStartup::Command(_) => {
                Some(request.config.initial_size)
            }
            SshSessionStartup::NoShell => None,
        };

        Self {
            pty_size,
            startup: request.startup.clone(),
        }
    }
}

pub struct SshChannelSession<C> {
    channel: C,
}

impl<C> SshChannelSession<C> {
    #[must_use]
    pub const fn new(channel: C) -> Self {
        Self { channel }
    }

    #[must_use]
    pub fn into_channel(self) -> C {
        self.channel
    }
}

impl<C> SshShellSession for SshChannelSession<C>
where
    C: SshChannel,
{
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SshSessionError> {
        self.channel.read_channel(buffer)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<usize, SshSessionError> {
        self.channel.write_channel(bytes)
    }

    fn resize(&mut self, size: TerminalSize) -> Result<(), SshSessionError> {
        self.channel.resize_pty(size)
    }

    fn keepalive(&mut self) -> Result<(), SshSessionError> {
        self.channel.send_keepalive()
    }

    fn close(&mut self) -> Result<(), SshSessionError> {
        self.channel.close_channel()
    }
}

pub trait SshChannelOpener {
    type Channel: SshChannel;

    /// Opens an authenticated remote channel for the requested SSH session.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when connecting, authenticating, requesting a
    /// PTY, or starting the requested shell, command, or no-shell channel
    /// fails.
    fn open_channel(
        &mut self,
        request: SshConnectRequest,
    ) -> Result<Self::Channel, SshSessionError>;
}

pub struct SshChannelConnector<O> {
    opener: O,
}

impl<O> SshChannelConnector<O> {
    #[must_use]
    pub const fn new(opener: O) -> Self {
        Self { opener }
    }

    #[must_use]
    pub fn into_opener(self) -> O {
        self.opener
    }
}

impl<O> SshShellConnector for SshChannelConnector<O>
where
    O: SshChannelOpener,
    O::Channel: 'static,
{
    fn connect(
        &mut self,
        request: SshConnectRequest,
    ) -> Result<Box<dyn SshShellSession>, SshSessionError> {
        let channel = self.opener.open_channel(request)?;
        Ok(Box::new(SshChannelSession::new(channel)))
    }
}

/// Runs an SSH shell session by copying local input to the remote session and
/// remote output to the provided output sink.
///
/// # Errors
///
/// Returns [`SshSessionError`] when connecting, reading local input, writing to
/// the SSH session, reading remote output, writing output, or closing the
/// session fails.
pub fn run_shell_with_io(
    connector: &mut dyn SshShellConnector,
    request: SshConnectRequest,
    input: &mut dyn std::io::Read,
    output: &mut dyn std::io::Write,
) -> Result<(), SshSessionError> {
    let mut session = connector.connect(request)?;
    copy_input_to_session(input, session.as_mut())?;

    let mut buffer = [0; 8192];
    loop {
        let count = session.read(&mut buffer)?;
        if count == 0 {
            break;
        }

        output
            .write_all(&buffer[..count])
            .map_err(|error| SshSessionError::new(error.to_string()))?;
        output
            .flush()
            .map_err(|error| SshSessionError::new(error.to_string()))?;
    }

    session.close()
}

fn copy_input_to_session(
    input: &mut dyn std::io::Read,
    session: &mut dyn SshShellSession,
) -> Result<(), SshSessionError> {
    let mut buffer = [0; 8192];

    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| SshSessionError::new(error.to_string()))?;
        if count == 0 {
            return Ok(());
        }

        let mut written = 0;
        while written < count {
            let next = session.write(&buffer[written..count])?;
            if next == 0 {
                return Err(SshSessionError::new(
                    "SSH session write returned zero bytes",
                ));
            }
            written += next;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use rssh_core::TerminalSize;

    use super::{
        SshAuthError, SshAuthMethod, SshChannel, SshChannelConnector, SshChannelOpenPlan,
        SshChannelOpener, SshChannelSession, SshConnectRequest, SshSessionConfig,
        SshSessionStartup, SshShellConnector, SshShellSession, SshStartupError,
    };

    use crate::{
        RusshAuthOutcome, RusshAuthPlan, RusshAuthRequest, RusshChannelStartupPlan,
        RusshChannelStartupRequest, RusshConnectPlan, RusshHostKeyPolicy,
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
    fn ssh_channel_session_delegates_shell_io_and_lifecycle() {
        let channel = MockSshChannel::new(b"pong".to_vec());
        let mut session = SshChannelSession::new(channel);
        let mut output = [0; 4];

        assert_eq!(session.read(&mut output).unwrap(), 4);
        assert_eq!(&output, b"pong");
        assert_eq!(session.write(b"ping").unwrap(), 4);
        session.resize(TerminalSize::new(132, 43)).unwrap();
        session.keepalive().unwrap();
        session.close().unwrap();

        let channel = session.into_channel();
        assert_eq!(channel.written, b"ping");
        assert_eq!(channel.sizes, vec![TerminalSize::new(132, 43)]);
        assert_eq!(channel.keepalives, 1);
        assert!(channel.closed);
    }

    #[test]
    fn ssh_channel_connector_opens_channel_and_returns_shell_session() {
        let request = SshConnectRequest::agent(valid_config());
        let mut connector = SshChannelConnector::new(MockSshOpener::new(b"pong".to_vec()));

        let mut session = connector.connect(request.clone()).unwrap();
        let mut output = [0; 4];

        assert_eq!(session.read(&mut output).unwrap(), 4);
        assert_eq!(&output, b"pong");
        assert_eq!(session.write(b"ping").unwrap(), 4);
        session.resize(TerminalSize::new(132, 43)).unwrap();
        session.keepalive().unwrap();
        session.close().unwrap();

        let opener = connector.into_opener();
        assert_eq!(opener.last_request, Some(request));
        let channel = opener.recorded.lock().unwrap().take().unwrap();
        assert_eq!(channel.written, b"ping");
        assert_eq!(channel.sizes, vec![TerminalSize::new(132, 43)]);
        assert_eq!(channel.keepalives, 1);
        assert!(channel.closed);
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
    fn connect_request_defaults_to_shell_startup() {
        let request = SshConnectRequest::agent(valid_config());

        assert_eq!(request.startup, SshSessionStartup::Shell);
    }

    #[test]
    fn connect_request_accepts_remote_command_startup() {
        let request = SshConnectRequest::agent(valid_config()).with_startup(
            SshSessionStartup::command(["uname".to_owned(), "-a".to_owned()]).unwrap(),
        );

        assert_eq!(
            request.startup,
            SshSessionStartup::Command(vec!["uname".to_owned(), "-a".to_owned()])
        );
    }

    #[test]
    fn session_startup_rejects_empty_remote_command() {
        assert_eq!(
            SshSessionStartup::command(Vec::<String>::new()),
            Err(SshStartupError::EmptyCommand)
        );
    }

    #[test]
    fn channel_open_plan_requests_pty_for_shell_startup() {
        let request = SshConnectRequest::agent(valid_config());

        let plan = SshChannelOpenPlan::from_request(&request);

        assert_eq!(plan.pty_size, Some(TerminalSize::new(100, 40)));
        assert_eq!(plan.startup, SshSessionStartup::Shell);
    }

    #[test]
    fn channel_open_plan_requests_pty_for_remote_command_startup() {
        let request = SshConnectRequest::agent(valid_config()).with_startup(
            SshSessionStartup::command(["uname".to_owned(), "-a".to_owned()]).unwrap(),
        );

        let plan = SshChannelOpenPlan::from_request(&request);

        assert_eq!(plan.pty_size, Some(TerminalSize::new(100, 40)));
        assert_eq!(
            plan.startup,
            SshSessionStartup::Command(vec!["uname".to_owned(), "-a".to_owned()])
        );
    }

    #[test]
    fn channel_open_plan_skips_pty_for_no_shell_startup() {
        let request =
            SshConnectRequest::agent(valid_config()).with_startup(SshSessionStartup::NoShell);

        let plan = SshChannelOpenPlan::from_request(&request);

        assert_eq!(plan.pty_size, None);
        assert_eq!(plan.startup, SshSessionStartup::NoShell);
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

    #[test]
    fn shell_runner_streams_remote_output_and_closes_session() {
        let request = SshConnectRequest::agent(valid_config());
        let state = Arc::new(Mutex::new(MockRunnerState::default()));
        let mut connector = MockRunnerConnector {
            state: Arc::clone(&state),
        };
        let mut output = Vec::new();

        super::run_shell_with_io(
            &mut connector,
            request.clone(),
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.last_request.as_ref(), Some(&request));
        assert_eq!(output, b"remote\n");
        assert!(state.closed);
    }

    #[test]
    fn shell_runner_writes_local_input_to_remote_session() {
        let request = SshConnectRequest::agent(valid_config());
        let state = Arc::new(Mutex::new(MockRunnerState::default()));
        let mut connector = MockRunnerConnector {
            state: Arc::clone(&state),
        };
        let mut input = &b"echo hi\n"[..];
        let mut output = Vec::new();

        super::run_shell_with_io(&mut connector, request, &mut input, &mut output).unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.written, b"echo hi\n");
        assert_eq!(output, b"remote\n");
        assert!(state.closed);
    }

    #[test]
    fn russh_channel_opener_exposes_client_config() {
        let opener = super::RusshChannelOpener::default();

        let config = opener.client_config();

        assert!(config.keepalive_interval.is_some());
    }

    #[test]
    fn russh_channel_opener_defaults_to_rejecting_unknown_host_keys() {
        let opener = super::RusshChannelOpener::default();

        assert_eq!(opener.host_key_policy(), RusshHostKeyPolicy::RejectUnknown);
    }

    #[test]
    fn russh_channel_opener_can_be_configured_to_accept_unknown_host_keys() {
        let opener = super::RusshChannelOpener::default()
            .with_host_key_policy(RusshHostKeyPolicy::AcceptUnknown);

        assert_eq!(opener.host_key_policy(), RusshHostKeyPolicy::AcceptUnknown);
        assert!(opener.into_handler().accepts_unknown_host_keys());
    }

    #[test]
    fn russh_connect_plan_builds_socket_address_and_username_from_request() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new(
                "ssh.example.com",
                2222,
                "deploy",
                TerminalSize::new(120, 30),
            )
            .unwrap(),
        );

        let plan = RusshConnectPlan::from_request(&request);

        assert_eq!(plan.socket_addr(), ("ssh.example.com", 2222));
        assert_eq!(plan.username(), "deploy");
    }

    #[test]
    fn russh_connect_plan_carries_channel_open_plan_from_request() {
        let request = SshConnectRequest::agent(valid_config())
            .with_startup(SshSessionStartup::command(["uptime".to_owned()]).unwrap());

        let plan = RusshConnectPlan::from_request(&request);

        assert_eq!(
            plan.channel_open_plan(),
            &SshChannelOpenPlan {
                pty_size: Some(TerminalSize::new(100, 40)),
                startup: SshSessionStartup::Command(vec!["uptime".to_owned()])
            }
        );
    }

    #[test]
    fn russh_channel_opener_builds_connect_plan_for_request() {
        let request =
            SshConnectRequest::agent(valid_config()).with_startup(SshSessionStartup::NoShell);
        let opener = super::RusshChannelOpener::default();

        let plan = opener.connect_plan(&request);

        assert_eq!(plan.socket_addr(), ("example.com", 22));
        assert_eq!(plan.username(), "ops");
        assert_eq!(
            plan.channel_open_plan(),
            &SshChannelOpenPlan {
                pty_size: None,
                startup: SshSessionStartup::NoShell
            }
        );
    }

    #[test]
    fn russh_channel_opener_exposes_async_connect_entrypoint() {
        let opener = super::RusshChannelOpener::default();
        let request = SshConnectRequest::agent(valid_config());

        let future = opener.connect_async(request);

        drop(future);
    }

    #[test]
    fn russh_connect_plan_derives_channel_startup_plan() {
        let request = SshConnectRequest::agent(valid_config());
        let plan = RusshConnectPlan::from_request(&request);

        assert_eq!(
            plan.channel_startup_plan().requests(),
            &[
                RusshChannelStartupRequest::RequestPty {
                    term: "xterm-256color".to_owned(),
                    columns: 100,
                    rows: 40,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                RusshChannelStartupRequest::RequestShell
            ]
        );
    }

    #[test]
    fn russh_auth_plan_maps_password_value_authentication() {
        let request = SshConnectRequest::password(valid_config(), "secret").unwrap();

        let plan = RusshAuthPlan::from_request(&request);

        assert_eq!(plan.username(), "ops");
        assert_eq!(
            plan.request(),
            &RusshAuthRequest::Password {
                password: "secret".to_owned()
            }
        );
    }

    #[test]
    fn russh_auth_plan_maps_password_prompt_authentication() {
        let request = SshConnectRequest::password_prompt(valid_config());

        let plan = RusshAuthPlan::from_request(&request);

        assert_eq!(plan.username(), "ops");
        assert_eq!(plan.request(), &RusshAuthRequest::PasswordPrompt);
    }

    #[test]
    fn russh_auth_plan_maps_private_key_authentication() {
        let request = SshConnectRequest::private_key(
            valid_config(),
            PathBuf::from("C:/Users/ops/.ssh/id_ed25519"),
            Some("secret"),
        )
        .unwrap();

        let plan = RusshAuthPlan::from_request(&request);

        assert_eq!(plan.username(), "ops");
        assert_eq!(
            plan.request(),
            &RusshAuthRequest::PrivateKey {
                path: PathBuf::from("C:/Users/ops/.ssh/id_ed25519"),
                passphrase: Some("secret".to_owned())
            }
        );
    }

    #[test]
    fn russh_auth_plan_maps_agent_authentication() {
        let request = SshConnectRequest::agent(valid_config());

        let plan = RusshAuthPlan::from_request(&request);

        assert_eq!(plan.username(), "ops");
        assert_eq!(plan.request(), &RusshAuthRequest::Agent);
    }

    #[test]
    fn russh_connect_plan_derives_auth_plan() {
        let request = SshConnectRequest::agent(valid_config());
        let plan = RusshConnectPlan::from_request(&request);

        assert_eq!(plan.auth_plan(), &RusshAuthPlan::from_request(&request));
    }

    #[test]
    fn russh_auth_outcome_accepts_successful_authentication() {
        let outcome =
            RusshAuthOutcome::from_auth_result(&russh::client::AuthResult::Success).unwrap();

        assert_eq!(outcome, RusshAuthOutcome::Authenticated);
    }

    #[test]
    fn russh_auth_outcome_rejects_failed_authentication() {
        let error = RusshAuthOutcome::from_auth_result(&russh::client::AuthResult::Failure {
            remaining_methods: russh::MethodSet::empty(),
            partial_success: false,
        })
        .unwrap_err();

        assert!(error.to_string().contains("SSH authentication failed"));
    }

    #[test]
    fn russh_channel_opener_exposes_async_authentication_entrypoint() {
        let authenticate = super::RusshChannelOpener::authenticate_async;

        let _ = authenticate;
    }

    #[test]
    fn russh_channel_opener_exposes_async_session_channel_entrypoint() {
        let open_channel = super::RusshChannelOpener::open_session_channel_async;

        let _ = open_channel;
    }

    #[test]
    fn russh_channel_opener_exposes_async_channel_startup_entrypoint() {
        let start_channel = super::RusshChannelOpener::start_channel_async;

        let _ = start_channel;
    }

    #[test]
    fn russh_channel_startup_plan_requests_pty_then_shell_for_shell_startup() {
        let open_plan = SshChannelOpenPlan {
            pty_size: Some(TerminalSize::new(120, 30)),
            startup: SshSessionStartup::Shell,
        };

        let plan = RusshChannelStartupPlan::from_open_plan(&open_plan);

        assert_eq!(
            plan.requests(),
            &[
                RusshChannelStartupRequest::RequestPty {
                    term: "xterm-256color".to_owned(),
                    columns: 120,
                    rows: 30,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                RusshChannelStartupRequest::RequestShell
            ]
        );
    }

    #[test]
    fn russh_channel_startup_plan_requests_pty_then_exec_for_remote_command() {
        let open_plan = SshChannelOpenPlan {
            pty_size: Some(TerminalSize::new(100, 40)),
            startup: SshSessionStartup::Command(vec!["uptime".to_owned()]),
        };

        let plan = RusshChannelStartupPlan::from_open_plan(&open_plan);

        assert_eq!(
            plan.requests(),
            &[
                RusshChannelStartupRequest::RequestPty {
                    term: "xterm-256color".to_owned(),
                    columns: 100,
                    rows: 40,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                RusshChannelStartupRequest::Exec {
                    command: "uptime".to_owned()
                }
            ]
        );
    }

    #[test]
    fn russh_channel_startup_plan_skips_channel_requests_for_no_shell_startup() {
        let open_plan = SshChannelOpenPlan {
            pty_size: None,
            startup: SshSessionStartup::NoShell,
        };

        let plan = RusshChannelStartupPlan::from_open_plan(&open_plan);

        assert_eq!(plan.requests(), &[]);
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

    #[derive(Default)]
    struct MockRunnerState {
        last_request: Option<SshConnectRequest>,
        written: Vec<u8>,
        closed: bool,
    }

    struct MockRunnerConnector {
        state: Arc<Mutex<MockRunnerState>>,
    }

    impl SshShellConnector for MockRunnerConnector {
        fn connect(
            &mut self,
            request: SshConnectRequest,
        ) -> Result<Box<dyn SshShellSession>, super::SshSessionError> {
            self.state.lock().unwrap().last_request = Some(request);
            Ok(Box::new(MockRunnerSession {
                state: Arc::clone(&self.state),
                read_once: false,
            }))
        }
    }

    struct MockRunnerSession {
        state: Arc<Mutex<MockRunnerState>>,
        read_once: bool,
    }

    impl SshShellSession for MockRunnerSession {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, super::SshSessionError> {
            if self.read_once {
                return Ok(0);
            }
            self.read_once = true;
            buffer[..7].copy_from_slice(b"remote\n");
            Ok(7)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, super::SshSessionError> {
            self.state.lock().unwrap().written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn resize(&mut self, _size: TerminalSize) -> Result<(), super::SshSessionError> {
            Ok(())
        }

        fn keepalive(&mut self) -> Result<(), super::SshSessionError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), super::SshSessionError> {
            self.state.lock().unwrap().closed = true;
            Ok(())
        }
    }

    struct MockSshChannel {
        output: Vec<u8>,
        written: Vec<u8>,
        sizes: Vec<TerminalSize>,
        keepalives: u32,
        closed: bool,
    }

    impl MockSshChannel {
        fn new(output: Vec<u8>) -> Self {
            Self {
                output,
                written: Vec::new(),
                sizes: Vec::new(),
                keepalives: 0,
                closed: false,
            }
        }
    }

    impl SshChannel for MockSshChannel {
        fn read_channel(&mut self, buffer: &mut [u8]) -> Result<usize, super::SshSessionError> {
            let count = buffer.len().min(self.output.len());
            buffer[..count].copy_from_slice(&self.output[..count]);
            self.output.drain(..count);
            Ok(count)
        }

        fn write_channel(&mut self, bytes: &[u8]) -> Result<usize, super::SshSessionError> {
            self.written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn resize_pty(&mut self, size: TerminalSize) -> Result<(), super::SshSessionError> {
            self.sizes.push(size);
            Ok(())
        }

        fn send_keepalive(&mut self) -> Result<(), super::SshSessionError> {
            self.keepalives += 1;
            Ok(())
        }

        fn close_channel(&mut self) -> Result<(), super::SshSessionError> {
            self.closed = true;
            Ok(())
        }
    }

    struct MockSshOpener {
        output: Vec<u8>,
        last_request: Option<SshConnectRequest>,
        recorded: Arc<Mutex<Option<MockSshChannel>>>,
    }

    impl MockSshOpener {
        fn new(output: Vec<u8>) -> Self {
            Self {
                output,
                last_request: None,
                recorded: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl SshChannelOpener for MockSshOpener {
        type Channel = RecordingSshChannel;

        fn open_channel(
            &mut self,
            request: SshConnectRequest,
        ) -> Result<Self::Channel, super::SshSessionError> {
            self.last_request = Some(request);
            Ok(RecordingSshChannel {
                channel: MockSshChannel::new(std::mem::take(&mut self.output)),
                recorded: Arc::clone(&self.recorded),
            })
        }
    }

    struct RecordingSshChannel {
        channel: MockSshChannel,
        recorded: Arc<Mutex<Option<MockSshChannel>>>,
    }

    impl SshChannel for RecordingSshChannel {
        fn read_channel(&mut self, buffer: &mut [u8]) -> Result<usize, super::SshSessionError> {
            self.channel.read_channel(buffer)
        }

        fn write_channel(&mut self, bytes: &[u8]) -> Result<usize, super::SshSessionError> {
            self.channel.write_channel(bytes)
        }

        fn resize_pty(&mut self, size: TerminalSize) -> Result<(), super::SshSessionError> {
            self.channel.resize_pty(size)
        }

        fn send_keepalive(&mut self) -> Result<(), super::SshSessionError> {
            self.channel.send_keepalive()
        }

        fn close_channel(&mut self) -> Result<(), super::SshSessionError> {
            self.channel.close_channel()?;
            *self.recorded.lock().unwrap() = Some(std::mem::replace(
                &mut self.channel,
                MockSshChannel::new(Vec::new()),
            ));
            Ok(())
        }
    }
}
