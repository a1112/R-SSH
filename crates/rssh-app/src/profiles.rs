use std::{collections::BTreeMap, error::Error, fs, io};

use serde::{Deserialize, Serialize};

use crate::cli::{
    self, AppCommand, ProfileCheckOptions, ProfileInitOptions, ProfileListOptions, ProfileOptions,
    ProfileShowOptions,
};

const PROFILE_TEMPLATE: &str = include_str!("../../../examples/rssh-profiles.toml");

#[derive(Deserialize)]
struct ProfileDocument {
    profiles: BTreeMap<String, ProfileDefinition>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileDefinition {
    kind: String,
    host: Option<String>,
    target: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    cols: Option<u16>,
    rows: Option<u16>,
    frames: Option<u64>,
    mouse: Option<bool>,
    metrics: Option<bool>,
    osc52: Option<String>,
    log: Option<String>,
    command: Option<Vec<String>>,
    auth: Option<String>,
    key: Option<String>,
    remote_command: Option<Vec<String>>,
    local_forward: Option<Vec<String>>,
    remote_forward: Option<Vec<String>>,
    dynamic_forward: Option<Vec<String>>,
    no_shell: Option<bool>,
    recursive: Option<bool>,
    upload: Option<Vec<String>>,
    download: Option<Vec<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProfileSummary {
    pub name: String,
    pub kind: String,
}

#[derive(Serialize)]
struct ProfileListJsonEntry {
    name: String,
    kind: String,
    command: String,
    argv: Vec<String>,
}

pub fn load_command(options: &ProfileOptions) -> Result<AppCommand, Box<dyn Error>> {
    let contents = fs::read_to_string(&options.file)?;
    command_from_toml(&options.name, &contents).map_err(profile_error)
}

pub fn profile_command_line(options: &ProfileShowOptions) -> Result<String, Box<dyn Error>> {
    let contents = fs::read_to_string(&options.file)?;
    let args = args_from_toml(&options.name, &contents).map_err(profile_error)?;
    Ok(command_line_from_args(&args))
}

pub fn print_profile_show(options: &ProfileShowOptions) -> Result<(), Box<dyn Error>> {
    println!("{}", profile_command_line(options)?);

    Ok(())
}

pub fn list_profiles(options: &ProfileListOptions) -> Result<Vec<ProfileSummary>, Box<dyn Error>> {
    let contents = fs::read_to_string(&options.file)?;
    summaries_from_toml(&contents).map_err(profile_error)
}

pub fn check_profiles(options: &ProfileCheckOptions) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(&options.file)?;
    validate_profiles_from_toml(&contents).map_err(profile_error)
}

pub fn print_profile_check(options: &ProfileCheckOptions) -> Result<(), Box<dyn Error>> {
    check_profiles(options)?;
    println!("profile check ok");

    Ok(())
}

pub fn init_profile_file(options: &ProfileInitOptions) -> Result<(), Box<dyn Error>> {
    if options.file.exists() && !options.force {
        return Err(profile_error(format!(
            "profile file already exists: {}; use --force to overwrite",
            options.file.display()
        )));
    }

    fs::write(&options.file, PROFILE_TEMPLATE)?;
    Ok(())
}

pub fn print_profile_init(options: &ProfileInitOptions) -> Result<(), Box<dyn Error>> {
    init_profile_file(options)?;
    println!("profile file initialized: {}", options.file.display());

    Ok(())
}

pub fn print_profile_list(options: &ProfileListOptions) -> Result<(), Box<dyn Error>> {
    if options.json {
        println!("{}", profile_list_json(options)?);
        return Ok(());
    }

    for line in profile_list_lines(options)? {
        println!("{line}");
    }

    Ok(())
}

pub fn profile_list_json(options: &ProfileListOptions) -> Result<String, Box<dyn Error>> {
    let contents = fs::read_to_string(&options.file)?;
    let profiles = summaries_from_toml(&contents).map_err(profile_error)?;
    let entries = profiles
        .into_iter()
        .map(|profile| {
            let argv = args_from_toml(&profile.name, &contents).map_err(profile_error)?;
            Ok(ProfileListJsonEntry {
                name: profile.name,
                kind: profile.kind,
                command: command_line_from_args(&argv),
                argv,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;

    Ok(serde_json::to_string(&entries)?)
}

pub fn profile_list_lines(options: &ProfileListOptions) -> Result<Vec<String>, Box<dyn Error>> {
    if !options.verbose {
        return Ok(list_profiles(options)?
            .into_iter()
            .map(|profile| format!("{}\t{}", profile.name, profile.kind))
            .collect());
    }

    let contents = fs::read_to_string(&options.file)?;
    let profiles = summaries_from_toml(&contents).map_err(profile_error)?;

    profiles
        .into_iter()
        .map(|profile| {
            let args = args_from_toml(&profile.name, &contents).map_err(profile_error)?;
            Ok(format!(
                "{}\t{}\t{}",
                profile.name,
                profile.kind,
                command_line_from_args(&args)
            ))
        })
        .collect()
}

fn command_from_toml(name: &str, contents: &str) -> Result<AppCommand, String> {
    cli::parse_args(args_from_toml(name, contents)?)
}

fn args_from_toml(name: &str, contents: &str) -> Result<Vec<String>, String> {
    let document =
        toml::from_str::<ProfileDocument>(contents).map_err(|error| error.to_string())?;
    let profile = document
        .profiles
        .get(name)
        .ok_or_else(|| format!("profile not found: {name}"))?;

    profile.to_args()
}

fn command_line_from_args(args: &[String]) -> String {
    args.iter()
        .map(|argument| quote_command_argument(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_command_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|character| character.is_whitespace() || matches!(character, '"' | '\\'))
    {
        return argument.to_owned();
    }

    let escaped = argument.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn summaries_from_toml(contents: &str) -> Result<Vec<ProfileSummary>, String> {
    let document =
        toml::from_str::<ProfileDocument>(contents).map_err(|error| error.to_string())?;

    Ok(document
        .profiles
        .into_iter()
        .map(|(name, profile)| ProfileSummary {
            name,
            kind: profile.kind,
        })
        .collect())
}

fn validate_profiles_from_toml(contents: &str) -> Result<(), String> {
    let document =
        toml::from_str::<ProfileDocument>(contents).map_err(|error| error.to_string())?;
    let errors = document
        .profiles
        .iter()
        .filter_map(|(name, profile)| {
            profile
                .to_command()
                .err()
                .map(|error| format!("{name} ({}): {error}", profile.kind))
        })
        .collect::<Vec<_>>();

    if errors.is_empty() {
        return Ok(());
    }

    Err(format!("profile check failed: {}", errors.join("; ")))
}

impl ProfileDefinition {
    fn to_command(&self) -> Result<AppCommand, String> {
        cli::parse_args(self.to_args()?)
    }

    fn to_args(&self) -> Result<Vec<String>, String> {
        let mut args = vec!["rssh-app".to_owned()];
        match self.kind.as_str() {
            "local" => self.append_local_args(&mut args)?,
            "scp" => self.append_scp_args(&mut args)?,
            "sftp" => self.append_sftp_args(&mut args)?,
            "ssh" => self.append_ssh_args(&mut args)?,
            "window" => self.append_window_args(&mut args)?,
            value => return Err(format!("invalid profile kind: {value}")),
        }

        Ok(args)
    }

    fn append_local_args(&self, args: &mut Vec<String>) -> Result<(), String> {
        args.push("local".to_owned());
        append_dimensions(args, self.cols, self.rows);
        if self.mouse.unwrap_or(false) {
            args.push("--mouse".to_owned());
        }
        append_optional(args, "--osc52", self.osc52.as_ref());
        append_optional(args, "--log", self.log.as_ref());
        append_command(args, self.command.as_ref(), "local command")?;
        Ok(())
    }

    fn append_ssh_args(&self, args: &mut Vec<String>) -> Result<(), String> {
        args.push("ssh".to_owned());
        append_optional(args, "--host", self.host.as_ref());
        append_optional(args, "--target", self.target.as_ref());
        append_optional(args, "--user", self.user.as_ref());
        append_optional_u16(args, "--port", self.port);
        append_dimensions(args, self.cols, self.rows);
        append_auth_args(args, self.auth.as_deref(), self.key.as_ref())?;
        append_optional(args, "--osc52", self.osc52.as_ref());
        append_optional(args, "--log", self.log.as_ref());
        append_forwards(args, "--local-forward", self.local_forward.as_ref());
        append_forwards(args, "--remote-forward", self.remote_forward.as_ref());
        append_forwards(args, "--dynamic-forward", self.dynamic_forward.as_ref());
        if self.no_shell.unwrap_or(false) {
            args.push("--no-shell".to_owned());
        }
        append_command(args, self.remote_command.as_ref(), "remote command")?;
        Ok(())
    }

    fn append_sftp_args(&self, args: &mut Vec<String>) -> Result<(), String> {
        args.push("sftp".to_owned());
        append_optional(args, "--host", self.host.as_ref());
        append_optional(args, "--target", self.target.as_ref());
        append_optional(args, "--user", self.user.as_ref());
        append_optional_u16(args, "--port", self.port);
        append_dimensions(args, self.cols, self.rows);
        append_auth_args(args, self.auth.as_deref(), self.key.as_ref())?;
        append_optional(args, "--log", self.log.as_ref());
        Ok(())
    }

    fn append_scp_args(&self, args: &mut Vec<String>) -> Result<(), String> {
        args.push("scp".to_owned());
        append_optional(args, "--host", self.host.as_ref());
        append_optional(args, "--target", self.target.as_ref());
        append_optional(args, "--user", self.user.as_ref());
        append_optional_u16(args, "--port", self.port);
        append_dimensions(args, self.cols, self.rows);
        append_auth_args(args, self.auth.as_deref(), self.key.as_ref())?;
        if self.recursive.unwrap_or(false) {
            args.push("--recursive".to_owned());
        }
        append_optional(args, "--log", self.log.as_ref());
        append_transfer(args, "--upload", self.upload.as_ref(), "scp upload")?;
        append_transfer(args, "--download", self.download.as_ref(), "scp download")?;
        Ok(())
    }

    fn append_window_args(&self, args: &mut Vec<String>) -> Result<(), String> {
        args.push("window".to_owned());
        append_optional_u64(args, "--frames", self.frames);
        append_optional(args, "--osc52", self.osc52.as_ref());
        if self.metrics.unwrap_or(false) {
            args.push("--metrics".to_owned());
        }
        append_optional(args, "--log", self.log.as_ref());
        append_command(args, self.command.as_ref(), "window command")?;
        Ok(())
    }
}

fn append_optional(args: &mut Vec<String>, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        args.push(name.to_owned());
        args.push(value.clone());
    }
}

fn append_optional_u16(args: &mut Vec<String>, name: &str, value: Option<u16>) {
    if let Some(value) = value {
        args.push(name.to_owned());
        args.push(value.to_string());
    }
}

fn append_optional_u64(args: &mut Vec<String>, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        args.push(name.to_owned());
        args.push(value.to_string());
    }
}

fn append_dimensions(args: &mut Vec<String>, cols: Option<u16>, rows: Option<u16>) {
    append_optional_u16(args, "--cols", cols);
    append_optional_u16(args, "--rows", rows);
}

fn append_forwards(args: &mut Vec<String>, name: &str, values: Option<&Vec<String>>) {
    if let Some(values) = values {
        for value in values {
            args.push(name.to_owned());
            args.push(value.clone());
        }
    }
}

fn append_transfer(
    args: &mut Vec<String>,
    name: &str,
    transfer: Option<&Vec<String>>,
    label: &str,
) -> Result<(), String> {
    let Some(transfer) = transfer else {
        return Ok(());
    };
    if transfer.len() != 2 {
        return Err(format!("{label} requires exactly two paths"));
    }

    args.push(name.to_owned());
    args.extend(transfer.iter().cloned());
    Ok(())
}

fn append_command(
    args: &mut Vec<String>,
    command: Option<&Vec<String>>,
    label: &str,
) -> Result<(), String> {
    let Some(command) = command else {
        return Ok(());
    };
    if command.is_empty() {
        return Err(format!("{label} cannot be empty"));
    }

    args.push("--".to_owned());
    args.extend(command.iter().cloned());
    Ok(())
}

fn append_auth_args(
    args: &mut Vec<String>,
    auth: Option<&str>,
    key: Option<&String>,
) -> Result<(), String> {
    match (auth, key) {
        (None, None) => Ok(()),
        (None | Some("key"), Some(key)) => {
            args.push("--key".to_owned());
            args.push(key.clone());
            Ok(())
        }
        (Some("agent"), None) => {
            args.push("--agent".to_owned());
            Ok(())
        }
        (Some("password"), None) => {
            args.push("--password".to_owned());
            Ok(())
        }
        (Some("key"), None) => Err("profile ssh auth = \"key\" requires key".to_owned()),
        (Some("agent" | "password"), Some(_)) => {
            Err("profile ssh key cannot be combined with agent or password auth".to_owned())
        }
        (Some(value), _) => Err(format!("invalid profile ssh auth: {value}")),
    }
}

fn profile_error(message: String) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use rssh_core::TerminalSize;
    use rssh_ssh::SshAuthMethod;

    use crate::cli::{
        AppCommand, LocalOptions, NativeHostKeyPolicy, OpenSshTarget, ProfileOptions, SftpOptions,
        SshForward, SshTarget, WindowOptions,
    };

    fn temp_profile_file(name: &str, contents: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("rssh-{name}-{}.toml", std::process::id()));
        fs::write(&path, contents).unwrap();
        path
    }

    fn remove_file(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn loads_ssh_profile_from_toml_file() {
        let file = temp_profile_file(
            "ssh-profile",
            r#"
[profiles.prod]
kind = "ssh"
target = "prod"
user = "ops"
port = 2222
auth = "agent"
cols = 120
rows = 30
osc52 = "off"
remote_command = ["uname", "-a"]
local_forward = ["127.0.0.1:15432:db.internal:5432"]
dynamic_forward = ["127.0.0.1:1080"]
"#,
        );

        let command = super::load_command(&ProfileOptions {
            name: "prod".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command,
            AppCommand::Ssh(crate::cli::SshOptions {
                target: SshTarget::OpenSsh(OpenSshTarget {
                    target: "prod".to_owned(),
                    username: Some("ops".to_owned()),
                    port: Some(2222),
                    initial_size: TerminalSize::new(120, 30),
                    auth: SshAuthMethod::Agent,
                }),
                remote_command: vec!["uname".to_owned(), "-a".to_owned()],
                forwards: vec![
                    SshForward::Local("127.0.0.1:15432:db.internal:5432".to_owned()),
                    SshForward::Dynamic("127.0.0.1:1080".to_owned()),
                ],
                no_shell: false,
                native: false,
                native_host_key_policy: NativeHostKeyPolicy::RejectUnknown,
                osc52_policy: crate::cli::Osc52Policy::Off,
                log: None,
            })
        );
    }

    #[test]
    fn loads_local_profile_from_toml_file() {
        let file = temp_profile_file(
            "local-profile",
            r#"
[profiles.dev]
kind = "local"
cols = 100
rows = 32
mouse = true
osc52 = "write"
log = "dev.log"
command = ["pwsh", "-NoLogo"]
"#,
        );

        let command = super::load_command(&ProfileOptions {
            name: "dev".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command,
            AppCommand::Local(LocalOptions {
                command: rssh_pty::PtyCommand::new("pwsh").with_args(["-NoLogo"]),
                size: Some(rssh_pty::PtySize::try_new(100, 32).unwrap()),
                mouse: true,
                osc52_policy: crate::cli::Osc52Policy::WriteOnly,
                log: Some(PathBuf::from("dev.log")),
            })
        );
    }

    #[test]
    fn loads_sftp_profile_from_toml_file() {
        let file = temp_profile_file(
            "sftp-profile",
            r#"
[profiles.files]
kind = "sftp"
target = "prod"
user = "ops"
port = 2222
auth = "key"
key = "C:/Users/ops/.ssh/id_ed25519"
log = "sftp.log"
"#,
        );

        let command = super::load_command(&ProfileOptions {
            name: "files".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command,
            AppCommand::Sftp(SftpOptions {
                target: SshTarget::OpenSsh(OpenSshTarget {
                    target: "prod".to_owned(),
                    username: Some("ops".to_owned()),
                    port: Some(2222),
                    initial_size: TerminalSize::new(80, 24),
                    auth: SshAuthMethod::PrivateKey {
                        path: "C:/Users/ops/.ssh/id_ed25519".into(),
                        passphrase: None,
                    },
                }),
                log: Some(PathBuf::from("sftp.log")),
            })
        );
    }

    #[test]
    fn loads_scp_upload_profile_from_toml_file() {
        let file = temp_profile_file(
            "scp-profile",
            r#"
[profiles.upload]
kind = "scp"
target = "prod"
auth = "agent"
recursive = true
upload = ["local", "/tmp/remote"]
log = "scp.log"
"#,
        );

        let command = super::load_command(&ProfileOptions {
            name: "upload".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command,
            AppCommand::Scp(crate::cli::ScpOptions {
                target: SshTarget::OpenSsh(OpenSshTarget {
                    target: "prod".to_owned(),
                    username: None,
                    port: None,
                    initial_size: TerminalSize::new(80, 24),
                    auth: SshAuthMethod::Agent,
                }),
                transfer: crate::cli::ScpTransfer::Upload {
                    local: "local".into(),
                    remote: "/tmp/remote".to_owned(),
                },
                recursive: true,
                log: Some(PathBuf::from("scp.log")),
            })
        );
    }

    #[test]
    fn loads_window_profile_from_toml_file() {
        let file = temp_profile_file(
            "window-profile",
            r#"
[profiles.ops-window]
kind = "window"
frames = 120
metrics = true
osc52 = "write"
log = "window.log"
command = ["cmd.exe", "/K", "echo", "window-profile-smoke"]
"#,
        );

        let command = super::load_command(&ProfileOptions {
            name: "ops-window".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command,
            AppCommand::Window(WindowOptions {
                frame_limit: Some(120),
                osc52_policy: crate::cli::Osc52Policy::WriteOnly,
                metrics: true,
                command: rssh_pty::PtyCommand::new("cmd.exe").with_args([
                    "/K",
                    "echo",
                    "window-profile-smoke"
                ]),
                log: Some(PathBuf::from("window.log")),
            })
        );
    }

    #[test]
    fn lists_profile_names_and_kinds_from_toml_file() {
        let file = temp_profile_file(
            "list-profile",
            r#"
[profiles.prod-shell]
kind = "ssh"
target = "prod"
auth = "agent"

[profiles.local-smoke]
kind = "local"
command = ["pwsh", "-NoLogo"]
"#,
        );

        let profiles = super::list_profiles(&crate::cli::ProfileListOptions {
            file: file.clone(),
            verbose: false,
            json: false,
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            profiles,
            vec![
                super::ProfileSummary {
                    name: "local-smoke".to_owned(),
                    kind: "local".to_owned(),
                },
                super::ProfileSummary {
                    name: "prod-shell".to_owned(),
                    kind: "ssh".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn lists_verbose_profile_lines_with_resolved_commands() {
        let file = temp_profile_file(
            "verbose-list-profile",
            r#"
[profiles.prod-shell]
kind = "ssh"
target = "prod"
auth = "agent"

[profiles.local-smoke]
kind = "local"
command = ["pwsh", "-NoLogo"]
"#,
        );

        let lines = super::profile_list_lines(&crate::cli::ProfileListOptions {
            file: file.clone(),
            verbose: true,
            json: false,
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            lines,
            vec![
                "local-smoke\tlocal\trssh-app local -- pwsh -NoLogo".to_owned(),
                "prod-shell\tssh\trssh-app ssh --target prod --agent".to_owned(),
            ]
        );
    }

    #[test]
    fn lists_profiles_as_json_with_resolved_commands() {
        let file = temp_profile_file(
            "json-list-profile",
            r#"
[profiles.prod-shell]
kind = "ssh"
target = "prod"
auth = "agent"

[profiles.local-smoke]
kind = "local"
command = ["pwsh", "-NoLogo"]
"#,
        );

        let json = super::profile_list_json(&crate::cli::ProfileListOptions {
            file: file.clone(),
            verbose: false,
            json: true,
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            json,
            "[{\"name\":\"local-smoke\",\"kind\":\"local\",\"command\":\"rssh-app local -- pwsh -NoLogo\",\"argv\":[\"rssh-app\",\"local\",\"--\",\"pwsh\",\"-NoLogo\"]},{\"name\":\"prod-shell\",\"kind\":\"ssh\",\"command\":\"rssh-app ssh --target prod --agent\",\"argv\":[\"rssh-app\",\"ssh\",\"--target\",\"prod\",\"--agent\"]}]"
        );
    }

    #[test]
    fn checks_all_profiles_and_reports_invalid_entries() {
        let file = temp_profile_file(
            "check-profile",
            r#"
[profiles.good]
kind = "local"
command = ["pwsh", "-NoLogo"]

[profiles.bad]
kind = "ssh"
auth = "agent"
"#,
        );

        let error = super::check_profiles(&crate::cli::ProfileCheckOptions { file: file.clone() })
            .unwrap_err();

        remove_file(&file);

        assert_eq!(
            error.to_string(),
            "profile check failed: bad (ssh): --host or --target is required"
        );
    }

    #[test]
    fn initializes_missing_profile_file_from_template() {
        let mut file = std::env::temp_dir();
        file.push(format!("rssh-init-profile-{}.toml", std::process::id()));
        remove_file(&file);

        super::init_profile_file(&crate::cli::ProfileInitOptions {
            file: file.clone(),
            force: false,
        })
        .unwrap();

        let contents = fs::read_to_string(&file).unwrap();
        remove_file(&file);

        assert!(contents.contains("[profiles.local-smoke]"));
        assert!(contents.contains("[profiles.prod-shell]"));
        assert!(contents.contains("cargo run -p rssh-app -- profile --check"));
    }

    #[test]
    fn refuses_to_overwrite_existing_profile_file_without_force() {
        let file = temp_profile_file("init-existing-profile", "existing");

        let error = super::init_profile_file(&crate::cli::ProfileInitOptions {
            file: file.clone(),
            force: false,
        })
        .unwrap_err();

        let contents = fs::read_to_string(&file).unwrap();
        remove_file(&file);

        assert_eq!(
            error.to_string(),
            format!(
                "profile file already exists: {}; use --force to overwrite",
                file.display()
            )
        );
        assert_eq!(contents, "existing");
    }

    #[test]
    fn shows_profile_as_resolved_command_line_from_toml_file() {
        let file = temp_profile_file(
            "show-profile",
            r#"
[profiles.prod]
kind = "ssh"
target = "prod"
auth = "agent"
log = "prod.log"
"#,
        );

        let command_line = super::profile_command_line(&crate::cli::ProfileShowOptions {
            name: "prod".to_owned(),
            file: file.clone(),
        })
        .unwrap();

        remove_file(&file);

        assert_eq!(
            command_line,
            "rssh-app ssh --target prod --agent --log prod.log"
        );
    }
}
