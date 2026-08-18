use std::sync::Arc;

use rterm_fonts::FontConfig;
use rterm_render_core::TerminalRenderSnapshot;
use rterm_render_cpu::PixelRenderer;
use rterm_render_wgpu::gpu::{GpuContextOptions, RgbaFrameLayout};
use rterm_runtime::{
    PaneMetadataDelta, PaneTokenAllocator, RuntimeBatch, RuntimeBatchMetrics, RuntimeRevision,
    TerminalRuntime,
};
use rterm_terminal::Terminal;
use rterm_types::{DamageRegion, PaneId, TerminalSize};

fn main() {
    let size = TerminalSize::new(10, 2);
    let damage = DamageRegion::new(0, 0, size.columns, size.rows);

    let mut terminal = Terminal::new(size);
    terminal.feed(b"R-Term 0.1");
    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);

    let mut runtime = TerminalRuntime::new(size);
    let responses = runtime.feed_pty_output(b"runtime probe");
    let runtime_snapshot = Arc::new(TerminalRenderSnapshot::from_terminal(runtime.terminal()));
    let token = PaneTokenAllocator::new()
        .issue(PaneId::new(1))
        .expect("consumer probe pane token");
    let batch = RuntimeBatch {
        pane: token,
        revision: RuntimeRevision::FIRST,
        snapshot: Some(runtime_snapshot),
        damage: vec![damage],
        metadata: PaneMetadataDelta::default(),
        effects: Vec::new(),
        metrics: RuntimeBatchMetrics::default(),
    };

    let font = FontConfig::new("R-Term Contract");
    let mut pixels = vec![0; 80 * 32 * 4];
    PixelRenderer::new().render(&snapshot, &mut pixels, 80, 32, 8, 16);

    let gpu_options = GpuContextOptions::default();
    let gpu_layout = RgbaFrameLayout::new(80, 32, 16_384, 1 << 20)
        .expect("bounded RGBA layout must be accepted");

    println!(
        "{}:{}:{}:{}:{}:{}",
        batch.pane.pane().get(),
        batch.damage.len(),
        responses.len(),
        pixels.len(),
        gpu_layout.byte_len,
        std::mem::size_of_val(&gpu_options) + std::mem::size_of_val(&font),
    );
}
