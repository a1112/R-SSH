use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io::{self, Read, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use portable_pty::{
    CommandBuilder, ExitStatus as PortableExitStatus, MasterPty, PtySize as PortablePtySize,
    native_pty_system,
};

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
    use std::{
        io::{Read, Write},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

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
            &PtyCommand::platform_identity_command(),
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

    #[test]
    #[ignore = "spawns a real platform PTY"]
    fn local_pty_exposes_owned_reader_and_writer() {
        let command = PtyCommand::default_shell();
        let mut session = PtySession::spawn(&command, PtySize::try_new(80, 24).unwrap()).unwrap();

        let _reader = session.take_reader().unwrap();
        let _writer = session.take_writer().unwrap();
    }

    #[test]
    #[ignore = "spawns a real platform shell"]
    fn local_pty_supports_interactive_shell_roundtrip() {
        let marker = "rssh-pty-interactive-smoke";
        let command = PtyCommand::default_shell();
        let mut session = PtySession::spawn(&command, PtySize::try_new(80, 24).unwrap()).unwrap();
        let mut reader = session.take_reader().unwrap();
        let mut writer = session.take_writer().unwrap();
        let (sender, receiver) = mpsc::channel();

        let reader_thread = thread::spawn(move || {
            let mut buffer = [0; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = sender.send(Ok(Vec::new()));
                        return;
                    }
                    Ok(count) => {
                        if sender.send(Ok(buffer[..count].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        return;
                    }
                }
            }
        });

        thread::sleep(Duration::from_millis(300));
        writer
            .write_all(format!("echo {marker}\r\n").as_bytes())
            .unwrap();
        writer.flush().unwrap();

        let mut captured = Vec::new();
        let started = Instant::now();
        let timeout = Duration::from_secs(5);

        while started.elapsed() < timeout {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(chunk)) => {
                    if chunk
                        .windows(b"\x1b[6n".len())
                        .any(|window| window == b"\x1b[6n")
                    {
                        writer.write_all(b"\x1b[1;1R").unwrap();
                        writer.flush().unwrap();
                    }
                    captured.extend_from_slice(&chunk);
                    if String::from_utf8_lossy(&captured).contains(marker) {
                        break;
                    }
                }
                Ok(Err(error)) => panic!("failed to read PTY output: {error}"),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let output = String::from_utf8_lossy(&captured);
        assert!(
            output.contains(marker),
            "interactive PTY output did not contain marker; captured: {output:?}"
        );

        writer.write_all(b"exit\r\n").unwrap();
        writer.flush().unwrap();
        drop(writer);
        session.wait().unwrap();
        drop(session);
        reader_thread.join().unwrap();
    }

    #[test]
    #[ignore = "spawns a real platform PTY"]
    fn local_pty_reports_child_exit_status() {
        let command = if cfg!(windows) {
            PtyCommand::new("cmd.exe").with_args(["/C", "exit", "7"])
        } else {
            PtyCommand::new("/bin/sh").with_args(["-lc", "exit 7"])
        };
        let mut session = PtySession::spawn(&command, PtySize::try_new(80, 24).unwrap()).unwrap();
        let mut reader = session.take_reader().unwrap();
        let mut writer = session.take_writer().unwrap();
        let _io_thread = thread::spawn(move || {
            let mut buffer = [0; 4096];
            let mut probe = Vec::new();

            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => return,
                    Ok(count) => respond_to_cursor_position_queries(
                        &buffer[..count],
                        &mut probe,
                        &mut writer,
                    ),
                }
            }
        });

        let started = Instant::now();
        let timeout = Duration::from_secs(5);
        let status = loop {
            if let Some(status) = session.try_wait().unwrap() {
                break status;
            }
            if started.elapsed() >= timeout {
                let _ = session.kill();
                panic!("PTY child did not exit within {timeout:?}");
            }
            thread::sleep(Duration::from_millis(20));
        };

        assert_eq!(status.exit_code(), 7);
        assert!(!status.success());
    }

    fn respond_to_cursor_position_queries(
        chunk: &[u8],
        probe: &mut Vec<u8>,
        writer: &mut dyn Write,
    ) {
        const QUERY: &[u8] = b"\x1b[6n";
        const RESPONSE: &[u8] = b"\x1b[1;1R";

        probe.extend_from_slice(chunk);
        while let Some(index) = find_subslice(probe, QUERY) {
            writer.write_all(RESPONSE).unwrap();
            writer.flush().unwrap();
            probe.drain(..index + QUERY.len());
        }

        let retained = QUERY.len().saturating_sub(1);
        let removable = probe.len().saturating_sub(retained);
        if removable > 0 {
            probe.drain(..removable);
        }
    }

    fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }

        haystack
            .windows(needle.len())
            .position(|window| window == needle)
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

    /// Validate that this command can be passed to a PTY backend.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::InvalidCommand`] when the command program is empty.
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
    /// Create a PTY size in character cells.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::InvalidSize`] when either dimension is zero.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExitStatus {
    code: u32,
    signal: Option<String>,
}

impl PtyExitStatus {
    #[must_use]
    pub const fn from_exit_code(code: u32) -> Self {
        Self { code, signal: None }
    }

    #[must_use]
    pub const fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }

    #[must_use]
    pub const fn exit_code(&self) -> u32 {
        self.code
    }

    #[must_use]
    pub fn signal(&self) -> Option<&str> {
        self.signal.as_deref()
    }
}

impl From<PortableExitStatus> for PtyExitStatus {
    fn from(status: PortableExitStatus) -> Self {
        Self {
            code: status.exit_code(),
            signal: status.signal().map(str::to_owned),
        }
    }
}

pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Option<Box<dyn Write + Send>>,
}

impl PtySession {
    /// Spawn a command inside a new platform PTY.
    ///
    /// # Errors
    ///
    /// Returns an error when the command is invalid, the PTY backend cannot be
    /// opened, the child process cannot be spawned, or PTY streams cannot be
    /// acquired.
    pub fn spawn(command: &PtyCommand, size: PtySize) -> Result<Self, PtyError> {
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
            reader: Some(reader),
            writer: Some(writer),
        })
    }

    /// Spawn a command and collect PTY output until the child exits.
    ///
    /// # Errors
    ///
    /// Returns an error when the command cannot be spawned, PTY output cannot be
    /// read, or the operation exceeds `timeout`.
    pub fn capture_output(
        command: &PtyCommand,
        size: PtySize,
        timeout: Duration,
    ) -> Result<Vec<u8>, PtyError> {
        Self::capture_with_input(command, "", size, timeout)
    }

    /// Spawn the platform shell, write input, and collect output until exit.
    ///
    /// # Errors
    ///
    /// Returns an error when the shell cannot be spawned, writing input fails,
    /// reading output fails, or the operation exceeds `timeout`.
    pub fn capture_shell_output(
        input: &str,
        size: PtySize,
        timeout: Duration,
    ) -> Result<Vec<u8>, PtyError> {
        let command = PtyCommand::default_shell();
        Self::capture_with_input(&command, input, size, timeout)
    }

    fn capture_with_input(
        command: &PtyCommand,
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

    /// Borrow the PTY reader stream.
    ///
    /// # Panics
    ///
    /// Panics when the reader has already been moved out with
    /// [`PtySession::take_reader`].
    pub fn reader(&mut self) -> &mut dyn Read {
        self.reader
            .as_mut()
            .expect("PTY reader was already taken")
            .as_mut()
    }

    /// Borrow the PTY writer stream.
    ///
    /// # Panics
    ///
    /// Panics when the writer has already been moved out with
    /// [`PtySession::take_writer`].
    pub fn writer(&mut self) -> &mut dyn Write {
        self.writer
            .as_mut()
            .expect("PTY writer was already taken")
            .as_mut()
    }

    /// Move the PTY reader stream out of the session.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::StreamTaken`] when the reader was already moved out.
    pub fn take_reader(&mut self) -> Result<Box<dyn Read + Send>, PtyError> {
        self.reader.take().ok_or(PtyError::StreamTaken("reader"))
    }

    /// Move the PTY writer stream out of the session.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::StreamTaken`] when the writer was already moved out.
    pub fn take_writer(&mut self) -> Result<Box<dyn Write + Send>, PtyError> {
        self.writer.take().ok_or(PtyError::StreamTaken("writer"))
    }

    /// Resize the PTY in character cells.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend rejects the resize operation.
    pub fn resize(&mut self, size: PtySize) -> Result<(), PtyError> {
        self.master
            .resize(size.to_portable())
            .map_err(|error| PtyError::Backend(error.to_string()))
    }

    /// Read one blocking chunk from the PTY reader.
    ///
    /// # Errors
    ///
    /// Returns an error when the reader stream fails.
    pub fn read_blocking(&mut self) -> Result<Vec<u8>, PtyError> {
        let mut buffer = [0; 8192];
        let count = self.reader().read(&mut buffer)?;

        Ok(buffer[..count].to_vec())
    }

    /// Wait until the child process exits.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend wait operation fails.
    pub fn wait(&mut self) -> Result<PtyExitStatus, PtyError> {
        self.child
            .wait()
            .map(PtyExitStatus::from)
            .map_err(|error| PtyError::Backend(error.to_string()))
    }

    /// Terminate the child process.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend cannot terminate the child.
    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill().map_err(PtyError::Io)
    }

    /// Check whether the child process has exited without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend status check fails.
    pub fn try_wait(&mut self) -> Result<Option<PtyExitStatus>, PtyError> {
        self.child
            .try_wait()
            .map(|status| status.map(PtyExitStatus::from))
            .map_err(PtyError::Io)
    }
}

#[derive(Debug)]
pub enum PtyError {
    InvalidCommand(String),
    InvalidSize { columns: u16, rows: u16 },
    Io(io::Error),
    Backend(String),
    Timeout(Duration),
    StreamTaken(&'static str),
}

impl Display for PtyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand(message) | Self::Backend(message) => formatter.write_str(message),
            Self::InvalidSize { columns, rows } => {
                write!(
                    formatter,
                    "invalid PTY size: {columns} columns, {rows} rows"
                )
            }
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Timeout(timeout) => {
                write!(formatter, "PTY operation timed out after {timeout:?}")
            }
            Self::StreamTaken(stream) => write!(formatter, "PTY {stream} stream was already taken"),
        }
    }
}

impl Error for PtyError {}

impl From<io::Error> for PtyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
