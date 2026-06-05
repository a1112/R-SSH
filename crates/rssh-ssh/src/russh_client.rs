use std::{
    collections::VecDeque,
    future::Future,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use crate::{
    SshAuthMethod, SshChannel, SshChannelOpenPlan, SshChannelOpener, SshConnectRequest,
    SshSessionError, SshSessionStartup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RusshHostKeyPolicy {
    RejectUnknown,
    TrustOnFirstUse,
    AcceptUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RusshKnownHosts {
    path: PathBuf,
}

impl RusshKnownHosts {
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Checks whether `key` matches the recorded host key for `host:port`.
    ///
    /// # Errors
    ///
    /// Returns russh key errors when the known-hosts file cannot be parsed or
    /// contains a changed key for the same host and algorithm.
    pub fn matches(
        &self,
        host: &str,
        port: u16,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, russh::keys::Error> {
        russh::keys::known_hosts::check_known_hosts_path(host, port, key, &self.path)
    }

    /// Records `key` for `host:port` in this known-hosts file.
    ///
    /// # Errors
    ///
    /// Returns russh key errors when the file cannot be created or written.
    pub fn learn(
        &self,
        host: &str,
        port: u16,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<(), russh::keys::Error> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&self.path)?;
        let mut needs_newline = false;
        if file.seek(SeekFrom::End(-1)).is_ok() {
            let mut last_byte = [0; 1];
            file.read_exact(&mut last_byte)?;
            needs_newline = last_byte[0] != b'\n';
        }

        file.seek(SeekFrom::End(0))?;
        if needs_newline {
            file.write_all(b"\n")?;
        }

        if port == 22 {
            write!(file, "{host} ")?;
        } else {
            write!(file, "[{host}]:{port} ")?;
        }
        file.write_all(key.to_openssh()?.as_bytes())?;
        file.write_all(b"\n")?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct RusshPrivateKeyAuth {
    key: Arc<russh::keys::PrivateKey>,
}

impl RusshPrivateKeyAuth {
    /// Loads an OpenSSH-compatible private key from disk.
    ///
    /// # Errors
    ///
    /// Returns russh key errors when the key file cannot be read, decoded, or
    /// decrypted with the supplied passphrase.
    pub fn load(
        path: impl AsRef<std::path::Path>,
        passphrase: Option<&str>,
    ) -> Result<Self, russh::keys::Error> {
        let key = russh::keys::load_secret_key(path, passphrase)?;
        Ok(Self { key: Arc::new(key) })
    }

    /// Returns whether a private key requires a passphrase to decrypt.
    ///
    /// # Errors
    ///
    /// Returns russh key errors when the key file cannot be read or decoded.
    pub fn needs_passphrase(path: impl AsRef<std::path::Path>) -> Result<bool, russh::keys::Error> {
        match Self::load(path, None) {
            Ok(_) => Ok(false),
            Err(russh::keys::Error::KeyIsEncrypted) => Ok(true),
            Err(error) => Err(error),
        }
    }

    #[must_use]
    pub fn algorithm(&self) -> russh::keys::ssh_key::Algorithm {
        self.key.algorithm()
    }

    #[must_use]
    fn into_private_key_with_hash_alg(
        self,
        hash_alg: Option<russh::keys::HashAlg>,
    ) -> russh::keys::PrivateKeyWithHashAlg {
        russh::keys::PrivateKeyWithHashAlg::new(self.key, hash_alg)
    }
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

type RusshAuthFuture<'a> =
    Pin<Box<dyn Future<Output = Result<russh::client::AuthResult, SshSessionError>> + 'a>>;

trait RusshAuthenticationBackend {
    fn authenticate_password<'a>(
        &'a mut self,
        username: &'a str,
        password: &'a str,
    ) -> RusshAuthFuture<'a>;

    fn authenticate_private_key<'a>(
        &'a mut self,
        username: &'a str,
        path: &'a Path,
        passphrase: Option<&'a str>,
    ) -> RusshAuthFuture<'a>;

    fn authenticate_agent<'a>(&'a mut self, username: &'a str) -> RusshAuthFuture<'a>;
}

async fn authenticate_auth_plan_with_backend(
    backend: &mut impl RusshAuthenticationBackend,
    auth_plan: &RusshAuthPlan,
) -> Result<RusshAuthOutcome, SshSessionError> {
    let result = match auth_plan.request() {
        RusshAuthRequest::Password { password } => {
            backend
                .authenticate_password(auth_plan.username(), password)
                .await?
        }
        RusshAuthRequest::PasswordPrompt => {
            return Err(SshSessionError::new(
                "SSH password prompt authentication is not wired into russh yet",
            ));
        }
        RusshAuthRequest::PrivateKey { path, passphrase } => {
            backend
                .authenticate_private_key(auth_plan.username(), path, passphrase.as_deref())
                .await?
        }
        RusshAuthRequest::Agent => backend.authenticate_agent(auth_plan.username()).await?,
    };

    RusshAuthOutcome::from_auth_result(&result)
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
    known_hosts_path: Option<PathBuf>,
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
            known_hosts_path: None,
        }
    }
}

impl RusshChannelOpener {
    #[must_use]
    pub fn new(client_config: russh::client::Config) -> Self {
        Self {
            client_config: Arc::new(client_config),
            host_key_policy: RusshHostKeyPolicy::RejectUnknown,
            known_hosts_path: None,
        }
    }

    #[must_use]
    pub const fn with_host_key_policy(mut self, host_key_policy: RusshHostKeyPolicy) -> Self {
        self.host_key_policy = host_key_policy;
        self
    }

    #[must_use]
    pub fn with_known_hosts_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.known_hosts_path = Some(path.into());
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
    pub fn known_hosts_path(&self) -> Option<&std::path::Path> {
        self.known_hosts_path.as_deref()
    }

    #[must_use]
    pub fn into_handler(self) -> RusshClientHandler {
        RusshClientHandler {
            host_key_policy: self.host_key_policy,
            host: None,
            port: None,
            known_hosts_path: self.known_hosts_path,
        }
    }

    #[must_use]
    pub fn handler(&self) -> RusshClientHandler {
        RusshClientHandler {
            host_key_policy: self.host_key_policy,
            host: None,
            port: None,
            known_hosts_path: self.known_hosts_path.clone(),
        }
    }

    #[must_use]
    pub fn handler_for_host(&self, host: impl Into<String>, port: u16) -> RusshClientHandler {
        RusshClientHandler {
            host_key_policy: self.host_key_policy,
            host: Some(host.into()),
            port: Some(port),
            known_hosts_path: self.known_hosts_path.clone(),
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
        let (host, port) = plan.socket_addr();
        russh::client::connect(
            Arc::clone(&self.client_config),
            (host, port),
            self.handler_for_host(host, port),
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
        let mut backend = RusshHandleAuthenticationBackend { handle };
        authenticate_auth_plan_with_backend(&mut backend, auth_plan).await
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

struct RusshHandleAuthenticationBackend<'a> {
    handle: &'a mut russh::client::Handle<RusshClientHandler>,
}

impl RusshAuthenticationBackend for RusshHandleAuthenticationBackend<'_> {
    fn authenticate_password<'a>(
        &'a mut self,
        username: &'a str,
        password: &'a str,
    ) -> RusshAuthFuture<'a> {
        Box::pin(async move {
            self.handle
                .authenticate_password(username, password)
                .await
                .map_err(|error| {
                    SshSessionError::new(format!("SSH password authentication failed: {error}"))
                })
        })
    }

    fn authenticate_private_key<'a>(
        &'a mut self,
        username: &'a str,
        path: &'a Path,
        passphrase: Option<&'a str>,
    ) -> RusshAuthFuture<'a> {
        Box::pin(async move {
            let key = RusshPrivateKeyAuth::load(path, passphrase).map_err(|error| {
                SshSessionError::new(format!("SSH private-key load failed: {error}"))
            })?;
            let rsa_hash = self
                .handle
                .best_supported_rsa_hash()
                .await
                .map_err(|error| {
                    SshSessionError::new(format!(
                        "SSH private-key algorithm negotiation failed: {error}"
                    ))
                })?
                .flatten();
            self.handle
                .authenticate_publickey(username, key.into_private_key_with_hash_alg(rsa_hash))
                .await
                .map_err(|error| {
                    SshSessionError::new(format!("SSH private-key authentication failed: {error}"))
                })
        })
    }

    fn authenticate_agent<'a>(&'a mut self, username: &'a str) -> RusshAuthFuture<'a> {
        Box::pin(async move { authenticate_agent_with_default_client(self.handle, username).await })
    }
}

type DynamicAgentClient<'a> = russh::keys::agent::client::AgentClient<
    Box<dyn russh::keys::agent::client::AgentStream + Send + Unpin + 'a>,
>;

#[cfg(unix)]
async fn connect_default_agent_client() -> Result<DynamicAgentClient<'static>, russh::keys::Error> {
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map(russh::keys::agent::client::AgentClient::dynamic)
}

#[cfg(windows)]
async fn connect_default_agent_client() -> Result<DynamicAgentClient<'static>, russh::keys::Error> {
    if let Ok(path) = std::env::var("SSH_AUTH_SOCK") {
        return russh::keys::agent::client::AgentClient::connect_named_pipe(path)
            .await
            .map(russh::keys::agent::client::AgentClient::dynamic);
    }

    if let Ok(client) = russh::keys::agent::client::AgentClient::connect_pageant().await {
        return Ok(client.dynamic());
    }

    russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map(russh::keys::agent::client::AgentClient::dynamic)
}

async fn authenticate_agent_with_default_client(
    handle: &mut russh::client::Handle<RusshClientHandler>,
    username: &str,
) -> Result<russh::client::AuthResult, SshSessionError> {
    let mut agent = connect_default_agent_client()
        .await
        .map_err(|error| SshSessionError::new(format!("SSH agent connection failed: {error}")))?;

    authenticate_agent_with_client(handle, username, &mut agent).await
}

async fn authenticate_agent_with_client<S>(
    handle: &mut russh::client::Handle<RusshClientHandler>,
    username: &str,
    agent: &mut russh::keys::agent::client::AgentClient<S>,
) -> Result<russh::client::AuthResult, SshSessionError>
where
    S: russh::keys::agent::client::AgentStream + Send + Unpin,
{
    let identities = agent.request_identities().await.map_err(|error| {
        SshSessionError::new(format!("SSH agent identity lookup failed: {error}"))
    })?;
    if identities.is_empty() {
        return Err(SshSessionError::new("SSH agent has no identities"));
    }

    let rsa_hash = handle
        .best_supported_rsa_hash()
        .await
        .map_err(|error| {
            SshSessionError::new(format!("SSH agent algorithm negotiation failed: {error}"))
        })?
        .flatten();

    for identity in identities {
        let result = match identity {
            russh::keys::agent::AgentIdentity::PublicKey { key, .. } => {
                handle
                    .authenticate_publickey_with(username, key, rsa_hash, agent)
                    .await
            }
            russh::keys::agent::AgentIdentity::Certificate { certificate, .. } => {
                handle
                    .authenticate_certificate_with(username, certificate, rsa_hash, agent)
                    .await
            }
        }
        .map_err(|error| {
            SshSessionError::new(format!("SSH agent authentication failed: {error}"))
        })?;

        if result.success() {
            return Ok(result);
        }
    }

    Ok(russh::client::AuthResult::Failure {
        remaining_methods: russh::MethodSet::empty(),
        partial_success: false,
    })
}

#[derive(Debug, Clone)]
pub struct RusshClientHandler {
    host_key_policy: RusshHostKeyPolicy,
    host: Option<String>,
    port: Option<u16>,
    known_hosts_path: Option<PathBuf>,
}

impl RusshClientHandler {
    #[must_use]
    pub const fn accepts_unknown_host_keys(&self) -> bool {
        matches!(self.host_key_policy, RusshHostKeyPolicy::AcceptUnknown)
    }
}

impl russh::client::Handler for RusshClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        if self.accepts_unknown_host_keys() {
            return Ok(true);
        }

        let (Some(host), Some(port)) = (self.host.as_deref(), self.port) else {
            return Ok(false);
        };

        let matches = if let Some(path) = &self.known_hosts_path {
            let known_hosts = RusshKnownHosts::new(path.clone());
            match known_hosts.matches(host, port, server_public_key) {
                Ok(false)
                    if matches!(self.host_key_policy, RusshHostKeyPolicy::TrustOnFirstUse) =>
                {
                    known_hosts
                        .learn(host, port, server_public_key)
                        .map_err(russh::Error::from)?;
                    return Ok(true);
                }
                result => result,
            }
        } else {
            russh::keys::known_hosts::check_known_hosts(host, port, server_public_key)
        };

        Ok(matches.unwrap_or(false))
    }
}

pub struct RusshSshChannel {
    read_half: russh::ChannelReadHalf,
    write_half: russh::ChannelWriteHalf<russh::client::Msg>,
    handle: russh::client::Handle<RusshClientHandler>,
    runtime: tokio::runtime::Runtime,
    pending_read: VecDeque<u8>,
}

impl RusshSshChannel {
    #[must_use]
    pub fn new(
        channel: russh::Channel<russh::client::Msg>,
        handle: russh::client::Handle<RusshClientHandler>,
        runtime: tokio::runtime::Runtime,
    ) -> Self {
        let (read_half, write_half) = channel.split();

        Self {
            read_half,
            write_half,
            handle,
            runtime,
            pending_read: VecDeque::new(),
        }
    }

    fn fill_from_pending(&mut self, buffer: &mut [u8]) -> usize {
        let count = buffer.len().min(self.pending_read.len());
        for slot in buffer.iter_mut().take(count) {
            if let Some(byte) = self.pending_read.pop_front() {
                *slot = byte;
            }
        }
        count
    }

    fn queue_read_bytes(&mut self, bytes: &[u8]) {
        self.pending_read.extend(bytes);
    }
}

impl SshChannel for RusshSshChannel {
    fn read_channel(&mut self, buffer: &mut [u8]) -> Result<usize, SshSessionError> {
        if buffer.is_empty() {
            return Ok(0);
        }

        let pending_count = self.fill_from_pending(buffer);
        if pending_count > 0 {
            return Ok(pending_count);
        }

        loop {
            let message = self.runtime.block_on(self.read_half.wait());
            match message {
                Some(
                    russh::ChannelMsg::Data { data } | russh::ChannelMsg::ExtendedData { data, .. },
                ) => {
                    self.queue_read_bytes(&data);
                    return Ok(self.fill_from_pending(buffer));
                }
                Some(russh::ChannelMsg::Eof | russh::ChannelMsg::Close) | None => {
                    return Ok(0);
                }
                Some(_) => {}
            }
        }
    }

    fn write_channel(&mut self, bytes: &[u8]) -> Result<usize, SshSessionError> {
        self.runtime
            .block_on(self.write_half.data_bytes(bytes.to_vec()))
            .map_err(|error| SshSessionError::new(format!("SSH channel write failed: {error}")))?;

        Ok(bytes.len())
    }

    fn resize_pty(&mut self, size: rssh_core::TerminalSize) -> Result<(), SshSessionError> {
        self.runtime
            .block_on(self.write_half.window_change(
                u32::from(size.columns),
                u32::from(size.rows),
                0,
                0,
            ))
            .map_err(|error| SshSessionError::new(format!("SSH PTY resize failed: {error}")))
    }

    fn send_keepalive(&mut self) -> Result<(), SshSessionError> {
        self.runtime
            .block_on(self.handle.send_keepalive(false))
            .map_err(|error| SshSessionError::new(format!("SSH keepalive failed: {error}")))
    }

    fn close_channel(&mut self) -> Result<(), SshSessionError> {
        self.runtime
            .block_on(self.write_half.close())
            .map_err(|error| SshSessionError::new(format!("SSH channel close failed: {error}")))
    }
}

impl SshChannelOpener for RusshChannelOpener {
    type Channel = RusshSshChannel;

    fn open_channel(
        &mut self,
        request: SshConnectRequest,
    ) -> Result<Self::Channel, SshSessionError> {
        let plan = self.connect_plan(&request);
        let startup_plan = plan.channel_startup_plan();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                SshSessionError::new(format!("SSH async runtime creation failed: {error}"))
            })?;

        let (handle, channel) = runtime.block_on(async {
            let mut handle = self.connect_async(request).await?;
            self.authenticate_async(&mut handle, plan.auth_plan())
                .await?;
            let channel = self.open_session_channel_async(&handle).await?;
            self.start_channel_async(&channel, &startup_plan).await?;

            Ok::<_, SshSessionError>((handle, channel))
        })?;

        Ok(RusshSshChannel::new(channel, handle, runtime))
    }
}

#[cfg(test)]
mod tests {
    use std::{future::Future, path::Path, pin::Pin};

    use rssh_core::TerminalSize;

    use super::*;
    use crate::{SshConnectRequest, SshSessionConfig};

    type TestAuthFuture<'a> =
        Pin<Box<dyn Future<Output = Result<russh::client::AuthResult, SshSessionError>> + 'a>>;

    #[derive(Default)]
    struct MockAuthBackend {
        calls: Vec<String>,
    }

    impl RusshAuthenticationBackend for MockAuthBackend {
        fn authenticate_password<'a>(
            &'a mut self,
            username: &'a str,
            _password: &'a str,
        ) -> TestAuthFuture<'a> {
            self.calls.push(format!("password:{username}"));
            Box::pin(async { Ok(russh::client::AuthResult::Success) })
        }

        fn authenticate_private_key<'a>(
            &'a mut self,
            username: &'a str,
            _path: &'a Path,
            _passphrase: Option<&'a str>,
        ) -> TestAuthFuture<'a> {
            self.calls.push(format!("private-key:{username}"));
            Box::pin(async { Ok(russh::client::AuthResult::Success) })
        }

        fn authenticate_agent<'a>(&'a mut self, username: &'a str) -> TestAuthFuture<'a> {
            self.calls.push(format!("agent:{username}"));
            Box::pin(async { Ok(russh::client::AuthResult::Success) })
        }
    }

    #[test]
    fn authenticate_auth_plan_uses_agent_backend_for_agent_authentication() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let plan = RusshAuthPlan::from_request(&request);
        let mut backend = MockAuthBackend::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let outcome = runtime
            .block_on(authenticate_auth_plan_with_backend(&mut backend, &plan))
            .unwrap();

        assert_eq!(outcome, RusshAuthOutcome::Authenticated);
        assert_eq!(backend.calls, ["agent:ops"]);
    }
}
