use std::sync::Arc;

use rssh_runtime::{MetadataChange, RuntimeBatch, RuntimeEffectKind, TerminalStateSummary};

use crate::{
    ClipboardEffect, CommandIntent, ConfigDiff, HostEffectContext, NotificationEffect,
    PaneLifecycleIntent, PaneState, PlatformIntent, RendererEffect, RuntimePortEffect, SpawnEffect,
    TimerIntent, UriEffect, WindowEffect, WindowIntent, WindowPortEffect, WindowState,
};

/// Applies one intent atomically and appends typed commands for external owners.
pub fn reduce(state: &mut WindowState, intent: WindowIntent, effects: &mut Vec<WindowEffect>) {
    match intent {
        WindowIntent::Platform(intent) => reduce_platform(state, intent, effects),
        WindowIntent::Command(intent) => reduce_command(state, intent, effects),
        WindowIntent::RuntimeBatch(batch) => reduce_runtime_batch(state, batch, effects),
        WindowIntent::Config(diff) => reduce_config(state, diff, effects),
        WindowIntent::Timer(intent) => reduce_timer(state, intent, effects),
        WindowIntent::PaneLifecycle(intent) => reduce_pane_lifecycle(state, intent, effects),
        WindowIntent::RedrawRequested => {
            if state.presentation.redraw_pending {
                state.presentation.redraw_pending = false;
                effects.push(WindowEffect::Renderer(RendererEffect::Present));
            }
        }
        WindowIntent::CloseRequested => reduce_close(state, effects),
    }
}

fn reduce_platform(
    state: &mut WindowState,
    intent: PlatformIntent,
    effects: &mut Vec<WindowEffect>,
) {
    match intent {
        PlatformIntent::Focused(focused) => {
            state.platform.focused = focused;
            effects.push(WindowEffect::Window(WindowPortEffect::SetFocused(focused)));
        }
        PlatformIntent::Resized(size) => {
            state.presentation.size = size;
            effects.push(WindowEffect::Renderer(RendererEffect::ResizeSurface(size)));
            request_redraw(state, effects);
        }
    }
}

fn reduce_command(state: &mut WindowState, intent: CommandIntent, effects: &mut Vec<WindowEffect>) {
    match intent {
        CommandIntent::OpenUri(uri) => effects.push(WindowEffect::Uri(UriEffect::Open(uri))),
        CommandIntent::Copy(contents) => {
            effects.push(WindowEffect::Clipboard(ClipboardEffect::Write {
                context: None,
                selection: None,
                contents,
            }));
        }
        CommandIntent::Paste { pane, bytes } => {
            if let Some(pane) = state.panes.get(&pane) {
                effects.push(WindowEffect::Runtime(RuntimePortEffect::SubmitInput {
                    pane: pane.token,
                    bytes,
                }));
            }
        }
        CommandIntent::SpawnPane => effects.push(WindowEffect::Spawn(SpawnEffect::Pane)),
        CommandIntent::SpawnWindow => effects.push(WindowEffect::Spawn(SpawnEffect::Window)),
        CommandIntent::RestartPane(pane_id) => {
            if let Some(pane) = state.panes.get_mut(&pane_id) {
                pane.restarting = true;
                effects.push(WindowEffect::Runtime(RuntimePortEffect::Restart {
                    pane: pane.token,
                }));
            }
        }
        CommandIntent::SetTitle(title) => {
            effects.push(WindowEffect::Window(WindowPortEffect::SetTitle(title)));
        }
        CommandIntent::Persist => {
            effects.push(WindowEffect::Persistence(crate::PersistenceEffect::Save));
        }
    }
}

fn reduce_runtime_batch(
    state: &mut WindowState,
    batch: RuntimeBatch<TerminalStateSummary>,
    effects: &mut Vec<WindowEffect>,
) {
    let pane_id = batch.pane.pane();
    let Some(pane) = state.panes.get(&pane_id) else {
        return;
    };
    if pane.token != batch.pane
        || pane
            .revision
            .is_some_and(|revision| batch.revision <= revision)
    {
        return;
    }
    if batch.snapshot.is_none() && !batch.damage.is_empty() {
        effects.push(WindowEffect::Window(WindowPortEffect::ReportDiagnostic(
            "runtime batch carried damage without a snapshot".to_owned(),
        )));
        return;
    }
    let mut sequence = pane.effect_sequence;
    if let Err(error) = sequence.validate_batch(&batch) {
        effects.push(WindowEffect::Window(WindowPortEffect::ReportDiagnostic(
            error.to_string(),
        )));
        return;
    }

    for runtime_effect in &batch.effects {
        let context = HostEffectContext {
            pane: batch.pane,
            revision: batch.revision,
            sequence: runtime_effect.sequence(),
        };
        effects.push(route_runtime_effect(context, runtime_effect.kind().clone()));
    }

    let metadata_changed = batch.metadata != rssh_runtime::PaneMetadataDelta::default();
    let pane = state
        .panes
        .get_mut(&pane_id)
        .expect("pane generation was checked before mutation");
    pane.effect_sequence = sequence;
    pane.revision = Some(batch.revision);
    pane.restarting = false;
    apply_metadata(pane, batch.metadata);
    if let Some(snapshot) = batch.snapshot {
        let snapshot = Arc::unwrap_or_clone(snapshot);
        pane.snapshot = Some(snapshot);
        effects.push(WindowEffect::Renderer(RendererEffect::ApplyPane {
            pane: batch.pane,
            revision: batch.revision,
            snapshot,
            damage: batch.damage,
        }));
        request_redraw(state, effects);
    } else if metadata_changed {
        request_redraw(state, effects);
    }
}

fn route_runtime_effect(context: HostEffectContext, effect: RuntimeEffectKind) -> WindowEffect {
    match effect {
        RuntimeEffectKind::TransportWrite(bytes) => {
            WindowEffect::Runtime(RuntimePortEffect::WriteTransport { context, bytes })
        }
        RuntimeEffectKind::Bell { count } => {
            WindowEffect::Renderer(RendererEffect::Bell { context, count })
        }
        RuntimeEffectKind::ClipboardWrite {
            selection,
            contents,
        } => WindowEffect::Clipboard(ClipboardEffect::Write {
            context: Some(context),
            selection,
            contents,
        }),
        RuntimeEffectKind::ClipboardRead { selection } => {
            WindowEffect::Clipboard(ClipboardEffect::Read { context, selection })
        }
        RuntimeEffectKind::Notification { title, body } => {
            WindowEffect::Notification(NotificationEffect::Show {
                context,
                title,
                body,
            })
        }
        RuntimeEffectKind::Diagnostic { message } => {
            WindowEffect::Window(WindowPortEffect::RuntimeDiagnostic { context, message })
        }
    }
}

fn apply_metadata(pane: &mut PaneState, metadata: rssh_runtime::PaneMetadataDelta) {
    apply_value(&mut pane.title, metadata.title);
    apply_value(&mut pane.working_directory, metadata.working_directory);
    apply_value(&mut pane.badge_format, metadata.badge_format);
    if let Some(progress) = metadata.progress {
        pane.progress = match progress {
            MetadataChange::Set(progress) => progress,
            MetadataChange::Clear => rssh_runtime::RuntimeProgress::None,
        };
    }
    for change in metadata.user_vars {
        match change.value {
            MetadataChange::Set(value) => {
                pane.user_vars.insert(change.name, value);
            }
            MetadataChange::Clear => {
                pane.user_vars.remove(&change.name);
            }
        }
    }
}

fn apply_value(target: &mut Option<String>, change: Option<MetadataChange<String>>) {
    if let Some(change) = change {
        *target = match change {
            MetadataChange::Set(value) => Some(value),
            MetadataChange::Clear => None,
        };
    }
}

fn reduce_config(state: &mut WindowState, diff: ConfigDiff, effects: &mut Vec<WindowEffect>) {
    if diff.revision <= state.config.revision {
        return;
    }
    state.config.revision = diff.revision;
    state.config.theme.clone_from(&diff.theme);
    effects.push(WindowEffect::Renderer(RendererEffect::ApplyConfig {
        revision: diff.revision,
        theme: diff.theme,
    }));
    request_redraw(state, effects);
}

fn reduce_timer(state: &mut WindowState, intent: TimerIntent, effects: &mut Vec<WindowEffect>) {
    match intent {
        TimerIntent::Arm { timer, epoch } => {
            state.timers.epochs.insert(timer, epoch);
        }
        TimerIntent::Fired { timer, epoch } => {
            if state.timers.epochs.get(&timer) == Some(&epoch) {
                state.timers.epochs.remove(&timer);
                request_redraw(state, effects);
            }
        }
        TimerIntent::Cancel { timer } => {
            state.timers.epochs.remove(&timer);
        }
    }
}

fn reduce_pane_lifecycle(
    state: &mut WindowState,
    intent: PaneLifecycleIntent,
    effects: &mut Vec<WindowEffect>,
) {
    match intent {
        PaneLifecycleIntent::Opened(token) => {
            if state
                .panes
                .get(&token.pane())
                .is_some_and(|pane| pane.token.generation() >= token.generation())
            {
                return;
            }
            state.panes.insert(token.pane(), PaneState::new(token));
        }
        PaneLifecycleIntent::Closed(token) => {
            if state.panes.get(&token.pane()).map(|pane| pane.token) == Some(token) {
                state.panes.remove(&token.pane());
                if state.lifecycle.closing && state.panes.is_empty() {
                    effects.push(WindowEffect::Window(WindowPortEffect::CloseNow));
                }
            }
        }
    }
}

fn reduce_close(state: &mut WindowState, effects: &mut Vec<WindowEffect>) {
    if state.lifecycle.closing {
        return;
    }
    state.lifecycle.closing = true;
    let mut panes = state
        .panes
        .values()
        .map(|pane| pane.token)
        .collect::<Vec<_>>();
    panes.sort_unstable_by_key(|token| token.pane().get());
    for pane in panes {
        effects.push(WindowEffect::Runtime(RuntimePortEffect::BeginClose {
            pane,
        }));
    }
    effects.push(WindowEffect::Window(if state.panes.is_empty() {
        WindowPortEffect::CloseNow
    } else {
        WindowPortEffect::CloseAfterRuntimes
    }));
}

fn request_redraw(state: &mut WindowState, effects: &mut Vec<WindowEffect>) {
    if state.presentation.redraw_pending {
        return;
    }
    state.presentation.redraw_pending = true;
    effects.push(WindowEffect::Window(WindowPortEffect::RequestRedraw));
}
