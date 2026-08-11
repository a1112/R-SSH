use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use rssh_core::{PaneId, TerminalSize};
use rssh_runtime::testing::{ExitAction, ReadAction, ScriptedTransport, VirtualClock, WriteAction};
use rssh_runtime::{
    BatchPolicy, MailboxLimits, PaneNotice, PaneWorkerConfig, RuntimeEffectKind, RuntimeHub,
    SubmitResult, TerminalRuntime, TryRecvError,
};

fn scripted_session(
    reads: impl IntoIterator<Item = ReadAction>,
    writes: impl IntoIterator<Item = WriteAction>,
) -> (
    ScriptedTransport,
    rssh_runtime::testing::ScriptedSessionDriver,
) {
    ScriptedTransport::new(reads, writes, [ExitAction::Pending])
}

#[test]
fn pane_notices_invoke_the_host_waker_without_idle_polling() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let wake_counter = Arc::clone(&wakes);
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new_with_notice_waker(
        clock,
        Arc::new(move || {
            wake_counter.fetch_add(1, Ordering::AcqRel);
        }),
    );
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX)],
    );
    let handle = hub
        .open(PaneId::new(6), transport, PaneWorkerConfig::default())
        .expect("open pane");
    let token = handle.token();

    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));
    assert_eq!(wakes.load(Ordering::Acquire), 1);
    driver.push_read(ReadAction::bytes(b"wake"));
    assert_eq!(hub.recv_notice().expect("output"), PaneNotice::Wake(token));
    assert_eq!(wakes.load(Ordering::Acquire), 2);
    hub.shutdown();
}

#[test]
fn worker_is_ready_and_serializes_user_input_terminal_replies_and_resize() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX); 4],
    );
    let handle = hub
        .open(PaneId::new(7), transport, PaneWorkerConfig::default())
        .expect("open pane");
    let token = handle.token();

    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));
    assert_eq!(
        handle.submit_input(b"user".to_vec()),
        SubmitResult::Accepted
    );
    driver.wait_until_accepted_write_len(4);
    driver.push_read(ReadAction::bytes(b"\x1b[5n"));
    driver.wait_until_accepted_write_len(8);
    assert_eq!(driver.accepted_writes(), b"user\x1b[0n");
    assert_eq!(
        hub.recv_notice().expect("output wake"),
        PaneNotice::Wake(token)
    );
    let output = hub.drain_pane(token, usize::MAX).expect("output drain");
    assert!(!output.frame.expect("output frame").snapshot_changed);

    let size = TerminalSize::new(132, 43);
    assert_eq!(handle.resize(size), SubmitResult::Accepted);
    driver.wait_until_control_call_count(1);
    assert_eq!(driver.control_log().resizes, vec![size]);
    assert_eq!(
        hub.recv_notice().expect("resize wake"),
        PaneNotice::Wake(token)
    );
    let resize = hub.drain_pane(token, usize::MAX).expect("resize drain");
    assert!(resize.frame.expect("resize frame").snapshot_changed);

    assert!(hub.begin_close(token, Duration::from_secs(1)));
    let closed = hub.recv_notice().expect("closed notice");
    assert!(
        matches!(
            closed,
            PaneNotice::Closed { pane, .. } if pane == token
        ),
        "unexpected close notice: {closed:?}"
    );
    hub.shutdown();
    assert_eq!(hub.live_thread_count(), 0);
}

#[test]
fn worker_starts_from_the_configured_terminal_runtime() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX)],
    );
    let config = PaneWorkerConfig::default();
    let mut runtime = TerminalRuntime::new(config.size);
    runtime.set_enq_answerback("configured-worker");
    let handle = hub
        .open_with_runtime(PaneId::new(8), transport, config, runtime)
        .expect("open configured pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    driver.push_read(ReadAction::bytes(b"\x05"));
    driver.wait_until_accepted_write_len("configured-worker".len());
    assert_eq!(driver.accepted_writes(), b"configured-worker");
    hub.shutdown();
}

#[test]
fn explicitly_enabled_host_stream_is_published_losslessly_and_in_order() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX)],
    );
    let config = PaneWorkerConfig {
        capture_host_stream: true,
        ..PaneWorkerConfig::default()
    };
    let handle = hub
        .open(PaneId::new(9), transport, config)
        .expect("open capture pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    driver.push_read(ReadAction::bytes(b"\x1b[?1hhost-stream-marker"));
    assert_eq!(hub.recv_notice().expect("wake"), PaneNotice::Wake(token));
    let drain = hub.drain_pane(token, usize::MAX).expect("drain");
    let streams = drain
        .effects
        .iter()
        .filter_map(|effect| match effect.effect.kind() {
            RuntimeEffectKind::HostStream(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(drain.effects.iter().any(|effect| matches!(
        effect.effect.kind(),
        RuntimeEffectKind::ModeChange(rssh_runtime::TerminalModeChange::ApplicationCursorKeys(
            true
        ))
    )));

    assert_eq!(streams, b"\x1b[?1hhost-stream-marker");
    hub.shutdown();
}

#[test]
fn explicitly_enabled_visible_output_is_published_without_terminal_controls() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX)],
    );
    let config = PaneWorkerConfig {
        capture_visible_output: true,
        ..PaneWorkerConfig::default()
    };
    let handle = hub
        .open(PaneId::new(10), transport, config)
        .expect("open visible-output pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    driver.push_read(ReadAction::bytes(b"A\x1b]0;hidden-title\x07B"));
    assert_eq!(hub.recv_notice().expect("wake"), PaneNotice::Wake(token));
    let drain = hub.drain_pane(token, usize::MAX).expect("drain");
    let visible = drain
        .effects
        .iter()
        .filter_map(|effect| match effect.effect.kind() {
            RuntimeEffectKind::VisibleOutput(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();

    assert_eq!(visible, b"AB");
    assert!(
        !drain
            .effects
            .iter()
            .any(|effect| matches!(effect.effect.kind(), RuntimeEffectKind::HostStream(_)))
    );
    hub.shutdown();
}

#[test]
fn restart_issues_a_new_generation_rejects_old_handle_and_filters_stale_notices() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (first_transport, _) =
        scripted_session([ReadAction::Block], [WriteAction::accept(usize::MAX); 2]);
    let first = hub
        .open(
            PaneId::new(11),
            first_transport,
            PaneWorkerConfig::default(),
        )
        .expect("first generation");
    assert_eq!(
        hub.recv_notice().expect("first ready"),
        PaneNotice::Ready(first.token())
    );

    let (second_transport, _) =
        scripted_session([ReadAction::Block], [WriteAction::accept(usize::MAX); 2]);
    let second = hub
        .restart(
            PaneId::new(11),
            second_transport,
            PaneWorkerConfig::default(),
        )
        .expect("replacement generation");

    assert_ne!(first.token().generation(), second.token().generation());
    assert_eq!(first.submit_input(b"stale".to_vec()), SubmitResult::Closed);
    assert_eq!(
        hub.recv_notice().expect("replacement ready"),
        PaneNotice::Ready(second.token())
    );
    hub.shutdown();
    assert_eq!(hub.live_thread_count(), 0);
}

#[test]
fn expired_close_interrupts_blocked_writer_and_hands_join_to_reaper() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock.clone());
    let (transport, driver) = scripted_session([ReadAction::Block], [WriteAction::Block]);
    let handle = hub
        .open(PaneId::new(19), transport, PaneWorkerConfig::default())
        .expect("open blocked pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));
    assert_eq!(
        handle.submit_input(b"blocked".to_vec()),
        SubmitResult::Accepted
    );
    driver.wait_until_writer_blocked();

    assert!(hub.begin_close(token, Duration::from_millis(10)));
    clock.advance(Duration::from_millis(9)).expect("advance");
    assert_eq!(hub.reap_expired(), 0);
    clock.advance(Duration::from_millis(1)).expect("deadline");
    assert_eq!(hub.reap_expired(), 1);
    assert!(driver.interrupt_calls() >= 1);

    hub.shutdown();
    assert_eq!(hub.live_thread_count(), 0);
    assert_eq!(driver.interrupt_calls(), 2);
    assert!(driver.accepted_writes().is_empty());
}

#[test]
fn close_publishes_synchronized_final_damage_and_effects_before_closed() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let (transport, driver) = scripted_session(
        [ReadAction::Block, ReadAction::Block],
        [WriteAction::accept(usize::MAX); 2],
    );
    let handle = hub
        .open(PaneId::new(23), transport, PaneWorkerConfig::default())
        .expect("open pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    driver.push_read(ReadAction::bytes(b"\x1b[?2026hheld\x07"));
    assert_eq!(
        hub.recv_notice().expect("held wake"),
        PaneNotice::Wake(token)
    );
    let held = hub.drain_pane(token, usize::MAX).expect("held drain");
    assert!(!held.frame.expect("held frame").snapshot_changed);
    assert!(matches!(
        held.effects.as_slice(),
        [effect] if matches!(effect.effect.kind(), RuntimeEffectKind::Bell { .. })
    ));

    assert!(hub.begin_close(token, Duration::from_secs(1)));
    assert_eq!(
        hub.recv_notice().expect("final wake"),
        PaneNotice::Wake(token)
    );
    let final_drain = hub.drain_pane(token, usize::MAX).expect("final drain");
    let final_frame = final_drain.frame.expect("final frame");
    assert!(final_frame.snapshot_changed);
    assert!(!final_frame.damage.is_empty());
    assert!(matches!(
        hub.recv_notice().expect("closed"),
        PaneNotice::Closed { pane, .. } if pane == token
    ));
    hub.shutdown();
    assert_eq!(hub.live_thread_count(), 0);
}

#[test]
fn queued_output_batches_once_wakes_once_replaces_frames_and_preserves_effects() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let mut reads = vec![ReadAction::bytes(b"\x07"); 32];
    reads.push(ReadAction::Block);
    let (transport, driver) = scripted_session(
        [ReadAction::Block],
        [WriteAction::Block, WriteAction::accept(usize::MAX)],
    );
    let config = PaneWorkerConfig {
        batch_policy: BatchPolicy::try_new(16, 16, Duration::from_millis(3)).expect("batch policy"),
        effect_limits: MailboxLimits::try_new(64, 4096).expect("effect limits"),
        ..PaneWorkerConfig::default()
    };
    let handle = hub
        .open(PaneId::new(29), transport, config)
        .expect("open pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    assert_eq!(
        handle.submit_input(b"gate".to_vec()),
        SubmitResult::Accepted
    );
    driver.wait_until_writer_blocked();
    driver.push_reads(reads);
    driver.wait_until_reader_blocked();
    assert_eq!(
        handle.resize(TerminalSize::new(81, 24)),
        SubmitResult::Accepted
    );
    driver.push_write(WriteAction::accept(usize::MAX));
    driver.wait_until_control_call_count(1);
    assert_eq!(
        handle.submit_input(b"barrier".to_vec()),
        SubmitResult::Accepted
    );
    driver.wait_until_accepted_write_len(11);

    assert_eq!(
        hub.recv_notice().expect("single wake"),
        PaneNotice::Wake(token)
    );
    assert_eq!(hub.try_recv_notice(), Err(TryRecvError::Empty));
    let drain = hub.drain_pane(token, usize::MAX).expect("drain");
    let frame = drain.frame.expect("latest frame");
    assert_eq!(frame.revision.get(), 3);
    assert!(frame.full_repaint);
    assert_eq!(frame.snapshot.grid().size(), TerminalSize::new(81, 24));
    assert_eq!(
        rssh_runtime::TerminalStateSummary::capture_terminal(&frame.snapshot),
        frame.state
    );
    assert_eq!(
        drain
            .effects
            .iter()
            .map(|effect| match effect.effect.kind() {
                RuntimeEffectKind::Bell { count } => count.get(),
                other => panic!("unexpected effect {other:?}"),
            })
            .sum::<u64>(),
        32
    );
    assert!(
        drain
            .effects
            .windows(2)
            .all(|pair| pair[0].effect.sequence() < pair[1].effect.sequence())
    );

    let metrics = hub.publication_metrics(token).expect("metrics");
    assert_eq!(metrics.batches, 3);
    assert_eq!(metrics.source_items, 32);
    assert_eq!(metrics.max_batch_items, 16);
    assert_eq!(metrics.latest.wakes, 1);
    assert_eq!(metrics.latest.replaced_frames, 2);
    hub.shutdown();
}

#[test]
fn effect_backpressure_wakes_before_the_worker_waits_for_mailbox_space() {
    let clock = VirtualClock::new(Instant::now());
    let mut hub = RuntimeHub::new(clock);
    let payload =
        b"\x1b]777;notify;test;one\x07\x1b]777;notify;test;two\x07\x1b]777;notify;test;three\x07";
    let (transport, _) = scripted_session(
        [ReadAction::bytes(payload), ReadAction::Block],
        [WriteAction::accept(usize::MAX)],
    );
    let config = PaneWorkerConfig {
        effect_limits: MailboxLimits::try_new(1, 512).expect("single-effect limit"),
        ..PaneWorkerConfig::default()
    };
    let handle = hub
        .open(PaneId::new(31), transport, config)
        .expect("open pane");
    let token = handle.token();
    assert_eq!(hub.recv_notice().expect("ready"), PaneNotice::Ready(token));

    let mut bodies = Vec::new();
    while bodies.len() < 3 {
        assert_eq!(
            hub.recv_notice().expect("effect wake"),
            PaneNotice::Wake(token)
        );
        let mut continuation = true;
        while continuation {
            let drain = hub.drain_pane(token, 1).expect("single effect drain");
            for effect in drain.effects {
                match effect.effect.kind() {
                    RuntimeEffectKind::Notification { body, .. } => bodies.push(body.clone()),
                    other => panic!("unexpected effect {other:?}"),
                }
            }
            continuation = drain.continuation;
        }
    }
    assert_eq!(bodies, ["one", "two", "three"]);
    assert_eq!(
        hub.publication_metrics(token)
            .expect("publication metrics")
            .effects
            .high_water_items,
        1
    );
    hub.shutdown();
}
