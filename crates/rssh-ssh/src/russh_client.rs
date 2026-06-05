use std::time::Duration;

#[derive(Debug)]
pub struct RusshChannelOpener {
    client_config: russh::client::Config,
}

impl Default for RusshChannelOpener {
    fn default() -> Self {
        let client_config = russh::client::Config {
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        };

        Self { client_config }
    }
}

impl RusshChannelOpener {
    #[must_use]
    pub fn new(client_config: russh::client::Config) -> Self {
        Self { client_config }
    }

    #[must_use]
    pub const fn client_config(&self) -> &russh::client::Config {
        &self.client_config
    }
}
