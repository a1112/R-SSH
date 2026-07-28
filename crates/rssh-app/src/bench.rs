use std::{
    error::Error,
    io, thread,
    time::{Duration, Instant},
};

use rssh_core::TerminalSize;
use rssh_renderer::{PixelRenderer, TerminalRenderSnapshot};
use rssh_terminal::Terminal;
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

use crate::{
    cli::{BenchOptions, BenchThresholds, BenchWorkload},
    terminal_runtime::TerminalRuntime,
};

const BENCH_CELL_WIDTH: u32 = 8;
const BENCH_CELL_HEIGHT: u32 = 16;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct BenchReport {
    pub ok: bool,
    pub workload: String,
    pub bytes: usize,
    pub chunk_size: usize,
    pub chunks: usize,
    pub columns: u16,
    pub rows: u16,
    pub elapsed_ms: u128,
    pub throughput_bytes_per_sec: u64,
    pub chunk_p95_us: u128,
    pub render_frames: usize,
    pub render_frame_p95_us: u128,
    pub rendered_pixels: u128,
    pub render_pixels_per_sec: u64,
    pub idle_sample_ms: usize,
    pub idle_cpu_usage_percent: f32,
    pub process_memory_bytes: u64,
    pub process_virtual_memory_bytes: u64,
    pub process_accumulated_cpu_ms: u64,
    pub threshold_violations: Vec<BenchThresholdViolation>,
    pub display_bytes: usize,
    pub responses: usize,
    pub bells: u64,
    pub scrollback_lines: usize,
    pub inspected_query_bytes: u64,
    pub scrolled_survivor_cell_clones: u64,
    pub history_row_relocations: u64,
    pub metadata_rebase_batches: u64,
    pub cursor_row: u16,
    pub cursor_column: u16,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct BenchThresholdViolation {
    pub metric: String,
    pub actual: String,
    pub limit: String,
}

pub fn print_bench(options: &BenchOptions) -> Result<(), Box<dyn Error>> {
    let report = run_bench(options);

    if options.json {
        println!("{}", bench_json(&report)?);
    } else {
        for line in bench_text_lines(&report) {
            println!("{line}");
        }
    }

    if report.ok {
        Ok(())
    } else {
        Err(bench_threshold_error(&report))
    }
}

pub fn run_bench(options: &BenchOptions) -> BenchReport {
    let workload = build_benchmark_workload(options.workload, options.bytes);
    let mut report = run_benchmark_workload(
        options.workload,
        &workload,
        options.chunk_size,
        options.render_frames,
        options.idle_ms,
        options.size,
    );
    apply_bench_thresholds(&mut report, &options.thresholds);
    report
}

pub fn bench_json(report: &BenchReport) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(report)?)
}

pub fn bench_text_lines(report: &BenchReport) -> Vec<String> {
    let mut lines = vec![
        format!(
            "ok\tbench\tworkload={} bytes={} chunks={} chunk_size={} size={}x{}",
            report.workload,
            report.bytes,
            report.chunks,
            report.chunk_size,
            report.columns,
            report.rows
        ),
        format!(
            "metric\tthroughput_bytes_per_sec={}\tchunk_p95_us={}\telapsed_ms={}",
            report.throughput_bytes_per_sec, report.chunk_p95_us, report.elapsed_ms
        ),
        format!(
            "metric\trender_frames={}\trender_frame_p95_us={}\trendered_pixels={}\trender_pixels_per_sec={}",
            report.render_frames,
            report.render_frame_p95_us,
            report.rendered_pixels,
            report.render_pixels_per_sec
        ),
        format!(
            "metric\tidle_sample_ms={}\tidle_cpu_usage_percent={:.2}\tprocess_memory_bytes={}\tprocess_virtual_memory_bytes={}\tprocess_accumulated_cpu_ms={}",
            report.idle_sample_ms,
            report.idle_cpu_usage_percent,
            report.process_memory_bytes,
            report.process_virtual_memory_bytes,
            report.process_accumulated_cpu_ms
        ),
        format!(
            "metric\tdisplay_bytes={}\tresponses={}\tbells={}\tscrollback_lines={}",
            report.display_bytes, report.responses, report.bells, report.scrollback_lines
        ),
        format!(
            "work\tinspected_query_bytes={}\tscrolled_survivor_cell_clones={}\thistory_row_relocations={}\tmetadata_rebase_batches={}",
            report.inspected_query_bytes,
            report.scrolled_survivor_cell_clones,
            report.history_row_relocations,
            report.metadata_rebase_batches
        ),
    ];

    lines.extend(report.threshold_violations.iter().map(|violation| {
        format!(
            "fail\tthreshold\tmetric={} actual={} limit={}",
            violation.metric, violation.actual, violation.limit
        )
    }));

    lines
}

fn run_benchmark_workload(
    workload_kind: BenchWorkload,
    workload: &[u8],
    chunk_size: usize,
    render_frames: usize,
    idle_ms: usize,
    size: TerminalSize,
) -> BenchReport {
    let mut runtime = BenchmarkRuntime::new(workload_kind, size);
    let inspected_query_bytes_start = runtime.inspected_query_bytes();
    let terminal_work_start = runtime.terminal().work_counters();
    let mut chunk_timings = Vec::new();
    let mut responses = 0_usize;
    let mut display_bytes = 0_usize;
    let mut bells = 0_u64;

    let started = Instant::now();
    for chunk in workload.chunks(chunk_size) {
        let chunk_started = Instant::now();
        let output = runtime.feed(chunk);
        chunk_timings.push(chunk_started.elapsed().as_micros());
        responses = responses.saturating_add(output.responses);
        display_bytes = display_bytes.saturating_add(output.display_bytes);
        bells = bells.saturating_add(output.bells);
    }
    let elapsed = started.elapsed();
    let (cursor_row, cursor_column) = runtime.terminal().cursor();
    let work = runtime
        .terminal()
        .work_counters()
        .saturating_delta_since(terminal_work_start);
    let render_report = benchmark_rendering(runtime.terminal(), render_frames, size);
    let resource_report = sample_process_resources(idle_ms);

    BenchReport {
        ok: true,
        workload: workload_kind.as_str().to_owned(),
        bytes: workload.len(),
        chunk_size,
        chunks: chunk_timings.len(),
        columns: size.columns,
        rows: size.rows,
        elapsed_ms: elapsed.as_millis(),
        throughput_bytes_per_sec: bytes_per_second(workload.len(), elapsed.as_nanos()),
        chunk_p95_us: percentile_95(&mut chunk_timings),
        render_frames,
        render_frame_p95_us: render_report.frame_p95_us,
        rendered_pixels: render_report.rendered_pixels,
        render_pixels_per_sec: render_report.pixels_per_sec,
        idle_sample_ms: resource_report.idle_sample_ms,
        idle_cpu_usage_percent: resource_report.idle_cpu_usage_percent,
        process_memory_bytes: resource_report.process_memory_bytes,
        process_virtual_memory_bytes: resource_report.process_virtual_memory_bytes,
        process_accumulated_cpu_ms: resource_report.process_accumulated_cpu_ms,
        threshold_violations: Vec::new(),
        display_bytes,
        responses,
        bells,
        scrollback_lines: runtime.terminal().scrollback().len(),
        inspected_query_bytes: saturating_counter_delta(
            runtime.inspected_query_bytes(),
            inspected_query_bytes_start,
        ),
        scrolled_survivor_cell_clones: work.scrolled_survivor_cell_clones,
        history_row_relocations: work.history_row_relocations,
        metadata_rebase_batches: work.metadata_rebase_batches,
        cursor_row,
        cursor_column,
    }
}

struct BenchmarkChunkOutput {
    responses: usize,
    display_bytes: usize,
    bells: u64,
}

enum BenchmarkRuntime {
    Plain(Box<Terminal>),
    Filtered(Box<TerminalRuntime>),
}

impl BenchmarkRuntime {
    fn new(workload: BenchWorkload, size: TerminalSize) -> Self {
        match workload {
            BenchWorkload::PlainScroll => Self::Plain(Box::new(Terminal::new(size))),
            BenchWorkload::AnsiScroll | BenchWorkload::AnsiScrollQuery => {
                Self::Filtered(Box::new(TerminalRuntime::new_with_query_scan_work(size)))
            }
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> BenchmarkChunkOutput {
        match self {
            Self::Plain(terminal) => {
                terminal.feed(bytes);
                BenchmarkChunkOutput {
                    responses: 0,
                    display_bytes: bytes.len(),
                    bells: terminal.take_bell_count(),
                }
            }
            Self::Filtered(runtime) => {
                let output = runtime.feed_pty_output_with_display(bytes);
                BenchmarkChunkOutput {
                    responses: output.responses.len(),
                    display_bytes: output.display.len(),
                    bells: output.bells,
                }
            }
        }
    }

    fn terminal(&self) -> &Terminal {
        match self {
            Self::Plain(terminal) => terminal,
            Self::Filtered(runtime) => runtime.terminal(),
        }
    }

    fn inspected_query_bytes(&self) -> u64 {
        match self {
            Self::Plain(_) => 0,
            Self::Filtered(runtime) => runtime.inspected_query_bytes(),
        }
    }
}

fn apply_bench_thresholds(report: &mut BenchReport, thresholds: &BenchThresholds) {
    report.threshold_violations.clear();

    if let Some(limit) = thresholds.min_throughput_bytes_per_sec {
        record_min_violation(
            &mut report.threshold_violations,
            "throughput_bytes_per_sec",
            u128::from(report.throughput_bytes_per_sec),
            usize_to_u128(limit),
        );
    }

    if let Some(limit) = thresholds.max_chunk_p95_us {
        record_max_violation(
            &mut report.threshold_violations,
            "chunk_p95_us",
            report.chunk_p95_us,
            usize_to_u128(limit),
        );
    }

    if let Some(limit) = thresholds.max_render_frame_p95_us {
        record_max_violation(
            &mut report.threshold_violations,
            "render_frame_p95_us",
            report.render_frame_p95_us,
            usize_to_u128(limit),
        );
    }

    if let Some(limit) = thresholds.max_idle_cpu_percent {
        record_max_float_violation(
            &mut report.threshold_violations,
            "idle_cpu_usage_percent",
            report.idle_cpu_usage_percent,
            f32::from(limit),
        );
    }

    if let Some(limit) = thresholds.max_process_memory_bytes {
        record_max_violation(
            &mut report.threshold_violations,
            "process_memory_bytes",
            u128::from(report.process_memory_bytes),
            usize_to_u128(limit),
        );
    }

    report.ok = report.threshold_violations.is_empty();
}

fn record_min_violation(
    violations: &mut Vec<BenchThresholdViolation>,
    metric: &str,
    actual: u128,
    limit: u128,
) {
    if actual >= limit {
        return;
    }

    violations.push(BenchThresholdViolation {
        metric: metric.to_owned(),
        actual: actual.to_string(),
        limit: format!(">={limit}"),
    });
}

fn record_max_violation(
    violations: &mut Vec<BenchThresholdViolation>,
    metric: &str,
    actual: u128,
    limit: u128,
) {
    if actual <= limit {
        return;
    }

    violations.push(BenchThresholdViolation {
        metric: metric.to_owned(),
        actual: actual.to_string(),
        limit: format!("<={limit}"),
    });
}

fn record_max_float_violation(
    violations: &mut Vec<BenchThresholdViolation>,
    metric: &str,
    actual: f32,
    limit: f32,
) {
    if actual <= limit {
        return;
    }

    violations.push(BenchThresholdViolation {
        metric: metric.to_owned(),
        actual: format!("{actual:.2}"),
        limit: format!("<={limit:.2}"),
    });
}

fn bench_threshold_error(report: &BenchReport) -> Box<dyn Error> {
    Box::new(io::Error::other(format!(
        "bench thresholds failed: {}",
        report
            .threshold_violations
            .iter()
            .map(|violation| violation.metric.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

#[derive(Default)]
struct ProcessResourceReport {
    idle_sample_ms: usize,
    idle_cpu_usage_percent: f32,
    process_memory_bytes: u64,
    process_virtual_memory_bytes: u64,
    process_accumulated_cpu_ms: u64,
}

fn sample_process_resources(idle_ms: usize) -> ProcessResourceReport {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return ProcessResourceReport {
            idle_sample_ms: idle_ms,
            ..ProcessResourceReport::default()
        };
    };

    let refreshes = RefreshKind::nothing().with_processes(
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .without_tasks(),
    );
    let mut system = System::new_with_specifics(refreshes);
    let _ = system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    thread::sleep(Duration::from_millis(usize_to_u64(idle_ms)));

    let _ = system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    system
        .process(pid)
        .map(|process| ProcessResourceReport {
            idle_sample_ms: idle_ms,
            idle_cpu_usage_percent: process.cpu_usage(),
            process_memory_bytes: process.memory(),
            process_virtual_memory_bytes: process.virtual_memory(),
            process_accumulated_cpu_ms: process.accumulated_cpu_time(),
        })
        .unwrap_or(ProcessResourceReport {
            idle_sample_ms: idle_ms,
            ..ProcessResourceReport::default()
        })
}

struct RenderBenchReport {
    frame_p95_us: u128,
    rendered_pixels: u128,
    pixels_per_sec: u64,
}

fn benchmark_rendering(
    terminal: &rssh_terminal::Terminal,
    render_frames: usize,
    size: TerminalSize,
) -> RenderBenchReport {
    let snapshot = TerminalRenderSnapshot::from_terminal(terminal);
    let renderer = PixelRenderer::new();
    let target_width = u32::from(size.columns).saturating_mul(BENCH_CELL_WIDTH);
    let target_height = u32::from(size.rows).saturating_mul(BENCH_CELL_HEIGHT);
    let buffer_len = usize::try_from(
        u64::from(target_width)
            .saturating_mul(u64::from(target_height))
            .saturating_mul(4),
    )
    .unwrap_or(usize::MAX);
    let mut target = vec![0; buffer_len];
    let mut frame_timings = Vec::with_capacity(render_frames);

    let started = Instant::now();
    for _ in 0..render_frames {
        let frame_started = Instant::now();
        renderer.render(
            &snapshot,
            &mut target,
            target_width,
            target_height,
            BENCH_CELL_WIDTH,
            BENCH_CELL_HEIGHT,
        );
        frame_timings.push(frame_started.elapsed().as_micros());
    }
    let elapsed_nanos = started.elapsed().as_nanos();
    let pixels_per_frame = u128::from(target_width).saturating_mul(u128::from(target_height));
    let rendered_pixels = pixels_per_frame.saturating_mul(usize_to_u128(render_frames));

    RenderBenchReport {
        frame_p95_us: percentile_95(&mut frame_timings),
        rendered_pixels,
        pixels_per_sec: u128_to_u64(
            rendered_pixels
                .saturating_mul(1_000_000_000)
                .checked_div(elapsed_nanos)
                .unwrap_or(u128::from(u64::MAX)),
        ),
    }
}

fn build_benchmark_workload(workload_kind: BenchWorkload, target_bytes: usize) -> Vec<u8> {
    let mut workload = Vec::with_capacity(target_bytes);
    let mut line = 0_u64;

    while workload.len() < target_bytes {
        let record = benchmark_record(workload_kind, line);
        let remaining = target_bytes - workload.len();
        let record_bytes = record.as_bytes();
        workload.extend_from_slice(&record_bytes[..record_bytes.len().min(remaining)]);
        line = line.saturating_add(1);
    }

    workload
}

fn benchmark_record(workload_kind: BenchWorkload, line: u64) -> String {
    const TEXT_SUFFIX: &str = " ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789";
    match workload_kind {
        BenchWorkload::PlainScroll => format!("bench line {line:08}{TEXT_SUFFIX}\r\n"),
        BenchWorkload::AnsiScroll => {
            let color = line % 256;
            format!("\x1b[38;5;{color}mbench line {line:08}{TEXT_SUFFIX}\x1b[0m\r\n")
        }
        BenchWorkload::AnsiScrollQuery => {
            let color = line % 256;
            format!(
                "\x1b[38;5;{color}mbench line {line:08}{TEXT_SUFFIX}\x1b[0m\r\n\
                 \x1b[6n\x1b[18t\x1b]0;R-SSH bench {line}\x07"
            )
        }
    }
}

fn percentile_95(values: &mut [u128]) -> u128 {
    if values.is_empty() {
        return 0;
    }

    values.sort_unstable();
    let index = values
        .len()
        .saturating_mul(95)
        .saturating_add(99)
        .saturating_div(100)
        .saturating_sub(1);
    values[index]
}

fn bytes_per_second(bytes: usize, elapsed_nanos: u128) -> u64 {
    if elapsed_nanos == 0 {
        return u64::MAX;
    }

    let rate = usize_to_u128(bytes)
        .saturating_mul(1_000_000_000)
        .saturating_div(elapsed_nanos);
    u128_to_u64(rate)
}

fn usize_to_u128(value: usize) -> u128 {
    u128::try_from(value).expect("usize fits into u128")
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn u128_to_u64(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

const fn saturating_counter_delta(current: u64, earlier: u64) -> u64 {
    current.saturating_sub(earlier)
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use crate::cli::BenchOptions;

    #[test]
    fn counter_delta_excludes_prior_query_work_and_saturates() {
        assert_eq!(super::saturating_counter_delta(125, 100), 25);
        assert_eq!(super::saturating_counter_delta(35, 40), 0);
    }

    #[test]
    fn builds_exact_sized_benchmark_workload() {
        for workload in [
            crate::cli::BenchWorkload::PlainScroll,
            crate::cli::BenchWorkload::AnsiScroll,
            crate::cli::BenchWorkload::AnsiScrollQuery,
        ] {
            let bytes = super::build_benchmark_workload(workload, 513);
            assert_eq!(bytes.len(), 513);
            assert!(String::from_utf8_lossy(&bytes).contains("bench line"));
        }
    }

    #[test]
    fn ansi_scroll_query_first_record_matches_legacy_payload_byte_for_byte() {
        assert_eq!(
            super::benchmark_record(crate::cli::BenchWorkload::AnsiScrollQuery, 0).as_bytes(),
            b"\x1b[38;5;0mbench line 00000000 ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789\x1b[0m\r\n\x1b[6n\x1b[18t\x1b]0;R-SSH bench 0\x07"
        );
    }

    #[test]
    fn exact_target_size_intentionally_keeps_a_truncated_final_record() {
        let full_record = super::benchmark_record(crate::cli::BenchWorkload::AnsiScrollQuery, 0);
        let target_bytes = full_record.len() - 1;

        let workload = super::build_benchmark_workload(
            crate::cli::BenchWorkload::AnsiScrollQuery,
            target_bytes,
        );

        assert_eq!(workload.len(), target_bytes);
        assert_eq!(workload, full_record.as_bytes()[..target_bytes]);
        assert_ne!(workload.last(), full_record.as_bytes().last());
    }

    #[test]
    fn console_benchmark_report_tracks_terminal_runtime_metrics() {
        let report = super::run_bench(&BenchOptions {
            json: false,
            workload: crate::cli::BenchWorkload::AnsiScrollQuery,
            bytes: 2048,
            chunk_size: 256,
            render_frames: 3,
            idle_ms: 1,
            thresholds: crate::cli::BenchThresholds::default(),
            size: TerminalSize::new(40, 10),
        });

        assert!(report.ok);
        assert_eq!(report.workload, "ansi-scroll-query");
        assert_eq!(report.bytes, 2048);
        assert_eq!(report.chunk_size, 256);
        assert_eq!(report.chunks, 8);
        assert_eq!(report.columns, 40);
        assert_eq!(report.rows, 10);
        assert!(report.throughput_bytes_per_sec > 0);
        assert_eq!(report.render_frames, 3);
        assert!(report.render_frame_p95_us > 0);
        assert!(report.rendered_pixels > 0);
        assert!(report.render_pixels_per_sec > 0);
        assert_eq!(report.idle_sample_ms, 1);
        assert!(report.process_memory_bytes > 0);
        assert!(report.process_virtual_memory_bytes > 0);
        assert!(report.process_accumulated_cpu_ms > 0);
        assert!(report.idle_cpu_usage_percent >= 0.0);
        assert!(report.display_bytes > 0);
        assert!(report.responses > 0);
        assert!(report.inspected_query_bytes > 0);
        assert_eq!(report.scrolled_survivor_cell_clones, 0);
        assert!(report.cursor_row < report.rows);
        assert!(report.cursor_column < report.columns);
    }

    #[test]
    fn benchmark_json_report_is_machine_readable() {
        let report = super::BenchReport {
            ok: true,
            workload: "ansi-scroll-query".to_owned(),
            bytes: 1024,
            chunk_size: 128,
            chunks: 8,
            columns: 80,
            rows: 24,
            elapsed_ms: 12,
            throughput_bytes_per_sec: 85_333,
            chunk_p95_us: 9,
            render_frames: 3,
            render_frame_p95_us: 11,
            rendered_pixels: 737_280,
            render_pixels_per_sec: 61_440_000,
            idle_sample_ms: 200,
            idle_cpu_usage_percent: 1.25,
            process_memory_bytes: 2_097_152,
            process_virtual_memory_bytes: 67_108_864,
            process_accumulated_cpu_ms: 123,
            threshold_violations: Vec::new(),
            display_bytes: 900,
            responses: 4,
            bells: 1,
            scrollback_lines: 2,
            inspected_query_bytes: 10_240,
            scrolled_survivor_cell_clones: 800,
            history_row_relocations: 12,
            metadata_rebase_batches: 3,
            cursor_row: 3,
            cursor_column: 7,
        };

        let json = super::bench_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["workload"], "ansi-scroll-query");
        assert_eq!(value["throughput_bytes_per_sec"], 85_333);
        assert_eq!(value["chunk_p95_us"], 9);
        assert_eq!(value["render_frames"], 3);
        assert_eq!(value["render_frame_p95_us"], 11);
        assert_eq!(value["render_pixels_per_sec"], 61_440_000);
        assert_eq!(value["idle_sample_ms"], 200);
        assert_eq!(value["idle_cpu_usage_percent"], 1.25);
        assert_eq!(value["process_memory_bytes"], 2_097_152);
        assert_eq!(value["process_virtual_memory_bytes"], 67_108_864);
        assert_eq!(value["process_accumulated_cpu_ms"], 123);
        assert_eq!(value["inspected_query_bytes"], 10_240);
        assert_eq!(value["scrolled_survivor_cell_clones"], 800);
        assert_eq!(value["history_row_relocations"], 12);
        assert_eq!(value["metadata_rebase_batches"], 3);
    }

    #[test]
    fn benchmark_text_report_includes_comparison_metrics() {
        let report = super::BenchReport {
            ok: true,
            workload: "ansi-scroll-query".to_owned(),
            bytes: 1024,
            chunk_size: 128,
            chunks: 8,
            columns: 80,
            rows: 24,
            elapsed_ms: 12,
            throughput_bytes_per_sec: 85_333,
            chunk_p95_us: 9,
            render_frames: 3,
            render_frame_p95_us: 11,
            rendered_pixels: 737_280,
            render_pixels_per_sec: 61_440_000,
            idle_sample_ms: 200,
            idle_cpu_usage_percent: 1.25,
            process_memory_bytes: 2_097_152,
            process_virtual_memory_bytes: 67_108_864,
            process_accumulated_cpu_ms: 123,
            threshold_violations: Vec::new(),
            display_bytes: 900,
            responses: 4,
            bells: 1,
            scrollback_lines: 2,
            inspected_query_bytes: 10_240,
            scrolled_survivor_cell_clones: 800,
            history_row_relocations: 12,
            metadata_rebase_batches: 3,
            cursor_row: 3,
            cursor_column: 7,
        };

        let lines = super::bench_text_lines(&report);

        assert!(lines.iter().any(|line| line.contains("ok\tbench")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("throughput_bytes_per_sec=85333"))
        );
        assert!(lines.iter().any(|line| line.contains("chunk_p95_us=9")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("render_pixels_per_sec=61440000"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("idle_cpu_usage_percent=1.25"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("process_memory_bytes=2097152"))
        );
        assert!(lines.iter().any(|line| {
            line.contains("inspected_query_bytes=10240")
                && line.contains("scrolled_survivor_cell_clones=800")
                && line.contains("history_row_relocations=12")
                && line.contains("metadata_rebase_batches=3")
        }));
    }

    #[test]
    fn benchmark_thresholds_mark_report_failed_with_violation_details() {
        let mut report = super::BenchReport {
            ok: true,
            workload: "ansi-scroll-query".to_owned(),
            bytes: 1024,
            chunk_size: 128,
            chunks: 8,
            columns: 80,
            rows: 24,
            elapsed_ms: 12,
            throughput_bytes_per_sec: 85_333,
            chunk_p95_us: 9,
            render_frames: 3,
            render_frame_p95_us: 11,
            rendered_pixels: 737_280,
            render_pixels_per_sec: 61_440_000,
            idle_sample_ms: 200,
            idle_cpu_usage_percent: 1.25,
            process_memory_bytes: 2_097_152,
            process_virtual_memory_bytes: 67_108_864,
            process_accumulated_cpu_ms: 123,
            threshold_violations: Vec::new(),
            display_bytes: 900,
            responses: 4,
            bells: 1,
            scrollback_lines: 2,
            inspected_query_bytes: 10_240,
            scrolled_survivor_cell_clones: 800,
            history_row_relocations: 12,
            metadata_rebase_batches: 3,
            cursor_row: 3,
            cursor_column: 7,
        };
        let thresholds = crate::cli::BenchThresholds {
            min_throughput_bytes_per_sec: Some(100_000),
            max_chunk_p95_us: Some(8),
            max_render_frame_p95_us: Some(10),
            max_idle_cpu_percent: Some(1),
            max_process_memory_bytes: Some(1_048_576),
        };

        super::apply_bench_thresholds(&mut report, &thresholds);

        assert!(!report.ok);
        assert_eq!(report.threshold_violations.len(), 5);
        assert!(
            report
                .threshold_violations
                .iter()
                .any(|violation| violation.metric == "throughput_bytes_per_sec")
        );
        assert!(
            report
                .threshold_violations
                .iter()
                .any(|violation| violation.metric == "process_memory_bytes")
        );

        let lines = super::bench_text_lines(&report);
        assert!(lines.iter().any(|line| line.contains("fail\tthreshold")));

        let json = super::bench_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["threshold_violations"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn workload_reports_linear_scanner_and_scroll_costs() {
        let options = |workload| BenchOptions {
            json: false,
            workload,
            bytes: 4096,
            chunk_size: 256,
            render_frames: 1,
            idle_ms: 1,
            thresholds: crate::cli::BenchThresholds::default(),
            size: TerminalSize::new(20, 4),
        };

        let plain = super::run_bench(&options(crate::cli::BenchWorkload::PlainScroll));
        let ansi = super::run_bench(&options(crate::cli::BenchWorkload::AnsiScroll));
        let query = super::run_bench(&options(crate::cli::BenchWorkload::AnsiScrollQuery));

        for (report, expected_name) in [
            (&plain, "plain-scroll"),
            (&ansi, "ansi-scroll"),
            (&query, "ansi-scroll-query"),
        ] {
            let json = super::bench_json(report).expect("serialize benchmark report");
            let decoded: super::BenchReport =
                serde_json::from_str(&json).expect("deserialize benchmark report");
            assert_eq!(decoded.workload, expected_name);
            assert_eq!(&decoded, report);
        }

        assert_eq!(plain.inspected_query_bytes, 0);
        assert_eq!(plain.scrolled_survivor_cell_clones, 0);
        assert!(ansi.inspected_query_bytes > 0);
        assert!(query.inspected_query_bytes > 0);
        assert!(ansi.inspected_query_bytes >= ansi.bytes as u64);
        assert!(ansi.inspected_query_bytes <= ansi.bytes as u64 * 4);
        assert!(query.inspected_query_bytes >= query.bytes as u64);
        assert!(query.inspected_query_bytes <= query.bytes as u64 * 4);
        assert!(query.inspected_query_bytes > ansi.inspected_query_bytes);
        assert!(query.responses > 0);
        assert_eq!(plain.responses, 0);
        assert_eq!(ansi.responses, 0);
    }
}
