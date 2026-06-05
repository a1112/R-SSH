use std::time::Duration;

use crate::{SshChannelOpenPlan, SshConnectRequest, SshSessionStartup};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RusshHostKeyPolicy {
    RejectUnknown,
    AcceptUnknown,
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
    channel_open_plan: SshChannelOpenPlan,
}

impl RusshConnectPlan {
    #[must_use]
    pub fn from_request(request: &SshConnectRequest) -> Self {
        Self {
            host: request.config.host.clone(),
            port: request.config.port,
            username: request.config.username.clone(),
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
    pub fn channel_startup_plan(&self) -> RusshChannelStartupPlan {
        RusshChannelStartupPlan::from_open_plan(&self.channel_open_plan)
    }
}

#[derive(Debug)]
pub struct RusshChannelOpener {
    client_config: russh::client::Config,
    host_key_policy: RusshHostKeyPolicy,
}

impl Default for RusshChannelOpener {
    fn default() -> Self {
        let client_config = russh::client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        };

        Self {
            client_config,
            host_key_policy: RusshHostKeyPolicy::RejectUnknown,
        }
    }
}

impl RusshChannelOpener {
    #[must_use]
    pub fn new(client_config: russh::client::Config) -> Self {
        Self {
            client_config,
            host_key_policy: RusshHostKeyPolicy::RejectUnknown,
        }
    }

    #[must_use]
    pub const fn with_host_key_policy(mut self, host_key_policy: RusshHostKeyPolicy) -> Self {
        self.host_key_policy = host_key_policy;
        self
    }

    #[must_use]
    pub const fn client_config(&self) -> &russh::client::Config {
        &self.client_config
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
    pub fn connect_plan(&self, request: &SshConnectRequest) -> RusshConnectPlan {
        RusshConnectPlan::from_request(request)
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
