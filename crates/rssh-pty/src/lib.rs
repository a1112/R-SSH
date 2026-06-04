use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, Read, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use portable_pty::{CommandBuilder, MasterPty, PtySize as PortablePtySize, native_pty_system};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyBackend {
    WindowsConpty,
    UnixPty,
}

impl PtyBackend {
    #[must_use]
    pub fn current_platform() -> Self {
        if cfg!(windows) {
            Self::WindowsConpty
        } else {
            Self::UnixPty
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{PtyBackend, PtyCommand, PtySession, PtySize};

    #[test]
    fn selects_a_platform_backend() {
        let backend = PtyBackend::current_platform();

        assert!(matches!(
            backend,
            PtyBackend::WindowsConpty | PtyBackend::UnixPty
        ));
    }

    #[test]
    fn default_shell_uses_current_platform_command() {
        let command = PtyCommand::default_shell();

        assert!(!command.program().is_empty());
    }

    #[test]
    fn command_validation_rejects_empty_program() {
        let command = PtyCommand::new("");

        assert!(command.validate().is_err());
    }

    #[test]
    fn pty_size_rejects_zero_dimensions() {
        assert!(PtySize::try_new(0, 24).is_err());
        assert!(PtySize::try_new(80, 0).is_err());
    }

    #[test]
    fn pty_size_accepts_columns_and_rows() {
        let size = PtySize::try_new(80, 24).unwrap();

        assert_eq!(size.columns(), 80);
        assert_eq!(size.rows(), 24);
    }

    #[test]
    #[ignore = "spawns a real platform PTY"]
    fn local_pty_captures_process_output() {
        let output = PtySession::capture_output(
            PtyCommand::platform_identity_command(),
            PtySize::try_new(80, 24).unwrap(),
            Duration::from_secs(5),
        )
        .unwrap();

        let output = String::from_utf8_lossy(&output);

        assert!(
            !output.trim_matches(char::from(0)).trim().is_empty(),
            "captured PTY output: {output:?}"
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyCommand {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

impl PtyCommand {
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
        }
    }

    #[must_use]
    pub fn default_shell() -> Self {
        if cfg!(windows) {
            Self::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned()))
        } else {
            Self::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned()))
        }
    }

    #[must_use]
    pub fn platform_echo(text: impl Into<String>) -> Self {
        let text = text.into();
        if cfg!(windows) {
            Self::new("cmd.exe").with_args(["/C", "echo", text.as_str()])
        } else {
            Self::new("/bin/sh").with_args(["-lc", format!("printf '%s\\n' {text:?}").as_str()])
        }
    }

    #[must_use]
    pub fn platform_identity_command() -> Self {
        if cfg!(windows) {
            Self::new("whoami.exe")
        } else {
            Self::new("whoami")
        }
    }

    #[must_use]
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn validate(&self) -> Result<(), PtyError> {
        if self.program.trim().is_empty() {
            return Err(PtyError::InvalidCommand(
                "PTY command program cannot be empty".to_owned(),
            ));
        }

        Ok(())
    }

    fn to_builder(&self) -> CommandBuilder {
        let mut builder = CommandBuilder::new(&self.program);
        for arg in &self.args {
            builder.arg(arg);
        }
        if let Some(cwd) = &self.cwd {
            builder.cwd(cwd);
        }
        builder
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    columns: u16,
    rows: u16,
}

impl PtySize {
    pub fn try_new(columns: u16, rows: u16) -> Result<Self, PtyError> {
        if columns == 0 || rows == 0 {
            return Err(PtyError::InvalidSize { columns, rows });
        }

        Ok(Self { columns, rows })
    }

    #[must_use]
    pub const fn columns(self) -> u16 {
        self.columns
    }

    #[must_use]
    pub const fn rows(self) -> u16 {
        self.rows
    }

    const fn to_portable(self) -> PortablePtySize {
        PortablePtySize {
            rows: self.rows,
            cols: self.columns,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
}

impl PtySession {
    pub fn spawn(command: PtyCommand, size: PtySize) -> Result<Self, PtyError> {
        command.validate()?;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.to_portable())
            .map_err(|error| PtyError::Backend(error.to_string()))?;

        let child = pair
            .slave
            .spawn_command(command.to_builder())
            .map_err(|error| PtyError::Backend(error.to_string()))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| PtyError::Backend(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| PtyError::Backend(error.to_string()))?;

        Ok(Self {
            master: pair.master,
            child,
            reader,
            writer,
        })
    }

    pub fn capture_output(
        command: PtyCommand,
        size: PtySize,
        timeout: Duration,
    ) -> Result<Vec<u8>, PtyError> {
        Self::capture_with_input(command, "", size, timeout)
    }

    pub fn capture_shell_output(
        input: &str,
        size: PtySize,
        timeout: Duration,
    ) -> Result<Vec<u8>, PtyError> {
        Self::capture_with_input(PtyCommand::default_shell(), input, size, timeout)
    }

    fn capture_with_input(
        command: PtyCommand,
        input: &str,
        size: PtySize,
        timeout: Duration,
    ) -> Result<Vec<u8>, PtyError> {
        command.validate()?;

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(size.to_portable())
            .map_err(|error| PtyError::Backend(error.to_string()))?;

        let mut child = pair
            .slave
            .spawn_command(command.to_builder())
            .map_err(|error| PtyError::Backend(error.to_string()))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| PtyError::Backend(error.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| PtyError::Backend(error.to_string()))?;
        let input = input.as_bytes().to_vec();
        let writer_thread = thread::spawn(move || -> io::Result<()> {
            let mut writer = writer;
            if !input.is_empty() {
                thread::sleep(Duration::from_millis(200));
                writer.write_all(&input)?;
                writer.flush()?;
                thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        });

        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = reader.read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });

        let started = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                break;
            }

            if started.elapsed() >= timeout {
                let _ = child.kill();
                return Err(PtyError::Timeout(timeout));
            }

            thread::sleep(Duration::from_millis(10));
        }

        writer_thread
            .join()
            .map_err(|_| PtyError::Backend("PTY writer thread panicked".to_owned()))??;
        drop(pair.master);

        match receiver.recv_timeout(timeout) {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) => Err(PtyError::Io(error)),
            Err(_) => Err(PtyError::Timeout(timeout)),
        }
    }

    pub fn reader(&mut self) -> &mut dyn Read {
        self.reader.as_mut()
    }

    pub fn writer(&mut self) -> &mut dyn Write {
        self.writer.as_mut()
    }

    pub fn resize(&mut self, size: PtySize) -> Result<(), PtyError> {
        self.master
            .resize(size.to_portable())
            .map_err(|error| PtyError::Backend(error.to_string()))
    }

    pub fn read_blocking(&mut self) -> Result<Vec<u8>, PtyError> {
        let mut buffer = [0; 8192];
        let count = self.reader.read(&mut buffer)?;

        Ok(buffer[..count].to_vec())
    }

    pub fn wait(&mut self) -> Result<(), PtyError> {
        self.child
            .wait()
            .map(|_| ())
            .map_err(|error| PtyError::Backend(error.to_string()))
    }
}

#[derive(Debug)]
pub enum PtyError {
    InvalidCommand(String),
    InvalidSize { columns: u16, rows: u16 },
    Io(io::Error),
    Backend(String),
    Timeout(Duration),
}

impl Display for PtyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand(message) => formatter.write_str(message),
            Self::InvalidSize { columns, rows } => {
                write!(formatter, "invalid PTY size: {columns} columns, {rows} rows")
            }
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Backend(message) => formatter.write_str(message),
            Self::Timeout(timeout) => write!(formatter, "PTY operation timed out after {timeout:?}"),
        }
    }
}

impl Error for PtyError {}

impl From<io::Error> for PtyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
