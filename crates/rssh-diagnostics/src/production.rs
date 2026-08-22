use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdout, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rssh_test_support::ssh::HermeticSshServer;

#[cfg(target_os = "windows")]
use crate::WindowsPrivateWorkingSetSampler;

use crate::{
    CollectedMarkers, ConnectionState, ConnectionSummary, DiagnosticFailure, DiagnosticsResult,
    LauncherOptions, LauncherPhase, LauncherStateMachine, MarkerCollector, MarkerDisposition,
    MarkerIdentity, MarkerKind, MemoryMetric, MemorySample, MemorySampler, MemoryStatistics,
    MemorySummary, Platform, ProcessExitKind, ProcessSummary, Readiness, ReadinessStatus,
    RendererSummary, RunIdentity, SamplerError, Scenario, SchemaVersion, StartupMilestones,
    summarize_bytes,
};

const READINESS_TIMEOUT: Duration = Duration::from_secs(20);
const PIPE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const OUTPUT_TAIL_LIMIT: usize = 64 * 1024;
const FIXTURE_USER: &str = "rssh-diagnostics";

#[derive(Debug)]
pub struct LauncherExecution {
    pub result: DiagnosticsResult,
    pub success: bool,
}

/// Runs one fully owned GUI diagnostic child and returns its schema-v2 result.
///
/// Runtime failures are represented inside the returned JSON model so the launcher
/// binary can always emit one machine-readable object after argument validation.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "process, pipe, sampler, fixture, and cleanup ownership are kept in one auditable lifetime"
)]
pub fn execute_launcher(options: &LauncherOptions) -> LauncherExecution {
    let started = Instant::now();
    let run_id = unique_run_id(options.scenario);
    let run = run_identity(options, &run_id);
    let metric = native_metric();
    let fixture = match SshFixtureContext::start(options.scenario) {
        Ok(fixture) => fixture,
        Err(message) => {
            return failed_execution(
                run,
                options,
                metric,
                0,
                RunFailure::new("fixture_start_failed", "launch", message),
                ProcessExitKind::Natural,
                None,
            );
        }
    };
    let mut command = diagnostic_command(options, &run_id, fixture.as_ref());
    let secret = fixture.as_ref().map(|fixture| fixture.secret.clone());
    let mut child = match command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = stop_fixture(fixture);
            return failed_execution(
                run,
                options,
                metric,
                0,
                RunFailure::from_io("child_spawn_failed", "launch", error),
                ProcessExitKind::Natural,
                None,
            );
        }
    };
    let pid = child.id();
    let stdout_tail = BoundedTail::new(OUTPUT_TAIL_LIMIT);
    let stderr_tail = BoundedTail::new(OUTPUT_TAIL_LIMIT);
    let (line_sender, line_receiver) = mpsc::channel();
    let (Some(child_stdout), Some(child_stderr)) = (child.stdout.take(), child.stderr.take())
    else {
        let status = force_reap(&mut child);
        let _ = stop_fixture(fixture);
        return failed_execution(
            run,
            options,
            metric,
            pid,
            RunFailure::new(
                "pipe_setup_failed",
                "launch",
                "configured child pipes were not available",
            ),
            ProcessExitKind::Forced,
            status.as_ref().and_then(ExitStatus::code),
        );
    };
    let stdout_thread = spawn_stdout_drain(child_stdout, stdout_tail.clone(), line_sender);
    let stderr_thread = spawn_stderr_drain(child_stderr, stderr_tail.clone());

    let mut child_memory_source = match native_sampler(pid) {
        Ok(child_memory_source) => child_memory_source,
        Err(error) => {
            let status = force_reap(&mut child);
            join_pipe_threads(stdout_thread, stderr_thread);
            let _ = stop_fixture(fixture);
            return failed_execution(
                run,
                options,
                metric,
                pid,
                failure_with_tails(
                    RunFailure::new("sampler_init_failed", "launch", error.to_string()),
                    &stdout_tail,
                    &stderr_tail,
                    secret.as_deref(),
                ),
                ProcessExitKind::Forced,
                status.as_ref().and_then(ExitStatus::code),
            );
        }
    };

    let mut collector = MarkerCollector::new(MarkerIdentity::new(run_id, pid, options.scenario));
    let mut state = LauncherStateMachine::new(options.configuration());
    let _ = state.child_started(pid);
    let run_outcome = run_child(
        options,
        started,
        &mut child,
        &line_receiver,
        &mut collector,
        &mut state,
        child_memory_source.as_mut(),
    );

    let (memory_samples, exit_kind, exit_status, teardown_ms) = match run_outcome {
        Ok(outcome) => outcome,
        Err(failure) => {
            let cleanup_started = Instant::now();
            let status = force_reap(&mut child);
            join_pipe_threads(stdout_thread, stderr_thread);
            drain_late_markers(&line_receiver, &mut collector);
            let fixture_failure = stop_fixture(fixture).err();
            let mut failure =
                failure_with_tails(failure, &stdout_tail, &stderr_tail, secret.as_deref());
            if let Some(fixture_failure) = fixture_failure {
                failure.message =
                    format!("{}; fixture teardown: {fixture_failure}", failure.message);
            }
            return failed_execution_with_trace(
                run,
                options,
                metric,
                pid,
                failure,
                ProcessExitKind::Forced,
                status.as_ref().and_then(ExitStatus::code),
                Some(duration_millis(cleanup_started.elapsed())),
                collector.trace().clone(),
            );
        }
    };

    join_pipe_threads(stdout_thread, stderr_thread);
    drain_late_markers(&line_receiver, &mut collector);
    if let Err(error) = stop_fixture(fixture) {
        return failed_execution_with_trace(
            run,
            options,
            metric,
            pid,
            failure_with_tails(
                RunFailure::new("fixture_teardown_failed", "reap", error),
                &stdout_tail,
                &stderr_tail,
                secret.as_deref(),
            ),
            exit_kind,
            exit_status.code(),
            Some(teardown_ms),
            collector.trace().clone(),
        );
    }

    match successful_result(
        run,
        options,
        metric,
        pid,
        memory_samples,
        exit_kind,
        exit_status.code(),
        teardown_ms,
        collector.trace().clone(),
    ) {
        Ok(result) => LauncherExecution {
            result,
            success: true,
        },
        Err(failure) => failed_execution_with_trace(
            run_identity(options, "result-build-failure"),
            options,
            metric,
            pid,
            failure,
            exit_kind,
            exit_status.code(),
            Some(teardown_ms),
            collector.trace().clone(),
        ),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the state-machine phases remain explicit and linear for deadline auditing"
)]
fn run_child(
    options: &LauncherOptions,
    started: Instant,
    child: &mut Child,
    lines: &mpsc::Receiver<String>,
    collector: &mut MarkerCollector,
    state: &mut LauncherStateMachine,
    sampler: &mut dyn MemorySampler,
) -> Result<(Vec<MemorySample>, ProcessExitKind, ExitStatus, u64), RunFailure> {
    let readiness_deadline = Instant::now() + READINESS_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| RunFailure::from_io("child_poll_failed", "await_markers", error))?
        {
            return Err(RunFailure::new(
                "child_exited_early",
                "await_scenario_ready",
                format!("child exited before readiness with {:?}", status.code()),
            ));
        }
        if Instant::now() >= readiness_deadline {
            return Err(RunFailure::new(
                "readiness_timeout",
                "await_scenario_ready",
                format!("scenario readiness exceeded {READINESS_TIMEOUT:?}"),
            ));
        }
        match lines.recv_timeout(PIPE_POLL_INTERVAL) {
            Ok(line) => {
                let ready = process_marker_line(&line, started, collector, Some(state))?;
                if ready {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::yield_now(),
        }
    }

    let stabilization_deadline = state.next_deadline_ms().ok_or_else(|| {
        RunFailure::new("missing_deadline", "stabilize", "no stabilization deadline")
    })?;
    wait_until_elapsed(
        stabilization_deadline,
        started,
        child,
        lines,
        collector,
        state,
    )?;
    state
        .advance_to(elapsed_ms(started))
        .map_err(state_failure)?;

    let sampling_started_ms = elapsed_ms(started);
    let mut memory_samples = Vec::with_capacity(usize::try_from(options.sample_count).unwrap_or(0));
    for sequence in 0..options.sample_count {
        let due = state.next_deadline_ms().ok_or_else(|| {
            RunFailure::new("missing_deadline", "sample", "sample deadline is missing")
        })?;
        wait_until_elapsed(due, started, child, lines, collector, state)?;
        let observed_ms = elapsed_ms(started);
        let bytes = sampler.sample().map_err(|error| {
            RunFailure::new("memory_sample_failed", "sample", error.to_string())
        })?;
        state
            .record_sample(observed_ms, bytes)
            .map_err(state_failure)?;
        memory_samples.push(MemorySample {
            sequence,
            elapsed_ms: observed_ms,
            bytes,
        });
    }
    let sampling_finished_ms = elapsed_ms(started);

    state
        .graceful_shutdown_requested(sampling_finished_ms)
        .map_err(state_failure)?;
    let teardown_started = Instant::now();
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(b"shutdown\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| {
                RunFailure::from_io("shutdown_signal_failed", "request_shutdown", error)
            })?;
    }
    child.stdin.take();

    let shutdown_deadline = Instant::now() + options.shutdown_timeout;
    let (exit_kind, status) = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| RunFailure::from_io("child_poll_failed", "reap", error))?
        {
            break (ProcessExitKind::Requested, status);
        }
        drain_available_markers(lines, started, collector, Some(state))?;
        if Instant::now() >= shutdown_deadline {
            state
                .force_shutdown(elapsed_ms(started))
                .map_err(state_failure)?;
            child
                .kill()
                .map_err(|error| RunFailure::from_io("child_kill_failed", "reap", error))?;
            let status = child
                .wait()
                .map_err(|error| RunFailure::from_io("child_wait_failed", "reap", error))?;
            break (ProcessExitKind::Forced, status);
        }
        thread::sleep(PIPE_POLL_INTERVAL);
    };
    state
        .child_reaped(status.code(), elapsed_ms(started))
        .map_err(state_failure)?;
    let teardown_ms = duration_millis(teardown_started.elapsed());

    let trace = collector.trace();
    let mut milestones = trace.milestones.clone();
    milestones.sampling_started_ms = Some(sampling_started_ms);
    milestones.sampling_finished_ms = Some(sampling_finished_ms);
    // The launcher-owned boundaries are installed by `successful_result`; this
    // assignment keeps their values available without inventing wire markers.
    let _ = milestones;
    Ok((memory_samples, exit_kind, status, teardown_ms))
}

fn wait_until_elapsed(
    due_ms: u64,
    started: Instant,
    child: &mut Child,
    lines: &mpsc::Receiver<String>,
    collector: &mut MarkerCollector,
    state: &mut LauncherStateMachine,
) -> Result<(), RunFailure> {
    loop {
        let now_ms = elapsed_ms(started);
        if now_ms >= due_ms {
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| RunFailure::from_io("child_poll_failed", "sample", error))?
        {
            return Err(RunFailure::new(
                "child_exited_early",
                "sample",
                format!("child exited during sampling with {:?}", status.code()),
            ));
        }
        let wait = Duration::from_millis(due_ms.saturating_sub(now_ms)).min(PIPE_POLL_INTERVAL);
        match lines.recv_timeout(wait) {
            Ok(line) => {
                process_marker_line(&line, started, collector, Some(state))?;
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
}

fn process_marker_line(
    line: &str,
    started: Instant,
    collector: &mut MarkerCollector,
    state: Option<&mut LauncherStateMachine>,
) -> Result<bool, RunFailure> {
    match collector
        .push_line(line)
        .map_err(|error| RunFailure::new("marker_invalid", "await_markers", error.to_string()))?
    {
        MarkerDisposition::Ignored => Ok(false),
        MarkerDisposition::Accepted(record) => {
            if let Some(state) = state {
                state
                    .observe_marker(record.kind, elapsed_ms(started))
                    .map_err(state_failure)?;
            }
            Ok(record.kind == MarkerKind::ScenarioReady)
        }
    }
}

fn drain_available_markers(
    lines: &mpsc::Receiver<String>,
    started: Instant,
    collector: &mut MarkerCollector,
    mut state: Option<&mut LauncherStateMachine>,
) -> Result<(), RunFailure> {
    while let Ok(line) = lines.try_recv() {
        process_marker_line(&line, started, collector, state.as_deref_mut())?;
    }
    Ok(())
}

fn drain_late_markers(lines: &mpsc::Receiver<String>, collector: &mut MarkerCollector) {
    while let Ok(line) = lines.try_recv() {
        let _ = collector.push_line(&line);
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "schema assembly names each independently verified lifecycle artifact"
)]
fn successful_result(
    run: RunIdentity,
    options: &LauncherOptions,
    metric: MemoryMetric,
    pid: u32,
    samples: Vec<MemorySample>,
    exit_kind: ProcessExitKind,
    exit_code: Option<i32>,
    teardown_ms: u64,
    trace: CollectedMarkers,
) -> Result<DiagnosticsResult, RunFailure> {
    let statistics = summarize_bytes(
        &samples
            .iter()
            .map(|sample| sample.bytes)
            .collect::<Vec<_>>(),
    )
    .map_err(|error| RunFailure::new("statistics_failed", "emit_result", error.to_string()))?;
    let mut milestones = trace.milestones;
    let first_sample = samples.first().map(|sample| sample.elapsed_ms);
    let last_sample = samples.last().map(|sample| sample.elapsed_ms);
    milestones.sampling_started_ms = first_sample;
    milestones.sampling_finished_ms = last_sample;
    let result = DiagnosticsResult {
        schema: SchemaVersion::V2,
        run,
        configuration: options.configuration(),
        milestones,
        readiness: Readiness {
            status: ReadinessStatus::Ready,
            evidence: vec!["validated scenario_ready marker after a presented frame".to_owned()],
        },
        renderer: RendererSummary {
            first: trace.first_renderer,
            final_renderer: trace.final_renderer,
            backend: trace.gpu_backend,
            adapter_name: trace.gpu_adapter_name,
            adapter_vendor_id: trace.gpu_adapter_vendor_id,
            adapter_device_id: trace.gpu_adapter_device_id,
            adapter_type: trace.gpu_adapter_type,
        },
        connection: ConnectionSummary {
            final_state: trace
                .connection_state
                .unwrap_or(ConnectionState::NotStarted),
        },
        memory: MemorySummary {
            metric,
            unit: "bytes".to_owned(),
            samples,
            statistics,
        },
        process: ProcessSummary {
            pid,
            exit_kind,
            exit_code,
            teardown_ms: Some(teardown_ms),
        },
        failures: Vec::new(),
    };
    result.validate().map_err(|error| {
        RunFailure::new("schema_validation_failed", "emit_result", error.to_string())
    })?;
    Ok(result)
}

fn failed_execution(
    run: RunIdentity,
    options: &LauncherOptions,
    metric: MemoryMetric,
    pid: u32,
    failure: RunFailure,
    exit_kind: ProcessExitKind,
    exit_code: Option<i32>,
) -> LauncherExecution {
    failed_execution_with_trace(
        run,
        options,
        metric,
        pid,
        failure,
        exit_kind,
        exit_code,
        None,
        empty_trace(),
    )
}

#[allow(clippy::too_many_arguments)]
fn failed_execution_with_trace(
    run: RunIdentity,
    options: &LauncherOptions,
    metric: MemoryMetric,
    pid: u32,
    failure: RunFailure,
    exit_kind: ProcessExitKind,
    exit_code: Option<i32>,
    teardown_ms: Option<u64>,
    trace: CollectedMarkers,
) -> LauncherExecution {
    LauncherExecution {
        result: DiagnosticsResult {
            schema: SchemaVersion::V2,
            run,
            configuration: options.configuration(),
            milestones: trace.milestones,
            readiness: Readiness {
                status: ReadinessStatus::Failed,
                evidence: vec![failure.message.clone()],
            },
            renderer: RendererSummary {
                first: trace.first_renderer,
                final_renderer: trace.final_renderer,
                backend: trace.gpu_backend,
                adapter_name: trace.gpu_adapter_name,
                adapter_vendor_id: trace.gpu_adapter_vendor_id,
                adapter_device_id: trace.gpu_adapter_device_id,
                adapter_type: trace.gpu_adapter_type,
            },
            connection: ConnectionSummary {
                final_state: trace
                    .connection_state
                    .unwrap_or(ConnectionState::NotStarted),
            },
            memory: MemorySummary {
                metric,
                unit: "bytes".to_owned(),
                samples: Vec::new(),
                statistics: empty_statistics(),
            },
            process: ProcessSummary {
                pid,
                exit_kind,
                exit_code,
                teardown_ms,
            },
            failures: vec![failure.into_schema()],
        },
        success: false,
    }
}

fn diagnostic_command(
    options: &LauncherOptions,
    run_id: &str,
    fixture: Option<&SshFixtureContext>,
) -> Command {
    let mut command = Command::new(&options.app);
    command.args(diagnostic_arguments(options, run_id));
    command.env("RSSH_BENCHMARK_WINDOW_SCALE_FACTOR", "1");
    if let Some(fixture) = fixture {
        let address = fixture.server.address();
        command.args([
            "--ssh-host",
            &address.ip().to_string(),
            "--ssh-port",
            &address.port().to_string(),
            "--ssh-user",
            FIXTURE_USER,
            "--log",
            fixture.session_log.to_string_lossy().as_ref(),
        ]);
        fixture.server.temp_home().apply_to(&mut command);
        command
            .env("RSSH_DIAGNOSTIC_SSH_SECRET", &fixture.secret)
            .env("SSH_AUTH_SOCK", "rssh-diagnostics-invalid-agent");
    }
    command
}

pub(crate) fn diagnostic_arguments(options: &LauncherOptions, run_id: &str) -> Vec<String> {
    let scenario = match options.scenario {
        Scenario::EmptyWindow => "empty-window",
        Scenario::Ssh1 => "ssh1",
    };
    let hold_ms = options
        .stabilization
        .saturating_add(options.sample_interval.saturating_mul(options.sample_count))
        .saturating_add(options.shutdown_timeout)
        .saturating_add(Duration::from_secs(30));
    let mut arguments = vec![
        "diagnostic-gui".to_owned(),
        "--run-id".to_owned(),
        run_id.to_owned(),
        "--scenario".to_owned(),
        scenario.to_owned(),
        "--hold-ms".to_owned(),
        duration_millis(hold_ms).to_string(),
        "--renderer".to_owned(),
        options.renderer.to_string(),
    ];
    if let Some(backend) = options.gpu_backend {
        arguments.extend(["--gpu-backend".to_owned(), backend.to_string()]);
    }
    arguments.extend([
        "--cols".to_owned(),
        options.columns.to_string(),
        "--rows".to_owned(),
        options.rows.to_string(),
    ]);
    arguments
}

struct SshFixtureContext {
    server: HermeticSshServer,
    secret: String,
    session_log: std::path::PathBuf,
}

impl SshFixtureContext {
    fn start(scenario: Scenario) -> Result<Option<Self>, String> {
        if scenario == Scenario::EmptyWindow {
            return Ok(None);
        }
        let secret = format!("rssh-diagnostic-secret-{}", unique_nonce());
        let server = HermeticSshServer::builder()
            .password(FIXTURE_USER, &secret)
            .start(Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        let session_log = server.temp_home().path().join("ssh1-session.log");
        Ok(Some(Self {
            server,
            secret,
            session_log,
        }))
    }

    fn stop(self) -> Result<(), String> {
        self.server
            .stop(Duration::from_secs(5))
            .map_err(|error| error.to_string())
    }
}

fn stop_fixture(fixture: Option<SshFixtureContext>) -> Result<(), String> {
    fixture.map_or(Ok(()), SshFixtureContext::stop)
}

fn native_sampler(pid: u32) -> Result<Box<dyn MemorySampler>, SamplerError> {
    #[cfg(target_os = "windows")]
    {
        return WindowsPrivateWorkingSetSampler::new(pid)
            .map(|sampler| Box::new(sampler) as Box<dyn MemorySampler>);
    }
    #[cfg(target_os = "linux")]
    {
        return crate::LinuxPssSampler::new(pid)
            .map(|sampler| Box::new(sampler) as Box<dyn MemorySampler>);
    }
    #[cfg(target_os = "macos")]
    {
        return crate::MacosPhysFootprintSampler::new(pid)
            .map(|sampler| Box::new(sampler) as Box<dyn MemorySampler>);
    }
    #[allow(unreachable_code)]
    Err(SamplerError::Unsupported {
        metric: native_metric(),
        detail: "the launcher has no native sampler for this target".to_owned(),
    })
}

const fn native_metric() -> MemoryMetric {
    #[cfg(target_os = "windows")]
    {
        return MemoryMetric::WindowsPrivateWorkingSetBytes;
    }
    #[cfg(target_os = "linux")]
    {
        return MemoryMetric::LinuxPssBytes;
    }
    #[cfg(target_os = "macos")]
    {
        return MemoryMetric::MacosPhysFootprintBytes;
    }
    #[allow(unreachable_code)]
    MemoryMetric::LinuxPssBytes
}

const fn native_platform() -> Platform {
    #[cfg(target_os = "windows")]
    {
        return Platform::Windows;
    }
    #[cfg(target_os = "linux")]
    {
        return Platform::Linux;
    }
    #[cfg(target_os = "macos")]
    {
        return Platform::Macos;
    }
    #[allow(unreachable_code)]
    Platform::Linux
}

fn run_identity(options: &LauncherOptions, run_id: &str) -> RunIdentity {
    RunIdentity {
        id: run_id.to_owned(),
        scenario: options.scenario,
        platform: native_platform(),
        architecture: std::env::consts::ARCH.to_owned(),
        app_path: options.app.to_string_lossy().into_owned(),
        app_version: "unknown".to_owned(),
        started_at_utc: format!("unix:{}", unix_seconds()),
        launcher_version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

fn unique_run_id(scenario: Scenario) -> String {
    format!(
        "{}-{}-{}",
        match scenario {
            Scenario::EmptyWindow => "empty-window",
            Scenario::Ssh1 => "ssh1",
        },
        std::process::id(),
        unique_nonce()
    )
}

fn unique_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn elapsed_ms(started: Instant) -> u64 {
    duration_millis(started.elapsed())
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "map_err transfers the typed failure into its schema-facing representation"
)]
fn state_failure(failure: crate::LauncherFailure) -> RunFailure {
    RunFailure::new(
        "launcher_state_failed",
        phase_name(failure.phase),
        failure.to_string(),
    )
}

const fn phase_name(phase: LauncherPhase) -> &'static str {
    match phase {
        LauncherPhase::Launch => "launch",
        LauncherPhase::AwaitMarkers => "await_markers",
        LauncherPhase::AwaitScenarioReady => "await_scenario_ready",
        LauncherPhase::Stabilize => "stabilize",
        LauncherPhase::Sample => "sample",
        LauncherPhase::RequestShutdown => "request_shutdown",
        LauncherPhase::Reap => "reap",
        LauncherPhase::EmitResult => "emit_result",
        LauncherPhase::Failed => "failed",
    }
}

fn force_reap(child: &mut Child) -> Option<ExitStatus> {
    if let Ok(Some(status)) = child.try_wait() {
        return Some(status);
    }
    let _ = child.kill();
    child.wait().ok()
}

#[derive(Clone)]
struct BoundedTail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    limit: usize,
}

impl BoundedTail {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Arc::new(Mutex::new(VecDeque::with_capacity(limit))),
            limit,
        }
    }

    fn push(&self, bytes: &[u8]) {
        let mut tail = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        tail.extend(bytes);
        let excess = tail.len().saturating_sub(self.limit);
        tail.drain(..excess);
    }

    fn text(&self) -> String {
        let tail = self
            .bytes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        String::from_utf8_lossy(&tail.iter().copied().collect::<Vec<_>>()).into_owned()
    }
}

fn spawn_stdout_drain(
    stdout: ChildStdout,
    tail: BoundedTail,
    sender: mpsc::Sender<String>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match reader.read_until(b'\n', &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    tail.push(&bytes);
                    let line = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned();
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

fn spawn_stderr_drain(
    mut stderr: impl Read + Send + 'static,
    tail: BoundedTail,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => tail.push(&buffer[..count]),
            }
        }
    })
}

fn join_pipe_threads(stdout: thread::JoinHandle<()>, stderr: thread::JoinHandle<()>) {
    let _ = stdout.join();
    let _ = stderr.join();
}

#[derive(Debug)]
struct RunFailure {
    code: String,
    phase: String,
    message: String,
    os_error_code: Option<i64>,
    context: Option<String>,
}

impl RunFailure {
    fn new(code: impl Into<String>, phase: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            phase: phase.into(),
            message: message.into(),
            os_error_code: None,
            context: None,
        }
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "the source error is intentionally consumed at the error boundary"
    )]
    fn from_io(code: &'static str, phase: &'static str, error: std::io::Error) -> Self {
        let os_error_code = error.raw_os_error().map(i64::from);
        let mut failure = Self::new(code, phase, error.to_string());
        failure.os_error_code = os_error_code;
        failure
    }

    fn into_schema(self) -> DiagnosticFailure {
        DiagnosticFailure {
            code: self.code,
            phase: self.phase,
            message: self.message,
            os_error_code: self.os_error_code,
            recoverable: false,
            context: self.context,
        }
    }
}

fn failure_with_tails(
    mut failure: RunFailure,
    stdout: &BoundedTail,
    stderr: &BoundedTail,
    secret: Option<&str>,
) -> RunFailure {
    let mut context = format!(
        "stdout_tail={:?}; stderr_tail={:?}",
        stdout.text(),
        stderr.text()
    );
    if let Some(secret) = secret {
        context = context.replace(secret, "<redacted>");
    }
    failure.context = Some(context);
    failure
}

fn empty_trace() -> CollectedMarkers {
    CollectedMarkers {
        milestones: StartupMilestones::default(),
        first_renderer: None,
        final_renderer: None,
        connection_state: None,
        gpu_backend: None,
        gpu_adapter_name: None,
        gpu_adapter_vendor_id: None,
        gpu_adapter_device_id: None,
        gpu_adapter_type: None,
    }
}

const fn empty_statistics() -> MemoryStatistics {
    MemoryStatistics {
        count: 0,
        min: 0,
        max: 0,
        mean: 0,
        median: 0,
        p50: 0,
        p95: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{DiagnosticGpuBackend, DiagnosticRendererMode};

    fn fixture_options() -> LauncherOptions {
        LauncherOptions {
            app: PathBuf::from("rssh-app"),
            scenario: Scenario::EmptyWindow,
            stabilization: Duration::from_millis(1),
            sample_interval: Duration::from_millis(1),
            sample_count: 1,
            shutdown_timeout: Duration::from_millis(1),
            columns: 80,
            rows: 24,
            renderer: DiagnosticRendererMode::Gpu,
            gpu_backend: Some(DiagnosticGpuBackend::Dx12),
            json: true,
        }
    }

    fn gpu_identity_trace() -> CollectedMarkers {
        CollectedMarkers {
            milestones: StartupMilestones::default(),
            first_renderer: Some(crate::RendererKind::Cpu),
            final_renderer: Some(crate::RendererKind::Gpu),
            connection_state: Some(ConnectionState::NotStarted),
            gpu_backend: Some(DiagnosticGpuBackend::Dx12),
            gpu_adapter_name: Some("fixture-adapter".to_owned()),
            gpu_adapter_vendor_id: Some(0x10de),
            gpu_adapter_device_id: Some(0x2684),
            gpu_adapter_type: Some("discrete-gpu".to_owned()),
        }
    }

    fn assert_gpu_identity(renderer: &RendererSummary) {
        assert_eq!(renderer.backend, Some(DiagnosticGpuBackend::Dx12));
        assert_eq!(renderer.adapter_name.as_deref(), Some("fixture-adapter"));
        assert_eq!(renderer.adapter_vendor_id, Some(0x10de));
        assert_eq!(renderer.adapter_device_id, Some(0x2684));
        assert_eq!(renderer.adapter_type.as_deref(), Some("discrete-gpu"));
    }

    #[test]
    fn successful_result_copies_collected_gpu_identity() {
        let options = fixture_options();
        let result = successful_result(
            RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
            &options,
            MemoryMetric::WindowsPrivateWorkingSetBytes,
            42,
            vec![MemorySample {
                sequence: 0,
                elapsed_ms: 10,
                bytes: 1,
            }],
            ProcessExitKind::Requested,
            Some(0),
            1,
            gpu_identity_trace(),
        )
        .expect("valid successful fixture result");

        assert_gpu_identity(&result.renderer);
    }

    #[test]
    fn failed_result_copies_collected_gpu_identity() {
        let options = fixture_options();
        let execution = failed_execution_with_trace(
            RunIdentity::fixture(Scenario::EmptyWindow, Platform::Windows),
            &options,
            MemoryMetric::WindowsPrivateWorkingSetBytes,
            42,
            RunFailure::new("fixture_failure", "fixture", "fixture failure"),
            ProcessExitKind::Forced,
            Some(1),
            Some(1),
            gpu_identity_trace(),
        );

        assert_gpu_identity(&execution.result.renderer);
    }

    #[test]
    fn diagnostic_arguments_forward_requested_renderer_and_backend_exactly() {
        let options = LauncherOptions {
            app: PathBuf::from("rssh-app"),
            scenario: Scenario::EmptyWindow,
            stabilization: Duration::from_millis(5_000),
            sample_interval: Duration::from_millis(100),
            sample_count: 10,
            shutdown_timeout: Duration::from_millis(2_000),
            columns: 80,
            rows: 24,
            renderer: DiagnosticRendererMode::Gpu,
            gpu_backend: Some(DiagnosticGpuBackend::Dx12),
            json: true,
        };

        assert_eq!(
            diagnostic_arguments(&options, "probe-run"),
            [
                "diagnostic-gui",
                "--run-id",
                "probe-run",
                "--scenario",
                "empty-window",
                "--hold-ms",
                "38000",
                "--renderer",
                "gpu",
                "--gpu-backend",
                "dx12",
                "--cols",
                "80",
                "--rows",
                "24",
            ]
            .map(str::to_owned)
        );
    }

    #[test]
    fn diagnostic_arguments_keep_auto_default_without_a_backend_override() {
        let options = LauncherOptions {
            app: PathBuf::from("rssh-app"),
            scenario: Scenario::EmptyWindow,
            stabilization: Duration::from_millis(5_000),
            sample_interval: Duration::from_millis(100),
            sample_count: 10,
            shutdown_timeout: Duration::from_millis(2_000),
            columns: 80,
            rows: 24,
            renderer: DiagnosticRendererMode::Auto,
            gpu_backend: None,
            json: true,
        };

        let arguments = diagnostic_arguments(&options, "default-run");

        assert_eq!(
            arguments
                .windows(2)
                .find(|pair| pair[0] == "--renderer")
                .map(|pair| pair[1].as_str()),
            Some("auto")
        );
        assert!(!arguments.iter().any(|argument| argument == "--gpu-backend"));
    }
}
