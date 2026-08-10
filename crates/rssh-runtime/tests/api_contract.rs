use std::{
    fs,
    io::{self, Cursor, Read, Write},
    num::NonZeroU64,
    path::Path,
    sync::Arc,
    time::Duration,
};

use rssh_core::{DamageRegion, PaneId, TerminalSize};
use rssh_runtime::{
    EffectSequence, EffectSequenceCursor, EffectSequenceError, MetadataChange, PaneGeneration,
    PaneMetadataDelta, PaneToken, PaneTokenAllocator, RuntimeBatch, RuntimeBatchMetrics,
    RuntimeEffect, RuntimeEffectKind, RuntimeProgress, RuntimeRevision, SequenceKind,
    SessionControl, SessionExit, SessionParts, SessionTransport, SubmitResult, UserVarDelta,
};

#[derive(Debug, PartialEq, Eq)]
struct NeutralSnapshot {
    text: String,
}

fn effect_batch(
    pane: PaneToken,
    revision: RuntimeRevision,
    sequences: &[EffectSequence],
) -> RuntimeBatch<NeutralSnapshot> {
    RuntimeBatch {
        pane,
        revision,
        snapshot: None,
        damage: Vec::new(),
        metadata: PaneMetadataDelta::default(),
        effects: sequences
            .iter()
            .copied()
            .map(|sequence| {
                RuntimeEffect::new(
                    sequence,
                    RuntimeEffectKind::Bell {
                        count: NonZeroU64::MIN,
                    },
                )
            })
            .collect(),
        metrics: RuntimeBatchMetrics::default(),
    }
}

#[test]
fn pane_generations_are_nonzero_global_and_exhaust_explicitly() {
    let mut allocator = PaneTokenAllocator::new();
    let first = allocator.issue(PaneId::new(41)).expect("first token");
    let second = allocator.issue(PaneId::new(7)).expect("second token");

    assert_eq!(first.pane(), PaneId::new(41));
    assert_eq!(first.generation().get(), 1);
    assert_eq!(second.pane(), PaneId::new(7));
    assert_eq!(second.generation().get(), 2);

    let mut final_allocator =
        PaneTokenAllocator::from_next_generation(PaneGeneration::from_non_zero(NonZeroU64::MAX));
    let final_token = final_allocator
        .issue(PaneId::new(99))
        .expect("the maximum generation remains issuable");
    assert_eq!(final_token.generation(), PaneGeneration::MAX);

    let exhausted = final_allocator
        .issue(PaneId::new(100))
        .expect_err("generation must never wrap or be reused");
    assert_eq!(exhausted.kind(), SequenceKind::PaneGeneration);
}

#[test]
fn revisions_and_effect_sequences_are_strictly_monotonic() {
    let first_revision = RuntimeRevision::FIRST;
    let second_revision = first_revision.next().expect("next revision");
    assert_eq!(first_revision.get(), 1);
    assert_eq!(second_revision.get(), 2);
    assert_eq!(
        RuntimeRevision::MAX
            .next()
            .expect_err("revision must not wrap")
            .kind(),
        SequenceKind::RuntimeRevision
    );

    let first_effect = EffectSequence::FIRST;
    let second_effect = first_effect.next().expect("next effect sequence");
    assert_eq!(first_effect.get(), 1);
    assert_eq!(second_effect.get(), 2);
    assert_eq!(
        EffectSequence::MAX
            .next()
            .expect_err("effect sequence must not wrap")
            .kind(),
        SequenceKind::EffectSequence
    );
}

#[test]
fn effect_sequence_cursor_retains_continuity_across_runtime_batches() {
    let mut allocator = PaneTokenAllocator::new();
    let pane = allocator.issue(PaneId::new(5)).expect("pane token");
    let first = EffectSequence::FIRST;
    let second = first.next().expect("second sequence");
    let third = second.next().expect("third sequence");
    let fourth = third.next().expect("fourth sequence");
    let first_batch = effect_batch(pane, RuntimeRevision::FIRST, &[first, second]);
    let second_batch = effect_batch(
        pane,
        RuntimeRevision::FIRST.next().expect("second revision"),
        &[third],
    );
    let mut cursor = EffectSequenceCursor::default();

    cursor
        .validate_batch(&first_batch)
        .expect("first batch is contiguous");
    assert_eq!(cursor.expected(), Some(third));
    cursor
        .validate_batch(&second_batch)
        .expect("second batch continues the same stream");
    assert_eq!(cursor.expected(), Some(fourth));
}

#[test]
fn effect_sequence_cursor_reports_cross_batch_gaps() {
    let mut allocator = PaneTokenAllocator::new();
    let pane = allocator.issue(PaneId::new(6)).expect("pane token");
    let first = EffectSequence::FIRST;
    let expected = first.next().expect("second sequence");
    let observed = expected.next().expect("third sequence");
    let first_batch = effect_batch(pane, RuntimeRevision::FIRST, &[first]);
    let gap_batch = effect_batch(
        pane,
        RuntimeRevision::FIRST.next().expect("second revision"),
        &[observed],
    );
    let mut cursor = EffectSequenceCursor::default();

    cursor
        .validate_batch(&first_batch)
        .expect("initial effect is contiguous");
    assert_eq!(
        cursor
            .validate_batch(&gap_batch)
            .expect_err("a missing cross-batch effect must be rejected"),
        EffectSequenceError::Gap { expected, observed }
    );
    assert_eq!(cursor.expected(), Some(expected));
}

#[test]
fn effect_sequence_cursor_reports_duplicates_and_out_of_order_effects() {
    let mut allocator = PaneTokenAllocator::new();
    let pane = allocator.issue(PaneId::new(7)).expect("pane token");
    let first = EffectSequence::FIRST;
    let second = first.next().expect("second sequence");
    let expected = second.next().expect("third sequence");
    let accepted_batch = effect_batch(pane, RuntimeRevision::FIRST, &[first, second]);
    let duplicate_batch = effect_batch(
        pane,
        RuntimeRevision::FIRST.next().expect("second revision"),
        &[second],
    );
    let out_of_order_batch = effect_batch(
        pane,
        RuntimeRevision::FIRST.next().expect("second revision"),
        &[first],
    );
    let mut cursor = EffectSequenceCursor::default();

    cursor
        .validate_batch(&accepted_batch)
        .expect("initial effects are contiguous");
    assert_eq!(
        cursor
            .validate_batch(&duplicate_batch)
            .expect_err("a duplicate must be rejected"),
        EffectSequenceError::DuplicateOrOutOfOrder {
            expected,
            observed: second,
        }
    );
    assert_eq!(
        cursor
            .validate_batch(&out_of_order_batch)
            .expect_err("an out-of-order effect must be rejected"),
        EffectSequenceError::DuplicateOrOutOfOrder {
            expected,
            observed: first,
        }
    );
    assert_eq!(cursor.expected(), Some(expected));
}

#[test]
fn effect_sequence_cursor_exhausts_after_accepting_max_without_wrapping() {
    let mut allocator = PaneTokenAllocator::new();
    let pane = allocator.issue(PaneId::new(8)).expect("pane token");
    let max_batch = effect_batch(pane, RuntimeRevision::FIRST, &[EffectSequence::MAX]);
    let after_max_batch = effect_batch(
        pane,
        RuntimeRevision::FIRST.next().expect("second revision"),
        &[EffectSequence::MAX],
    );
    let mut cursor = EffectSequenceCursor::new(EffectSequence::MAX);

    cursor
        .validate_batch(&max_batch)
        .expect("the maximum sequence remains valid once");
    assert_eq!(cursor.expected(), None);
    assert_eq!(
        cursor
            .validate_batch(&after_max_batch)
            .expect_err("an exhausted cursor must reject every later effect"),
        EffectSequenceError::Exhausted {
            observed: EffectSequence::MAX,
        }
    );
    assert_eq!(cursor.expected(), None);
}

#[test]
fn submit_result_preserves_explicit_retry_hint() {
    let retry_after = Duration::from_millis(17);
    let result = SubmitResult::Backpressured { retry_after };

    assert_eq!(
        result,
        SubmitResult::Backpressured {
            retry_after: Duration::from_millis(17)
        }
    );
    assert_ne!(result, SubmitResult::Accepted);
    assert_ne!(result, SubmitResult::Closed);
}

#[test]
fn runtime_progress_preserves_every_neutral_state() {
    assert_eq!(RuntimeProgress::default(), RuntimeProgress::None);
    assert_eq!(
        RuntimeProgress::Percentage(73),
        RuntimeProgress::Percentage(73)
    );
    assert_eq!(RuntimeProgress::Error(19), RuntimeProgress::Error(19));
    assert_eq!(
        RuntimeProgress::Indeterminate,
        RuntimeProgress::Indeterminate
    );
}

#[test]
fn neutral_effect_and_exit_payloads_are_lossless() {
    let clipboard_write = RuntimeEffectKind::ClipboardWrite {
        contents: "copied text".to_owned(),
    };
    let clipboard_read = RuntimeEffectKind::ClipboardRead {
        selection: "c".to_owned(),
    };
    let diagnostic = RuntimeEffectKind::Diagnostic {
        message: "writer closed".to_owned(),
    };
    let untitled_notification = RuntimeEffectKind::Notification {
        title: None,
        body: "plain body".to_owned(),
    };

    assert!(matches!(
        clipboard_write,
        RuntimeEffectKind::ClipboardWrite { contents } if contents == "copied text"
    ));
    assert!(matches!(
        clipboard_read,
        RuntimeEffectKind::ClipboardRead { selection } if selection == "c"
    ));
    assert!(matches!(
        diagnostic,
        RuntimeEffectKind::Diagnostic { message } if message == "writer closed"
    ));
    assert!(matches!(
        untitled_notification,
        RuntimeEffectKind::Notification { title: None, body } if body == "plain body"
    ));
    assert_eq!(
        SessionExit::Signaled {
            signal: "TERM".to_owned()
        },
        SessionExit::Signaled {
            signal: "TERM".to_owned()
        }
    );
    assert_eq!(SessionExit::Unknown, SessionExit::Unknown);
}

#[test]
fn runtime_batches_carry_snapshots_damage_metadata_metrics_and_ordered_effects() {
    let mut allocator = PaneTokenAllocator::new();
    let pane = allocator.issue(PaneId::new(3)).expect("pane token");
    let first = EffectSequence::FIRST;
    let second = first.next().expect("second effect sequence");
    let third = second.next().expect("third effect sequence");

    let batch_one = RuntimeBatch {
        pane,
        revision: RuntimeRevision::FIRST,
        snapshot: Some(Arc::new(NeutralSnapshot {
            text: "ready".to_owned(),
        })),
        damage: vec![DamageRegion::new(1, 2, 3, 4)],
        metadata: PaneMetadataDelta {
            title: Some(MetadataChange::Set("shell".to_owned())),
            working_directory: Some(MetadataChange::Clear),
            badge_format: Some(MetadataChange::Set("production".to_owned())),
            progress: Some(MetadataChange::Set(RuntimeProgress::Percentage(42))),
            user_vars: vec![UserVarDelta {
                name: "profile".to_owned(),
                value: MetadataChange::Set("dev".to_owned()),
            }],
        },
        effects: vec![
            RuntimeEffect::new(first, RuntimeEffectKind::TransportWrite(vec![1, 2])),
            RuntimeEffect::new(
                second,
                RuntimeEffectKind::Bell {
                    count: NonZeroU64::new(3).expect("nonzero bell count"),
                },
            ),
        ],
        metrics: RuntimeBatchMetrics {
            transport_bytes: 32,
            coalesced_reads: 2,
            parse_duration: Duration::from_micros(40),
            snapshot_duration: Duration::from_micros(5),
        },
    };
    let batch_two = RuntimeBatch::<NeutralSnapshot> {
        pane,
        revision: RuntimeRevision::FIRST.next().expect("second revision"),
        snapshot: None,
        damage: Vec::new(),
        metadata: PaneMetadataDelta::default(),
        effects: vec![RuntimeEffect::new(
            third,
            RuntimeEffectKind::Notification {
                title: Some("build".to_owned()),
                body: "complete".to_owned(),
            },
        )],
        metrics: RuntimeBatchMetrics::default(),
    };

    assert_eq!(batch_one.pane, pane);
    assert_eq!(
        batch_one.snapshot.as_deref().map(|item| item.text.as_str()),
        Some("ready")
    );
    assert_eq!(batch_one.damage, vec![DamageRegion::new(1, 2, 3, 4)]);
    assert_eq!(batch_one.metadata.user_vars[0].name, "profile");
    assert_eq!(
        batch_one.metadata.badge_format,
        Some(MetadataChange::Set("production".to_owned()))
    );
    assert_eq!(
        batch_one.metadata.progress,
        Some(MetadataChange::Set(RuntimeProgress::Percentage(42)))
    );
    assert_eq!(batch_one.metrics.transport_bytes, 32);

    let effects = batch_one
        .effects
        .iter()
        .chain(&batch_two.effects)
        .collect::<Vec<_>>();
    assert_eq!(
        effects
            .iter()
            .map(|effect| effect.sequence())
            .collect::<Vec<_>>(),
        vec![first, second, third]
    );
    assert!(matches!(
        effects.iter().map(|effect| effect.kind()).collect::<Vec<_>>().as_slice(),
        [
            RuntimeEffectKind::TransportWrite(bytes),
            RuntimeEffectKind::Bell { count },
            RuntimeEffectKind::Notification { title, body }
        ] if bytes == &[1, 2]
            && count.get() == 3
            && title.as_deref() == Some("build")
            && body == "complete"
    ));

    for pair in effects.windows(2) {
        assert_eq!(
            pair[0].sequence().next().expect("sequence successor"),
            pair[1].sequence(),
            "a lost or duplicated effect must be detectable across batch boundaries"
        );
    }
}

#[derive(Debug, Default)]
struct RecordingControl {
    resized_to: Option<TerminalSize>,
    close_started: bool,
    exit: Option<SessionExit>,
}

impl SessionControl for RecordingControl {
    fn resize(&mut self, size: TerminalSize) -> io::Result<()> {
        self.resized_to = Some(size);
        Ok(())
    }

    fn poll_exit(&mut self) -> io::Result<Option<SessionExit>> {
        Ok(self.exit.clone())
    }

    fn begin_close(&mut self) -> io::Result<()> {
        self.close_started = true;
        Ok(())
    }
}

struct MemoryTransport {
    reader: Cursor<Vec<u8>>,
    writer: Cursor<Vec<u8>>,
    control: RecordingControl,
}

impl SessionTransport for MemoryTransport {
    type Reader = Cursor<Vec<u8>>;
    type Writer = Cursor<Vec<u8>>;
    type Control = RecordingControl;

    fn split(self) -> SessionParts<Self::Reader, Self::Writer, Self::Control> {
        SessionParts::new(self.reader, self.writer, self.control)
    }
}

#[test]
fn session_transport_uses_only_standard_io_and_neutral_control_values() -> io::Result<()> {
    let transport = MemoryTransport {
        reader: Cursor::new(b"terminal output".to_vec()),
        writer: Cursor::new(Vec::new()),
        control: RecordingControl {
            exit: Some(SessionExit::Exited { code: 23 }),
            ..RecordingControl::default()
        },
    };
    let SessionParts {
        mut reader,
        mut writer,
        mut control,
    } = transport.split();

    let mut output = String::new();
    reader.read_to_string(&mut output)?;
    writer.write_all(b"input")?;
    control.resize(TerminalSize::new(120, 40))?;
    assert_eq!(control.poll_exit()?, Some(SessionExit::Exited { code: 23 }));
    control.begin_close()?;

    assert_eq!(output, "terminal output");
    assert_eq!(writer.into_inner(), b"input");
    assert_eq!(control.resized_to, Some(TerminalSize::new(120, 40)));
    assert!(control.close_started);
    Ok(())
}

#[test]
fn public_runtime_values_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<PaneGeneration>();
    assert_send_sync::<RuntimeRevision>();
    assert_send_sync::<EffectSequence>();
    assert_send_sync::<EffectSequenceCursor>();
    assert_send_sync::<EffectSequenceError>();
    assert_send_sync::<PaneTokenAllocator>();
    assert_send_sync::<RuntimeEffect>();
    assert_send_sync::<PaneMetadataDelta>();
    assert_send_sync::<RuntimeBatch<NeutralSnapshot>>();
    assert_send_sync::<RuntimeBatchMetrics>();
    assert_send_sync::<SubmitResult>();
    assert_send_sync::<SessionExit>();
    assert_send_sync::<SessionParts<Cursor<Vec<u8>>, Cursor<Vec<u8>>, RecordingControl>>();
}

#[test]
fn crate_manifest_and_public_source_are_transport_and_platform_neutral() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("read runtime manifest");
    for forbidden in [
        "rssh-app",
        "rssh-native",
        "rssh-renderer",
        "rssh-pty",
        "rssh-ssh",
        "winit",
        "wgpu",
        "raw-window-handle",
        "notify",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "runtime manifest must not depend on {forbidden}"
        );
    }

    let source = fs::read_dir(root.join("src"))
        .expect("read runtime source directory")
        .map(|entry| {
            let path = entry.expect("source entry").path();
            fs::read_to_string(path).expect("read runtime source")
        })
        .collect::<String>();
    for forbidden in [
        "winit::",
        "wgpu::",
        "raw_window_handle",
        "app_shell",
        "std::sync::mpsc",
        "crossbeam_channel",
        "Sender<",
        "Receiver<",
    ] {
        assert!(
            !source.contains(forbidden),
            "runtime public source must not contain {forbidden}"
        );
    }
}
