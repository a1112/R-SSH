use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RusshHostKeyPolicy {
    RejectUnknown,
    AcceptUnknown,
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
