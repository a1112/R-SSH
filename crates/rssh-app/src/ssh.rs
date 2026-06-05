use std::{
    error::Error,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use rssh_pty::{PtyCommand, PtyExitStatus, PtySize};
use rssh_ssh::{
    RusshChannelOpener, RusshHostKeyPolicy, RusshPrivateKeyAuth, SshAuthMethod,
    SshChannelConnector, SshConnectRequest, SshSessionStartup, SshShellConnector,
};

use crate::{
    cli::{LocalOptions, NativeHostKeyPolicy, OpenSshTarget, SshForward, SshOptions, SshTarget},
    local,
};

type SecretPrompt<'a> = dyn FnMut() -> Result<String, Box<dyn Error>> + 'a;
type KeyPassphrasePrompt<'a> = dyn FnMut(&Path) -> Result<String, Box<dyn Error>> + 'a;
type KeyPassphraseDetector<'a> = dyn FnMut(&Path) -> Result<bool, Box<dyn Error>> + 'a;

pub fn run(options: &SshOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    if options.native {
        return run_native(options);
    }

    let local_options = local_options_for_options(options)?;

    local::run(&local_options)
}

fn run_native(options: &SshOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut connector = SshChannelConnector::new(native_channel_opener_for_options(options));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    run_native_with_connector_prompt_and_io(
        options,
        &mut connector,
        &mut || native_password_prompt(options),
        &mut input,
        &mut output,
    )
}

fn native_channel_opener_for_options(options: &SshOptions) -> RusshChannelOpener {
    let opener = RusshChannelOpener::default();
    match options.native_host_key_policy {
        NativeHostKeyPolicy::RejectUnknown => opener,
        NativeHostKeyPolicy::AcceptUnknown => {
            opener.with_host_key_policy(RusshHostKeyPolicy::AcceptUnknown)
        }
        NativeHostKeyPolicy::TrustOnFirstUse => {
            let opener = opener.with_host_key_policy(RusshHostKeyPolicy::TrustOnFirstUse);
            if let Some(path) = default_known_hosts_path() {
                return opener.with_known_hosts_path(path);
            }
            opener
        }
    }
}

fn default_known_hosts_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".ssh").join("known_hosts"))
}

#[must_use]
fn openssh_command_for_options(options: &SshOptions) -> PtyCommand {
    let mut command = match &options.target {
        SshTarget::Direct(request) => openssh_command_for_request(request, options),
        SshTarget::OpenSsh(target) => openssh_command_for_target(target, options),
    };

    if !options.remote_command.is_empty() {
        command = command.with_args(options.remote_command.iter().cloned());
    }

    command
}

#[cfg(test)]
fn native_request_for_options(options: &SshOptions) -> Result<SshConnectRequest, Box<dyn Error>> {
    native_request_for_options_with_password_prompt(options, &mut || {
        Err("native SSH password prompt requires a password provider".into())
    })
}

#[cfg(test)]
fn native_request_for_options_with_password_prompt(
    options: &SshOptions,
    password_prompt: &mut SecretPrompt<'_>,
) -> Result<SshConnectRequest, Box<dyn Error>> {
    native_request_for_options_with_secret_prompts(
        options,
        password_prompt,
        &mut |_| {
            Err("native SSH private-key passphrase prompt requires a passphrase provider".into())
        },
        &mut native_key_needs_passphrase,
    )
}

fn native_request_for_options_with_secret_prompts(
    options: &SshOptions,
    password_prompt: &mut SecretPrompt<'_>,
    key_passphrase_prompt: &mut KeyPassphrasePrompt<'_>,
    key_needs_passphrase: &mut KeyPassphraseDetector<'_>,
) -> Result<SshConnectRequest, Box<dyn Error>> {
    let SshTarget::Direct(request) = &options.target else {
        return Err("native SSH connector only supports direct SSH targets".into());
    };

    if !options.forwards.is_empty() {
        return Err("native SSH forwarding is not wired yet".into());
    }

    let startup = if options.no_shell {
        SshSessionStartup::NoShell
    } else if options.remote_command.is_empty() {
        SshSessionStartup::Shell
    } else {
        SshSessionStartup::command(options.remote_command.clone())?
    };

    let mut request = request.clone().with_startup(startup);
    if matches!(request.auth, SshAuthMethod::PasswordPrompt) {
        request.auth = SshAuthMethod::password(password_prompt()?)?;
    }
    if let SshAuthMethod::PrivateKey { path, passphrase } = &mut request.auth {
        if passphrase.is_none() && key_needs_passphrase(path)? {
            *passphrase = Some(key_passphrase_prompt(path)?);
        }
    }

    Ok(request)
}

#[must_use]
fn openssh_command_for_request(request: &SshConnectRequest, options: &SshOptions) -> PtyCommand {
    let mut args = openssh_start_args(options);

    append_auth_args(&mut args, &request.auth);
    append_forward_args(&mut args, &options.forwards);

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
fn openssh_command_for_target(target: &OpenSshTarget, options: &SshOptions) -> PtyCommand {
    let mut args = openssh_start_args(options);

    append_auth_args(&mut args, &target.auth);
    append_forward_args(&mut args, &options.forwards);

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

fn openssh_start_args(options: &SshOptions) -> Vec<String> {
    if options.no_shell {
        vec!["-N".to_owned()]
    } else {
        vec!["-tt".to_owned()]
    }
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

fn append_forward_args(args: &mut Vec<String>, forwards: &[SshForward]) {
    for forward in forwards {
        match forward {
            SshForward::Local(spec) => {
                args.push("-L".to_owned());
                args.push(spec.clone());
            }
            SshForward::Remote(spec) => {
                args.push("-R".to_owned());
                args.push(spec.clone());
            }
            SshForward::Dynamic(spec) => {
                args.push("-D".to_owned());
                args.push(spec.clone());
            }
        }
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
        osc52_policy: options.osc52_policy,
        log: options.log.clone(),
    })
}

#[cfg(test)]
fn local_options_for_request(request: &SshConnectRequest) -> Result<LocalOptions, Box<dyn Error>> {
    local_options_for_options(&SshOptions {
        target: SshTarget::Direct(request.clone()),
        remote_command: Vec::new(),
        forwards: Vec::new(),
        no_shell: false,
        native: false,
        native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
        osc52_policy: crate::cli::Osc52Policy::default(),
        log: None,
    })
}

#[cfg(test)]
fn run_with_connector_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let request = native_request_for_options(options)?;
    rssh_ssh::run_shell_with_io(connector, request, input, output).map_err(Into::into)
}

#[cfg(test)]
fn run_native_with_connector_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    run_with_connector_and_io(options, connector, input, output)?;

    Ok(PtyExitStatus::from_exit_code(0))
}

fn run_native_with_connector_prompt_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    password_prompt: &mut SecretPrompt<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    run_native_with_connector_secret_prompts_and_io(
        options,
        connector,
        password_prompt,
        &mut native_key_passphrase_prompt,
        &mut native_key_needs_passphrase,
        input,
        output,
    )
}

fn run_native_with_connector_secret_prompts_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    password_prompt: &mut SecretPrompt<'_>,
    key_passphrase_prompt: &mut KeyPassphrasePrompt<'_>,
    key_needs_passphrase: &mut KeyPassphraseDetector<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let request = native_request_for_options_with_secret_prompts(
        options,
        password_prompt,
        key_passphrase_prompt,
        key_needs_passphrase,
    )?;
    rssh_ssh::run_shell_with_io(connector, request, input, output)?;

    Ok(PtyExitStatus::from_exit_code(0))
}

fn native_password_prompt(options: &SshOptions) -> Result<String, Box<dyn Error>> {
    let SshTarget::Direct(request) = &options.target else {
        return Err("native SSH connector only supports direct SSH targets".into());
    };

    rpassword::prompt_password(format!(
        "Password for {}@{}: ",
        request.config.username, request.config.host
    ))
    .map_err(Into::into)
}

fn native_key_passphrase_prompt(path: &Path) -> Result<String, Box<dyn Error>> {
    rpassword::prompt_password(format!("Passphrase for key {}: ", path.display()))
        .map_err(Into::into)
}

fn native_key_needs_passphrase(path: &Path) -> Result<bool, Box<dyn Error>> {
    RusshPrivateKeyAuth::needs_passphrase(path).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use rssh_core::TerminalSize;
    use rssh_ssh::{
        SshConnectRequest, SshSessionConfig, SshSessionError, SshSessionStartup, SshShellConnector,
        SshShellSession,
    };

    use crate::cli::{NativeHostKeyPolicy, OpenSshTarget, Osc52Policy, SshOptions, SshTarget};

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
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
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
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
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
    fn ssh_runner_passes_remote_command_startup_to_native_connector() {
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
                remote_command: vec!["uname".to_owned(), "-a".to_owned()],
                forwards: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        let request = state.last_request.as_ref().unwrap();
        assert_eq!(
            request.startup,
            SshSessionStartup::Command(vec!["uname".to_owned(), "-a".to_owned()])
        );
    }

    #[test]
    fn ssh_runner_passes_no_shell_startup_to_native_connector() {
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
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: true,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        let request = state.last_request.as_ref().unwrap();
        assert_eq!(request.startup, SshSessionStartup::NoShell);
    }

    #[test]
    fn native_ssh_runner_uses_connector_and_returns_success_status() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut output = Vec::new();

        let status = super::run_native_with_connector_and_io(
            &SshOptions {
                target: SshTarget::Direct(request.clone()),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        assert!(status.success());
        assert_eq!(state.last_request.as_ref(), Some(&request));
        assert_eq!(output, b"remote\n");
        assert!(state.closed);
    }

    #[test]
    fn native_ssh_runner_rejects_forwarding_until_native_tunnels_exist() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector { state };
        let mut output = Vec::new();

        let error = super::run_native_with_connector_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: vec![crate::cli::SshForward::Local(
                    "127.0.0.1:15432:db.internal:5432".to_owned(),
                )],
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut io::empty(),
            &mut output,
        )
        .unwrap_err();

        assert!(error.to_string().contains("native SSH forwarding"));
    }

    #[test]
    fn native_ssh_runner_resolves_password_prompt_before_connecting() {
        let request = SshConnectRequest::new(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
            rssh_ssh::SshAuthMethod::PasswordPrompt,
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_prompt_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut || Ok("secret".to_owned()),
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        let request = state.last_request.as_ref().unwrap();
        assert_eq!(
            request.auth,
            rssh_ssh::SshAuthMethod::Password {
                password: "secret".to_owned()
            }
        );
    }

    #[test]
    fn native_ssh_runner_resolves_private_key_passphrase_before_connecting() {
        let key_path = PathBuf::from("C:/Users/ops/.ssh/id_ed25519");
        let request = SshConnectRequest::private_key(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
            key_path.clone(),
            None::<String>,
        )
        .unwrap();
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_secret_prompts_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut || Err("password prompt should not be used".into()),
            &mut |path: &Path| {
                assert_eq!(path, key_path.as_path());
                Ok("key-secret".to_owned())
            },
            &mut |path: &Path| {
                assert_eq!(path, key_path.as_path());
                Ok(true)
            },
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        let request = state.last_request.as_ref().unwrap();
        assert_eq!(
            request.auth,
            rssh_ssh::SshAuthMethod::PrivateKey {
                path: key_path,
                passphrase: Some("key-secret".to_owned()),
            }
        );
    }

    #[test]
    fn native_ssh_opener_uses_explicit_accept_unknown_host_key_policy() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let opener = super::native_channel_opener_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::AcceptUnknown,
            osc52_policy: Osc52Policy::default(),
            log: None,
        });

        assert_eq!(
            opener.host_key_policy(),
            rssh_ssh::RusshHostKeyPolicy::AcceptUnknown
        );
    }

    #[test]
    fn native_ssh_opener_uses_trust_on_first_use_host_key_policy() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let opener = super::native_channel_opener_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::TrustOnFirstUse,
            osc52_policy: Osc52Policy::default(),
            log: None,
        });

        assert_eq!(
            opener.host_key_policy(),
            rssh_ssh::RusshHostKeyPolicy::TrustOnFirstUse
        );
    }

    #[test]
    fn native_ssh_opener_uses_default_known_hosts_for_trust_on_first_use() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let opener = super::native_channel_opener_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::TrustOnFirstUse,
            osc52_policy: Osc52Policy::default(),
            log: None,
        });

        let known_hosts_path = opener.known_hosts_path().unwrap();
        assert_eq!(
            known_hosts_path.file_name().unwrap(),
            std::ffi::OsStr::new("known_hosts")
        );
        assert_eq!(
            known_hosts_path.parent().unwrap().file_name().unwrap(),
            std::ffi::OsStr::new(".ssh")
        );
    }

    #[test]
    fn openssh_command_uses_target_port_and_tty() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 2222, "ops", TerminalSize::new(120, 30))
                .unwrap(),
        );

        let command = super::openssh_command_for_options(&direct_options(request));

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

        let command = super::openssh_command_for_options(&direct_options(request));
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

        let command = super::openssh_command_for_options(&direct_options(request));
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
    fn openssh_local_options_preserve_osc52_policy() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let options = SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            osc52_policy: Osc52Policy::Off,
            log: None,
        };

        let local_options = super::local_options_for_options(&options).unwrap();

        assert_eq!(local_options.osc52_policy, Osc52Policy::Off);
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
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            osc52_policy: Osc52Policy::default(),
            log: None,
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

    #[test]
    fn openssh_command_appends_remote_command_after_target() {
        let options = SshOptions {
            target: SshTarget::OpenSsh(OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: TerminalSize::new(80, 24),
                auth: rssh_ssh::SshAuthMethod::Agent,
            }),
            remote_command: vec!["uname".to_owned(), "-a".to_owned()],
            forwards: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            osc52_policy: Osc52Policy::default(),
            log: None,
        };

        let command = super::openssh_command_for_options(&options);

        assert_eq!(command.args(), ["-tt", "prod", "uname", "-a"]);
    }

    #[test]
    fn openssh_command_adds_forwarding_and_no_shell_before_target() {
        let options = SshOptions {
            target: SshTarget::OpenSsh(OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: TerminalSize::new(80, 24),
                auth: rssh_ssh::SshAuthMethod::Agent,
            }),
            remote_command: Vec::new(),
            forwards: vec![
                crate::cli::SshForward::Local("127.0.0.1:15432:db.internal:5432".to_owned()),
                crate::cli::SshForward::Remote("8080:127.0.0.1:80".to_owned()),
                crate::cli::SshForward::Dynamic("127.0.0.1:1080".to_owned()),
            ],
            no_shell: true,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            osc52_policy: Osc52Policy::default(),
            log: None,
        };

        let command = super::openssh_command_for_options(&options);

        assert_eq!(
            command.args(),
            [
                "-N",
                "-L",
                "127.0.0.1:15432:db.internal:5432",
                "-R",
                "8080:127.0.0.1:80",
                "-D",
                "127.0.0.1:1080",
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

    fn direct_options(request: SshConnectRequest) -> SshOptions {
        SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            osc52_policy: Osc52Policy::default(),
            log: None,
        }
    }
}
