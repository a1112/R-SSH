use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ConnectionState, RendererKind, Scenario, SchemaVersion, StartupMilestones};

pub const MARKER_PREFIX: &str = "rssh_diagnostic ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerIdentity {
    pub run_id: String,
    pub pid: u32,
    pub scenario: Scenario,
}

impl MarkerIdentity {
    #[must_use]
    pub fn new(run_id: impl Into<String>, pid: u32, scenario: Scenario) -> Self {
        Self {
            run_id: run_id.into(),
            pid,
            scenario,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerKind {
    ProcessStarted,
    WindowCreated,
    FirstPresent,
    ConfigReady,
    TransportStarted,
    TransportReady,
    GpuReady,
    ScenarioReady,
    SamplingStarted,
    SamplingFinished,
    ProcessExited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerRecord {
    pub schema: SchemaVersion,
    pub run_id: String,
    pub pid: u32,
    pub scenario: Scenario,
    pub kind: MarkerKind,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub renderer: Option<RendererKind>,
    #[serde(default)]
    pub connection_state: Option<ConnectionState>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarkerDisposition {
    Ignored,
    Accepted(MarkerRecord),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectedMarkers {
    pub milestones: StartupMilestones,
    pub first_renderer: Option<RendererKind>,
    pub final_renderer: Option<RendererKind>,
    pub connection_state: Option<ConnectionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerError {
    Malformed {
        message: String,
    },
    IdentityMismatch {
        field: &'static str,
        expected: String,
        observed: String,
    },
    DecreasingElapsed {
        previous_ms: u64,
        observed_ms: u64,
    },
    Duplicate(MarkerKind),
}

impl Display for MarkerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { message } => write!(formatter, "malformed marker: {message}"),
            Self::IdentityMismatch {
                field,
                expected,
                observed,
            } => write!(
                formatter,
                "marker {field} mismatch: expected {expected}, observed {observed}"
            ),
            Self::DecreasingElapsed {
                previous_ms,
                observed_ms,
            } => write!(
                formatter,
                "marker elapsed time decreased from {previous_ms} ms to {observed_ms} ms"
            ),
            Self::Duplicate(kind) => write!(formatter, "duplicate marker {kind:?}"),
        }
    }
}

impl Error for MarkerError {}

#[derive(Debug)]
pub struct MarkerCollector {
    identity: MarkerIdentity,
    seen: HashSet<MarkerKind>,
    last_elapsed_ms: Option<u64>,
    trace: CollectedMarkers,
}

impl MarkerCollector {
    #[must_use]
    pub fn new(identity: MarkerIdentity) -> Self {
        Self {
            identity,
            seen: HashSet::new(),
            last_elapsed_ms: None,
            trace: CollectedMarkers {
                milestones: StartupMilestones::default(),
                first_renderer: None,
                final_renderer: None,
                connection_state: None,
            },
        }
    }

    /// Parses and validates one application output line.
    ///
    /// # Errors
    ///
    /// Returns an error when a prefixed line is malformed, belongs to a different
    /// run/process/scenario, moves elapsed time backwards, or repeats a marker kind.
    pub fn push_line(&mut self, line: &str) -> Result<MarkerDisposition, MarkerError> {
        let Some(payload) = line.strip_prefix(MARKER_PREFIX) else {
            return Ok(MarkerDisposition::Ignored);
        };
        let record: MarkerRecord =
            serde_json::from_str(payload).map_err(|error| MarkerError::Malformed {
                message: error.to_string(),
            })?;
        self.validate_identity(&record)?;
        if let Some(previous_ms) = self.last_elapsed_ms
            && record.elapsed_ms < previous_ms
        {
            return Err(MarkerError::DecreasingElapsed {
                previous_ms,
                observed_ms: record.elapsed_ms,
            });
        }
        if self.seen.contains(&record.kind) {
            return Err(MarkerError::Duplicate(record.kind));
        }

        self.apply(&record);
        self.seen.insert(record.kind);
        self.last_elapsed_ms = Some(record.elapsed_ms);
        Ok(MarkerDisposition::Accepted(record))
    }

    #[must_use]
    pub const fn trace(&self) -> &CollectedMarkers {
        &self.trace
    }

    fn validate_identity(&self, record: &MarkerRecord) -> Result<(), MarkerError> {
        if record.run_id != self.identity.run_id {
            return Err(identity_mismatch(
                "run_id",
                self.identity.run_id.clone(),
                record.run_id.clone(),
            ));
        }
        if record.pid != self.identity.pid {
            return Err(identity_mismatch(
                "pid",
                self.identity.pid.to_string(),
                record.pid.to_string(),
            ));
        }
        if record.scenario != self.identity.scenario {
            return Err(identity_mismatch(
                "scenario",
                format!("{:?}", self.identity.scenario),
                format!("{:?}", record.scenario),
            ));
        }
        Ok(())
    }

    fn apply(&mut self, record: &MarkerRecord) {
        match record.kind {
            MarkerKind::ProcessStarted => {
                self.trace.milestones.process_started_ms = record.elapsed_ms;
            }
            MarkerKind::WindowCreated => {
                self.trace.milestones.window_created_ms = Some(record.elapsed_ms);
            }
            MarkerKind::FirstPresent => {
                self.trace.milestones.first_present_ms = Some(record.elapsed_ms);
                self.trace.first_renderer = record.renderer;
                if record.renderer.is_some() {
                    self.trace.final_renderer = record.renderer;
                }
            }
            MarkerKind::ConfigReady => {
                self.trace.milestones.config_ready_ms = Some(record.elapsed_ms);
            }
            MarkerKind::TransportStarted => {
                self.trace.milestones.transport_started_ms = Some(record.elapsed_ms);
            }
            MarkerKind::TransportReady => {
                self.trace.milestones.transport_ready_ms = Some(record.elapsed_ms);
            }
            MarkerKind::GpuReady => {
                self.trace.milestones.gpu_ready_ms = Some(record.elapsed_ms);
                self.trace.final_renderer = record.renderer.or(Some(RendererKind::Gpu));
            }
            MarkerKind::ScenarioReady => {
                self.trace.milestones.scenario_ready_ms = Some(record.elapsed_ms);
            }
            MarkerKind::SamplingStarted => {
                self.trace.milestones.sampling_started_ms = Some(record.elapsed_ms);
            }
            MarkerKind::SamplingFinished => {
                self.trace.milestones.sampling_finished_ms = Some(record.elapsed_ms);
            }
            MarkerKind::ProcessExited => {
                self.trace.milestones.process_exited_ms = Some(record.elapsed_ms);
            }
        }
        if record.connection_state.is_some() {
            self.trace.connection_state = record.connection_state;
        }
    }
}

fn identity_mismatch(field: &'static str, expected: String, observed: String) -> MarkerError {
    MarkerError::IdentityMismatch {
        field,
        expected,
        observed,
    }
}
