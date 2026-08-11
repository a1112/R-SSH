use std::time::{Duration, Instant};

use rssh_core::{PaneId, TerminalSize};
use rssh_runtime::testing::{ExitAction, ReadAction, ScriptedTransport, VirtualClock, WriteAction};
use rssh_runtime::{PaneNotice, PaneWorkerConfig, RuntimeEffectKind, RuntimeHub, SubmitResult};

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
    assert!(matches!(
        hub.recv_notice().expect("output notice"),
        PaneNotice::Advanced {
            pane,
            snapshot_changed: false,
            ..
        } if pane == token
    ));

    let size = TerminalSize::new(132, 43);
    assert_eq!(handle.resize(size), SubmitResult::Accepted);
    driver.wait_until_control_call_count(1);
    assert_eq!(driver.control_log().resizes, vec![size]);
    assert!(matches!(
        hub.recv_notice().expect("resize notice"),
        PaneNotice::Advanced { pane, .. } if pane == token
    ));

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
    let held = hub.recv_notice().expect("held output");
    assert!(matches!(
        held,
        PaneNotice::Advanced {
            snapshot_changed: false,
            ref effects,
            ..
        } if matches!(effects.as_slice(), [RuntimeEffectKind::Bell { .. }])
    ));

    assert!(hub.begin_close(token, Duration::from_secs(1)));
    assert!(matches!(
        hub.recv_notice().expect("final damage"),
        PaneNotice::Advanced {
            pane,
            snapshot_changed: true,
            ref damage,
            ..
        } if pane == token && !damage.is_empty()
    ));
    assert!(matches!(
        hub.recv_notice().expect("closed"),
        PaneNotice::Closed { pane, .. } if pane == token
    ));
    hub.shutdown();
    assert_eq!(hub.live_thread_count(), 0);
}
