use super::{
    Arc, ConnectionState, Duration, Error, EventLoopProxy, HostKeyChallenge, HostKeyDecision,
    HostKeyVerifier, Instant, Key, NamedKey, NativeSshCommand, NativeSshWriter, NativeWindowApp,
    Ordering, PaneRuntime, PaneStableViewport, PaneUiState, RusshChannelOpener, SecretPrompt,
    SecretProvider, SshChannelConnector, SshConnectionPhase, SshKnownHostsPolicy, SshPaneLaunch,
    SshSecretPromptState, SshShellConnector, SshShellWriter, WindowUserEvent, Write, mpsc,
    russh_host_key_policy, ssh_known_hosts_path, ssh_request_from_pane_launch,
    terminal_runtime_snapshot, thread,
};

impl NativeWindowApp {
    /// Starts a native SSH channel without creating a local PTY.  The
    /// terminal runtime is deliberately the same one used by local panes;
    /// only the transport and lifecycle workers differ.  Secrets are
    /// represented by prompt auth descriptors and never enter this method's
    /// launch metadata, events, or terminal grid.
    #[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
    pub(super) fn spawn_native_ssh_runtime(
        &mut self,
        pane_id: rssh_core::PaneId,
        launch: &SshPaneLaunch,
        runtime_generation: u64,
        event_proxy: EventLoopProxy<WindowUserEvent>,
    ) -> Result<PaneRuntime, Box<dyn Error>> {
        let (pty_size, runtime) = self.prepare_pane_spawn_runtime(pane_id)?;
        let request = ssh_request_from_pane_launch(launch, pty_size)?;
        let app_window_id = self.app_window_id;
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (command_sender, command_receiver) = mpsc::sync_channel::<NativeSshCommand>(64);
        self.ssh_writer_senders
            .insert(pane_id, command_sender.clone());
        let _ = event_proxy.send_event(WindowUserEvent::SshState {
            window_id: app_window_id,
            pane_id,
            runtime_generation,
            state: ConnectionState::Pending,
        });

        // Input is serialized through a small worker so terminal event
        // handling never blocks on network backpressure.  Data received while
        // the connection overlay is pending is intentionally discarded by
        // NativeSshWriter rather than cached.
        let writer_event_proxy = event_proxy.clone();
        let writer_connected = Arc::clone(&connected);
        let writer_thread = thread::Builder::new()
            .name(format!("rssh-ssh-writer-{}", pane_id.get()))
            .spawn(move || {
                let mut remote_writer: Option<Box<dyn SshShellWriter>> = None;
                while let Ok(command) = command_receiver.recv() {
                    match command {
                        NativeSshCommand::Attach(writer) => {
                            remote_writer = Some(writer);
                        }
                        NativeSshCommand::Data(bytes) => {
                            let Some(writer) = remote_writer.as_mut() else {
                                continue;
                            };
                            let started = Instant::now();
                            match writer.write(&bytes) {
                                Ok(byte_count) => {
                                    let _ = writer_event_proxy.send_event(
                                        WindowUserEvent::WriteCompleted {
                                            window_id: app_window_id,
                                            pane_id,
                                            runtime_generation,
                                            byte_count,
                                            elapsed: started.elapsed(),
                                        },
                                    );
                                }
                                Err(error) => {
                                    writer_connected.store(false, Ordering::Release);
                                    let _ = writer_event_proxy.send_event(WindowUserEvent::WriteError {
                                        window_id: app_window_id,
                                        pane_id,
                                        runtime_generation,
                                        error: error.to_string(),
                                    });
                                    break;
                                }
                            }
                        }
                        NativeSshCommand::Resize(size) => {
                            if let Some(writer) = remote_writer.as_mut()
                                && let Err(error) = writer.resize(size)
                            {
                                writer_connected.store(false, Ordering::Release);
                                let _ = writer_event_proxy.send_event(WindowUserEvent::WriteError {
                                    window_id: app_window_id,
                                    pane_id,
                                    runtime_generation,
                                    error: format!("SSH PTY resize failed: {error}"),
                                });
                                break;
                            }
                        }
                        NativeSshCommand::Cancel => {
                            writer_connected.store(false, Ordering::Release);
                            if let Some(mut writer) = remote_writer.take() {
                                let _ = writer.close();
                            }
                            break;
                        }
                    }
                }
                writer_connected.store(false, Ordering::Release);
                if let Some(mut writer) = remote_writer {
                    let _ = writer.close();
                }
            })?;

        let connector_event_proxy = event_proxy.clone();
        let connector_connected = Arc::clone(&connected);
        let connector_command_sender = command_sender.clone();
        let policy = launch.known_hosts_policy();
        let known_hosts_path = ssh_known_hosts_path();
        let connector_thread = thread::Builder::new()
            .name(format!("rssh-ssh-connector-{}", pane_id.get()))
            .spawn(move || {
                let phase_proxy = connector_event_proxy.clone();
                let phase_reporter = move |phase: SshConnectionPhase| {
                    let state = match phase {
                        SshConnectionPhase::Connecting | SshConnectionPhase::Opening => {
                            ConnectionState::Connecting
                        }
                        SshConnectionPhase::Authenticating => ConnectionState::AwaitingSecret,
                        SshConnectionPhase::Connected => ConnectionState::Connected,
                    };
                    let _ = phase_proxy.send_event(WindowUserEvent::SshState {
                        window_id: app_window_id,
                        pane_id,
                        runtime_generation,
                        state,
                    });
                };

                let mut opener = RusshChannelOpener::default()
                    .with_host_key_policy(russh_host_key_policy(policy))
                    .with_phase_reporter(phase_reporter);
                if let Some(path) = known_hosts_path.clone() {
                    opener = opener.with_known_hosts_path(path);
                }
                if policy == SshKnownHostsPolicy::Prompt {
                    let prompt_proxy = connector_event_proxy.clone();
                    let verifier = HostKeyVerifier::new(
                        move |challenge: HostKeyChallenge| {
                            let (decision_sender, decision_receiver) = mpsc::sync_channel(1);
                            let _ = prompt_proxy.send_event(WindowUserEvent::HostKeyPrompt {
                                window_id: app_window_id,
                                pane_id,
                                runtime_generation,
                                challenge,
                                decision: decision_sender,
                            });
                            async move {
                                decision_receiver
                                    .recv_timeout(Duration::from_secs(120))
                                    .unwrap_or(HostKeyDecision::Cancel)
                            }
                        },
                    );
                    opener = opener.with_host_key_verifier_handle(verifier);
                }

                let secret_proxy = connector_event_proxy.clone();
                let secret_provider = SecretProvider::new(move |prompt: SecretPrompt| {
                    let (response_sender, response_receiver) = mpsc::sync_channel(1);
                    let _ = secret_proxy.send_event(WindowUserEvent::SecretPrompt {
                        window_id: app_window_id,
                        pane_id,
                        runtime_generation,
                        prompt,
                        response: response_sender,
                    });
                    async move {
                        response_receiver
                            .recv_timeout(Duration::from_secs(120))
                            .ok()
                            .flatten()
                    }
                });
                opener = opener.with_secret_provider_handle(secret_provider);

                let mut connector = SshChannelConnector::new(opener);
                let session = match connector.connect(request) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = connector_event_proxy.send_event(WindowUserEvent::SshState {
                            window_id: app_window_id,
                            pane_id,
                            runtime_generation,
                            state: ConnectionState::Failed,
                        });
                        eprintln!("SSH connection failed: {error}");
                        return;
                    }
                };
                let (mut reader, writer) = session.into_read_writer();
                if connector_command_sender
                    .send(NativeSshCommand::Attach(writer))
                    .is_err()
                {
                    return;
                }
                connector_connected.store(true, Ordering::Release);
                let _ = connector_event_proxy.send_event(WindowUserEvent::SshState {
                    window_id: app_window_id,
                    pane_id,
                    runtime_generation,
                    state: ConnectionState::Connected,
                });

                let mut buffer = [0_u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            connector_connected.store(false, Ordering::Release);
                            let _ = connector_event_proxy.send_event(WindowUserEvent::SshState {
                                window_id: app_window_id,
                                pane_id,
                                runtime_generation,
                                state: ConnectionState::Disconnected,
                            });
                            break;
                        }
                        Ok(count) => {
                            if connector_event_proxy
                                .send_event(WindowUserEvent::Output {
                                    window_id: app_window_id,
                                    pane_id,
                                    runtime_generation,
                                    bytes: buffer[..count].to_vec(),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(error) => {
                            connector_connected.store(false, Ordering::Release);
                            let _ = connector_event_proxy.send_event(WindowUserEvent::SshState {
                                window_id: app_window_id,
                                pane_id,
                                runtime_generation,
                                state: ConnectionState::Failed,
                            });
                            eprintln!("SSH read failed: {error}");
                            break;
                        }
                    }
                }
                let _ = connector_command_sender.send(NativeSshCommand::Cancel);
            })?;

        let writer: Box<dyn Write + Send> = Box::new(NativeSshWriter {
            sender: command_sender,
            connected,
        });
        let snapshot = terminal_runtime_snapshot(&runtime, PaneStableViewport::default());
        Ok(PaneRuntime {
            runtime,
            session: None,
            session_process_id: None,
            session_tty_name: None,
            writer: Some(writer),
            reader_thread: Some(connector_thread),
            writer_thread: Some(writer_thread),
            runtime_generation,
            snapshot,
            ui: PaneUiState::default(),
        })
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn handle_ssh_state(&mut self, pane_id: rssh_core::PaneId, state: ConnectionState) {
        if matches!(state, ConnectionState::Disconnected | ConnectionState::Failed) {
            self.resolve_host_key_prompt_for_pane(pane_id, HostKeyDecision::Cancel);
            self.resolve_secret_prompt_for_pane(pane_id, None);
        }
        self.metrics.mark_connection_state(state);
        if matches!(state, ConnectionState::Connecting | ConnectionState::AwaitingSecret) {
            self.metrics.mark_ssh_started();
        }
        if state == ConnectionState::Connected {
            self.metrics.mark_ssh_connected();
        }
        self.apply_window_title();
    }

    pub(super) fn handle_host_key_prompt(
        &mut self,
        pane_id: rssh_core::PaneId,
        challenge: HostKeyChallenge,
        decision: mpsc::SyncSender<HostKeyDecision>,
    ) {
        self.ssh_host_key_prompts
            .insert(pane_id, (challenge, decision));
        self.metrics.mark_connection_state(ConnectionState::AwaitingHostKey);
    }

    fn resolve_host_key_prompt(&mut self, decision: HostKeyDecision) {
        self.resolve_host_key_prompt_for_pane(self.app_shell.active_pane_id(), decision);
        self.apply_window_title();
    }

    pub(super) fn resolve_host_key_prompt_for_pane(
        &mut self,
        pane_id: rssh_core::PaneId,
        decision: HostKeyDecision,
    ) {
        if let Some((_, sender)) = self.ssh_host_key_prompts.remove(&pane_id) {
            let _ = sender.send(decision);
        }
        self.apply_window_title();
    }

    pub(super) fn handle_secret_prompt(
        &mut self,
        pane_id: rssh_core::PaneId,
        prompt: SecretPrompt,
        response: mpsc::SyncSender<Option<String>>,
    ) {
        self.ssh_secret_prompts.insert(pane_id, SshSecretPromptState {
            prompt,
            response,
            input: String::new(),
        });
        self.metrics
            .mark_connection_state(ConnectionState::AwaitingSecret);
    }

    fn resolve_secret_prompt(&mut self, value: Option<String>) {
        if let Some(mut prompt) = self
            .ssh_secret_prompts
            .remove(&self.app_shell.active_pane_id())
        {
            let _ = prompt.response.send(value);
            prompt.input.clear();
        }
        self.apply_window_title();
    }

    pub(super) fn resolve_secret_prompt_for_pane(
        &mut self,
        pane_id: rssh_core::PaneId,
        value: Option<String>,
    ) {
        if let Some(mut prompt) = self.ssh_secret_prompts.remove(&pane_id) {
            let _ = prompt.response.send(value);
            prompt.input.clear();
        }
        self.apply_window_title();
    }

    fn handle_secret_prompt_key(&mut self, logical_key: &Key, text: Option<&str>) -> bool {
        let pane_id = self.app_shell.active_pane_id();
        if !self.ssh_secret_prompts.contains_key(&pane_id) {
            return false;
        }
        match logical_key {
            Key::Named(NamedKey::Enter) => {
                let value = self
                    .ssh_secret_prompts
                    .get(&pane_id)
                    .map(|prompt| prompt.input.clone());
                self.resolve_secret_prompt(value);
                true
            }
            Key::Named(NamedKey::Escape) => {
                self.resolve_secret_prompt(None);
                true
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(prompt) = self.ssh_secret_prompts.get_mut(&pane_id) {
                    prompt.input.pop();
                }
                true
            }
            _ => {
                if let Some(text) = text
                    && let Some(prompt) = self.ssh_secret_prompts.get_mut(&pane_id)
                {
                    prompt
                        .input
                        .extend(text.chars().filter(|character| !character.is_control()));
                }
                true
            }
        }
    }

    pub(super) fn handle_ssh_prompt_key_event(
        &mut self,
        logical_key: &Key,
        text: Option<&str>,
    ) -> bool {
        if self
            .ssh_host_key_prompts
            .contains_key(&self.app_shell.active_pane_id())
        {
            let decision = match logical_key.as_ref() {
                Key::Character("1") => Some(HostKeyDecision::AcceptOnce),
                Key::Character("2") => Some(HostKeyDecision::AcceptAndStore),
                Key::Named(NamedKey::Escape) => Some(HostKeyDecision::Cancel),
                _ => None,
            };
            if let Some(decision) = decision {
                self.resolve_host_key_prompt(decision);
            }
            return true;
        }

        self.handle_secret_prompt_key(logical_key, text)
    }
}
