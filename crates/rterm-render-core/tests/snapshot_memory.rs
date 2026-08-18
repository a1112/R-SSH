use std::sync::Arc;

use rssh_terminal::Terminal;
use rterm_render_core::TerminalRenderSnapshot;
use rterm_types::TerminalSize;

#[test]
fn repeated_cells_share_grapheme_style_and_hyperlink_allocations() {
    let mut terminal = Terminal::new(TerminalSize::new(8, 1));
    terminal.feed(b"\x1b]8;;https://example.invalid/shared\x1b\\aaaa\x1b]8;;\x1b\\");

    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let cells = snapshot.cells();
    assert_eq!(cells.len(), 4);

    for cell in &cells[1..] {
        assert!(
            Arc::ptr_eq(cells[0].grapheme(), cell.grapheme()),
            "repeated graphemes must use one immutable allocation"
        );
        assert!(
            Arc::ptr_eq(cells[0].style(), cell.style()),
            "identical visual styles must use one immutable allocation"
        );
        assert!(
            Arc::ptr_eq(
                cells[0].hyperlink().expect("first hyperlink"),
                cell.hyperlink().expect("repeated hyperlink"),
            ),
            "terminal-owned hyperlink identity must survive snapshot projection"
        );
    }
}

#[test]
fn repeated_inline_images_share_immutable_payload_allocations() {
    let mut terminal = Terminal::new(TerminalSize::new(4, 2));
    terminal.feed(b"\x1b]1337;File=inline=1:YWJjZA==\x07");
    terminal.feed(b"\r\n\x1b]1337;File=inline=1:YWJjZA==\x07");

    let snapshot = TerminalRenderSnapshot::from_terminal(&terminal);
    let images = snapshot.inline_images();
    assert_eq!(images.len(), 2);
    assert!(Arc::ptr_eq(images[0].payload(), images[1].payload()));

    let _: Arc<[u8]> = images[0].data.clone();
}

#[test]
fn renderer_core_manifest_stays_platform_and_decoder_free() {
    let manifest = std::fs::read_to_string(format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR")))
        .expect("read renderer-core manifest");

    for forbidden in [
        "wgpu",
        "winit",
        "raw-window-handle",
        "image =",
        "rssh-app",
        "rssh-ssh",
        "rssh-pty",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "renderer-core manifest must not contain {forbidden}"
        );
    }
}
