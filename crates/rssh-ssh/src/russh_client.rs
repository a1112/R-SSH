use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::{
    SshAuthMethod, SshChannelOpenPlan, SshConnectRequest, SshSessionError, SshSessionStartup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RusshHostKeyPolicy {
    RejectUnknown,
    AcceptUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RusshAuthRequest {
    Password {
        password: String,
    },
    PasswordPrompt,
    PrivateKey {
        path: PathBuf,
        passphrase: Option<String>,
    },
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RusshAuthOutcome {
    Authenticated,
}

impl RusshAuthOutcome {
    /// Converts russh's authentication result into the crate-local session
    /// contract used by the native SSH adapter.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the server rejects the attempted
    /// authentication method.
    pub fn from_auth_result(
        auth_result: &russh::client::AuthResult,
    ) -> Result<Self, SshSessionError> {
        if auth_result.success() {
            return Ok(Self::Authenticated);
        }

        Err(SshSessionError::new("SSH authentication failed"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RusshAuthPlan {
    username: String,
    request: RusshAuthRequest,
}

impl RusshAuthPlan {
    #[must_use]
    pub fn from_request(request: &SshConnectRequest) -> Self {
        let auth_request = match &request.auth {
            SshAuthMethod::Password { password } => RusshAuthRequest::Password {
                password: password.clone(),
            },
            SshAuthMethod::PasswordPrompt => RusshAuthRequest::PasswordPrompt,
            SshAuthMethod::PrivateKey { path, passphrase } => RusshAuthRequest::PrivateKey {
                path: path.clone(),
                passphrase: passphrase.clone(),
            },
            SshAuthMethod::Agent => RusshAuthRequest::Agent,
        };

        Self {
            username: request.config.username.clone(),
            request: auth_request,
        }
    }

    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    #[must_use]
    pub const fn request(&self) -> &RusshAuthRequest {
        &self.request
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RusshChannelStartupRequest {
    RequestPty {
        term: String,
        columns: u32,
        rows: u32,
        pixel_width: u32,
        pixel_height: u32,
    },
    RequestShell,
    Exec {
        command: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RusshChannelStartupPlan {
    requests: Vec<RusshChannelStartupRequest>,
}

impl RusshChannelStartupPlan {
    #[must_use]
    pub fn from_open_plan(open_plan: &SshChannelOpenPlan) -> Self {
        let mut requests = Vec::new();

        if let Some(size) = open_plan.pty_size {
            requests.push(RusshChannelStartupRequest::RequestPty {
                term: "xterm-256color".to_owned(),
                columns: u32::from(size.columns),
                rows: u32::from(size.rows),
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        match &open_plan.startup {
            SshSessionStartup::Shell => {
                requests.push(RusshChannelStartupRequest::RequestShell);
            }
            SshSessionStartup::Command(command) => {
                requests.push(RusshChannelStartupRequest::Exec {
                    command: command.join(" "),
                });
            }
            SshSessionStartup::NoShell => {}
        }

        Self { requests }
    }

    #[must_use]
    pub fn requests(&self) -> &[RusshChannelStartupRequest] {
        &self.requests
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RusshConnectPlan {
    host: String,
    port: u16,
    username: String,
    auth_plan: RusshAuthPlan,
    channel_open_plan: SshChannelOpenPlan,
}

impl RusshConnectPlan {
    #[must_use]
    pub fn from_request(request: &SshConnectRequest) -> Self {
        Self {
            host: request.config.host.clone(),
            port: request.config.port,
            username: request.config.username.clone(),
            auth_plan: RusshAuthPlan::from_request(request),
            channel_open_plan: SshChannelOpenPlan::from_request(request),
        }
    }

    #[must_use]
    pub fn socket_addr(&self) -> (&str, u16) {
        (&self.host, self.port)
    }

    #[must_use]
    pub fn username(&self) -> &str {
        &self.username
    }

    #[must_use]
    pub const fn channel_open_plan(&self) -> &SshChannelOpenPlan {
        &self.channel_open_plan
    }

    #[must_use]
    pub const fn auth_plan(&self) -> &RusshAuthPlan {
        &self.auth_plan
    }

    #[must_use]
    pub fn channel_startup_plan(&self) -> RusshChannelStartupPlan {
        RusshChannelStartupPlan::from_open_plan(&self.channel_open_plan)
    }
}

#[derive(Debug)]
pub struct RusshChannelOpener {
    client_config: Arc<russh::client::Config>,
    host_key_policy: RusshHostKeyPolicy,
}

impl Default for RusshChannelOpener {
    fn default() -> Self {
        let client_config = russh::client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        };

        Self {
            client_config: Arc::new(client_config),
            host_key_policy: RusshHostKeyPolicy::RejectUnknown,
        }
    }
}

impl RusshChannelOpener {
    #[must_use]
    pub fn new(client_config: russh::client::Config) -> Self {
        Self {
            client_config: Arc::new(client_config),
            host_key_policy: RusshHostKeyPolicy::RejectUnknown,
        }
    }

    #[must_use]
    pub const fn with_host_key_policy(mut self, host_key_policy: RusshHostKeyPolicy) -> Self {
        self.host_key_policy = host_key_policy;
        self
    }

    #[must_use]
    pub fn client_config(&self) -> &russh::client::Config {
        self.client_config.as_ref()
    }

    #[must_use]
    pub const fn host_key_policy(&self) -> RusshHostKeyPolicy {
        self.host_key_policy
    }

    #[must_use]
    pub fn into_handler(self) -> RusshClientHandler {
        RusshClientHandler {
            host_key_policy: self.host_key_policy,
        }
    }

    #[must_use]
    pub const fn handler(&self) -> RusshClientHandler {
        RusshClientHandler {
            host_key_policy: self.host_key_policy,
        }
    }

    #[must_use]
    pub fn connect_plan(&self, request: &SshConnectRequest) -> RusshConnectPlan {
        RusshConnectPlan::from_request(request)
    }

    /// Connects the underlying russh transport and returns a connected session
    /// handle. Authentication and channel opening are layered on top of this
    /// transport entry point.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the TCP connection or SSH handshake
    /// fails.
    pub async fn connect_async(
        &self,
        request: SshConnectRequest,
    ) -> Result<russh::client::Handle<RusshClientHandler>, SshSessionError> {
        let plan = self.connect_plan(&request);
        russh::client::connect(
            Arc::clone(&self.client_config),
            plan.socket_addr(),
            self.handler(),
        )
        .await
        .map_err(|error| SshSessionError::new(format!("SSH connect failed: {error}")))
    }

    /// Authenticates a connected russh handle using the planned authentication
    /// branch.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when authentication fails or when the
    /// requested authentication branch is not wired into the native russh
    /// adapter yet.
    pub async fn authenticate_async(
        &self,
        handle: &mut russh::client::Handle<RusshClientHandler>,
        auth_plan: &RusshAuthPlan,
    ) -> Result<RusshAuthOutcome, SshSessionError> {
        match auth_plan.request() {
            RusshAuthRequest::Password { password } => {
                let result = handle
                    .authenticate_password(auth_plan.username(), password)
                    .await
                    .map_err(|error| {
                        SshSessionError::new(format!("SSH password authentication failed: {error}"))
                    })?;
                RusshAuthOutcome::from_auth_result(&result)
            }
            RusshAuthRequest::PasswordPrompt => Err(SshSessionError::new(
                "SSH password prompt authentication is not wired into russh yet",
            )),
            RusshAuthRequest::PrivateKey { .. } => Err(SshSessionError::new(
                "SSH private-key authentication is not wired into russh yet",
            )),
            RusshAuthRequest::Agent => Err(SshSessionError::new(
                "SSH agent authentication is not wired into russh yet",
            )),
        }
    }

    /// Opens a russh session channel on an authenticated handle.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when the server rejects or fails the session
    /// channel open request.
    pub async fn open_session_channel_async(
        &self,
        handle: &russh::client::Handle<RusshClientHandler>,
    ) -> Result<russh::Channel<russh::client::Msg>, SshSessionError> {
        handle
            .channel_open_session()
            .await
            .map_err(|error| SshSessionError::new(format!("SSH channel open failed: {error}")))
    }

    /// Sends the planned PTY, shell, or exec requests to an opened russh
    /// session channel.
    ///
    /// # Errors
    ///
    /// Returns [`SshSessionError`] when a planned channel startup request fails.
    pub async fn start_channel_async(
        &self,
        channel: &russh::Channel<russh::client::Msg>,
        startup_plan: &RusshChannelStartupPlan,
    ) -> Result<(), SshSessionError> {
        for request in startup_plan.requests() {
            match request {
                RusshChannelStartupRequest::RequestPty {
                    term,
                    columns,
                    rows,
                    pixel_width,
                    pixel_height,
                } => {
                    channel
                        .request_pty(
                            true,
                            term,
                            *columns,
                            *rows,
                            *pixel_width,
                            *pixel_height,
                            &[],
                        )
                        .await
                        .map_err(|error| {
                            SshSessionError::new(format!("SSH PTY request failed: {error}"))
                        })?;
                }
                RusshChannelStartupRequest::RequestShell => {
                    channel.request_shell(true).await.map_err(|error| {
                        SshSessionError::new(format!("SSH shell request failed: {error}"))
                    })?;
                }
                RusshChannelStartupRequest::Exec { command } => {
                    channel
                        .exec(true, command.as_bytes())
                        .await
                        .map_err(|error| {
                            SshSessionError::new(format!("SSH exec request failed: {error}"))
                        })?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RusshClientHandler {
    host_key_policy: RusshHostKeyPolicy,
}

impl RusshClientHandler {
    #[must_use]
    pub const fn accepts_unknown_host_keys(self) -> bool {
        matches!(self.host_key_policy, RusshHostKeyPolicy::AcceptUnknown)
    }
}

impl russh::client::Handler for RusshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(self.accepts_unknown_host_keys())
    }
}
