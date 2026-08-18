use std::error::Error;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaVersion {
    #[serde(rename = "rssh.diagnostics/v2")]
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    EmptyWindow,
    Ssh1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Linux,
    Macos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunIdentity {
    pub id: String,
    pub scenario: Scenario,
    pub platform: Platform,
    pub architecture: String,
    pub app_path: String,
    pub app_version: String,
    pub started_at_utc: String,
    pub launcher_version: String,
}

impl RunIdentity {
    #[must_use]
    pub fn fixture(scenario: Scenario, platform: Platform) -> Self {
        Self {
            id: "fixture-run".to_owned(),
            scenario,
            platform,
            architecture: "fixture-arch".to_owned(),
            app_path: "fixture-app".to_owned(),
            app_version: "0.0.0-fixture".to_owned(),
            started_at_utc: "1970-01-01T00:00:00Z".to_owned(),
            launcher_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunConfiguration {
    pub stabilization_ms: u64,
    pub sample_interval_ms: u64,
    pub sample_count: u32,
    pub columns: u16,
    pub rows: u16,
    pub scale_factor_milli: u16,
}

impl Default for RunConfiguration {
    fn default() -> Self {
        Self {
            stabilization_ms: 5_000,
            sample_interval_ms: 100,
            sample_count: 10,
            columns: 80,
            rows: 24,
            scale_factor_milli: 1_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupMilestones {
    pub process_started_ms: u64,
    pub window_created_ms: Option<u64>,
    pub first_present_ms: Option<u64>,
    pub config_ready_ms: Option<u64>,
    pub transport_started_ms: Option<u64>,
    pub transport_ready_ms: Option<u64>,
    pub gpu_ready_ms: Option<u64>,
    pub scenario_ready_ms: Option<u64>,
    pub sampling_started_ms: Option<u64>,
    pub sampling_finished_ms: Option<u64>,
    pub process_exited_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Pending,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readiness {
    pub status: ReadinessStatus,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererKind {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererSummary {
    pub first: Option<RendererKind>,
    #[serde(rename = "final")]
    pub final_renderer: Option<RendererKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    NotStarted,
    Pending,
    Connecting,
    AwaitingSecret,
    AwaitingHostKey,
    Connected,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionSummary {
    pub final_state: ConnectionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMetric {
    WindowsPrivateWorkingSetBytes,
    LinuxPssBytes,
    MacosPhysFootprintBytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySample {
    pub sequence: u32,
    pub elapsed_ms: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryStatistics {
    pub count: u32,
    pub min: u64,
    pub max: u64,
    pub mean: u64,
    pub median: u64,
    pub p50: u64,
    pub p95: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySummary {
    pub metric: MemoryMetric,
    pub unit: String,
    pub samples: Vec<MemorySample>,
    pub statistics: MemoryStatistics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessExitKind {
    Running,
    Natural,
    Requested,
    Forced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub pid: u32,
    pub exit_kind: ProcessExitKind,
    pub exit_code: Option<i32>,
    pub teardown_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticFailure {
    pub code: String,
    pub phase: String,
    pub message: String,
    pub os_error_code: Option<i64>,
    pub recoverable: bool,
    pub context: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsResult {
    pub schema: SchemaVersion,
    pub run: RunIdentity,
    pub configuration: RunConfiguration,
    pub milestones: StartupMilestones,
    pub readiness: Readiness,
    pub renderer: RendererSummary,
    pub connection: ConnectionSummary,
    pub memory: MemorySummary,
    pub process: ProcessSummary,
    pub failures: Vec<DiagnosticFailure>,
}

impl DiagnosticsResult {
    #[must_use]
    pub fn successful_fixture(
        run: RunIdentity,
        metric: MemoryMetric,
        configuration: RunConfiguration,
    ) -> Self {
        let sample_count = configuration.sample_count;
        let samples = (0..configuration.sample_count)
            .map(|sequence| MemorySample {
                sequence,
                elapsed_ms: u64::from(sequence) * configuration.sample_interval_ms,
                bytes: 1,
            })
            .collect();
        Self {
            schema: SchemaVersion::V2,
            run,
            configuration,
            milestones: StartupMilestones::default(),
            readiness: Readiness {
                status: ReadinessStatus::Ready,
                evidence: vec!["fixture".to_owned()],
            },
            renderer: RendererSummary {
                first: Some(RendererKind::Cpu),
                final_renderer: Some(RendererKind::Cpu),
            },
            connection: ConnectionSummary {
                final_state: ConnectionState::NotStarted,
            },
            memory: MemorySummary {
                metric,
                unit: "bytes".to_owned(),
                samples,
                statistics: MemoryStatistics {
                    count: sample_count,
                    min: 1,
                    max: 1,
                    mean: 1,
                    median: 1,
                    p50: 1,
                    p95: 1,
                },
            },
            process: ProcessSummary {
                pid: 1,
                exit_kind: ProcessExitKind::Requested,
                exit_code: Some(0),
                teardown_ms: Some(0),
            },
            failures: Vec::new(),
        }
    }

    /// Validates the cross-field invariants required by schema v2.
    ///
    /// # Errors
    ///
    /// Returns an error when a sampling parameter is zero, a successful run does not
    /// contain the configured number of samples, or the memory unit is not bytes.
    pub fn validate(&self) -> Result<(), SchemaValidationError> {
        if self.configuration.stabilization_ms == 0 {
            return Err(SchemaValidationError::ZeroConfiguration("stabilization_ms"));
        }
        if self.configuration.sample_interval_ms == 0 {
            return Err(SchemaValidationError::ZeroConfiguration(
                "sample_interval_ms",
            ));
        }
        if self.configuration.sample_count == 0 {
            return Err(SchemaValidationError::ZeroConfiguration("sample_count"));
        }
        let observed = u32::try_from(self.memory.samples.len()).unwrap_or(u32::MAX);
        if self.failures.is_empty() && observed != self.configuration.sample_count {
            return Err(SchemaValidationError::SampleCount {
                expected: self.configuration.sample_count,
                observed,
            });
        }
        if self.memory.unit != "bytes" {
            return Err(SchemaValidationError::MemoryUnit(self.memory.unit.clone()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaValidationError {
    ZeroConfiguration(&'static str),
    SampleCount { expected: u32, observed: u32 },
    MemoryUnit(String),
}

impl Display for SchemaValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroConfiguration(field) => write!(formatter, "{field} must be positive"),
            Self::SampleCount { expected, observed } => {
                write!(
                    formatter,
                    "expected {expected} memory samples, observed {observed}"
                )
            }
            Self::MemoryUnit(unit) => {
                write!(formatter, "memory unit must be bytes, observed {unit}")
            }
        }
    }
}

impl Error for SchemaValidationError {}
