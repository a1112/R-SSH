use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;
use std::time::Duration;

use crate::{MarkerKind, RunConfiguration, Scenario};

pub const LAUNCHER_USAGE: &str = "Usage: rssh-bench-launcher --app PATH --scenario empty-window|ssh1 [--stabilization-ms N] [--sample-interval-ms N] [--sample-count N] [--json]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherOptions {
    pub app: PathBuf,
    pub scenario: Scenario,
    pub stabilization: Duration,
    pub sample_interval: Duration,
    pub sample_count: u32,
    pub json: bool,
}

impl LauncherOptions {
    /// Parses and validates the benchmark launcher command line.
    ///
    /// # Errors
    ///
    /// Returns an error for help, missing or repeated required arguments, unknown
    /// arguments, invalid scenario/value syntax, zero sampling values, or an app path
    /// that is not an existing file.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, LauncherCliError> {
        let mut arguments = args.into_iter();
        let _program = arguments.next();
        let mut app = None;
        let mut scenario = None;
        let mut stabilization_ms = 5_000_u64;
        let mut sample_interval_ms = 100_u64;
        let mut sample_count = 10_u32;
        let mut json = false;

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--help" | "-h" => return Err(LauncherCliError::HelpRequested),
                "--app" => {
                    assign_once(
                        &mut app,
                        PathBuf::from(next_value(&mut arguments, "--app")?),
                        "--app",
                    )?;
                }
                "--scenario" => {
                    let value = next_value(&mut arguments, "--scenario")?;
                    let parsed = match value.as_str() {
                        "empty-window" => Scenario::EmptyWindow,
                        "ssh1" => Scenario::Ssh1,
                        _ => return Err(LauncherCliError::InvalidScenario(value)),
                    };
                    assign_once(&mut scenario, parsed, "--scenario")?;
                }
                "--stabilization-ms" => {
                    stabilization_ms = parse_positive(
                        &next_value(&mut arguments, "--stabilization-ms")?,
                        "--stabilization-ms",
                    )?;
                }
                "--sample-interval-ms" => {
                    sample_interval_ms = parse_positive(
                        &next_value(&mut arguments, "--sample-interval-ms")?,
                        "--sample-interval-ms",
                    )?;
                }
                "--sample-count" => {
                    sample_count = parse_positive(
                        &next_value(&mut arguments, "--sample-count")?,
                        "--sample-count",
                    )?;
                }
                "--json" => json = true,
                _ => return Err(LauncherCliError::UnknownArgument(argument)),
            }
        }

        let app = app.ok_or(LauncherCliError::MissingArgument("--app"))?;
        if !app.is_file() {
            return Err(LauncherCliError::AppDoesNotExist(app));
        }
        Ok(Self {
            app,
            scenario: scenario.ok_or(LauncherCliError::MissingArgument("--scenario"))?,
            stabilization: Duration::from_millis(stabilization_ms),
            sample_interval: Duration::from_millis(sample_interval_ms),
            sample_count,
            json,
        })
    }

    #[must_use]
    pub fn configuration(&self) -> RunConfiguration {
        RunConfiguration {
            stabilization_ms: duration_millis(self.stabilization),
            sample_interval_ms: duration_millis(self.sample_interval),
            sample_count: self.sample_count,
            ..RunConfiguration::default()
        }
    }
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn next_value(
    arguments: &mut impl Iterator<Item = String>,
    option: &'static str,
) -> Result<String, LauncherCliError> {
    arguments
        .next()
        .ok_or(LauncherCliError::MissingValue(option))
}

fn assign_once<T>(
    destination: &mut Option<T>,
    value: T,
    option: &'static str,
) -> Result<(), LauncherCliError> {
    if destination.replace(value).is_some() {
        return Err(LauncherCliError::RepeatedArgument(option));
    }
    Ok(())
}

fn parse_positive<T>(value: &str, option: &'static str) -> Result<T, LauncherCliError>
where
    T: TryFrom<u64>,
{
    let parsed = value
        .parse::<u64>()
        .map_err(|_| LauncherCliError::InvalidPositiveValue {
            option,
            value: value.to_owned(),
        })?;
    if parsed == 0 {
        return Err(LauncherCliError::InvalidPositiveValue {
            option,
            value: value.to_owned(),
        });
    }
    T::try_from(parsed).map_err(|_| LauncherCliError::InvalidPositiveValue {
        option,
        value: value.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherCliError {
    HelpRequested,
    MissingArgument(&'static str),
    MissingValue(&'static str),
    RepeatedArgument(&'static str),
    UnknownArgument(String),
    InvalidScenario(String),
    InvalidPositiveValue { option: &'static str, value: String },
    AppDoesNotExist(PathBuf),
}

impl Display for LauncherCliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested => formatter.write_str(LAUNCHER_USAGE),
            Self::MissingArgument(option) => write!(formatter, "missing required {option}"),
            Self::MissingValue(option) => write!(formatter, "missing value for {option}"),
            Self::RepeatedArgument(option) => write!(formatter, "repeated argument {option}"),
            Self::UnknownArgument(argument) => write!(formatter, "unknown argument {argument}"),
            Self::InvalidScenario(value) => write!(
                formatter,
                "invalid scenario {value}; expected empty-window or ssh1"
            ),
            Self::InvalidPositiveValue { option, value } => {
                write!(
                    formatter,
                    "{option} must be a positive integer, observed {value}"
                )
            }
            Self::AppDoesNotExist(path) => {
                write!(formatter, "app path does not exist: {}", path.display())
            }
        }
    }
}

impl Error for LauncherCliError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherPhase {
    Launch,
    AwaitMarkers,
    AwaitScenarioReady,
    Stabilize,
    Sample,
    RequestShutdown,
    Reap,
    EmitResult,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherFailureCode {
    InvalidTransition,
    DecreasingClock,
    SampleBeforeDeadline,
    ChildExitedEarly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherFailure {
    pub code: LauncherFailureCode,
    pub phase: LauncherPhase,
    pub message: String,
}

impl Display for LauncherFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} ({:?})", self.message, self.phase)
    }
}

impl Error for LauncherFailure {}

#[derive(Debug, Clone)]
pub struct LauncherStateMachine {
    configuration: RunConfiguration,
    phase: LauncherPhase,
    pid: Option<u32>,
    last_elapsed_ms: u64,
    next_deadline_ms: Option<u64>,
    sample_bytes: Vec<u64>,
    forced_shutdown: bool,
    exit_code: Option<i32>,
}

impl LauncherStateMachine {
    #[must_use]
    pub const fn new(configuration: RunConfiguration) -> Self {
        Self {
            configuration,
            phase: LauncherPhase::Launch,
            pid: None,
            last_elapsed_ms: 0,
            next_deadline_ms: None,
            sample_bytes: Vec::new(),
            forced_shutdown: false,
            exit_code: None,
        }
    }

    #[must_use]
    pub const fn phase(&self) -> LauncherPhase {
        self.phase
    }

    #[must_use]
    pub const fn next_deadline_ms(&self) -> Option<u64> {
        self.next_deadline_ms
    }

    #[must_use]
    pub fn sample_bytes(&self) -> &[u64] {
        &self.sample_bytes
    }

    #[must_use]
    pub const fn forced_shutdown(&self) -> bool {
        self.forced_shutdown
    }

    /// Records successful child creation.
    ///
    /// # Errors
    ///
    /// Returns an invalid-transition failure unless called exactly once in `Launch`.
    pub fn child_started(&mut self, pid: u32) -> Result<(), LauncherFailure> {
        self.require_phase(LauncherPhase::Launch)?;
        self.pid = Some(pid);
        self.phase = LauncherPhase::AwaitMarkers;
        Ok(())
    }

    /// Applies a validated marker to launcher readiness state.
    ///
    /// # Errors
    ///
    /// Returns a failure if elapsed time moves backwards or the state is terminal.
    pub fn observe_marker(
        &mut self,
        kind: MarkerKind,
        elapsed_ms: u64,
    ) -> Result<(), LauncherFailure> {
        self.observe_time(elapsed_ms)?;
        if matches!(
            self.phase,
            LauncherPhase::Failed | LauncherPhase::EmitResult
        ) {
            return Err(self.invalid_transition("cannot observe markers in terminal state"));
        }
        match kind {
            MarkerKind::FirstPresent
                if matches!(
                    self.phase,
                    LauncherPhase::AwaitMarkers | LauncherPhase::AwaitScenarioReady
                ) =>
            {
                self.phase = LauncherPhase::AwaitScenarioReady;
            }
            MarkerKind::ScenarioReady
                if matches!(
                    self.phase,
                    LauncherPhase::AwaitMarkers | LauncherPhase::AwaitScenarioReady
                ) =>
            {
                self.phase = LauncherPhase::Stabilize;
                self.next_deadline_ms =
                    Some(elapsed_ms.saturating_add(self.configuration.stabilization_ms));
            }
            _ => {}
        }
        Ok(())
    }

    /// Advances the launcher's monotonic clock and performs due transitions.
    ///
    /// # Errors
    ///
    /// Returns a failure when time moves backwards.
    pub fn advance_to(&mut self, elapsed_ms: u64) -> Result<(), LauncherFailure> {
        self.observe_time(elapsed_ms)?;
        if self.phase == LauncherPhase::Stabilize
            && self.next_deadline_ms.is_some_and(|due| elapsed_ms >= due)
        {
            self.phase = LauncherPhase::Sample;
        }
        Ok(())
    }

    /// Records one child-memory sample at or after its scheduled deadline.
    ///
    /// # Errors
    ///
    /// Returns a failure outside `Sample`, when time decreases, or when sampling is
    /// attempted before the next deadline.
    pub fn record_sample(&mut self, elapsed_ms: u64, bytes: u64) -> Result<(), LauncherFailure> {
        self.require_phase(LauncherPhase::Sample)?;
        self.observe_time(elapsed_ms)?;
        let due = self
            .next_deadline_ms
            .ok_or_else(|| self.invalid_transition("sample deadline is missing"))?;
        if elapsed_ms < due {
            return Err(LauncherFailure {
                code: LauncherFailureCode::SampleBeforeDeadline,
                phase: self.phase,
                message: format!("sample at {elapsed_ms} ms precedes deadline {due} ms"),
            });
        }
        self.sample_bytes.push(bytes);
        if self.sample_bytes.len()
            == usize::try_from(self.configuration.sample_count).unwrap_or(usize::MAX)
        {
            self.phase = LauncherPhase::RequestShutdown;
            self.next_deadline_ms = None;
        } else {
            self.next_deadline_ms = Some(due.saturating_add(self.configuration.sample_interval_ms));
        }
        Ok(())
    }

    /// Records child exit, failing if it occurred before shutdown was requested.
    ///
    /// # Errors
    ///
    /// Returns `ChildExitedEarly` for an exit before `RequestShutdown` or `Reap`.
    pub fn child_exited(
        &mut self,
        exit_code: Option<i32>,
        elapsed_ms: u64,
    ) -> Result<(), LauncherFailure> {
        self.observe_time(elapsed_ms)?;
        self.exit_code = exit_code;
        if matches!(
            self.phase,
            LauncherPhase::RequestShutdown | LauncherPhase::Reap
        ) {
            self.phase = LauncherPhase::EmitResult;
            return Ok(());
        }
        let failure = LauncherFailure {
            code: LauncherFailureCode::ChildExitedEarly,
            phase: self.phase,
            message: format!("child exited before sampling completed with {exit_code:?}"),
        };
        self.phase = LauncherPhase::Failed;
        Err(failure)
    }

    /// Moves from completed sampling to graceful shutdown/reaping.
    ///
    /// # Errors
    ///
    /// Returns an invalid-transition failure unless sampling requested shutdown.
    pub fn graceful_shutdown_requested(&mut self, elapsed_ms: u64) -> Result<(), LauncherFailure> {
        self.require_phase(LauncherPhase::RequestShutdown)?;
        self.observe_time(elapsed_ms)?;
        self.phase = LauncherPhase::Reap;
        Ok(())
    }

    /// Marks escalation from graceful shutdown to forced termination.
    ///
    /// # Errors
    ///
    /// Returns an invalid-transition failure outside `Reap`.
    pub fn force_shutdown(&mut self, elapsed_ms: u64) -> Result<(), LauncherFailure> {
        self.require_phase(LauncherPhase::Reap)?;
        self.observe_time(elapsed_ms)?;
        self.forced_shutdown = true;
        Ok(())
    }

    /// Records that the owned child was reaped.
    ///
    /// # Errors
    ///
    /// Returns an invalid-transition failure outside `Reap`.
    pub fn child_reaped(
        &mut self,
        exit_code: Option<i32>,
        elapsed_ms: u64,
    ) -> Result<(), LauncherFailure> {
        self.require_phase(LauncherPhase::Reap)?;
        self.observe_time(elapsed_ms)?;
        self.exit_code = exit_code;
        self.phase = LauncherPhase::EmitResult;
        Ok(())
    }

    fn observe_time(&mut self, elapsed_ms: u64) -> Result<(), LauncherFailure> {
        if elapsed_ms < self.last_elapsed_ms {
            return Err(LauncherFailure {
                code: LauncherFailureCode::DecreasingClock,
                phase: self.phase,
                message: format!(
                    "launcher clock decreased from {} ms to {elapsed_ms} ms",
                    self.last_elapsed_ms
                ),
            });
        }
        self.last_elapsed_ms = elapsed_ms;
        Ok(())
    }

    fn require_phase(&self, expected: LauncherPhase) -> Result<(), LauncherFailure> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(self.invalid_transition(&format!(
                "expected phase {expected:?}, observed {:?}",
                self.phase
            )))
        }
    }

    fn invalid_transition(&self, message: &str) -> LauncherFailure {
        LauncherFailure {
            code: LauncherFailureCode::InvalidTransition,
            phase: self.phase,
            message: message.to_owned(),
        }
    }
}
