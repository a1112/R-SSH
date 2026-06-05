use std::path::PathBuf;

use rssh_core::TerminalSize;
use rssh_pty::{PtyCommand, PtySize};
use rssh_ssh::{SshAuthMethod, SshConnectRequest, SshSessionConfig};

const DEFAULT_SSH_COLUMNS: u16 = 80;
const DEFAULT_SSH_ROWS: u16 = 24;
const DEFAULT_PROFILE_FILE: &str = "rssh-profiles.toml";

#[derive(Debug, PartialEq, Eq)]
pub enum AppCommand {
    Local(LocalOptions),
    Profile(ProfileOptions),
    ProfileCheck(ProfileCheckOptions),
    ProfileInit(ProfileInitOptions),
    ProfileList(ProfileListOptions),
    ProfileShow(ProfileShowOptions),
    Scp(ScpOptions),
    Sftp(SftpOptions),
    Ssh(SshOptions),
    Window(WindowOptions),
    Help,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LocalOptions {
    pub command: PtyCommand,
    pub size: Option<PtySize>,
    pub mouse: bool,
    pub osc52_policy: Osc52Policy,
    pub log: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileOptions {
    pub name: String,
    pub file: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileCheckOptions {
    pub file: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileInitOptions {
    pub file: PathBuf,
    pub force: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileListOptions {
    pub file: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileShowOptions {
    pub name: String,
    pub file: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SshOptions {
    pub target: SshTarget,
    pub remote_command: Vec<String>,
    pub forwards: Vec<SshForward>,
    pub no_shell: bool,
    pub native: bool,
    pub native_host_key_policy: NativeHostKeyPolicy,
    pub osc52_policy: Osc52Policy,
    pub log: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SftpOptions {
    pub target: SshTarget,
    pub log: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ScpOptions {
    pub target: SshTarget,
    pub transfer: ScpTransfer,
    pub recursive: bool,
    pub log: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScpTransfer {
    Upload { local: PathBuf, remote: String },
    Download { remote: String, local: PathBuf },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshForward {
    Local(String),
    Remote(String),
    Dynamic(String),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NativeHostKeyPolicy {
    #[default]
    RejectUnknown,
    TrustOnFirstUse,
    AcceptUnknown,
}

#[derive(Default)]
struct SshParseState {
    host: Option<String>,
    target: Option<String>,
    username: Option<String>,
    port: Option<u16>,
    columns: Option<u16>,
    rows: Option<u16>,
    auth: Option<SshAuthMethod>,
    remote_command: Vec<String>,
    forwards: Vec<SshForward>,
    no_shell: bool,
    native: bool,
    native_host_key_policy: NativeHostKeyPolicy,
    osc52_policy: Osc52Policy,
    log: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WindowOptions {
    pub frame_limit: Option<u64>,
    pub osc52_policy: Osc52Policy,
    pub metrics: bool,
    pub command: PtyCommand,
    pub log: Option<PathBuf>,
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
            command: PtyCommand::default_shell(),
            log: None,
        }));
    };

    match command.as_str() {
        "local" => {
            let local_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&local_args) {
                return Ok(AppCommand::Help);
            }
            parse_local(&local_args)
        }
        "profile" => {
            let profile_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&profile_args) {
                return Ok(AppCommand::Help);
            }
            parse_profile(&profile_args)
        }
        "scp" => {
            let scp_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&scp_args) {
                return Ok(AppCommand::Help);
            }
            parse_scp(&scp_args)
        }
        "ssh" => {
            let ssh_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&ssh_args) {
                return Ok(AppCommand::Help);
            }
            parse_ssh(&ssh_args)
        }
        "sftp" => {
            let sftp_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&sftp_args) {
                return Ok(AppCommand::Help);
            }
            parse_sftp(&sftp_args)
        }
        "window" => {
            let window_args = args.collect::<Vec<_>>();
            if subcommand_help_requested(&window_args) {
                return Ok(AppCommand::Help);
            }
            parse_window(&window_args)
        }
        "-h" | "--help" | "help" => Ok(AppCommand::Help),
        unknown => Err(format!("unknown command: {unknown}")),
    }
}

pub fn help_text() -> &'static str {
    "R-SSH\n\nUsage:\n  rssh-app [window]\n  rssh-app window [--frames N] [--osc52 off|write|read-write] [--metrics] [--log PATH] [-- <program> [args...]]\n  rssh-app local [--cols N] [--rows N] [--mouse] [--osc52 off|write|read-write] [--log PATH] [-- <program> [args...]]\n  rssh-app ssh (--host HOST --user USER | --target NAME) [--native] [--accept-unknown-host-key | --trust-on-first-use] [--user USER] [--port N] [--cols N --rows N] [--agent | --password | --key PATH] [--local-forward SPEC] [--remote-forward SPEC] [--dynamic-forward SPEC] [--no-shell] [--osc52 off|write|read-write] [--log PATH]\n  rssh-app sftp (--host HOST --user USER | --target NAME) [--user USER] [--port N] [--cols N --rows N] [--agent | --password | --key PATH] [--log PATH]\n  rssh-app scp (--host HOST --user USER | --target NAME) [--user USER] [--port N] [--cols N --rows N] [--agent | --password | --key PATH] [--recursive] [--log PATH] (--upload LOCAL REMOTE | --download REMOTE LOCAL)\n  rssh-app profile NAME [--file PATH]\n  rssh-app profile --check [--file PATH]\n  rssh-app profile --init [--file PATH] [--force]\n  rssh-app profile --list [--file PATH]\n  rssh-app profile --show NAME [--file PATH]\n  rssh-app --help\n  rssh-app <command> --help\n"
}

fn subcommand_help_requested(args: &[String]) -> bool {
    args.iter()
        .take_while(|argument| argument.as_str() != "--")
        .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
}

fn parse_local(args: &[String]) -> Result<AppCommand, String> {
    let mut columns = None;
    let mut rows = None;
    let mut mouse = false;
    let mut osc52_policy = Osc52Policy::default();
    let mut log = None;
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
            "--osc52" => {
                index += 1;
                osc52_policy = parse_osc52_policy(args.get(index))?;
            }
            "--log" => {
                index += 1;
                log = Some(PathBuf::from(required_option_value(
                    args.get(index),
                    "--log",
                )?));
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
        osc52_policy,
        log,
    }))
}

fn parse_profile(args: &[String]) -> Result<AppCommand, String> {
    let mut name = None;
    let mut file = PathBuf::from(DEFAULT_PROFILE_FILE);
    let mut check = false;
    let mut init = false;
    let mut force = false;
    let mut list = false;
    let mut show = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--check" => {
                check = true;
            }
            "--force" => {
                force = true;
            }
            "--init" => {
                init = true;
            }
            "--list" => {
                list = true;
            }
            "--show" => {
                show = true;
            }
            "--file" => {
                index += 1;
                file = PathBuf::from(required_option_value(args.get(index), "--file")?);
            }
            value if !value.starts_with('-') && name.is_none() => {
                name = Some(value.to_owned());
            }
            value if !value.starts_with('-') => {
                return Err(format!("unexpected profile argument: {value}"));
            }
            value => return Err(format!("unexpected profile option: {value}")),
        }
        index += 1;
    }

    let selected_modes =
        usize::from(check) + usize::from(init) + usize::from(list) + usize::from(show);
    if selected_modes > 1 {
        return Err("profile mode flags cannot be combined".to_owned());
    }

    if force && !init {
        return Err("profile --force requires --init".to_owned());
    }

    if check {
        if name.is_some() {
            return Err("profile --check cannot be combined with a profile name".to_owned());
        }
        return Ok(AppCommand::ProfileCheck(ProfileCheckOptions { file }));
    }

    if init {
        if name.is_some() {
            return Err("profile --init cannot be combined with a profile name".to_owned());
        }
        return Ok(AppCommand::ProfileInit(ProfileInitOptions { file, force }));
    }

    if list {
        if name.is_some() {
            return Err("profile --list cannot be combined with a profile name".to_owned());
        }
        return Ok(AppCommand::ProfileList(ProfileListOptions { file }));
    }

    if show {
        let Some(name) = name else {
            return Err("profile --show requires a profile name".to_owned());
        };
        return Ok(AppCommand::ProfileShow(ProfileShowOptions { name, file }));
    }

    let Some(name) = name else {
        return Err("profile name is required".to_owned());
    };

    Ok(AppCommand::Profile(ProfileOptions { name, file }))
}

fn parse_ssh(args: &[String]) -> Result<AppCommand, String> {
    let mut state = SshParseState::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                state
                    .remote_command
                    .extend(args[index + 1..].iter().cloned());
                break;
            }
            _ => parse_ssh_option(args, &mut index, &mut state)?,
        }
        index += 1;
    }

    Ok(AppCommand::Ssh(ssh_options_from_state(state)?))
}

fn parse_sftp(args: &[String]) -> Result<AppCommand, String> {
    let mut state = SshParseState::default();
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--" => return Err("sftp does not accept a remote command".to_owned()),
            _ => parse_sftp_option(args, &mut index, &mut state)?,
        }
        index += 1;
    }

    let options = ssh_options_from_state(state)?;
    Ok(AppCommand::Sftp(SftpOptions {
        target: options.target,
        log: options.log,
    }))
}

fn parse_scp(args: &[String]) -> Result<AppCommand, String> {
    let mut state = SshParseState::default();
    let mut recursive = false;
    let mut transfer = None;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--" => return Err("scp does not accept a remote command separator".to_owned()),
            "--recursive" => recursive = true,
            "--upload" => {
                index += 1;
                let local = PathBuf::from(required_option_value(args.get(index), "--upload")?);
                index += 1;
                let remote = required_option_value(args.get(index), "--upload")?.to_owned();
                set_scp_transfer(&mut transfer, ScpTransfer::Upload { local, remote })?;
            }
            "--download" => {
                index += 1;
                let remote = required_option_value(args.get(index), "--download")?.to_owned();
                index += 1;
                let local = PathBuf::from(required_option_value(args.get(index), "--download")?);
                set_scp_transfer(&mut transfer, ScpTransfer::Download { remote, local })?;
            }
            _ => parse_sftp_option(args, &mut index, &mut state)?,
        }
        index += 1;
    }

    let Some(transfer) = transfer else {
        return Err("scp requires --upload or --download".to_owned());
    };
    let options = ssh_options_from_state(state)?;
    Ok(AppCommand::Scp(ScpOptions {
        target: options.target,
        transfer,
        recursive,
        log: options.log,
    }))
}

fn set_scp_transfer(transfer: &mut Option<ScpTransfer>, next: ScpTransfer) -> Result<(), String> {
    if transfer.is_some() {
        return Err("only one scp transfer direction can be selected".to_owned());
    }

    *transfer = Some(next);
    Ok(())
}

fn parse_sftp_option(
    args: &[String],
    index: &mut usize,
    state: &mut SshParseState,
) -> Result<(), String> {
    match args[*index].as_str() {
        "--host" => {
            *index += 1;
            state.host = Some(required_option_value(args.get(*index), "--host")?.to_owned());
        }
        "--target" => {
            *index += 1;
            state.target = Some(required_option_value(args.get(*index), "--target")?.to_owned());
        }
        "--user" => {
            *index += 1;
            state.username = Some(required_option_value(args.get(*index), "--user")?.to_owned());
        }
        "--port" => {
            *index += 1;
            state.port = Some(parse_port(args.get(*index), "--port")?);
        }
        "--cols" => {
            *index += 1;
            state.columns = Some(parse_dimension(args.get(*index), "--cols")?);
        }
        "--rows" => {
            *index += 1;
            state.rows = Some(parse_dimension(args.get(*index), "--rows")?);
        }
        "--agent" => {
            set_ssh_auth(&mut state.auth, SshAuthMethod::Agent)?;
        }
        "--password" => {
            set_ssh_auth(&mut state.auth, SshAuthMethod::PasswordPrompt)?;
        }
        "--key" => {
            *index += 1;
            let path = required_option_value(args.get(*index), "--key")?;
            set_ssh_auth(
                &mut state.auth,
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
        "--log" => {
            *index += 1;
            state.log = Some(PathBuf::from(required_option_value(
                args.get(*index),
                "--log",
            )?));
        }
        value => return Err(format!("unexpected sftp option: {value}")),
    }

    Ok(())
}

fn parse_ssh_option(
    args: &[String],
    index: &mut usize,
    state: &mut SshParseState,
) -> Result<(), String> {
    match args[*index].as_str() {
        "--host" => {
            *index += 1;
            state.host = Some(required_option_value(args.get(*index), "--host")?.to_owned());
        }
        "--target" => {
            *index += 1;
            state.target = Some(required_option_value(args.get(*index), "--target")?.to_owned());
        }
        "--user" => {
            *index += 1;
            state.username = Some(required_option_value(args.get(*index), "--user")?.to_owned());
        }
        "--port" => {
            *index += 1;
            state.port = Some(parse_port(args.get(*index), "--port")?);
        }
        "--cols" => {
            *index += 1;
            state.columns = Some(parse_dimension(args.get(*index), "--cols")?);
        }
        "--rows" => {
            *index += 1;
            state.rows = Some(parse_dimension(args.get(*index), "--rows")?);
        }
        "--agent" => {
            set_ssh_auth(&mut state.auth, SshAuthMethod::Agent)?;
        }
        "--password" => {
            set_ssh_auth(&mut state.auth, SshAuthMethod::PasswordPrompt)?;
        }
        "--key" => {
            *index += 1;
            let path = required_option_value(args.get(*index), "--key")?;
            set_ssh_auth(
                &mut state.auth,
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
        "--local-forward" => {
            *index += 1;
            state.forwards.push(SshForward::Local(required_forward_spec(
                args.get(*index),
                "--local-forward",
            )?));
        }
        "--remote-forward" => {
            *index += 1;
            state
                .forwards
                .push(SshForward::Remote(required_forward_spec(
                    args.get(*index),
                    "--remote-forward",
                )?));
        }
        "--dynamic-forward" => {
            *index += 1;
            state
                .forwards
                .push(SshForward::Dynamic(required_forward_spec(
                    args.get(*index),
                    "--dynamic-forward",
                )?));
        }
        "--no-shell" => {
            state.no_shell = true;
        }
        "--native" => {
            state.native = true;
        }
        "--accept-unknown-host-key" | "--trust-on-first-use" => {
            parse_native_host_key_policy(args[*index].as_str(), state)?;
        }
        "--osc52" => {
            *index += 1;
            state.osc52_policy = parse_osc52_policy(args.get(*index))?;
        }
        "--log" => {
            *index += 1;
            state.log = Some(PathBuf::from(required_option_value(
                args.get(*index),
                "--log",
            )?));
        }
        value => return Err(format!("unexpected ssh option: {value}")),
    }

    Ok(())
}

fn ssh_options_from_state(state: SshParseState) -> Result<SshOptions, String> {
    let SshParseState {
        host,
        target,
        username,
        port,
        columns,
        rows,
        auth,
        remote_command,
        forwards,
        no_shell,
        native,
        native_host_key_policy,
        osc52_policy,
        log,
    } = state;

    if no_shell && !remote_command.is_empty() {
        return Err("--no-shell cannot be combined with a remote command".to_owned());
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

    if matches!(native_host_key_policy, NativeHostKeyPolicy::AcceptUnknown) && !native {
        return Err("--accept-unknown-host-key requires --native".to_owned());
    }
    if matches!(native_host_key_policy, NativeHostKeyPolicy::TrustOnFirstUse) && !native {
        return Err("--trust-on-first-use requires --native".to_owned());
    }

    Ok(SshOptions {
        target,
        remote_command,
        forwards,
        no_shell,
        native,
        native_host_key_policy,
        osc52_policy,
        log,
    })
}

fn parse_window(args: &[String]) -> Result<AppCommand, String> {
    let mut frame_limit = None;
    let mut osc52_policy = Osc52Policy::default();
    let mut metrics = false;
    let mut log = None;
    let mut command_args = Vec::new();
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
            "--log" => {
                index += 1;
                log = Some(PathBuf::from(required_option_value(
                    args.get(index),
                    "--log",
                )?));
            }
            "--" => {
                command_args.extend(args[index + 1..].iter().cloned());
                break;
            }
            value => return Err(format!("unexpected window option: {value}")),
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

    Ok(AppCommand::Window(WindowOptions {
        frame_limit,
        osc52_policy,
        metrics,
        command,
        log,
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

fn required_forward_spec(value: Option<&String>, name: &str) -> Result<String, String> {
    let value = required_option_value(value, name)?;
    if value.trim().is_empty() {
        return Err(format!("{name} cannot be empty"));
    }

    Ok(value.to_owned())
}

fn set_ssh_auth(auth: &mut Option<SshAuthMethod>, next: SshAuthMethod) -> Result<(), String> {
    if auth.is_some() {
        return Err("only one ssh authentication method can be selected".to_owned());
    }

    *auth = Some(next);
    Ok(())
}

fn parse_native_host_key_policy(option: &str, state: &mut SshParseState) -> Result<(), String> {
    let policy = match option {
        "--accept-unknown-host-key" => NativeHostKeyPolicy::AcceptUnknown,
        "--trust-on-first-use" => NativeHostKeyPolicy::TrustOnFirstUse,
        _ => unreachable!("only native host-key policy options call this helper"),
    };

    set_native_host_key_policy(&mut state.native_host_key_policy, policy)
}

fn set_native_host_key_policy(
    policy: &mut NativeHostKeyPolicy,
    next: NativeHostKeyPolicy,
) -> Result<(), String> {
    if !matches!(policy, NativeHostKeyPolicy::RejectUnknown) {
        return Err("only one native SSH host-key policy can be selected".to_owned());
    }

    *policy = next;
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

    use super::{AppCommand, NativeHostKeyPolicy, parse_args};

    #[test]
    fn parses_default_window_command() {
        assert_eq!(
            parse_args(["rssh-app"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::ReadWrite,
                metrics: false,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
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
                metrics: false,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
            })
        );
    }

    #[test]
    fn parses_profile_command_with_config_file() {
        assert_eq!(
            parse_args(["rssh-app", "profile", "prod", "--file", "profiles.toml"]).unwrap(),
            AppCommand::Profile(super::ProfileOptions {
                name: "prod".to_owned(),
                file: std::path::PathBuf::from("profiles.toml"),
            })
        );
    }

    #[test]
    fn parses_profile_list_command_with_config_file() {
        assert_eq!(
            parse_args(["rssh-app", "profile", "--list", "--file", "profiles.toml"]).unwrap(),
            AppCommand::ProfileList(super::ProfileListOptions {
                file: std::path::PathBuf::from("profiles.toml"),
            })
        );
    }

    #[test]
    fn parses_profile_check_command_with_config_file() {
        assert_eq!(
            parse_args(["rssh-app", "profile", "--check", "--file", "profiles.toml"]).unwrap(),
            AppCommand::ProfileCheck(super::ProfileCheckOptions {
                file: std::path::PathBuf::from("profiles.toml"),
            })
        );
    }

    #[test]
    fn parses_profile_init_command_with_force_and_config_file() {
        assert_eq!(
            parse_args([
                "rssh-app",
                "profile",
                "--init",
                "--force",
                "--file",
                "profiles.toml"
            ])
            .unwrap(),
            AppCommand::ProfileInit(super::ProfileInitOptions {
                file: std::path::PathBuf::from("profiles.toml"),
                force: true,
            })
        );
    }

    #[test]
    fn parses_profile_show_command_with_config_file() {
        assert_eq!(
            parse_args([
                "rssh-app",
                "profile",
                "--show",
                "prod",
                "--file",
                "profiles.toml"
            ])
            .unwrap(),
            AppCommand::ProfileShow(super::ProfileShowOptions {
                name: "prod".to_owned(),
                file: std::path::PathBuf::from("profiles.toml"),
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
                metrics: false,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
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
                metrics: true,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
            })
        );
    }

    #[test]
    fn parses_window_custom_command_after_separator() {
        let parsed = parse_args([
            "rssh-app",
            "window",
            "--",
            "powershell",
            "-NoProfile",
            "-Command",
            "Write-Output window-smoke",
        ])
        .unwrap();

        let AppCommand::Window(options) = parsed else {
            panic!("expected window command");
        };

        assert_eq!(options.command.program(), "powershell");
        assert_eq!(
            options.command.args(),
            ["-NoProfile", "-Command", "Write-Output window-smoke"]
        );
    }

    #[test]
    fn parses_window_log_path() {
        let parsed = parse_args(["rssh-app", "window", "--log", "window.log"]).unwrap();

        let AppCommand::Window(options) = parsed else {
            panic!("expected window command");
        };

        assert_eq!(options.log, Some(std::path::PathBuf::from("window.log")));
    }

    #[test]
    fn parses_window_osc52_policy() {
        assert_eq!(
            parse_args(["rssh-app", "window", "--osc52", "off"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::Off,
                metrics: false,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
            })
        );
        assert_eq!(
            parse_args(["rssh-app", "window", "--osc52", "write"]).unwrap(),
            AppCommand::Window(super::WindowOptions {
                frame_limit: None,
                osc52_policy: super::Osc52Policy::WriteOnly,
                metrics: false,
                command: rssh_pty::PtyCommand::default_shell(),
                log: None
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
    fn parses_local_log_path() {
        let parsed = parse_args(["rssh-app", "local", "--log", "session.log"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        assert_eq!(options.log, Some(std::path::PathBuf::from("session.log")));
    }

    #[test]
    fn parses_local_osc52_policy() {
        let parsed = parse_args(["rssh-app", "local", "--osc52", "write"]).unwrap();

        let AppCommand::Local(options) = parsed else {
            panic!("expected local command");
        };

        assert_eq!(options.osc52_policy, super::Osc52Policy::WriteOnly);
        assert!(parse_args(["rssh-app", "local", "--osc52", "bad"]).is_err());
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
    fn parses_ssh_native_direct_backend() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--native",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert!(options.native);
        assert_eq!(
            options.native_host_key_policy,
            NativeHostKeyPolicy::RejectUnknown
        );
    }

    #[test]
    fn parses_ssh_native_accept_unknown_host_key_flag() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--native",
            "--accept-unknown-host-key",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(
            options.native_host_key_policy,
            NativeHostKeyPolicy::AcceptUnknown
        );
    }

    #[test]
    fn parses_ssh_native_trust_on_first_use_host_key_flag() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--native",
            "--trust-on-first-use",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(
            options.native_host_key_policy,
            NativeHostKeyPolicy::TrustOnFirstUse
        );
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
        assert!(options.remote_command.is_empty());
        assert!(!options.native);
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
        assert!(options.remote_command.is_empty());
    }

    #[test]
    fn parses_ssh_remote_command_for_openssh_config_target() {
        let parsed =
            parse_args(["rssh-app", "ssh", "--target", "prod", "--", "uname", "-a"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(options.remote_command, ["uname", "-a"]);
    }

    #[test]
    fn parses_ssh_remote_command_for_direct_target() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--host",
            "example.com",
            "--user",
            "ops",
            "--",
            "whoami",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        let super::SshTarget::Direct(request) = options.target else {
            panic!("expected direct SSH target");
        };

        assert_eq!(request.config.host, "example.com");
        assert_eq!(options.remote_command, ["whoami"]);
    }

    #[test]
    fn rejects_accept_unknown_host_key_without_native() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--accept-unknown-host-key",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap_err();

        assert!(error.contains("--accept-unknown-host-key requires --native"));
    }

    #[test]
    fn rejects_trust_on_first_use_without_native() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--trust-on-first-use",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap_err();

        assert!(error.contains("--trust-on-first-use requires --native"));
    }

    #[test]
    fn rejects_conflicting_native_host_key_policies() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--native",
            "--accept-unknown-host-key",
            "--trust-on-first-use",
            "--host",
            "example.com",
            "--user",
            "ops",
        ])
        .unwrap_err();

        assert!(error.contains("only one native SSH host-key policy"));
    }

    #[test]
    fn parses_ssh_log_path() {
        let parsed =
            parse_args(["rssh-app", "ssh", "--target", "prod", "--log", "ssh.log"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(options.log, Some(std::path::PathBuf::from("ssh.log")));
    }

    #[test]
    fn parses_ssh_osc52_policy() {
        let parsed = parse_args(["rssh-app", "ssh", "--target", "prod", "--osc52", "off"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(options.osc52_policy, super::Osc52Policy::Off);
        assert!(parse_args(["rssh-app", "ssh", "--target", "prod", "--osc52", "bad"]).is_err());
    }

    #[test]
    fn parses_ssh_forwarding_and_no_shell_options() {
        let parsed = parse_args([
            "rssh-app",
            "ssh",
            "--target",
            "prod",
            "--local-forward",
            "127.0.0.1:15432:db.internal:5432",
            "--remote-forward",
            "8080:127.0.0.1:80",
            "--dynamic-forward",
            "127.0.0.1:1080",
            "--no-shell",
        ])
        .unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };

        assert_eq!(
            options.forwards,
            [
                super::SshForward::Local("127.0.0.1:15432:db.internal:5432".to_owned()),
                super::SshForward::Remote("8080:127.0.0.1:80".to_owned()),
                super::SshForward::Dynamic("127.0.0.1:1080".to_owned())
            ]
        );
        assert!(options.no_shell);
    }

    #[test]
    fn rejects_empty_ssh_forwarding_spec() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--target",
            "prod",
            "--local-forward",
            " ",
        ])
        .unwrap_err();

        assert!(error.contains("--local-forward cannot be empty"));
    }

    #[test]
    fn rejects_no_shell_with_remote_command() {
        let error = parse_args([
            "rssh-app",
            "ssh",
            "--target",
            "prod",
            "--no-shell",
            "--",
            "uptime",
        ])
        .unwrap_err();

        assert!(error.contains("--no-shell cannot be combined with a remote command"));
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
    fn parses_sftp_openssh_config_target_with_key_and_log() {
        let parsed = parse_args([
            "rssh-app",
            "sftp",
            "--target",
            "prod",
            "--key",
            "C:/Users/ops/.ssh/id_ed25519",
            "--log",
            "sftp.log",
        ])
        .unwrap();

        let AppCommand::Sftp(options) = parsed else {
            panic!("expected sftp command");
        };

        assert_eq!(
            options.target,
            super::SshTarget::OpenSsh(super::OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: super::ssh_default_terminal_size(),
                auth: SshAuthMethod::PrivateKey {
                    path: "C:/Users/ops/.ssh/id_ed25519".into(),
                    passphrase: None
                }
            })
        );
        assert_eq!(options.log, Some(std::path::PathBuf::from("sftp.log")));
    }

    #[test]
    fn parses_scp_upload_for_openssh_config_target() {
        let parsed = parse_args([
            "rssh-app",
            "scp",
            "--target",
            "prod",
            "--key",
            "C:/Users/ops/.ssh/id_ed25519",
            "--recursive",
            "--log",
            "scp.log",
            "--upload",
            "local.txt",
            "/tmp/remote.txt",
        ])
        .unwrap();

        let AppCommand::Scp(options) = parsed else {
            panic!("expected scp command");
        };

        assert_eq!(
            options.target,
            super::SshTarget::OpenSsh(super::OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: super::ssh_default_terminal_size(),
                auth: SshAuthMethod::PrivateKey {
                    path: "C:/Users/ops/.ssh/id_ed25519".into(),
                    passphrase: None
                }
            })
        );
        assert_eq!(
            options.transfer,
            super::ScpTransfer::Upload {
                local: "local.txt".into(),
                remote: "/tmp/remote.txt".to_owned(),
            }
        );
        assert!(options.recursive);
        assert_eq!(options.log, Some(std::path::PathBuf::from("scp.log")));
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
        assert!(help.contains("--native"));
        assert!(help.contains("--accept-unknown-host-key"));
        assert!(help.contains("--target"));
        assert!(help.contains("rssh-app <command> --help"));
        assert!(!help.contains("PASSWORD"));
        assert!(!help.contains("PASSPHRASE"));
    }

    #[test]
    fn parses_subcommand_help_before_command_separator() {
        assert_eq!(
            parse_args(["rssh-app", "local", "--help"]).unwrap(),
            AppCommand::Help
        );
        assert_eq!(
            parse_args(["rssh-app", "window", "-h"]).unwrap(),
            AppCommand::Help
        );
        assert_eq!(
            parse_args(["rssh-app", "ssh", "--help"]).unwrap(),
            AppCommand::Help
        );
        assert_eq!(
            parse_args(["rssh-app", "profile", "--help"]).unwrap(),
            AppCommand::Help
        );
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
    fn parses_ssh_native_openssh_config_target() {
        let parsed = parse_args(["rssh-app", "ssh", "--native", "--target", "prod"]).unwrap();

        let AppCommand::Ssh(options) = parsed else {
            panic!("expected ssh command");
        };
        assert!(options.native);
        assert!(matches!(options.target, super::SshTarget::OpenSsh(_)));
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
