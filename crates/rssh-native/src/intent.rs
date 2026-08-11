use rssh_core::{PaneId, TerminalSize};
use rssh_runtime::{PaneToken, RuntimeBatch, TerminalStateSummary};

/// Stable identifier for a controller-owned timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

impl TimerId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Platform observations translated without platform-library types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformIntent {
    Focused(bool),
    Resized(TerminalSize),
}

/// Parsed user or automation commands accepted by the native controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandIntent {
    OpenUri(String),
    Copy(String),
    Paste { pane: PaneId, bytes: Vec<u8> },
    SpawnPane,
    SpawnWindow,
    RestartPane(PaneId),
    SetTitle(String),
    Persist,
}

/// Pure configuration changes relevant to the native presentation layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDiff {
    pub revision: u64,
    pub theme: Option<String>,
}

/// Timer lifecycle with an epoch that rejects stale callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerIntent {
    Arm { timer: TimerId, epoch: u64 },
    Fired { timer: TimerId, epoch: u64 },
    Cancel { timer: TimerId },
}

/// Pane ownership changes emitted by the runtime composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneLifecycleIntent {
    Opened(PaneToken),
    Closed(PaneToken),
}

/// Every input accepted by the deterministic native reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowIntent {
    Platform(PlatformIntent),
    Command(CommandIntent),
    RuntimeBatch(RuntimeBatch<TerminalStateSummary>),
    Config(ConfigDiff),
    Timer(TimerIntent),
    PaneLifecycle(PaneLifecycleIntent),
    RedrawRequested,
    CloseRequested,
}
