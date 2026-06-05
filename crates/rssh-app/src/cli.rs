use rssh_core::TerminalSize;
use rssh_pty::{PtyCommand, PtySize};
use rssh_ssh::{SshAuthMethod, SshConnectRequest, SshSessionConfig};

const DEFAULT_SSH_COLUMNS: u16 = 80;
const DEFAULT_SSH_ROWS: u16 = 24;

#[derive(Debug, PartialEq, Eq)]
pub enum AppCommand {
    Local(LocalOptions),
    Ssh(SshOptions),
    Window(WindowOptions),
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LocalOptions {
    pub command: PtyCommand,
    pub size: Option<PtySize>,
    pub mouse: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SshOptions {
    pub target: SshTarget,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SshTarget {
    Direct(SshConnectRequest),
    OpenSsh(OpenSshTarget),
}

#[derive(Debug, PartialEq, Eq)]
pub struct OpenSshTarget {
    pub target: String,
    pub username: Option<String>,
    pub port: Option<u16>,
    pub initial_size: TerminalSize,
    pub auth: SshAuthMethod,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WindowOptions {
    pub frame_limit: Option<u64>,
    pub osc52_policy: Osc52Policy,
    pub metrics: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Osc52Policy {
    Off,
    WriteOnly,
    #[default]
    ReadWrite,
}

impl Osc52Policy {
    pub const fn allows_write(self) -> bool {
        matches!(self, Self::WriteOnly | Self::ReadWrite)
    }

    pub const fn allows_query(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

pub fn parse_args<I, S>(args: I) -> Result<AppCommand, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let Some(command) = args.next() else {
        return Ok(AppCommand::Window(WindowOptions {
            frame_limit: None,
            osc52_policy: Osc52Policy::default(),
            metrics: false,
        }));
    };

    match command.as_str() {
        "local" => {
            let local_args = args.collect::<Vec<_>>();
            parse_local(&local_args)
        }
        "ssh" => {
            let ssh_args = args.collect::<Vec<_>>();
            parse_ssh(&ssh_args)
        }
        "window" => {
            let window_args = args.collect::<Vec<_>>();
            parse_window(&window_args)
        }
        "-h" | "--help" | "help" => Ok(AppCommand::Help),
        unknown => Err(format!("unknown command: {unknown}")),
    }
}

pub fn help_text() -> &'static str {
    "R-SSH\n\nUsage:\n  rssh-app [window]\n  rssh-app window [--frames N] [--osc52 off|write|read-write] [--metrics]\n  rssh-app local [--cols N] [--rows N] [--mouse] [-- <program> [args...]]\n  rssh-app ssh (--host HOST --user USER | --target NAME) [--user USER] [--port N] [--cols N --rows N] [--agent | --password | --key PATH]\n  rssh-app --help\n"
}

fn parse_local(args: &[String]) -> Result<AppCommand, String> {
    let mut columns = None;
    let mut rows = None;
    let mut mouse = false;
    let mut command_args = Vec::new();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--cols" => {
                index += 1;
                columns = Some(parse_dimension(args.get(index), "--cols")?);
            }
            "--rows" => {
                index += 1;
                rows = Some(parse_dimension(args.get(index), "--rows")?);
            }
            "--mouse" => {
                mouse = true;
            }
            "--" => {
                command_args.extend(args[index + 1..].iter().cloned());
                break;
            }
            value => return Err(format!("unexpected local option: {value}")),
        }
        index += 1;
    }

    let command = if command_args.is_empty() {
        PtyCommand::default_shell()
    } else {
        let mut iter = command_args.into_iter();
        let program = iter.next().expect("command_args is not empty");
        PtyCommand::new(program).with_args(iter)
    };

    let size = match (columns, rows) {
        (None, None) => None,
        (Some(columns), Some(rows)) => {
            Some(PtySize::try_new(columns, rows).map_err(|error| error.to_string())?)
        }
        (None, Some(_)) => return Err("--rows requires --cols".to_owned()),
        (Some(_), None) => return Err("--cols requires --rows".to_owned()),
    };

    Ok(AppCommand::Local(LocalOptions {
        command,
        size,
        mouse,
    }))
}

fn parse_ssh(args: &[String]) -> Result<AppCommand, String> {
    let mut host = None;
    let mut target = None;
    let mut username = None;
    let mut port = None;
    let mut columns = None;
    let mut rows = None;
    let mut auth = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--host" => {
                index += 1;
                host = Some(required_option_value(args.get(index), "--host")?.to_owned());
            }
            "--target" => {
                index += 1;
                target = Some(required_option_value(args.get(index), "--target")?.to_owned());
            }
            "--user" => {
                index += 1;
                username = Some(required_option_value(args.get(index), "--user")?.to_owned());
            }
            "--port" => {
                index += 1;
                port = Some(parse_port(args.get(index), "--port")?);
            }
            "--cols" => {
                index += 1;
                columns = Some(parse_dimension(args.get(index), "--cols")?);
            }
            "--rows" => {
                index += 1;
                rows = Some(parse_dimension(args.get(index), "--rows")?);
            }
            "--agent" => {
                set_ssh_auth(&mut auth, SshAuthMethod::Agent)?;
            }
            "--password" => {
                set_ssh_auth(&mut auth, SshAuthMethod::PasswordPrompt)?;
            }
            "--key" => {
                index += 1;
                let path = required_option_value(args.get(index), "--key")?;
                set_ssh_auth(
                    &mut auth,
                    SshAuthMethod::private_key(path, None::<String>)
                        .map_err(|error| error.to_string())?,
                )?;
            }
            "--passphrase" => {
                return Err(
                    "--passphrase is not accepted on the command line; use the terminal prompt"
                        .to_owned(),
                );
            }
            value => return Err(format!("unexpected ssh option: {value}")),
        }
        index += 1;
    }

    let size = ssh_terminal_size(columns, rows)?;
    let auth = auth.unwrap_or(SshAuthMethod::Agent);
    let target = match (host, target) {
        (Some(_), Some(_)) => {
            return Err("only one of --host or --target can be selected".to_owned());
        }
        (Some(host), None) => {
            let Some(username) = username else {
                return Err("--user is required with --host".to_owned());
            };
            let config = SshSessionConfig::try_new(host, port.unwrap_or(22), username, size)
                .map_err(|error| error.to_string())?;
            SshTarget::Direct(SshConnectRequest::new(config, auth))
        }
        (None, Some(target)) => SshTarget::OpenSsh(OpenSshTarget {
            target,
            username,
            port,
            initial_size: size,
            auth,
        }),
        (None, None) => return Err("--host or --target is required".to_owned()),
    };

    Ok(AppCommand::Ssh(SshOptions { target }))
}

fn parse_window(args: &[String]) -> Result<AppCommand, String> {
    let mut frame_limit = None;
    let mut osc52_policy = Osc52Policy::default();
    let mut metrics = false;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--frames" => {
                index += 1;
                frame_limit = Some(parse_frame_limit(args.get(index))?);
            }
            "--osc52" => {
                index += 1;
                osc52_policy = parse_osc52_policy(args.get(index))?;
            }
            "--metrics" => {
                metrics = true;
            }
            value => return Err(format!("unexpected window option: {value}")),
        }
        index += 1;
    }

    Ok(AppCommand::Window(WindowOptions {
        frame_limit,
        osc52_policy,
        metrics,
    }))
}

fn parse_osc52_policy(value: Option<&String>) -> Result<Osc52Policy, String> {
    let Some(value) = value else {
        return Err("missing value for --osc52".to_owned());
    };

    match value.as_str() {
        "off" => Ok(Osc52Policy::Off),
        "write" => Ok(Osc52Policy::WriteOnly),
        "read-write" => Ok(Osc52Policy::ReadWrite),
        _ => Err(format!("invalid value for --osc52: {value}")),
    }
}

fn parse_frame_limit(value: Option<&String>) -> Result<u64, String> {
    let Some(value) = value else {
        return Err("missing value for --frames".to_owned());
    };

    value
        .parse::<u64>()
        .map_err(|_| format!("invalid value for --frames: {value}"))
}

fn parse_port(value: Option<&String>, name: &str) -> Result<u16, String> {
    let Some(value) = value else {
        return Err(format!("missing value for {name}"));
    };

    value
        .parse::<u16>()
        .map_err(|_| format!("invalid value for {name}: {value}"))
}

fn parse_dimension(value: Option<&String>, name: &str) -> Result<u16, String> {
    let Some(value) = value else {
        return Err(format!("missing value for {name}"));
    };

    value
        .parse::<u16>()
        .map_err(|_| format!("invalid value for {name}: {value}"))
}

fn required_option_value<'a>(value: Option<&'a String>, name: &str) -> Result<&'a str, String> {
    value
        .map(String::as_str)
        .ok_or_else(|| format!("missing value for {name}"))
}

fn set_ssh_auth(auth: &mut Option<SshAuthMethod>, next: SshAuthMethod) -> Result<(), String> {
    if auth.is_some() {
        return Err("only one ssh authentication method can be selected".to_owned());
    }

    *auth = Some(next);
    Ok(())
}

fn ssh_terminal_size(columns: Option<u16>, rows: Option<u16>) -> Result<TerminalSize, String> {
    match (columns, rows) {
        (None, None) => Ok(ssh_default_terminal_size()),
        (Some(columns), Some(rows)) => Ok(TerminalSize::new(columns, rows)),
        (None, Some(_)) => Err("--rows requires --cols".to_owned()),
        (Some(_), None) => Err("--cols requires --rows".to_owned()),
    }
}

const fn ssh_default_terminal_size() -> TerminalSize {
    TerminalSize::new(DEFAULT_SSH_COLUMNS, DEFAULT_SSH_ROWS)
}

#[cfg(test)]
mod tests {
    use rssh_ssh::SshAuthMethod;

    use super::{AppCommand, parse_args};

    #[test]
    fn parses_default_window_command() {
        assert_eq!(
            parse_args(["rssh-app"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::ReadWrite,
                metrics: false
            })
        );
    }

    #[test]
    fn parses_explicit_window_command() {
        assert_eq!(
            parse_args(["rssh-app", "window"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::ReadWrite,
                metrics: false
            })
        );
    }

    #[test]
    fn parses_window_frame_limit() {
        assert_eq!(
            parse_args(["rssh-app", "window", "--frames", "1"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: Some(1),
                osc52_policy: super::Osc52Policy::ReadWrite,
                metrics: false
            })
        );
    }

    #[test]
    fn parses_window_metrics_flag() {
        assert_eq!(
            parse_args(["rssh-app", "window", "--metrics"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::ReadWrite,
                metrics: true
            })
        );
    }

    #[test]
    fn parses_window_osc52_policy() {
        assert_eq!(
            parse_args(["rssh-app", "window", "--osc52", "off"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::Off,
                metrics: false
            })
        );
        assert_eq!(
            parse_args(["rssh-app", "window", "--osc52", "write"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::WriteOnly,
                metrics: false
            })
        );
        assert!(parse_args(["rssh-app", "window", "--osc52", "bad"]).is_err());
    }

    #[test]
    fn parses_local_default_shell() {
        let parsed = parse_args(["rssh-app", "local"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        assert!(!options.command.program().is_empty());
        assert_eq!(options.size, None);
        assert!(!options.mouse);
    }

    #[test]
    fn parses_local_size() {
        let parsed = parse_args(["rssh-app", "local", "--cols", "100", "--rows", "30"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        let size = options.size.unwrap();
        assert_eq!(size.columns(), 100);
        assert_eq!(size.rows(), 30);
    }

    #[test]
    fn parses_custom_local_command_after_separator() {
        let parsed = parse_args(["rssh-app", "local", "--", "cmd.exe", "/K"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        assert_eq!(options.command.program(), "cmd.exe");
        assert_eq!(options.command.args(), ["/K"]);
        assert_eq!(options.size, None);
        assert!(!options.mouse);
    }

    #[test]
    fn parses_local_mouse_capture() {
        let parsed = parse_args(["rssh-app", "local", "--mouse"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        assert!(options.mouse);
    }

    #[test]
    fn parses_ssh_agent_connection_request() {
        let parsed =
            parse_args(["rssh-app", "ssh", "--host", "example.com", "--user", "ops"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        let super::SshTarget::Direct(request) = options.target else {
            panic!("expected direct SSH target");
        };

        assert_eq!(request.config.host, "example.com");
        assert_eq!(request.config.port, 22);
        assert_eq!(request.config.username, "ops");
        assert_eq!(request.auth, SshAuthMethod::Agent);
    }

    #[test]
    fn parses_ssh_openssh_config_target() {
        let parsed = parse_args(["rssh-app", "ssh", "--target", "prod"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(
            options.target,
            super::SshTarget::OpenSsh(super::OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: super::ssh_default_terminal_size(),
                auth: SshAuthMethod::Agent
            })
        );
    }

    #[test]
    fn parses_ssh_openssh_config_target_with_overrides() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--target",
            "prod",
            "--user",
            "ops",
            "--port",
            "2222",
            "--cols",
            "120",
            "--rows",
            "30",
            "--key",
            "C:/Users/ops/.ssh/id_ed25519",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(
            options.target,
            super::SshTarget::OpenSsh(super::OpenSshTarget {
                target: "prod".to_owned(),
                username: Some("ops".to_owned()),
                port: Some(2222),
                initial_size: rssh_core::TerminalSize::new(120, 30),
                auth: SshAuthMethod::PrivateKey {
                    path: "C:/Users/ops/.ssh/id_ed25519".into(),
                    passphrase: None
                }
            })
        );
    }

    #[test]
    fn parses_ssh_password_connection_request() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--user",
            "ops",
            "--port",
            "2222",
            "--cols",
            "120",
            "--rows",
            "30",
            "--password",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        let super::SshTarget::Direct(request) = options.target else {
            panic!("expected direct SSH target");
        };

        assert_eq!(request.config.port, 2222);
        assert_eq!(request.config.initial_size.columns, 120);
        assert_eq!(request.config.initial_size.rows, 30);
        assert_eq!(request.auth, SshAuthMethod::PasswordPrompt);
    }

    #[test]
    fn parses_ssh_private_key_connection_request() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--user",
            "ops",
            "--key",
            "C:/Users/ops/.ssh/id_ed25519",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        let super::SshTarget::Direct(request) = options.target else {
            panic!("expected direct SSH target");
        };

        assert_eq!(
            request.auth,
            SshAuthMethod::PrivateKey {
                path: "C:/Users/ops/.ssh/id_ed25519".into(),
                passphrase: None
            }
        );
    }

    #[test]
    fn rejects_ssh_password_command_line_secret() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--user",
            "ops",
            "--password",
            "secret",
        ])
        .unwrap_err();

        assert!(error.contains("unexpected ssh option: secret"));
    }

    #[test]
    fn rejects_ssh_passphrase_command_line_secret() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--user",
            "ops",
            "--key",
            "C:/Users/ops/.ssh/id_ed25519",
            "--passphrase",
            "secret",
        ])
        .unwrap_err();

        assert!(error.contains("--passphrase is not accepted"));
    }

    #[test]
    fn help_text_does_not_request_secret_values() {
        let help = super::help_text();

        assert!(help.contains("--password"));
        assert!(help.contains("--target"));
        assert!(!help.contains("PASSWORD"));
        assert!(!help.contains("PASSPHRASE"));
    }

    #[test]
    fn rejects_ssh_missing_host_or_user() {
        assert!(parse_args(["rssh-app", "ssh", "--user", "ops"]).is_err());
        assert!(parse_args(["rssh-app", "ssh", "--host", "example.com"]).is_err());
    }

    #[test]
    fn rejects_ssh_host_and_target_together() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--target",
            "prod",
            "--user",
            "ops",
        ])
        .unwrap_err();

        assert!(error.contains("only one of --host or --target can be selected"));
    }

    #[test]
    fn rejects_ssh_conflicting_auth_methods() {
        assert!(
            parse_args([
                "rssh-app",
                "ssh",
                "--host",
                "example.com",
                "--user",
                "ops",
                "--password",
                "--key",
                "C:/Users/ops/.ssh/id_ed25519",
            ])
            .is_err()
        );
    }

    #[test]
    fn rejects_unknown_command() {
        assert!(parse_args(["rssh-app", "wat"]).is_err());
    }

    #[test]
    fn rejects_partial_local_size() {
        assert!(parse_args(["rssh-app", "local", "--cols", "100"]).is_err());
        assert!(parse_args(["rssh-app", "local", "--rows", "30"]).is_err());
    }
}
