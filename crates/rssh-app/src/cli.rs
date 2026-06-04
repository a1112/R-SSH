use rssh_pty::{PtyCommand, PtySize};

#[derive(Debug, PartialEq, Eq)]
pub enum AppCommand {
    Local(LocalOptions),
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
        "window" => {
            let window_args = args.collect::<Vec<_>>();
            parse_window(&window_args)
        }
        "-h" | "--help" | "help" => Ok(AppCommand::Help),
        unknown => Err(format!("unknown command: {unknown}")),
    }
}

pub fn help_text() -> &'static str {
    "R-SSH\n\nUsage:\n  rssh-app [window]\n  rssh-app window [--frames N] [--osc52 off|write|read-write] [--metrics]\n  rssh-app local [--cols N] [--rows N] [--mouse] [-- <program> [args...]]\n  rssh-app --help\n"
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

fn parse_dimension(value: Option<&String>, name: &str) -> Result<u16, String> {
    let Some(value) = value else {
        return Err(format!("missing value for {name}"));
    };

    value
        .parse::<u16>()
        .map_err(|_| format!("invalid value for {name}: {value}"))
}

#[cfg(test)]
mod tests {
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
    fn rejects_unknown_command() {
        assert!(parse_args(["rssh-app", "wat"]).is_err());
    }

    #[test]
    fn rejects_partial_local_size() {
        assert!(parse_args(["rssh-app", "local", "--cols", "100"]).is_err());
        assert!(parse_args(["rssh-app", "local", "--rows", "30"]).is_err());
    }
}
