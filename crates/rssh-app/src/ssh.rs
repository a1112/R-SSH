use std::{
    error::Error,
    io::{self, Write},
};

use rssh_ssh::{SshConnectRequest, SshSessionError, SshShellConnector, SshShellSession};

use crate::cli::SshOptions;

pub fn run(options: &SshOptions) -> Result<(), Box<dyn Error>> {
    let mut connector = UnavailableSshConnector;
    let mut stdout = io::stdout().lock();

    run_with_connector(options, &mut connector, &mut stdout)
}

fn run_with_connector(
    options: &SshOptions,
    connector: &mut dyn SshShellConnector,
    output: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let mut session = connector.connect(options.request.clone())?;
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
    use std::sync::{Arc, Mutex};

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

        super::run_with_connector(
            &SshOptions {
                request: request.clone(),
            },
            &mut connector,
            &mut output,
        )
        .unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.last_request.as_ref(), Some(&request));
        assert_eq!(output, b"remote\n");
        assert!(state.closed);
    }

    #[derive(Default)]
    struct MockState {
        last_request: Option<SshConnectRequest>,
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

        fn write(&mut self, _bytes: &[u8]) -> Result<usize, SshSessionError> {
            Ok(0)
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
