use std::{
    error::Error,
    io::{self, Read, Write},
};

use rssh_ssh::{SshConnectRequest, SshSessionError, SshShellConnector, SshShellSession};

use crate::cli::SshOptions;

pub fn run(options: &SshOptions) -> Result<(), Box<dyn Error>> {
    let mut connector = UnavailableSshConnector;
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    run_with_connector_and_io(options, &mut connector, &mut stdin, &mut stdout)
}

fn run_with_connector_and_io(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let mut session = connector.connect(options.request.clone())?;
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

struct UnavailableSshConnector;

impl SshShellConnector for UnavailableSshConnector {
    fn connect(
        &mut self,
        _request: SshConnectRequest,
    ) -> Result<Box<dyn SshShellSession>, SshSessionError> {
        Err(SshSessionError::new(
            "ssh command parsing is available, but the ssh connector is not wired yet",
        ))
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

    use crate::cli::SshOptions;

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
                request: request.clone(),
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
            &SshOptions { request },
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
