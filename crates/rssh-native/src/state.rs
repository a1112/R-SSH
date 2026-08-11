use std::collections::HashMap;

use rssh_core::{PaneId, TerminalSize};
use rssh_runtime::{
    EffectSequenceCursor, PaneToken, RuntimeProgress, RuntimeRevision, TerminalStateSummary,
};

use crate::TimerId;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LifecycleState {
    pub closing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlatformState {
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationState {
    pub size: TerminalSize,
    pub redraw_pending: bool,
}

impl Default for PresentationState {
    fn default() -> Self {
        Self {
            size: TerminalSize::new(80, 24),
            redraw_pending: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigState {
    pub revision: u64,
    pub theme: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimerState {
    pub epochs: HashMap<TimerId, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneState {
    pub token: PaneToken,
    pub revision: Option<RuntimeRevision>,
    pub snapshot: Option<TerminalStateSummary>,
    pub title: Option<String>,
    pub working_directory: Option<String>,
    pub badge_format: Option<String>,
    pub progress: RuntimeProgress,
    pub user_vars: HashMap<String, String>,
    pub restarting: bool,
    pub effect_sequence: EffectSequenceCursor,
}

impl PaneState {
    #[must_use]
    pub fn new(token: PaneToken) -> Self {
        Self {
            token,
            revision: None,
            snapshot: None,
            title: None,
            working_directory: None,
            badge_format: None,
            progress: RuntimeProgress::None,
            user_vars: HashMap::new(),
            restarting: false,
            effect_sequence: EffectSequenceCursor::default(),
        }
    }
}

/// Pure native state grouped by ownership domain.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WindowState {
    pub lifecycle: LifecycleState,
    pub platform: PlatformState,
    pub presentation: PresentationState,
    pub config: ConfigState,
    pub timers: TimerState,
    pub panes: HashMap<PaneId, PaneState>,
}
