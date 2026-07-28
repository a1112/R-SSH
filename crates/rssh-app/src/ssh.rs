use std::{
    error::Error,
    io::{self, Read, Write},
    net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use rssh_core::{
    SessionId,
    session::{SessionLifecycle, SessionState},
};
use rssh_pty::{PtyCommand, PtyExitStatus, PtySize};
use rssh_ssh::{
    RusshChannelOpener, RusshDirectTcpIpOpenPlan, RusshHostKeyPolicy, RusshPrivateKeyAuth,
    RusshRemoteTcpIpForwardPlan, SshAuthMethod, SshChannelConnector, SshConnectRequest,
    SshSessionConfig, SshSessionStartup, SshShellConnector,
};
use serde::Serialize;

use crate::{
    cli::{LocalOptions, NativeHostKeyPolicy, OpenSshTarget, SshForward, SshOptions, SshTarget},
    local,
};

type SecretPrompt<'a> = dyn FnMut(&SshConnectRequest) -> Result<String, Box<dyn Error>> + 'a;
type KeyPassphrasePrompt<'a> = dyn FnMut(&Path) -> Result<String, Box<dyn Error>> + 'a;
type KeyPassphraseDetector<'a> = dyn FnMut(&Path) -> Result<bool, Box<dyn Error>> + 'a;
type OpenSshConfigResolver<'a> = dyn FnMut(&OpenSshTarget) -> Result<String, Box<dyn Error>> + 'a;

const NATIVE_SSH_SESSION_ID: SessionId = SessionId::new(2);

struct NativeSecretPrompts<'a> {
    password_prompt: &'a mut SecretPrompt<'a>,
    key_passphrase_prompt: &'a mut KeyPassphrasePrompt<'a>,
    key_needs_passphrase: &'a mut KeyPassphraseDetector<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeLocalForward {
    bind_host: String,
    bind_port: u16,
    target_host: String,
    target_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeDynamicForward {
    bind_host: String,
    bind_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeRemoteForward {
    bind_host: String,
    bind_port: u16,
    target_host: String,
    target_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct NativeForwardPlan {
    local: Vec<NativeLocalForward>,
    dynamic: Vec<NativeDynamicForward>,
    remote: Vec<NativeRemoteForward>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Socks5ConnectRequest {
    target_host: String,
    target_port: u16,
}

trait NativeLocalForwardStarter {
    fn start(
        &mut self,
        request: SshConnectRequest,
        forward: NativeLocalForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>>;

    fn start_dynamic(
        &mut self,
        request: SshConnectRequest,
        forward: NativeDynamicForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>>;

    fn start_remote(
        &mut self,
        request: SshConnectRequest,
        forward: NativeRemoteForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>>;
}

trait NativeLocalForwardHandle {
    fn wait(&mut self) -> Result<(), Box<dyn Error>>;
}

#[derive(Clone)]
struct ThreadedNativeLocalForwardStarter {
    opener: RusshChannelOpener,
}

impl ThreadedNativeLocalForwardStarter {
    fn new(opener: RusshChannelOpener) -> Self {
        Self { opener }
    }
}

impl NativeLocalForwardStarter for ThreadedNativeLocalForwardStarter {
    fn start(
        &mut self,
        request: SshConnectRequest,
        forward: NativeLocalForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        let listener = TcpListener::bind((forward.bind_host.as_str(), forward.bind_port))?;
        let opener = self.opener.clone();
        let join_handle = thread::spawn(move || {
            run_native_local_forward_listener(&listener, &opener, &request, &forward)
                .map_err(|error| error.to_string())
        });

        Ok(Box::new(ThreadedNativeLocalForwardHandle {
            join_handle: Some(join_handle),
        }))
    }

    fn start_dynamic(
        &mut self,
        request: SshConnectRequest,
        forward: NativeDynamicForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        let listener = TcpListener::bind((forward.bind_host.as_str(), forward.bind_port))?;
        let opener = self.opener.clone();
        let join_handle = thread::spawn(move || {
            run_native_dynamic_forward_listener(&listener, &opener, &request)
                .map_err(|error| error.to_string())
        });

        Ok(Box::new(ThreadedNativeLocalForwardHandle {
            join_handle: Some(join_handle),
        }))
    }

    fn start_remote(
        &mut self,
        request: SshConnectRequest,
        forward: NativeRemoteForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        let mut opener = self.opener.clone();
        let remote_forward_plan = native_remote_tcpip_plan_for_remote_forward(&forward);
        let join_handle = thread::spawn(move || {
            opener
                .start_remote_tcpip_forward(&request, &remote_forward_plan)
                .map_err(|error| error.to_string())
        });

        Ok(Box::new(ThreadedNativeLocalForwardHandle {
            join_handle: Some(join_handle),
        }))
    }
}

struct ThreadedNativeLocalForwardHandle {
    join_handle: Option<thread::JoinHandle<Result<(), String>>>,
}

impl NativeLocalForwardHandle for ThreadedNativeLocalForwardHandle {
    fn wait(&mut self) -> Result<(), Box<dyn Error>> {
        let Some(join_handle) = self.join_handle.take() else {
            return Ok(());
        };

        let result = join_handle
            .join()
            .map_err(|_| "native SSH local forwarding listener panicked")?;
        result.map_err(Into::into)
    }
}

pub fn run(options: &SshOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    if options.native {
        return run_native(options);
    }

    let local_options = local_options_for_options(options)?;

    local::run(&local_options)
}

fn run_native(options: &SshOptions) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut connector = SshChannelConnector::new(native_channel_opener_for_options(options));
    let mut forward_starter =
        ThreadedNativeLocalForwardStarter::new(native_channel_opener_for_options(options));
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();

    run_native_with_connector_prompt_and_io(
        options,
        &mut connector,
        &mut forward_starter,
        &mut |request| native_password_prompt(request),
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
    native_request_for_options_with_password_prompt(options, &mut |_| {
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

#[cfg(test)]
fn native_request_for_options_with_secret_prompts(
    options: &SshOptions,
    password_prompt: &mut SecretPrompt<'_>,
    key_passphrase_prompt: &mut KeyPassphrasePrompt<'_>,
    key_needs_passphrase: &mut KeyPassphraseDetector<'_>,
) -> Result<SshConnectRequest, Box<dyn Error>> {
    native_request_for_options_with_resolver_secret_prompts(
        options,
        &mut resolve_openssh_config_target,
        password_prompt,
        key_passphrase_prompt,
        key_needs_passphrase,
    )
}

fn native_request_for_options_with_resolver_secret_prompts(
    options: &SshOptions,
    openssh_config_resolver: &mut OpenSshConfigResolver<'_>,
    password_prompt: &mut SecretPrompt<'_>,
    key_passphrase_prompt: &mut KeyPassphrasePrompt<'_>,
    key_needs_passphrase: &mut KeyPassphraseDetector<'_>,
) -> Result<SshConnectRequest, Box<dyn Error>> {
    let request = match &options.target {
        SshTarget::Direct(request) => request.clone(),
        SshTarget::OpenSsh(target) => {
            let config_output = openssh_config_resolver(target)?;
            native_request_for_openssh_target_with_config_output(target, &config_output)?
        }
    };

    native_forward_plan_for_options(options)?;

    let startup = if options.no_shell {
        SshSessionStartup::NoShell
    } else if options.remote_command.is_empty() {
        SshSessionStartup::Shell
    } else {
        SshSessionStartup::command(options.remote_command.clone())?
    };

    let mut request = request.with_startup(startup);
    if matches!(request.auth, SshAuthMethod::PasswordPrompt) {
        request.auth = SshAuthMethod::password(password_prompt(&request)?)?;
    }
    if let SshAuthMethod::PrivateKey { path, passphrase } = &mut request.auth
        && passphrase.is_none()
        && key_needs_passphrase(path)?
    {
        *passphrase = Some(key_passphrase_prompt(path)?);
    }

    Ok(request)
}

fn native_request_for_openssh_target_with_config_output(
    target: &OpenSshTarget,
    config_output: &str,
) -> Result<SshConnectRequest, Box<dyn Error>> {
    let mut resolved_host = None;
    let mut resolved_user = None;
    let mut resolved_port = None;

    for line in config_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        match key.to_ascii_lowercase().as_str() {
            "hostname" => resolved_host = Some(value.to_owned()),
            "user" => resolved_user = Some(value.to_owned()),
            "port" => resolved_port = Some(value.parse::<u16>()?),
            _ => {}
        }
    }

    let host = resolved_host.unwrap_or_else(|| target.target.clone());
    let username = target
        .username
        .clone()
        .or(resolved_user)
        .ok_or("OpenSSH target did not resolve a user")?;
    let port = target.port.or(resolved_port).unwrap_or(22);
    let config = SshSessionConfig::try_new(host, port, username, target.initial_size)?;

    Ok(SshConnectRequest::new(config, target.auth.clone()))
}

fn resolve_openssh_config_target(target: &OpenSshTarget) -> Result<String, Box<dyn Error>> {
    let output = Command::new("ssh").arg("-G").arg(&target.target).output()?;
    if !output.status.success() {
        return Err(format!(
            "OpenSSH config resolution failed for target {}",
            target.target
        )
        .into());
    }

    String::from_utf8(output.stdout).map_err(Into::into)
}

fn native_forward_plan_for_options(
    options: &SshOptions,
) -> Result<NativeForwardPlan, Box<dyn Error>> {
    let mut plan = NativeForwardPlan::default();

    for forward in &options.forwards {
        match forward {
            SshForward::Local(spec) => plan.local.push(parse_native_local_forward(spec)?),
            SshForward::Dynamic(spec) => {
                plan.dynamic.push(parse_native_dynamic_forward(spec)?);
            }
            SshForward::Remote(spec) => {
                plan.remote.push(parse_native_remote_forward(spec)?);
            }
        }
    }

    Ok(plan)
}

fn parse_native_local_forward(spec: &str) -> Result<NativeLocalForward, Box<dyn Error>> {
    let parts = spec.split(':').collect::<Vec<_>>();
    let (bind_host, bind_port, target_host, target_port) = match parts.as_slice() {
        [bind_port, target_host, target_port] => {
            ("127.0.0.1", *bind_port, *target_host, *target_port)
        }
        [bind_host, bind_port, target_host, target_port] => {
            (*bind_host, *bind_port, *target_host, *target_port)
        }
        _ => {
            return Err(format!(
                "invalid native local-forward spec {spec:?}; expected [bind_host:]bind_port:target_host:target_port"
            )
            .into());
        }
    };

    if bind_host.trim().is_empty() || target_host.trim().is_empty() {
        return Err(
            format!("invalid native local-forward spec {spec:?}; host cannot be empty").into(),
        );
    }

    Ok(NativeLocalForward {
        bind_host: bind_host.to_owned(),
        bind_port: parse_forward_port(bind_port, "bind port")?,
        target_host: target_host.to_owned(),
        target_port: parse_forward_port(target_port, "target port")?,
    })
}

fn parse_native_dynamic_forward(spec: &str) -> Result<NativeDynamicForward, Box<dyn Error>> {
    let parts = spec.split(':').collect::<Vec<_>>();
    let (bind_host, bind_port) = match parts.as_slice() {
        [bind_port] => ("127.0.0.1", *bind_port),
        [bind_host, bind_port] => (*bind_host, *bind_port),
        _ => {
            return Err(format!(
                "invalid native dynamic-forward spec {spec:?}; expected [bind_host:]bind_port"
            )
            .into());
        }
    };

    if bind_host.trim().is_empty() {
        return Err(
            format!("invalid native dynamic-forward spec {spec:?}; host cannot be empty").into(),
        );
    }

    Ok(NativeDynamicForward {
        bind_host: bind_host.to_owned(),
        bind_port: parse_forward_port(bind_port, "bind port")?,
    })
}

fn parse_native_remote_forward(spec: &str) -> Result<NativeRemoteForward, Box<dyn Error>> {
    let parts = spec.split(':').collect::<Vec<_>>();
    let (bind_host, bind_port, target_host, target_port) = match parts.as_slice() {
        [bind_port, target_host, target_port] => {
            ("127.0.0.1", *bind_port, *target_host, *target_port)
        }
        [bind_host, bind_port, target_host, target_port] => {
            (*bind_host, *bind_port, *target_host, *target_port)
        }
        _ => {
            return Err(format!(
                "invalid native remote-forward spec {spec:?}; expected [bind_host:]bind_port:target_host:target_port"
            )
            .into());
        }
    };

    if bind_host.trim().is_empty() || target_host.trim().is_empty() {
        return Err(
            format!("invalid native remote-forward spec {spec:?}; host cannot be empty").into(),
        );
    }

    Ok(NativeRemoteForward {
        bind_host: bind_host.to_owned(),
        bind_port: parse_forward_port(bind_port, "bind port")?,
        target_host: target_host.to_owned(),
        target_port: parse_forward_port(target_port, "target port")?,
    })
}

fn parse_forward_port(value: &str, name: &str) -> Result<u16, Box<dyn Error>> {
    let port = value
        .parse::<u16>()
        .map_err(|_| format!("invalid native local-forward {name}: {value}"))?;
    if port == 0 {
        return Err(format!("invalid native local-forward {name}: {value}").into());
    }
    Ok(port)
}

fn read_socks5_connect_request(
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<Socks5ConnectRequest, Box<dyn Error>> {
    let mut greeting = [0; 2];
    input.read_exact(&mut greeting)?;
    if greeting[0] != 0x05 {
        return Err("SOCKS5 greeting must start with version 5".into());
    }

    let method_count = usize::from(greeting[1]);
    let mut methods = vec![0; method_count];
    input.read_exact(&mut methods)?;
    if !methods.contains(&0x00) {
        output.write_all(&[0x05, 0xff])?;
        output.flush()?;
        return Err("SOCKS5 client did not offer no-auth authentication".into());
    }
    output.write_all(&[0x05, 0x00])?;
    output.flush()?;

    let mut header = [0; 4];
    input.read_exact(&mut header)?;
    if header[0] != 0x05 || header[1] != 0x01 || header[2] != 0x00 {
        return Err("SOCKS5 request must be a CONNECT request".into());
    }

    let target_host = match header[3] {
        0x01 => {
            let mut octets = [0; 4];
            input.read_exact(&mut octets)?;
            Ipv4Addr::from(octets).to_string()
        }
        0x03 => {
            let mut length = [0; 1];
            input.read_exact(&mut length)?;
            let mut host = vec![0; usize::from(length[0])];
            input.read_exact(&mut host)?;
            String::from_utf8(host)?
        }
        0x04 => {
            let mut octets = [0; 16];
            input.read_exact(&mut octets)?;
            Ipv6Addr::from(octets).to_string()
        }
        _ => return Err("SOCKS5 address type is not supported".into()),
    };

    let mut port = [0; 2];
    input.read_exact(&mut port)?;
    let target_port = u16::from_be_bytes(port);
    if target_host.trim().is_empty() || target_port == 0 {
        return Err("SOCKS5 CONNECT target cannot be empty".into());
    }

    Ok(Socks5ConnectRequest {
        target_host,
        target_port,
    })
}

fn native_direct_tcpip_plan_for_local_forward(
    forward: &NativeLocalForward,
    originator_host: impl Into<String>,
    originator_port: u16,
) -> RusshDirectTcpIpOpenPlan {
    RusshDirectTcpIpOpenPlan::new(
        forward.target_host.clone(),
        forward.target_port,
        originator_host,
        originator_port,
    )
}

fn native_remote_tcpip_plan_for_remote_forward(
    forward: &NativeRemoteForward,
) -> RusshRemoteTcpIpForwardPlan {
    RusshRemoteTcpIpForwardPlan::new(
        forward.bind_host.clone(),
        forward.bind_port,
        forward.target_host.clone(),
        forward.target_port,
    )
}

fn run_native_local_forward_listener(
    listener: &TcpListener,
    opener: &RusshChannelOpener,
    request: &SshConnectRequest,
    forward: &NativeLocalForward,
) -> Result<(), Box<dyn Error>> {
    for stream in listener.incoming() {
        let stream = stream?;
        let mut opener = opener.clone();
        let request = request.clone();
        let forward = forward.clone();
        thread::spawn(move || {
            let _ = run_native_local_forward_connection(stream, &mut opener, request, &forward);
        });
    }

    Ok(())
}

fn run_native_local_forward_connection(
    local_stream: TcpStream,
    opener: &mut RusshChannelOpener,
    request: SshConnectRequest,
    forward: &NativeLocalForward,
) -> Result<(), Box<dyn Error>> {
    let peer_addr = local_stream.peer_addr()?;
    let direct_tcpip_plan = native_direct_tcpip_plan_for_local_forward(
        forward,
        peer_addr.ip().to_string(),
        peer_addr.port(),
    );
    let channel = opener.open_direct_tcpip_channel(request, &direct_tcpip_plan)?;
    let (mut remote_reader, mut remote_writer) = channel.into_read_writer();
    let mut local_reader = local_stream.try_clone()?;
    let mut local_writer = local_stream;

    let upload = thread::spawn(move || {
        io::copy(&mut local_reader, &mut remote_writer)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    let download = io::copy(&mut remote_reader, &mut local_writer).map(|_| ());
    let upload = upload
        .join()
        .map_err(|_| "native SSH local forwarding upload worker panicked")?;

    download?;
    upload.map_err(Into::into)
}

fn run_native_dynamic_forward_listener(
    listener: &TcpListener,
    opener: &RusshChannelOpener,
    request: &SshConnectRequest,
) -> Result<(), Box<dyn Error>> {
    for stream in listener.incoming() {
        let stream = stream?;
        let mut opener = opener.clone();
        let request = request.clone();
        thread::spawn(move || {
            let _ = run_native_dynamic_forward_connection(stream, &mut opener, request);
        });
    }

    Ok(())
}

fn run_native_dynamic_forward_connection(
    local_stream: TcpStream,
    opener: &mut RusshChannelOpener,
    request: SshConnectRequest,
) -> Result<(), Box<dyn Error>> {
    let peer_addr = local_stream.peer_addr()?;
    let mut socks_input = local_stream.try_clone()?;
    let mut socks_output = local_stream.try_clone()?;
    let socks_request = read_socks5_connect_request(&mut socks_input, &mut socks_output)?;
    let direct_tcpip_plan = RusshDirectTcpIpOpenPlan::new(
        socks_request.target_host,
        socks_request.target_port,
        peer_addr.ip().to_string(),
        peer_addr.port(),
    );
    let channel = match opener.open_direct_tcpip_channel(request, &direct_tcpip_plan) {
        Ok(channel) => channel,
        Err(error) => {
            write_socks5_connect_reply(&mut socks_output, 0x01)?;
            return Err(error.into());
        }
    };
    write_socks5_connect_reply(&mut socks_output, 0x00)?;

    let (mut remote_reader, mut remote_writer) = channel.into_read_writer();
    let mut local_reader = local_stream.try_clone()?;
    let mut local_writer = local_stream;

    let upload = thread::spawn(move || {
        io::copy(&mut local_reader, &mut remote_writer)
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    let download = io::copy(&mut remote_reader, &mut local_writer).map(|_| ());
    let upload = upload
        .join()
        .map_err(|_| "native SSH dynamic forwarding upload worker panicked")?;

    download?;
    upload.map_err(Into::into)
}

fn write_socks5_connect_reply(output: &mut dyn Write, status: u8) -> Result<(), Box<dyn Error>> {
    output.write_all(&[0x05, status, 0x00, 0x01, 0, 0, 0, 0, 0, 0])?;
    output.flush()?;
    Ok(())
}

#[must_use]
fn openssh_command_for_request(request: &SshConnectRequest, options: &SshOptions) -> PtyCommand {
    let mut args = openssh_start_args(options);

    args.extend(options.openssh_args.clone());
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

    args.extend(options.openssh_args.clone());
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
        console: options.console,
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
        openssh_args: Vec::new(),
        no_shell: false,
        native: false,
        native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
        console: crate::cli::ConsoleOptions::default(),
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
    let mut forward_starter = RejectingNativeLocalForwardStarter;
    run_native_with_connector_forward_starter_and_io(
        options,
        connector,
        &mut forward_starter,
        input,
        output,
    )
}

#[cfg(test)]
fn run_native_with_connector_forward_starter_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    forward_starter: &mut dyn NativeLocalForwardStarter,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut prompts = NativeSecretPrompts {
        password_prompt: &mut |_| Err("password prompt should not be used".into()),
        key_passphrase_prompt: &mut |_| {
            Err("native SSH private-key passphrase prompt requires a passphrase provider".into())
        },
        key_needs_passphrase: &mut native_key_needs_passphrase,
    };
    run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
        options,
        connector,
        forward_starter,
        &mut |_| Err("OpenSSH config target resolver is not available in this path".into()),
        &mut prompts,
        input,
        output,
    )
}

#[cfg(test)]
fn run_native_with_connector_forward_starter_resolver_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    forward_starter: &mut dyn NativeLocalForwardStarter,
    openssh_config_resolver: &mut OpenSshConfigResolver<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut prompts = NativeSecretPrompts {
        password_prompt: &mut |_| Err("password prompt should not be used".into()),
        key_passphrase_prompt: &mut |_| {
            Err("native SSH private-key passphrase prompt requires a passphrase provider".into())
        },
        key_needs_passphrase: &mut native_key_needs_passphrase,
    };
    run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
        options,
        connector,
        forward_starter,
        openssh_config_resolver,
        &mut prompts,
        input,
        output,
    )
}

fn run_native_with_connector_prompt_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    forward_starter: &mut dyn NativeLocalForwardStarter,
    password_prompt: &mut SecretPrompt<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut prompts = NativeSecretPrompts {
        password_prompt,
        key_passphrase_prompt: &mut native_key_passphrase_prompt,
        key_needs_passphrase: &mut native_key_needs_passphrase,
    };
    run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
        options,
        connector,
        forward_starter,
        &mut resolve_openssh_config_target,
        &mut prompts,
        input,
        output,
    )
}

#[cfg(test)]
fn run_native_with_connector_prompt_resolver_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    forward_starter: &mut dyn NativeLocalForwardStarter,
    openssh_config_resolver: &mut OpenSshConfigResolver<'_>,
    password_prompt: &mut SecretPrompt<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut prompts = NativeSecretPrompts {
        password_prompt,
        key_passphrase_prompt: &mut native_key_passphrase_prompt,
        key_needs_passphrase: &mut native_key_needs_passphrase,
    };
    run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
        options,
        connector,
        forward_starter,
        openssh_config_resolver,
        &mut prompts,
        input,
        output,
    )
}

#[cfg(test)]
fn run_native_with_connector_secret_prompts_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    password_prompt: &mut SecretPrompt<'_>,
    key_passphrase_prompt: &mut KeyPassphrasePrompt<'_>,
    key_needs_passphrase: &mut KeyPassphraseDetector<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let mut forward_starter = RejectingNativeLocalForwardStarter;
    let mut prompts = NativeSecretPrompts {
        password_prompt,
        key_passphrase_prompt,
        key_needs_passphrase,
    };
    run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
        options,
        connector,
        &mut forward_starter,
        &mut |_| Err("OpenSSH config target resolver is not available in this path".into()),
        &mut prompts,
        input,
        output,
    )
}

fn run_native_with_connector_forward_starter_resolver_secret_prompts_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    forward_starter: &mut dyn NativeLocalForwardStarter,
    openssh_config_resolver: &mut OpenSshConfigResolver<'_>,
    prompts: &mut NativeSecretPrompts<'_>,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    let metrics_started_at = Instant::now();
    let mut lifecycle = SessionLifecycle::new(NATIVE_SSH_SESSION_ID);
    lifecycle.start_connecting()?;
    let forward_plan = native_forward_plan_for_options(options)?;
    let request = native_request_for_options_with_resolver_secret_prompts(
        options,
        openssh_config_resolver,
        &mut *prompts.password_prompt,
        &mut *prompts.key_passphrase_prompt,
        &mut *prompts.key_needs_passphrase,
    )?;

    let mut forward_handles = Vec::new();
    for forward in forward_plan.local {
        forward_handles.push(forward_starter.start(request.clone(), forward)?);
    }
    for forward in forward_plan.dynamic {
        forward_handles.push(forward_starter.start_dynamic(request.clone(), forward)?);
    }
    for forward in forward_plan.remote {
        forward_handles.push(forward_starter.start_remote(request.clone(), forward)?);
    }

    if options.no_shell && !forward_handles.is_empty() {
        for handle in &mut forward_handles {
            handle.wait()?;
        }
        return finish_native_ssh_success(
            options,
            &request,
            NativeSshIoCounters::default(),
            &mut lifecycle,
            metrics_started_at.elapsed(),
            output,
        );
    }

    let mut counted_input = CountingRead::new(input);
    let io_counters = {
        let mut counted_output = CountingWrite::new(output);
        rssh_ssh::run_shell_with_io(
            connector,
            request.clone(),
            &mut counted_input,
            &mut counted_output,
        )?;
        NativeSshIoCounters {
            ssh_input_bytes: counted_input.byte_count(),
            ssh_output_bytes: counted_output.byte_count(),
        }
    };

    finish_native_ssh_success(
        options,
        &request,
        io_counters,
        &mut lifecycle,
        metrics_started_at.elapsed(),
        output,
    )
}

fn finish_native_ssh_success(
    options: &SshOptions,
    request: &SshConnectRequest,
    io_counters: NativeSshIoCounters,
    lifecycle: &mut SessionLifecycle,
    elapsed: Duration,
    output: &mut dyn Write,
) -> Result<PtyExitStatus, Box<dyn Error>> {
    lifecycle.mark_connected()?;
    lifecycle.mark_disconnected()?;
    lifecycle.close()?;

    let status = PtyExitStatus::from_exit_code(0);
    write_native_ssh_metrics_if_requested(
        options,
        request,
        io_counters,
        lifecycle.state(),
        elapsed,
        &status,
        output,
    )?;

    Ok(status)
}

fn write_native_ssh_metrics_if_requested(
    options: &SshOptions,
    request: &SshConnectRequest,
    io_counters: NativeSshIoCounters,
    session_state: SessionState,
    elapsed: Duration,
    status: &PtyExitStatus,
    output: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    if !options.console.metrics && !options.console.metrics_json {
        return Ok(());
    }

    let snapshot =
        NativeSshMetricsSnapshot::from_status(request, io_counters, elapsed, session_state, status);
    if options.console.metrics_json {
        writeln!(output, "{}", snapshot.json_report()?)?;
    } else {
        write!(output, "{}", snapshot.report())?;
    }
    output.flush()?;

    Ok(())
}

#[derive(Clone, Copy, Default)]
struct NativeSshIoCounters {
    ssh_input_bytes: u64,
    ssh_output_bytes: u64,
}

struct CountingRead<'a> {
    inner: &'a mut dyn Read,
    bytes: u64,
}

impl<'a> CountingRead<'a> {
    fn new(inner: &'a mut dyn Read) -> Self {
        Self { inner, bytes: 0 }
    }

    fn byte_count(&self) -> u64 {
        self.bytes
    }
}

impl Read for CountingRead<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.bytes += count as u64;
        Ok(count)
    }
}

struct CountingWrite<'a> {
    inner: &'a mut dyn Write,
    bytes: u64,
}

impl<'a> CountingWrite<'a> {
    fn new(inner: &'a mut dyn Write) -> Self {
        Self { inner, bytes: 0 }
    }

    fn byte_count(&self) -> u64 {
        self.bytes
    }
}

impl Write for CountingWrite<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buffer)?;
        self.bytes += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Serialize)]
struct NativeSshMetricsSnapshot {
    backend: String,
    host: String,
    username: String,
    port: u16,
    columns: u16,
    rows: u16,
    session_state: String,
    ssh_input_bytes: u64,
    ssh_output_bytes: u64,
    elapsed_ms: u128,
    exit_code: u32,
    signal: Option<String>,
    success: bool,
}

impl NativeSshMetricsSnapshot {
    fn from_status(
        request: &SshConnectRequest,
        io_counters: NativeSshIoCounters,
        elapsed: Duration,
        session_state: SessionState,
        status: &PtyExitStatus,
    ) -> Self {
        Self {
            backend: "NativeRussh".to_owned(),
            host: request.config.host.clone(),
            username: request.config.username.clone(),
            port: request.config.port,
            columns: request.config.initial_size.columns,
            rows: request.config.initial_size.rows,
            session_state: session_state.as_str().to_owned(),
            ssh_input_bytes: io_counters.ssh_input_bytes,
            ssh_output_bytes: io_counters.ssh_output_bytes,
            elapsed_ms: elapsed.as_millis(),
            exit_code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
            success: status.success(),
        }
    }

    fn report(&self) -> String {
        format!(
            "\
R-SSH native SSH metrics
backend={}
host={}
username={}
port={}
columns={}
rows={}
session_state={}
ssh_input_bytes={}
ssh_output_bytes={}
elapsed_ms={}
exit_code={}
signal={}
success={}
",
            self.backend,
            self.host,
            self.username,
            self.port,
            self.columns,
            self.rows,
            self.session_state,
            self.ssh_input_bytes,
            self.ssh_output_bytes,
            self.elapsed_ms,
            self.exit_code,
            self.signal.as_deref().unwrap_or("none"),
            self.success
        )
    }

    fn json_report(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
struct RejectingNativeLocalForwardStarter;

#[cfg(test)]
impl NativeLocalForwardStarter for RejectingNativeLocalForwardStarter {
    fn start(
        &mut self,
        _request: SshConnectRequest,
        _forward: NativeLocalForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        Err("native SSH local forwarding starter is not available in this path".into())
    }

    fn start_dynamic(
        &mut self,
        _request: SshConnectRequest,
        _forward: NativeDynamicForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        Err("native SSH dynamic forwarding starter is not available in this path".into())
    }

    fn start_remote(
        &mut self,
        _request: SshConnectRequest,
        _forward: NativeRemoteForward,
    ) -> Result<Box<dyn NativeLocalForwardHandle>, Box<dyn Error>> {
        Err("native SSH remote forwarding starter is not available in this path".into())
    }
}

fn native_password_prompt(request: &SshConnectRequest) -> Result<String, Box<dyn Error>> {
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
                openssh_args: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
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
                openssh_args: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
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
                openssh_args: Vec::new(),
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
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
                openssh_args: Vec::new(),
                no_shell: true,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
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
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
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
    fn native_ssh_runner_prints_json_metrics_when_requested() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(100, 30))
                .unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut input = &b"whoami\n"[..];
        let mut output = Vec::new();

        let status = super::run_native_with_connector_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions {
                    metrics_json: true,
                    ..crate::cli::ConsoleOptions::default()
                },
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut input,
            &mut output,
        )
        .unwrap();

        assert!(status.success());
        assert_eq!(state.lock().unwrap().written, b"whoami\n");
        let output = String::from_utf8(output).unwrap();
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some("remote"));
        let metrics: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();

        assert_eq!(metrics["backend"], "NativeRussh");
        assert_eq!(metrics["host"], "example.com");
        assert_eq!(metrics["username"], "ops");
        assert_eq!(metrics["port"], 22);
        assert_eq!(metrics["columns"], 100);
        assert_eq!(metrics["rows"], 30);
        assert_eq!(metrics["session_state"], "closed");
        assert_eq!(metrics["ssh_input_bytes"], 7);
        assert_eq!(metrics["ssh_output_bytes"], 7);
        assert_eq!(metrics["exit_code"], 0);
        assert_eq!(metrics["signal"], serde_json::Value::Null);
        assert_eq!(metrics["success"], true);
    }

    #[test]
    fn native_ssh_runner_starts_local_forward_before_shell() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let forward_state = Arc::new(Mutex::new(MockForwardState::default()));
        let mut forward_starter = MockForwardStarter {
            state: Arc::clone(&forward_state),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_forward_starter_and_io(
            &SshOptions {
                target: SshTarget::Direct(request.clone()),
                remote_command: Vec::new(),
                forwards: vec![crate::cli::SshForward::Local(
                    "127.0.0.1:15432:db.internal:5432".to_owned(),
                )],
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        assert_eq!(state.lock().unwrap().last_request.as_ref(), Some(&request));
        assert_eq!(
            forward_state.lock().unwrap().local,
            [super::NativeLocalForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 15432,
                target_host: "db.internal".to_owned(),
                target_port: 5432,
            }]
        );
    }

    #[test]
    fn native_ssh_runner_starts_dynamic_forward_before_shell() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let forward_state = Arc::new(Mutex::new(MockForwardState::default()));
        let mut forward_starter = MockForwardStarter {
            state: Arc::clone(&forward_state),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_forward_starter_and_io(
            &SshOptions {
                target: SshTarget::Direct(request.clone()),
                remote_command: Vec::new(),
                forwards: vec![crate::cli::SshForward::Dynamic("127.0.0.1:1080".to_owned())],
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        assert_eq!(state.lock().unwrap().last_request.as_ref(), Some(&request));
        assert_eq!(
            forward_state.lock().unwrap().dynamic,
            [super::NativeDynamicForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 1080,
            }]
        );
    }

    #[test]
    fn native_ssh_runner_keeps_no_shell_remote_forward_open_without_shell() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let forward_state = Arc::new(Mutex::new(MockForwardState::default()));
        let mut forward_starter = MockForwardStarter {
            state: Arc::clone(&forward_state),
        };
        let mut output = Vec::new();

        let status = super::run_native_with_connector_forward_starter_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: vec![crate::cli::SshForward::Remote(
                    "8080:127.0.0.1:80".to_owned(),
                )],
                openssh_args: Vec::new(),
                no_shell: true,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        assert!(status.success());
        assert!(state.lock().unwrap().last_request.is_none());
        assert_eq!(
            forward_state.lock().unwrap().remote,
            [super::NativeRemoteForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 8080,
                target_host: "127.0.0.1".to_owned(),
                target_port: 80,
            }]
        );
    }

    #[test]
    fn native_ssh_runner_resolves_openssh_target_before_connecting() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let forward_state = Arc::new(Mutex::new(MockForwardState::default()));
        let mut forward_starter = MockForwardStarter {
            state: Arc::clone(&forward_state),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_forward_starter_resolver_and_io(
            &SshOptions {
                target: SshTarget::OpenSsh(OpenSshTarget {
                    target: "prod".to_owned(),
                    username: Some("override".to_owned()),
                    port: Some(2200),
                    initial_size: TerminalSize::new(100, 40),
                    auth: rssh_ssh::SshAuthMethod::Agent,
                }),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut |target| {
                assert_eq!(target.target, "prod");
                Ok("hostname ssh.example.com\nuser deploy\nport 2222\n".to_owned())
            },
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        let request = state.last_request.as_ref().unwrap();
        assert_eq!(request.config.host, "ssh.example.com");
        assert_eq!(request.config.username, "override");
        assert_eq!(request.config.port, 2200);
        assert_eq!(request.config.initial_size, TerminalSize::new(100, 40));
    }

    #[test]
    fn native_local_forward_plan_parses_bind_and_target_endpoint() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let plan = super::native_forward_plan_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: vec![crate::cli::SshForward::Local(
                "127.0.0.1:15432:db.internal:5432".to_owned(),
            )],
            openssh_args: Vec::new(),
            no_shell: true,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
            osc52_policy: Osc52Policy::default(),
            log: None,
        })
        .unwrap();

        assert_eq!(
            plan.local,
            [super::NativeLocalForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 15432,
                target_host: "db.internal".to_owned(),
                target_port: 5432,
            }]
        );
    }

    #[test]
    fn native_local_forward_builds_direct_tcpip_plan_from_target_and_originator() {
        let forward = super::NativeLocalForward {
            bind_host: "127.0.0.1".to_owned(),
            bind_port: 15432,
            target_host: "db.internal".to_owned(),
            target_port: 5432,
        };

        let plan = super::native_direct_tcpip_plan_for_local_forward(&forward, "127.0.0.1", 61234);

        assert_eq!(plan.target(), ("db.internal", 5432));
        assert_eq!(plan.originator(), ("127.0.0.1", 61234));
    }

    #[test]
    fn native_forward_plan_accepts_remote_forwards() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let plan = super::native_forward_plan_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: vec![crate::cli::SshForward::Remote(
                "8080:127.0.0.1:80".to_owned(),
            )],
            openssh_args: Vec::new(),
            no_shell: true,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
            osc52_policy: Osc52Policy::default(),
            log: None,
        })
        .unwrap();

        assert_eq!(
            plan.remote,
            [super::NativeRemoteForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 8080,
                target_host: "127.0.0.1".to_owned(),
                target_port: 80,
            }]
        );
    }

    #[test]
    fn native_forward_plan_parses_dynamic_bind_endpoint() {
        let request = SshConnectRequest::agent(
            SshSessionConfig::try_new("example.com", 22, "ops", TerminalSize::new(80, 24)).unwrap(),
        );

        let plan = super::native_forward_plan_for_options(&SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: vec![crate::cli::SshForward::Dynamic("127.0.0.1:1080".to_owned())],
            openssh_args: Vec::new(),
            no_shell: true,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
            osc52_policy: Osc52Policy::default(),
            log: None,
        })
        .unwrap();

        assert_eq!(
            plan.dynamic,
            [super::NativeDynamicForward {
                bind_host: "127.0.0.1".to_owned(),
                bind_port: 1080,
            }]
        );
    }

    #[test]
    fn native_openssh_target_request_uses_resolved_host_user_and_port() {
        let target = OpenSshTarget {
            target: "prod".to_owned(),
            username: None,
            port: None,
            initial_size: TerminalSize::new(100, 40),
            auth: rssh_ssh::SshAuthMethod::Agent,
        };

        let request = super::native_request_for_openssh_target_with_config_output(
            &target,
            "hostname ssh.example.com\nuser deploy\nport 2222\n",
        )
        .unwrap();

        assert_eq!(request.config.host, "ssh.example.com");
        assert_eq!(request.config.username, "deploy");
        assert_eq!(request.config.port, 2222);
        assert_eq!(request.config.initial_size, TerminalSize::new(100, 40));
        assert_eq!(request.auth, rssh_ssh::SshAuthMethod::Agent);
    }

    #[test]
    fn socks5_connect_request_parses_domain_target_and_selects_no_auth() {
        let mut input = io::Cursor::new([
            0x05, 0x01, 0x00, 0x05, 0x01, 0x00, 0x03, 0x0b, b'e', b'x', b'a', b'm', b'p', b'l',
            b'e', b'.', b'c', b'o', b'm', 0x01, 0xbb,
        ]);
        let mut output = Vec::new();

        let request = super::read_socks5_connect_request(&mut input, &mut output).unwrap();

        assert_eq!(
            request,
            super::Socks5ConnectRequest {
                target_host: "example.com".to_owned(),
                target_port: 443,
            }
        );
        assert_eq!(output, [0x05, 0x00]);
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
        let mut forward_starter = MockForwardStarter {
            state: Arc::new(Mutex::new(MockForwardState::default())),
        };
        let mut output = Vec::new();

        super::run_native_with_connector_prompt_and_io(
            &SshOptions {
                target: SshTarget::Direct(request),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut |request| {
                assert_eq!(request.config.username, "ops");
                assert_eq!(request.config.host, "example.com");
                Ok("secret".to_owned())
            },
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
    fn native_ssh_runner_prompts_for_resolved_openssh_target_password() {
        let state = Arc::new(Mutex::new(MockState::default()));
        let mut connector = MockConnector {
            state: Arc::clone(&state),
        };
        let mut forward_starter = MockForwardStarter {
            state: Arc::new(Mutex::new(MockForwardState::default())),
        };
        let mut output = Vec::new();
        let prompted = Arc::new(Mutex::new(None));
        let prompted_clone = Arc::clone(&prompted);

        super::run_native_with_connector_prompt_resolver_and_io(
            &SshOptions {
                target: SshTarget::OpenSsh(OpenSshTarget {
                    target: "prod".to_owned(),
                    username: None,
                    port: None,
                    initial_size: TerminalSize::new(80, 24),
                    auth: rssh_ssh::SshAuthMethod::PasswordPrompt,
                }),
                remote_command: Vec::new(),
                forwards: Vec::new(),
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut forward_starter,
            &mut |target| {
                assert_eq!(target.target, "prod");
                Ok("hostname ssh.example.com\nuser deploy\nport 2222\n".to_owned())
            },
            &mut |request| {
                *prompted_clone.lock().unwrap() =
                    Some((request.config.username.clone(), request.config.host.clone()));
                Ok("secret".to_owned())
            },
            &mut io::empty(),
            &mut output,
        )
        .unwrap();

        assert_eq!(
            *prompted.lock().unwrap(),
            Some(("deploy".to_owned(), "ssh.example.com".to_owned()))
        );
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
                openssh_args: Vec::new(),
                no_shell: false,
                native: true,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                console: crate::cli::ConsoleOptions::default(),
                osc52_policy: Osc52Policy::default(),
                log: None,
            },
            &mut connector,
            &mut |_| Err("password prompt should not be used".into()),
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::AcceptUnknown,
            console: crate::cli::ConsoleOptions::default(),
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::TrustOnFirstUse,
            console: crate::cli::ConsoleOptions::default(),
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: true,
            native_host_key_policy: NativeHostKeyPolicy::TrustOnFirstUse,
            console: crate::cli::ConsoleOptions::default(),
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
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
    fn openssh_command_preserves_passthrough_options_before_target() {
        let options = SshOptions {
            target: SshTarget::OpenSsh(OpenSshTarget {
                target: "prod".to_owned(),
                username: None,
                port: None,
                initial_size: TerminalSize::new(80, 24),
                auth: rssh_ssh::SshAuthMethod::Agent,
            }),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            openssh_args: vec![
                "-F".to_owned(),
                "C:/Users/ops/.ssh/prod_config".to_owned(),
                "-o".to_owned(),
                "ProxyJump=bastion".to_owned(),
            ],
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
            osc52_policy: Osc52Policy::default(),
            log: None,
        };

        let command = super::openssh_command_for_options(&options);

        assert_eq!(
            command.args(),
            [
                "-tt",
                "-F",
                "C:/Users/ops/.ssh/prod_config",
                "-o",
                "ProxyJump=bastion",
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
            openssh_args: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
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
            openssh_args: Vec::new(),
            no_shell: true,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
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

    #[derive(Default)]
    struct MockForwardState {
        local: Vec<super::NativeLocalForward>,
        dynamic: Vec<super::NativeDynamicForward>,
        remote: Vec<super::NativeRemoteForward>,
    }

    struct MockForwardStarter {
        state: Arc<Mutex<MockForwardState>>,
    }

    impl super::NativeLocalForwardStarter for MockForwardStarter {
        fn start(
            &mut self,
            _request: SshConnectRequest,
            forward: super::NativeLocalForward,
        ) -> Result<Box<dyn super::NativeLocalForwardHandle>, Box<dyn std::error::Error>> {
            self.state.lock().unwrap().local.push(forward);
            Ok(Box::new(MockForwardHandle))
        }

        fn start_dynamic(
            &mut self,
            _request: SshConnectRequest,
            forward: super::NativeDynamicForward,
        ) -> Result<Box<dyn super::NativeLocalForwardHandle>, Box<dyn std::error::Error>> {
            self.state.lock().unwrap().dynamic.push(forward);
            Ok(Box::new(MockForwardHandle))
        }

        fn start_remote(
            &mut self,
            _request: SshConnectRequest,
            forward: super::NativeRemoteForward,
        ) -> Result<Box<dyn super::NativeLocalForwardHandle>, Box<dyn std::error::Error>> {
            self.state.lock().unwrap().remote.push(forward);
            Ok(Box::new(MockForwardHandle))
        }
    }

    struct MockForwardHandle;

    impl super::NativeLocalForwardHandle for MockForwardHandle {
        fn wait(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
    }

    fn direct_options(request: SshConnectRequest) -> SshOptions {
        SshOptions {
            target: SshTarget::Direct(request),
            remote_command: Vec::new(),
            forwards: Vec::new(),
            openssh_args: Vec::new(),
            no_shell: false,
            native: false,
            native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
            console: crate::cli::ConsoleOptions::default(),
            osc52_policy: Osc52Policy::default(),
            log: None,
        }
    }
}
