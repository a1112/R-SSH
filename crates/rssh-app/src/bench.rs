use std::{error::Error, time::Instant};

use rssh_core::TerminalSize;
use serde::Serialize;

use crate::{cli::BenchOptions, terminal_runtime::TerminalRuntime};

const WORKLOAD_NAME: &str = "ansi-scroll-query";

#[derive(Debug, PartialEq, Eq, Serialize)]
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
    pub display_bytes: usize,
    pub responses: usize,
    pub bells: u64,
    pub scrollback_lines: usize,
    pub cursor_row: u16,
    pub cursor_column: u16,
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

    Ok(())
}

pub fn run_bench(options: &BenchOptions) -> BenchReport {
    let workload = build_benchmark_workload(options.bytes);
    run_benchmark_workload(&workload, options.chunk_size, options.size)
}

pub fn bench_json(report: &BenchReport) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(report)?)
}

pub fn bench_text_lines(report: &BenchReport) -> Vec<String> {
    vec![
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
            "metric\tdisplay_bytes={}\tresponses={}\tbells={}\tscrollback_lines={}",
            report.display_bytes, report.responses, report.bells, report.scrollback_lines
        ),
    ]
}

fn run_benchmark_workload(workload: &[u8], chunk_size: usize, size: TerminalSize) -> BenchReport {
    let mut runtime = TerminalRuntime::new(size);
    let mut chunk_timings = Vec::new();
    let mut responses = 0_usize;
    let mut display_bytes = 0_usize;
    let mut bells = 0_u64;

    let started = Instant::now();
    for chunk in workload.chunks(chunk_size) {
        let chunk_started = Instant::now();
        let output = runtime.feed_pty_output_with_display(chunk);
        chunk_timings.push(chunk_started.elapsed().as_micros());
        responses = responses.saturating_add(output.responses.len());
        display_bytes = display_bytes.saturating_add(output.display.len());
        bells = bells.saturating_add(output.bells);
    }
    let elapsed = started.elapsed();
    let (cursor_row, cursor_column) = runtime.terminal().cursor();

    BenchReport {
        ok: true,
        workload: WORKLOAD_NAME.to_owned(),
        bytes: workload.len(),
        chunk_size,
        chunks: chunk_timings.len(),
        columns: size.columns,
        rows: size.rows,
        elapsed_ms: elapsed.as_millis(),
        throughput_bytes_per_sec: bytes_per_second(workload.len(), elapsed.as_nanos()),
        chunk_p95_us: percentile_95(&mut chunk_timings),
        display_bytes,
        responses,
        bells,
        scrollback_lines: runtime.terminal().scrollback().len(),
        cursor_row,
        cursor_column,
    }
}

fn build_benchmark_workload(target_bytes: usize) -> Vec<u8> {
    let mut workload = Vec::with_capacity(target_bytes);
    let mut line = 0_u64;

    while workload.len() < target_bytes {
        let color = line % 256;
        let record = format!(
            "\x1b[38;5;{color}mbench line {line:08} ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789\x1b[0m\r\n\
             \x1b[6n\x1b[18t\x1b]0;R-SSH bench {line}\x07"
        );
        let remaining = target_bytes - workload.len();
        let record_bytes = record.as_bytes();
        workload.extend_from_slice(&record_bytes[..record_bytes.len().min(remaining)]);
        line = line.saturating_add(1);
    }

    workload
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
    u64::try_from(rate).unwrap_or(u64::MAX)
}

fn usize_to_u128(value: usize) -> u128 {
    u128::try_from(value).expect("usize fits into u128")
}

#[cfg(test)]
mod tests {
    use rssh_core::TerminalSize;

    use crate::cli::BenchOptions;

    #[test]
    fn builds_exact_sized_benchmark_workload() {
        let workload = super::build_benchmark_workload(513);

        assert_eq!(workload.len(), 513);
        assert!(String::from_utf8_lossy(&workload).contains("bench line"));
    }

    #[test]
    fn console_benchmark_report_tracks_terminal_runtime_metrics() {
        let report = super::run_bench(&BenchOptions {
            json: false,
            bytes: 2048,
            chunk_size: 256,
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
        assert!(report.display_bytes > 0);
        assert!(report.responses > 0);
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
            display_bytes: 900,
            responses: 4,
            bells: 1,
            scrollback_lines: 2,
            cursor_row: 3,
            cursor_column: 7,
        };

        let json = super::bench_json(&report).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ok"], true);
        assert_eq!(value["workload"], "ansi-scroll-query");
        assert_eq!(value["throughput_bytes_per_sec"], 85_333);
        assert_eq!(value["chunk_p95_us"], 9);
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
            display_bytes: 900,
            responses: 4,
            bells: 1,
            scrollback_lines: 2,
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
    }
}
