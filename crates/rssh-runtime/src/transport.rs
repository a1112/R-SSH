use std::io::{self, Read, Write};

use rssh_core::TerminalSize;

/// Platform-neutral terminal-session completion status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionExit {
    /// The session exited normally with a process status code.
    Exited {
        /// Process exit code reported by the transport.
        code: i32,
    },
    /// The session was terminated by a named signal or remote equivalent.
    Signaled {
        /// Transport-provided signal name.
        signal: String,
    },
    /// The transport ended without a more specific status.
    Unknown,
}

/// Independently owned read, write, and lifecycle halves of a session.
#[derive(Debug)]
pub struct SessionParts<R, W, C> {
    /// Blocking or asynchronous-compatible byte reader.
    pub reader: R,
    /// Ordered byte writer.
    pub writer: W,
    /// Resize, exit, and close control plane.
    pub control: C,
}

impl<R, W, C> SessionParts<R, W, C> {
    /// Creates session parts from transport-owned resources.
    #[must_use]
    pub fn new(reader: R, writer: W, control: C) -> Self {
        Self {
            reader,
            writer,
            control,
        }
    }
}

/// Lifecycle operations retained after a session transport is split.
pub trait SessionControl: Send + 'static {
    /// Resizes the remote terminal grid.
    ///
    /// # Errors
    ///
    /// Returns a transport-specific I/O error when the resize cannot be sent.
    fn resize(&mut self, size: TerminalSize) -> io::Result<()>;

    /// Polls for a terminal session exit without imposing a wait strategy.
    ///
    /// # Errors
    ///
    /// Returns a transport-specific I/O error when status cannot be queried.
    fn poll_exit(&mut self) -> io::Result<Option<SessionExit>>;

    /// Begins the transport's orderly close operation.
    ///
    /// Calling this method more than once should be idempotent for concrete
    /// transports.
    ///
    /// # Errors
    ///
    /// Returns a transport-specific I/O error when closing cannot begin.
    fn begin_close(&mut self) -> io::Result<()>;
}

/// A source of terminal bytes with an independent ordered writer and control plane.
pub trait SessionTransport: Send + 'static {
    /// Reader owned by the runtime's transport-read path.
    type Reader: Read + Send + 'static;
    /// Writer owned by the pane worker's ordered write path.
    type Writer: Write + Send + 'static;
    /// Lifecycle control retained by the pane worker.
    type Control: SessionControl + Send + 'static;

    /// Transfers each independent session resource to the runtime.
    #[must_use]
    fn split(self) -> SessionParts<Self::Reader, Self::Writer, Self::Control>;
}
