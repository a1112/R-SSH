use std::error::Error;
#[cfg(test)]
use std::io::{self, Read, Write};

use rssh_pty::{PtyCommand, PtyExitStatus, PtySize};
use rssh_ssh::{SshAuthMethod, SshConnectRequest};
#[cfg(test)]
use rssh_ssh::{SshShellConnector, SshShellSession};

use crate::{
    cli::{LocalOptions, OpenSshTarget, SshOptions, SshTarget},
    local,
};

pub fn run(options: &SshOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    let local_options = local_options_for_options(options)?;

    local::run(&local_options)
}

#[must_use]
fn openssh_command_for_options(options: &SshOptions) -> PtyCommand {
    match &options.target {
        SshTarget::Direct(request) => openssh_command_for_request(request),
        SshTarget::OpenSsh(target) => openssh_command_for_target(target),
    }
}

#[must_use]
fn openssh_command_for_request(request: &SshConnectRequest) -> PtyCommand {
    let mut args = vec!["-tt".to_owned()];

    append_auth_args(&mut args, &request.auth);

    if request.config.port != 22 {
        args.push("-p".to_owned());
        args.push(request.config.port.to_string());
    }

    args.push(format!(
        "{}@{}",
        request.config.username, request.config.host
    ));

    PtyCommand::new("ssh").with_args(args)
}

#[must_use]
fn openssh_command_for_target(target: &OpenSshTarget) -> PtyCommand {
    let mut args = vec!["-tt".to_owned()];

    append_auth_args(&mut args, &target.auth);

    if let Some(username) = &target.username {
        args.push("-l".to_owned());
        args.push(username.clone());
    }
    if let Some(port) = target.port {
        args.push("-p".to_owned());
        args.push(port.to_string());
    }

    args.push(target.target.clone());

    PtyCommand::new("ssh").with_args(args)
}

fn append_auth_args(args: &mut Vec<String>, auth: &SshAuthMethod) {
    match auth {
        SshAuthMethod::PasswordPrompt | SshAuthMethod::Password { .. } => {
            args.push("-o".to_owned());
            args.push("PreferredAuthentications=password,keyboard-interactive".to_owned());
        }
        SshAuthMethod::PrivateKey { path, .. } => {
            args.push("-i".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        SshAuthMethod::Agent => {}
    }
}

fn local_options_for_options(options: &SshOptions) -> Result<LocalOptions, Box<dyn Error>> {
    let size = match &options.target {
        SshTarget::Direct(request) => request.config.initial_size,
        SshTarget::OpenSsh(target) => target.initial_size,
    };

    Ok(LocalOptions {
        command: openssh_command_for_options(options),
        size: Some(PtySize::try_new(size.columns, size.rows)?),
        mouse: true,
    })
}

#[cfg(test)]
fn local_options_for_request(request: &SshConnectRequest) -> Result<LocalOptions, Box<dyn Error>> {
    local_options_for_options(&SshOptions {
        target: SshTarget::Direct(request.clone()),
    })
}

#[cfg(test)]
fn run_with_connector_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let request = match &options.target {
        SshTarget::Direct(request) => request.clone(),
        SshTarget::OpenSsh(_) => {
            return Err("mock SSH connector only supports direct SSH targets".into());
        }
    };
    let mut session = connector.connect(request)?;
    copy_input_to_session(input, session.as_mut())?;
    let mut buffer = [0; 8192];

    loop {
        let count = session.read(&mut buffer)?;
        if count == 0 {
            break;
        }

        output.write_all(&buffer[..count])?;
        output.flush()?;
    }

    session.close()?;
    Ok(())
}

#[cfg(test)]
fn copy_input_to_session(
    input: &mut dyn Read,
    session: &mut dyn SshShellSession,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = [0; 8192];

    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }

        let mut written = 0;
        while written < count {
            let next = session.write(&buffer[written..count])?;
            if next == 0 {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "SSH session write returned zero bytes",
                )));
            }
            written += next;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        sync::{Arc, Mutex},
    };

    use rssh_core::TerminalSize;
    use rssh_ssh::{
        SshConnectRequest, SshSessionConfig, SshSessionError, SshShellConnector, SshShellSession,
    };

    use crate::cli::{OpenSshTarget, SshOptions, SshTarget};

    #[test]
    fn ssh_runner_streams_remote_output_and_closes_session() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut output = Vec::new();

        super::run_with_connector_and_io(
            &SshOptions {
                target: SshTarget::Direct(request.clone()),
            },
            &mut connector,
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
    fn ssh_runner_writes_local_input_to_remote_session() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut input = &b"echo hi\n"[..];
        let mut output = Vec::new();

        super::run_with_connector_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
            },
            &mut connector,
            &mut input,
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.written, b"echo hi\n");
        assert_eq!(output, b"remote\n");
        assert!(state.closed);
    }

    #[test]
    fn openssh_command_uses_target_port_and_tty() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 2222, "ops", TerminalSize::new(120, 30))
                .unwrap(),
        );

        let command = super::openssh_command_for_request(&request);

        assert_eq!(command.program(), "ssh");
        assert_eq!(command.args(), ["-tt", "-p", "2222", "ops@example.com"]);
    }

    #[test]
    fn openssh_command_adds_private_key_without_leaking_passphrase() {
        let request = SshConnectRequest::private_key(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
            "C:/Users/ops/.ssh/id_ed25519",
            Some("secret"),
        )
        .unwrap();

        let command = super::openssh_command_for_request(&request);
        let joined = command.args().join(" ");

        assert_eq!(
            command.args(),
            [
                "-tt",
                "-i",
                "C:/Users/ops/.ssh/id_ed25519",
                "ops@example.com"
            ]
        );
        assert!(!joined.contains("secret"));
    }

    #[test]
    fn openssh_command_uses_password_prompt_policy_without_leaking_password() {
        let request = SshConnectRequest::password(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
            "secret",
        )
        .unwrap();

        let command = super::openssh_command_for_request(&request);
        let joined = command.args().join(" ");

        assert_eq!(
            command.args(),
            [
                "-tt",
                "-o",
                "PreferredAuthentications=password,keyboard-interactive",
                "ops@example.com"
            ]
        );
        assert!(!joined.contains("secret"));
    }

    #[test]
    fn openssh_local_options_use_requested_terminal_size_and_mouse() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(132, 43))
                .unwrap(),
        );

        let options = super::local_options_for_request(&request).unwrap();

        let size = options.size.unwrap();
        assert_eq!(size.columns(), 132);
        assert_eq!(size.rows(), 43);
        assert!(options.mouse);
    }

    #[test]
    fn openssh_command_uses_config_target_with_overrides() {
        let options = SshOptions {
            target: SshTarget::OpenSsh(OpenSshTarget {
                target: "prod".to_owned(),
                username: Some("ops".to_owned()),
                port: Some(2222),
                initial_size: TerminalSize::new(120, 30),
                auth: rssh_ssh::SshAuthMethod::PrivateKey {
                    path: "C:/Users/ops/.ssh/id_ed25519".into(),
                    passphrase: None,
                },
            }),
        };

        let command = super::openssh_command_for_options(&options);

        assert_eq!(
            command.args(),
            [
                "-tt",
                "-i",
                "C:/Users/ops/.ssh/id_ed25519",
                "-l",
                "ops",
                "-p",
                "2222",
                "prod"
            ]
        );
    }

    #[derive(Default)]
    struct MockState {
        last_request: Option<SshConnectRequest>,
        written: Vec<u8>,
        closed: bool,
    }

    struct MockConnector {
        state: Arc<Mutex<MockState>>,
    }

    impl SshShellConnector for MockConnector {
        fn connect(
            &mut self,
            request: SshConnectRequest,
        ) -> Result<Box<dyn SshShellSession>, SshSessionError> {
            self.state.lock().unwrap().last_request = Some(request);
            Ok(Box::new(MockSession {
                state: Arc::clone(&self.state),
                read_once: false,
            }))
        }
    }

    struct MockSession {
        state: Arc<Mutex<MockState>>,
        read_once: bool,
    }

    impl SshShellSession for MockSession {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, SshSessionError> {
            if self.read_once {
                return Ok(0);
            }
            self.read_once = true;
            buffer[..7].copy_from_slice(b"remote\n");
            Ok(7)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, SshSessionError> {
            self.state.lock().unwrap().written.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn resize(&mut self, _size: TerminalSize) -> Result<(), SshSessionError> {
            Ok(())
        }

        fn keepalive(&mut self) -> Result<(), SshSessionError> {
            Ok(())
        }

        fn close(&mut self) -> Result<(), SshSessionError> {
            self.state.lock().unwrap().closed = true;
            Ok(())
        }
    }
}
