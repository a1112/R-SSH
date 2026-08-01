use std::{
    env,
    process::Command,
    time::{Duration, Instant},
};

use rssh_core::TerminalSize;
use rssh_pty::{PtyCommand, PtySession, PtySize};
use rssh_terminal::Terminal;
use rssh_test_support::ChildGuard;

const QUICK_EXIT_ATTEMPTS_PER_GROUP: usize = 100;
const QUICK_EXIT_PROCESS_BUDGET: Duration = Duration::from_secs(15);
const QUICK_EXIT_GROUP_BUDGET: Duration = Duration::from_secs(60);
const QUICK_EXIT_COLD_READY_BUDGET: Duration = Duration::from_secs(5);
const QUICK_EXIT_MARKER_BUDGET: Duration = Duration::from_secs(5);
const QUICK_EXIT_COMPLETION_BUDGET: Duration = Duration::from_secs(5);
const QUICK_EXIT_CLEANUP_BUDGET: Duration = Duration::from_secs(2);
const QUICK_EXIT_P99_BUDGET: Duration = Duration::from_secs(5);

#[test]
fn local_pty_output_feeds_terminal_grid() {
    let marker = "rssh-terminal-grid-smoke";
    let output = PtySession::capture_output(
        &PtyCommand::platform_echo(marker),
        PtySize::try_new(160, 30).unwrap(),
        Duration::from_secs(5),
    )
    .unwrap();

    let mut terminal = Terminal::new(TerminalSize::new(160, 30));
    terminal.feed(&output);

    assert!(
        terminal_text(&terminal).contains(marker),
        "terminal grid did not receive marker; grid: {:?}",
        terminal_text(&terminal)
    );
}

#[test]
fn local_app_drains_output_after_fast_child_exit() {
    let marker = "rssh-local-drain-smoke";
    let groups = quick_exit_groups();
    let test_started = Instant::now();
    let total_budget = QUICK_EXIT_GROUP_BUDGET
        .checked_mul(u32::try_from(groups).expect("quick-exit group count fits u32"))
        .expect("quick-exit total budget fits Duration");
    let test_deadline = test_started + total_budget;
    let mut attempt_durations = Vec::with_capacity(groups * QUICK_EXIT_ATTEMPTS_PER_GROUP);

    for group in 1..=groups {
        let group_started = Instant::now();
        let mut owned_process_ids = Vec::with_capacity(QUICK_EXIT_ATTEMPTS_PER_GROUP);
        let mut owned_pty_child_ids = Vec::with_capacity(QUICK_EXIT_ATTEMPTS_PER_GROUP);

        for attempt in 1..=QUICK_EXIT_ATTEMPTS_PER_GROUP {
            let result =
                run_quick_exit_attempt(group, attempt, marker, test_deadline, total_budget);
            owned_process_ids.push(result.app_process_id);
            owned_pty_child_ids.push(result.pty_child_id);
            attempt_durations.push(result.elapsed);
        }

        assert_no_owned_console_processes(&owned_process_ids, &owned_pty_child_ids);
        let group_elapsed = group_started.elapsed();
        assert!(
            quick_exit_group_within_budget(group_elapsed),
            "quick-exit group {group} exceeded independent {QUICK_EXIT_GROUP_BUDGET:?} budget: {group_elapsed:?}"
        );
        assert!(
            Instant::now() <= test_deadline,
            "quick-exit exceeded absolute {total_budget:?} budget after group {group}"
        );
    }

    let p99 = percentile_99(&mut attempt_durations);
    assert!(
        p99 <= QUICK_EXIT_P99_BUDGET,
        "quick-exit p99 {p99:?} exceeded {QUICK_EXIT_P99_BUDGET:?} across {} no-retry attempts",
        attempt_durations.len()
    );
}

struct QuickExitAttempt {
    app_process_id: u32,
    pty_child_id: u32,
    elapsed: Duration,
}

fn run_quick_exit_attempt(
    group: usize,
    attempt: usize,
    marker: &str,
    test_deadline: Instant,
    total_budget: Duration,
) -> QuickExitAttempt {
    let attempt_started = Instant::now();
    let remaining = test_deadline.saturating_duration_since(attempt_started);
    assert!(
        !remaining.is_zero(),
        "quick-exit exceeded absolute {total_budget:?} budget before group {group} attempt {attempt}"
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_rssh-app"));
    command.env("RSSH_LOCAL_PTY_TRACE", "1");
    command.env("RSSH_LOCAL_PTY_TRACE_MARKER", marker);
    command.args(["local", "--mouse", "--"]);

    #[cfg(windows)]
    command.args(["cmd.exe", "/C", "echo"]).arg(marker);

    #[cfg(not(windows))]
    command.args(["sh", "-lc"]).arg(format!("echo {marker}"));

    let guard = ChildGuard::spawn(command, QUICK_EXIT_PROCESS_BUDGET.min(remaining))
        .unwrap_or_else(|error| panic!("group {group} attempt {attempt} failed to spawn: {error}"));
    let app_process_id = guard.process_id().expect("guarded app process id");
    let output = guard.wait().unwrap_or_else(|error| {
        panic!(
            "group {group} attempt {attempt} app pid {app_process_id} exceeded its deadline: {error}"
        )
    });
    let elapsed = attempt_started.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let pty_child_id = traced_pty_child_id(&output.stderr).unwrap_or_else(|| {
        panic!("group {group} attempt {attempt} omitted traced PTY child id; stderr: {stderr}")
    });
    let phases = QuickExitPhases::parse(&stderr);
    phases.assert_budgets(group, attempt, &stderr);

    assert!(
        elapsed <= QUICK_EXIT_PROCESS_BUDGET,
        "group {group} attempt {attempt} exceeded overall process budget: {elapsed:?}"
    );
    assert!(
        output.status.success(),
        "group {group} attempt {attempt} exited with {:?}; stderr: {stderr}",
        output.status.code()
    );
    assert!(
        stdout.contains(marker),
        "group {group} attempt {attempt} missed final PTY output; stdout bytes: {:?}",
        output.stdout
    );
    QuickExitAttempt {
        app_process_id,
        pty_child_id,
        elapsed,
    }
}

fn quick_exit_group_within_budget(elapsed: Duration) -> bool {
    elapsed <= QUICK_EXIT_GROUP_BUDGET
}

fn quick_exit_groups() -> usize {
    match env::var("RSSH_PTY_QUICK_EXIT_GROUPS") {
        Ok(value) => {
            let groups = value.parse::<usize>().unwrap_or_else(|error| {
                panic!("RSSH_PTY_QUICK_EXIT_GROUPS must be an integer from 1 through 10: {error}")
            });
            assert!(
                (1..=10).contains(&groups),
                "RSSH_PTY_QUICK_EXIT_GROUPS must be from 1 through 10, got {groups}"
            );
            groups
        }
        Err(env::VarError::NotPresent) => 1,
        Err(error) => panic!("RSSH_PTY_QUICK_EXIT_GROUPS is not valid Unicode: {error}"),
    }
}

struct QuickExitPhases {
    ready: Duration,
    marker: Duration,
    child_done: Duration,
    reader_done: Duration,
    cleanup_started: Duration,
    cleanup_done: Duration,
}

impl QuickExitPhases {
    fn parse(stderr: &str) -> Self {
        let phases = Self {
            ready: trace_elapsed(stderr, "spawned child"),
            marker: trace_elapsed(stderr, "trace marker observed"),
            child_done: trace_elapsed(stderr, "child reaped"),
            reader_done: trace_elapsed(stderr, "reader completed"),
            cleanup_started: trace_elapsed(stderr, "cleanup started"),
            cleanup_done: trace_elapsed(stderr, "cleanup completed"),
        };
        assert!(
            phases.ready <= phases.marker,
            "marker preceded readiness; {stderr}"
        );
        assert!(
            phases.child_done <= phases.cleanup_started,
            "cleanup preceded child completion; {stderr}"
        );
        assert!(
            phases.marker <= phases.reader_done,
            "reader completed before marker observation; {stderr}"
        );
        assert!(
            phases.cleanup_started <= phases.cleanup_done,
            "cleanup completed before it started; {stderr}"
        );
        assert!(
            phases.child_done.max(phases.reader_done) <= phases.cleanup_done,
            "cleanup completed before child/output completion; {stderr}"
        );
        phases
    }

    fn assert_budgets(&self, group: usize, attempt: usize, stderr: &str) {
        let completion = self.child_done.max(self.reader_done);
        assert_phase_budget(
            group,
            attempt,
            "cold readiness",
            self.ready,
            QUICK_EXIT_COLD_READY_BUDGET,
            stderr,
        );
        assert_phase_budget(
            group,
            attempt,
            "marker output",
            self.marker.saturating_sub(self.ready),
            QUICK_EXIT_MARKER_BUDGET,
            stderr,
        );
        assert_phase_budget(
            group,
            attempt,
            "child/output completion",
            completion.saturating_sub(self.marker),
            QUICK_EXIT_COMPLETION_BUDGET,
            stderr,
        );
        assert_phase_budget(
            group,
            attempt,
            "cleanup",
            self.cleanup_done.saturating_sub(self.cleanup_started),
            QUICK_EXIT_CLEANUP_BUDGET,
            stderr,
        );
    }
}

fn trace_elapsed(stderr: &str, event: &str) -> Duration {
    let line = stderr
        .lines()
        .find(|line| line.contains(&format!(": {event}")))
        .unwrap_or_else(|| panic!("missing trace event {event:?}; stderr: {stderr}"));
    let value = line
        .strip_prefix("local-pty +")
        .and_then(|suffix| suffix.split(':').next())
        .unwrap_or_else(|| panic!("invalid trace timestamp for {event:?}: {line}"));
    parse_debug_duration(value)
        .unwrap_or_else(|| panic!("invalid trace duration {value:?} for {event:?}: {line}"))
}

fn parse_debug_duration(value: &str) -> Option<Duration> {
    let units = [("ns", 1e-9), ("µs", 1e-6), ("ms", 1e-3), ("s", 1.0)];
    for (suffix, multiplier) in units {
        if let Some(number) = value.strip_suffix(suffix) {
            return Duration::try_from_secs_f64(number.parse::<f64>().ok()? * multiplier).ok();
        }
    }
    None
}

fn assert_phase_budget(
    group: usize,
    attempt: usize,
    phase: &str,
    elapsed: Duration,
    budget: Duration,
    stderr: &str,
) {
    assert!(
        elapsed <= budget,
        "group {group} attempt {attempt} {phase} phase took {elapsed:?}, budget {budget:?}; stderr: {stderr}"
    );
}

fn percentile_99(samples: &mut [Duration]) -> Duration {
    assert!(!samples.is_empty());
    samples.sort_unstable();
    let rank = (samples.len() * 99).div_ceil(100);
    samples[rank.saturating_sub(1)]
}

#[test]
fn parses_local_pty_trace_duration_units() {
    assert_eq!(parse_debug_duration("2s"), Some(Duration::from_secs(2)));
    assert_eq!(
        parse_debug_duration("12.5ms"),
        Some(Duration::from_micros(12_500))
    );
    assert_eq!(
        parse_debug_duration("40µs"),
        Some(Duration::from_micros(40))
    );
    assert_eq!(parse_debug_duration("7ns"), Some(Duration::from_nanos(7)));
    assert_eq!(parse_debug_duration("oops"), None);
}

#[test]
fn quick_exit_cleanup_phase_uses_explicit_cleanup_milestones() {
    let trace = "local-pty +1ms: spawned child pid=Some(1)\n\
                 local-pty +2ms: trace marker observed\n\
                 local-pty +3ms: child reaped status=ok\n\
                 local-pty +4ms: cleanup started\n\
                 local-pty +40ms: reader completed result=Ok(())\n\
                 local-pty +54ms: cleanup completed errors=0";
    let phases = QuickExitPhases::parse(trace);

    assert_eq!(
        phases.cleanup_done - phases.cleanup_started,
        Duration::from_millis(50)
    );
    assert_eq!(
        phases.child_done.max(phases.reader_done) - phases.marker,
        Duration::from_millis(38)
    );
}

#[test]
fn quick_exit_group_budget_is_independent_of_other_groups() {
    assert!(quick_exit_group_within_budget(QUICK_EXIT_GROUP_BUDGET));
    assert!(!quick_exit_group_within_budget(
        QUICK_EXIT_GROUP_BUDGET + Duration::from_nanos(1)
    ));
}

fn traced_pty_child_id(stderr: &[u8]) -> Option<u32> {
    let stderr = String::from_utf8_lossy(stderr);
    let suffix = stderr.split("spawned child pid=Some(").nth(1)?;
    suffix.split(')').next()?.parse().ok()
}

#[cfg(windows)]
fn assert_no_owned_console_processes(app_process_ids: &[u32], pty_child_ids: &[u32]) {
    let app_ids = app_process_ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let owner_ids = app_process_ids
        .iter()
        .chain(pty_child_ids)
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$appIds=@({app_ids}); $ownerIds=@({owner_ids}); $probePid=$PID; $deadline=[DateTime]::UtcNow.AddSeconds(3); do {{ $owned=@(Get-CimInstance Win32_Process | Where-Object {{ ($appIds -contains $_.ProcessId -and $_.Name -eq 'rssh-app.exe') -or ($ownerIds -contains $_.ParentProcessId -and $_.ParentProcessId -ne $probePid -and $_.Name -in @('rssh-app.exe','cmd.exe','conhost.exe','OpenConsole.exe')) }}); if ($owned.Count -eq 0) {{ exit 0 }}; $owned | ForEach-Object {{ Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue }} | Wait-Process -Timeout 1 -ErrorAction SilentlyContinue }} while ([DateTime]::UtcNow -lt $deadline); $owned=@(Get-CimInstance Win32_Process | Where-Object {{ ($appIds -contains $_.ProcessId -and $_.Name -eq 'rssh-app.exe') -or ($ownerIds -contains $_.ParentProcessId -and $_.ParentProcessId -ne $probePid -and $_.Name -in @('rssh-app.exe','cmd.exe','conhost.exe','OpenConsole.exe')) }}); $owned | ForEach-Object {{ \"$($_.ProcessId):$($_.ParentProcessId):$($_.Name)\" }}"
    );
    let mut command = Command::new("powershell.exe");
    command.args(["-NoLogo", "-NoProfile", "-Command", &script]);
    let output = ChildGuard::spawn(command, Duration::from_secs(10))
        .expect("spawn owned-process probe")
        .wait()
        .expect("owned-process probe deadline");
    assert!(output.status.success(), "owned-process probe failed");
    assert!(
        output.stdout.is_empty(),
        "quick-exit left owned processes: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[cfg(not(windows))]
fn assert_no_owned_console_processes(_app_process_ids: &[u32], _pty_child_ids: &[u32]) {}

fn terminal_text(terminal: &Terminal) -> String {
    let size = terminal.grid().size();
    let mut text = String::new();

    for row in 0..size.rows {
        for column in 0..size.columns {
            text.push_str(terminal.grid().get(row, column).unwrap().text());
        }
        text.push('\n');
    }

    text
}
