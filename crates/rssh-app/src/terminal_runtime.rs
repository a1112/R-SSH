use base64::{Engine, engine::general_purpose::STANDARD};
use rssh_core::{DamageRegion, TerminalSize};
use rssh_terminal::{
    Cell, CellWidthOverride, Color, CursorShape, CursorStyle, Terminal, UnderlineStyle,
    VerticalAlign,
};

use crate::{
    terminal_modes::{
        KeyModifierOptionsQuery, KeyModifierOptionsSequence, KittyKeyboardFlagsQuery,
        KittyKeyboardModeSequence, MouseInputMode, SynchronizedOutputModeSequence,
        TerminalModeTracker, find_key_modifier_options_query, find_key_modifier_options_sequence,
        find_kitty_keyboard_flags_query, find_kitty_keyboard_mode_sequence,
        find_synchronized_output_mode_sequence, key_modifier_options_query_suffix_len,
        key_modifier_options_sequence_suffix_len, kitty_keyboard_flags_query_suffix_len,
        kitty_keyboard_mode_sequence_suffix_len, synchronized_output_mode_sequence_suffix_len,
    },
    visible_output::TerminalVisibleOutputFilter,
};

const DEFAULT_TERMINAL_NAME: &str = "xterm-256color";

pub struct TerminalRuntime {
    terminal: Terminal,
    output_filter: TerminalOutputFilter,
    visible_output_filter: TerminalVisibleOutputFilter,
    mode_tracker: TerminalModeTracker,
    enable_kitty_keyboard: bool,
    enable_checksum_rectangular_area: bool,
    enable_title_reporting: bool,
    allow_win32_input_mode: bool,
    clipboard_tracker: TerminalClipboardTracker,
    notification_tracker: TerminalNotificationTracker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TerminalNotification {
    pub(crate) title: Option<String>,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TerminalProgress {
    #[default]
    None,
    Percentage(u8),
    Error(u8),
    Indeterminate,
}

pub(crate) struct TerminalRuntimeOutput {
    pub(crate) responses: Vec<Vec<u8>>,
    pub(crate) display: Vec<u8>,
    pub(crate) damage: Vec<DamageRegion>,
    pub(crate) bells: u64,
    pub(crate) unknown_escape_sequences: Vec<String>,
}

impl TerminalRuntime {
    #[must_use]
    pub fn new(size: TerminalSize) -> Self {
        Self {
            terminal: Terminal::new(size),
            output_filter: TerminalOutputFilter::new(size),
            visible_output_filter: TerminalVisibleOutputFilter::default(),
            mode_tracker: TerminalModeTracker::default(),
            enable_kitty_keyboard: false,
            enable_checksum_rectangular_area: false,
            enable_title_reporting: false,
            allow_win32_input_mode: true,
            clipboard_tracker: TerminalClipboardTracker::default(),
            notification_tracker: TerminalNotificationTracker::default(),
        }
    }

    pub(crate) fn set_terminal_name(&mut self, terminal_name: impl Into<String>) {
        self.output_filter.set_terminal_name(terminal_name);
    }

    #[cfg(test)]
    pub fn feed_pty_output(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.feed_pty_output_with_display(bytes).responses
    }

    pub(crate) fn feed_pty_output_with_display(&mut self, bytes: &[u8]) -> TerminalRuntimeOutput {
        self.clipboard_tracker.process(bytes);
        self.notification_tracker.process(bytes);
        let output = self.output_filter.process(bytes);

        let mut responses = Vec::new();
        let mut display_bytes = Vec::new();
        let mut damage = Vec::new();
        let mut bells = 0_u64;
        let mut unknown_escape_sequences = Vec::new();
        for event in output.events {
            match event {
                FilteredOutputEvent::Display(display) => {
                    self.mode_tracker.process_without_emitting(&display);
                    self.terminal.feed(&display);
                    unknown_escape_sequences.extend(
                        self.terminal
                            .take_unknown_escape_sequences()
                            .into_iter()
                            .map(|sequence| sequence.sequence),
                    );
                    responses.extend(self.terminal.take_kitty_graphics_responses());
                    bells = bells.saturating_add(self.terminal.take_bell_count());
                    display_bytes.extend(self.visible_output_filter.process(&display));
                    if self.mode_tracker.synchronized_output() {
                        continue;
                    }
                    damage.extend(self.terminal.take_damage());
                }
                FilteredOutputEvent::Response(response) => {
                    if self.should_emit_response(&response) {
                        responses.push(self.output_filter.response_bytes(
                            response,
                            &self.terminal,
                            &self.mode_tracker,
                        ));
                    }
                }
                FilteredOutputEvent::ResponseBytes(bytes) => {
                    responses.push(bytes);
                }
                FilteredOutputEvent::SynchronizedOutputMode { bytes, enabled } => {
                    self.mode_tracker.process_without_emitting(&bytes);
                    if !enabled {
                        damage.extend(self.terminal.take_damage());
                    }
                }
                FilteredOutputEvent::KittyKeyboardMode { bytes } => {
                    if self.enable_kitty_keyboard {
                        self.mode_tracker.process_without_emitting(&bytes);
                    }
                }
                FilteredOutputEvent::KeyModifierOptions { bytes } => {
                    self.mode_tracker.process_without_emitting(&bytes);
                }
            }
        }

        TerminalRuntimeOutput {
            responses,
            display: display_bytes,
            damage,
            bells,
            unknown_escape_sequences,
        }
    }

    fn should_emit_response(&self, response: &TerminalResponse) -> bool {
        match response {
            TerminalResponse::KittyKeyboardFlags => self.enable_kitty_keyboard,
            TerminalResponse::ChecksumRectangularArea(_) => self.enable_checksum_rectangular_area,
            TerminalResponse::WindowTitle => self.enable_title_reporting,
            _ => true,
        }
    }

    pub fn take_clipboard_texts(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_tracker.texts)
    }

    pub fn take_clipboard_queries(&mut self) -> Vec<String> {
        std::mem::take(&mut self.clipboard_tracker.queries)
    }

    pub(crate) fn take_notifications(&mut self) -> Vec<TerminalNotification> {
        std::mem::take(&mut self.notification_tracker.notifications)
    }

    #[must_use]
    pub(crate) const fn progress(&self) -> TerminalProgress {
        self.notification_tracker.progress
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.terminal.resize(size);
        self.output_filter.resize(size);
    }

    pub fn set_scrollback_limit(&mut self, limit: usize) {
        self.terminal.set_scrollback_limit(limit);
    }

    pub fn set_default_cursor_style(&mut self, default_cursor_style: CursorStyle) {
        self.terminal.set_default_cursor_style(default_cursor_style);
    }

    #[must_use]
    pub(crate) fn cursor_color_override(&self) -> Option<Color> {
        self.output_filter.cursor_color_override()
    }

    pub(crate) fn erase_scrollback_and_viewport(&mut self) -> Vec<DamageRegion> {
        self.terminal.erase_scrollback_and_viewport();
        self.terminal.take_damage()
    }

    #[must_use]
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    #[must_use]
    pub fn application_cursor_keys(&self) -> bool {
        self.mode_tracker.application_cursor_keys()
    }

    #[must_use]
    pub fn focus_reporting(&self) -> bool {
        self.mode_tracker.focus_reporting()
    }

    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        self.mode_tracker.bracketed_paste()
    }

    #[cfg(test)]
    #[must_use]
    pub fn synchronized_output(&self) -> bool {
        self.mode_tracker.synchronized_output()
    }

    #[must_use]
    pub fn application_keypad(&self) -> bool {
        self.mode_tracker.application_keypad()
    }

    #[must_use]
    pub(crate) fn kitty_keyboard_flags(&self) -> u16 {
        if self.enable_kitty_keyboard {
            self.mode_tracker.kitty_keyboard_flags()
        } else {
            0
        }
    }

    #[must_use]
    pub(crate) fn modify_other_keys(&self) -> u8 {
        self.mode_tracker.modify_other_keys()
    }

    #[must_use]
    pub(crate) fn win32_input_mode(&self) -> bool {
        self.allow_win32_input_mode && self.mode_tracker.win32_input_mode()
    }

    #[must_use]
    pub fn mouse_input_mode(&self) -> MouseInputMode {
        self.mode_tracker.mouse_input_mode()
    }

    pub(crate) fn set_enable_kitty_keyboard(&mut self, enabled: bool) {
        self.enable_kitty_keyboard = enabled;
        if !enabled {
            self.mode_tracker.clear_kitty_keyboard_flags();
        }
    }

    pub(crate) fn set_enable_kitty_graphics(&mut self, enabled: bool) {
        self.terminal.set_enable_kitty_graphics(enabled);
    }

    pub(crate) fn set_enable_checksum_rectangular_area(&mut self, enabled: bool) {
        self.enable_checksum_rectangular_area = enabled;
    }

    pub(crate) fn set_enable_title_reporting(&mut self, enabled: bool) {
        self.enable_title_reporting = enabled;
    }

    pub(crate) fn set_allow_win32_input_mode(&mut self, allowed: bool) {
        self.allow_win32_input_mode = allowed;
        self.mode_tracker.set_allow_win32_input_mode(allowed);
    }

    pub(crate) fn set_treat_east_asian_ambiguous_width_as_wide(&mut self, enabled: bool) {
        self.terminal
            .set_treat_east_asian_ambiguous_width_as_wide(enabled);
    }

    pub(crate) fn set_normalize_output_to_unicode_nfc(&mut self, enabled: bool) {
        self.terminal.set_normalize_output_to_unicode_nfc(enabled);
    }

    pub(crate) fn set_unicode_version(&mut self, version: u32) {
        self.terminal.set_unicode_version(version);
    }

    pub(crate) fn set_cell_width_overrides(&mut self, overrides: Vec<CellWidthOverride>) {
        self.terminal.set_cell_width_overrides(overrides);
    }
}

struct TerminalOutputFilter {
    pending: Vec<u8>,
    size: TerminalSize,
    terminal_name: String,
    color_state: TerminalColorState,
}

struct FilteredOutput {
    events: Vec<FilteredOutputEvent>,
}

enum FilteredOutputEvent {
    Display(Vec<u8>),
    Response(TerminalResponse),
    ResponseBytes(Vec<u8>),
    SynchronizedOutputMode { bytes: Vec<u8>, enabled: bool },
    KittyKeyboardMode { bytes: Vec<u8> },
    KeyModifierOptions { bytes: Vec<u8> },
}

impl TerminalOutputFilter {
    const CELL_HEIGHT_PIXELS: u16 = 16;
    const CELL_WIDTH_PIXELS: u16 = 8;
    const PRIMARY_DEVICE_ATTRIBUTES: &'static [u8] = b"\x1b[?65;4;6;18;22;52c";
    const SECONDARY_DEVICE_ATTRIBUTES: &'static [u8] = b"\x1b[>1;277;0c";
    const TERTIARY_DEVICE_ATTRIBUTES: &'static [u8] = b"\x1bP!|00000000\x1b\\";
    const TERMINAL_PARAMETERS_0: &'static [u8] = b"\x1b[2;1;1;128;128;1;0x";
    const TERMINAL_PARAMETERS_1: &'static [u8] = b"\x1b[3;1;1;128;128;1;0x";
    const RESPONSES: &'static [TerminalQueryResponse] = &[
        TerminalQueryResponse {
            query: b"\x1b[6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x9b6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b6n",
            response: TerminalResponse::CursorPosition { private: false },
        },
        TerminalQueryResponse {
            query: b"\x1b[c",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[0c",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9bc",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9bc",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9b0c",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b0c",
            response: TerminalResponse::Static(Self::PRIMARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[>c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[>0c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9b>c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9b>0c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b>c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b>0c",
            response: TerminalResponse::Static(Self::SECONDARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[=c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[=0c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9b=c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x9b=0c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b=c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b=0c",
            response: TerminalResponse::Static(Self::TERTIARY_DEVICE_ATTRIBUTES),
        },
        TerminalQueryResponse {
            query: b"\x1b[x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\x1b[0x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\x1b[1x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_1),
        },
        TerminalQueryResponse {
            query: b"\x9bx",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\x9b0x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\x9b1x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_1),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9bx",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b0x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_0),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b1x",
            response: TerminalResponse::Static(Self::TERMINAL_PARAMETERS_1),
        },
        TerminalQueryResponse {
            query: b"\x1b[>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x1b[>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b>q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x9b>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b>0q",
            response: TerminalResponse::XtVersion,
        },
        TerminalQueryResponse {
            query: b"\x1b[5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x9b5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b5n",
            response: TerminalResponse::Static(b"\x1b[0n"),
        },
        TerminalQueryResponse {
            query: b"\x1b[14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x9b14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b14t",
            response: TerminalResponse::WindowPixelSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x9b16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b16t",
            response: TerminalResponse::CharacterCellSize,
        },
        TerminalQueryResponse {
            query: b"\x1b[18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\x9b18t",
            response: TerminalResponse::TextAreaSize,
        },
        TerminalQueryResponse {
            query: b"\xc2\x9b18t",
            response: TerminalResponse::TextAreaSize,
        },
    ];
    const IGNORED_QUERIES: &'static [&'static [u8]] = &[
        b"\x1b[?6n".as_slice(),
        b"\x9b?6n".as_slice(),
        b"\xc2\x9b?6n".as_slice(),
    ];

    fn new(size: TerminalSize) -> Self {
        Self {
            pending: Vec::new(),
            size,
            terminal_name: DEFAULT_TERMINAL_NAME.to_owned(),
            color_state: TerminalColorState::default(),
        }
    }

    fn set_terminal_name(&mut self, terminal_name: impl Into<String>) {
        self.terminal_name = terminal_name.into();
    }

    fn resize(&mut self, size: TerminalSize) {
        self.size = size;
    }

    fn process(&mut self, bytes: &[u8]) -> FilteredOutput {
        self.pending.extend_from_slice(bytes);

        let mut events = Vec::new();

        while let Some((index, event)) = self.find_next_event() {
            if index > 0 {
                let display = self.pending[..index].to_vec();
                self.color_state.process(&display);
                events.push(FilteredOutputEvent::Display(display));
            }
            let consumed_end = index + event.consumed;
            let output_event = match event.event {
                MatchedTerminalEventKind::Response(response) => {
                    Some(self.filtered_response(response))
                }
                MatchedTerminalEventKind::SynchronizedOutputMode { enabled } => {
                    Some(FilteredOutputEvent::SynchronizedOutputMode {
                        bytes: self.pending[index..consumed_end].to_vec(),
                        enabled,
                    })
                }
                MatchedTerminalEventKind::KittyKeyboardMode => {
                    Some(FilteredOutputEvent::KittyKeyboardMode {
                        bytes: self.pending[index..consumed_end].to_vec(),
                    })
                }
                MatchedTerminalEventKind::KeyModifierOptions => {
                    Some(FilteredOutputEvent::KeyModifierOptions {
                        bytes: self.pending[index..consumed_end].to_vec(),
                    })
                }
                MatchedTerminalEventKind::IgnoredControl => None,
            };
            if let Some(output_event) = output_event {
                events.push(output_event);
            }
            self.pending.drain(..consumed_end);
        }

        let retained = Self::suffix_len_matching_query_prefix(&self.pending);
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            let display = self.pending[..writable].to_vec();
            self.color_state.process(&display);
            events.push(FilteredOutputEvent::Display(display));
            self.pending.drain(..writable);
        }

        FilteredOutput { events }
    }

    fn filtered_response(&self, response: TerminalResponse) -> FilteredOutputEvent {
        match response {
            TerminalResponse::OscColor(query) => {
                FilteredOutputEvent::ResponseBytes(self.color_state.response(query))
            }
            response => FilteredOutputEvent::Response(response),
        }
    }

    fn find_next_event(&self) -> Option<(usize, MatchedTerminalEvent)> {
        let response = self
            .find_next_response()
            .map(|(index, response)| (index, response.into()));
        let synchronized_output = find_synchronized_output_mode_sequence(&self.pending).map(
            |SynchronizedOutputModeSequence {
                 index,
                 consumed,
                 enabled,
             }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::SynchronizedOutputMode { enabled },
                    },
                )
            },
        );
        let kitty_keyboard_mode = find_kitty_keyboard_mode_sequence(&self.pending).map(
            |KittyKeyboardModeSequence { index, consumed }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::KittyKeyboardMode,
                    },
                )
            },
        );
        let key_modifier_options = find_key_modifier_options_sequence(&self.pending).map(
            |KeyModifierOptionsSequence { index, consumed }| {
                (
                    index,
                    MatchedTerminalEvent {
                        consumed,
                        event: MatchedTerminalEventKind::KeyModifierOptions,
                    },
                )
            },
        );
        let ignored_window_query = find_ignored_wezterm_window_query(&self.pending)
            .filter(|control| !is_inside_osc_or_st_control_string(&self.pending, control.index))
            .map(|control| {
                (
                    control.index,
                    MatchedTerminalEvent {
                        consumed: control.consumed,
                        event: MatchedTerminalEventKind::IgnoredControl,
                    },
                )
            });
        let ignored_static_query = find_ignored_static_query(&self.pending, Self::IGNORED_QUERIES)
            .filter(|control| !is_inside_osc_or_st_control_string(&self.pending, control.index))
            .map(|control| {
                (
                    control.index,
                    MatchedTerminalEvent {
                        consumed: control.consumed,
                        event: MatchedTerminalEventKind::IgnoredControl,
                    },
                )
            });
        let st_control = find_st_control(&self.pending)
            .filter(|control| !is_inside_osc_or_st_control_string(&self.pending, control.index))
            .map(|control| {
                (
                    control.index,
                    MatchedTerminalEvent {
                        consumed: control.consumed,
                        event: MatchedTerminalEventKind::IgnoredControl,
                    },
                )
            });

        response
            .into_iter()
            .chain(synchronized_output)
            .chain(kitty_keyboard_mode)
            .chain(key_modifier_options)
            .chain(ignored_window_query)
            .chain(ignored_static_query)
            .chain(st_control)
            .min_by_key(|(index, _)| *index)
    }

    #[allow(clippy::too_many_lines)]
    fn find_next_response(&self) -> Option<(usize, MatchedTerminalResponse)> {
        let static_response = Self::RESPONSES
            .iter()
            .filter_map(|response| {
                find_subslice(&self.pending, response.query).map(|index| {
                    (
                        index,
                        MatchedTerminalResponse {
                            consumed: response.query.len(),
                            response: response.response.clone(),
                        },
                    )
                })
            })
            .min_by_key(|(index, _)| *index);
        let window_report_response = find_wezterm_window_report_query(&self.pending).map(
            |WindowReportQuery {
                 index,
                 consumed,
                 response,
             }| { (index, MatchedTerminalResponse { consumed, response }) },
        );
        let mode_response = find_private_mode_status_query(&self.pending).map(
            |PrivateModeStatusQuery {
                 index,
                 consumed,
                 mode,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::PrivateModeStatus(mode),
                    },
                )
            },
        );
        let ansi_mode_response = find_ansi_mode_status_query(&self.pending).map(
            |AnsiModeStatusQuery {
                 index,
                 consumed,
                 mode,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::AnsiModeStatus(mode),
                    },
                )
            },
        );
        let osc_color_response = find_osc_color_query(&self.pending).map(
            |OscColorQuery {
                 index,
                 consumed,
                 query,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::OscColor(query),
                    },
                )
            },
        );
        let iterm_report_cell_size_response = find_iterm_report_cell_size_query(&self.pending).map(
            |ItermReportCellSizeQuery { index, consumed }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::ItermReportCellSize,
                    },
                )
            },
        );
        let checksum_rectangular_area_response = find_decrqcra_query(&self.pending).map(
            |DecrqcraQuery {
                 index,
                 consumed,
                 request,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::ChecksumRectangularArea(request),
                    },
                )
            },
        );
        let decrqss_response = find_decrqss_query(&self.pending).map(
            |DecrqssQuery {
                 index,
                 consumed,
                 response,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::Decrqss(response),
                    },
                )
            },
        );
        let xtgettcap_response =
            find_xtgettcap_query(&self.pending, self.size, &self.terminal_name).map(
                |XtGetTcapQuery {
                     index,
                     consumed,
                     response,
                 }| {
                    (
                        index,
                        MatchedTerminalResponse {
                            consumed,
                            response: TerminalResponse::XtGetTcap(response),
                        },
                    )
                },
            );
        let xtsmgraphics_response = find_xtsmgraphics_query(&self.pending).map(
            |XtSmGraphicsQuery {
                 index,
                 consumed,
                 request,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::XtSmGraphics(request),
                    },
                )
            },
        );
        let kitty_keyboard_flags_response = find_kitty_keyboard_flags_query(&self.pending).map(
            |KittyKeyboardFlagsQuery { index, consumed }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::KittyKeyboardFlags,
                    },
                )
            },
        );
        let key_modifier_options_response = find_key_modifier_options_query(&self.pending).map(
            |KeyModifierOptionsQuery {
                 index,
                 consumed,
                 resource,
             }| {
                (
                    index,
                    MatchedTerminalResponse {
                        consumed,
                        response: TerminalResponse::KeyModifierOptions(resource),
                    },
                )
            },
        );

        static_response
            .into_iter()
            .chain(window_report_response)
            .chain(mode_response)
            .chain(ansi_mode_response)
            .chain(osc_color_response)
            .chain(iterm_report_cell_size_response)
            .chain(checksum_rectangular_area_response)
            .chain(decrqss_response)
            .chain(xtgettcap_response)
            .chain(xtsmgraphics_response)
            .chain(kitty_keyboard_flags_response)
            .chain(key_modifier_options_response)
            .filter(|(index, _)| !is_inside_osc_or_st_control_string(&self.pending, *index))
            .min_by_key(|(index, _)| *index)
    }

    fn suffix_len_matching_query_prefix(pending: &[u8]) -> usize {
        let static_query_suffix = Self::RESPONSES
            .iter()
            .map(|response| suffix_prefix_len(pending, response.query))
            .max()
            .unwrap_or(0);
        static_query_suffix
            .max(private_mode_status_query_suffix_len(pending))
            .max(ansi_mode_status_query_suffix_len(pending))
            .max(synchronized_output_mode_sequence_suffix_len(pending))
            .max(osc_color_query_suffix_len(pending))
            .max(decrqcra_query_suffix_len(pending))
            .max(decrqss_query_suffix_len(pending))
            .max(xtgettcap_query_suffix_len(pending))
            .max(xtsmgraphics_query_suffix_len(pending))
            .max(kitty_keyboard_flags_query_suffix_len(pending))
            .max(kitty_keyboard_mode_sequence_suffix_len(pending))
            .max(key_modifier_options_query_suffix_len(pending))
            .max(key_modifier_options_sequence_suffix_len(pending))
            .max(st_control_suffix_len(pending))
            .max(incomplete_osc_control_sequence_suffix_len(pending))
            .max(incomplete_st_control_sequence_suffix_len(pending))
    }

    fn response_bytes(
        &self,
        response: TerminalResponse,
        terminal: &Terminal,
        modes: &TerminalModeTracker,
    ) -> Vec<u8> {
        response.response_bytes(self.size, terminal, modes, &self.color_state)
    }

    fn cursor_color_override(&self) -> Option<Color> {
        self.color_state
            .cursor_override()
            .map(DynamicColor::to_color)
    }
}

struct TerminalQueryResponse {
    query: &'static [u8],
    response: TerminalResponse,
}

struct MatchedTerminalResponse {
    consumed: usize,
    response: TerminalResponse,
}

struct WindowReportQuery {
    index: usize,
    consumed: usize,
    response: TerminalResponse,
}

struct MatchedTerminalEvent {
    consumed: usize,
    event: MatchedTerminalEventKind,
}

enum MatchedTerminalEventKind {
    Response(TerminalResponse),
    SynchronizedOutputMode { enabled: bool },
    KittyKeyboardMode,
    KeyModifierOptions,
    IgnoredControl,
}

impl From<MatchedTerminalResponse> for MatchedTerminalEvent {
    fn from(response: MatchedTerminalResponse) -> Self {
        Self {
            consumed: response.consumed,
            event: MatchedTerminalEventKind::Response(response.response),
        }
    }
}

#[derive(Clone)]
enum TerminalResponse {
    Static(&'static [u8]),
    CursorPosition { private: bool },
    WindowPixelSize,
    CharacterCellSize,
    TextAreaSize,
    WindowTitle,
    PrivateModeStatus(u16),
    AnsiModeStatus(u16),
    OscColor(OscColorResponse),
    ItermReportCellSize,
    ChecksumRectangularArea(DecrqcraRequest),
    Decrqss(DecrqssResponse),
    XtGetTcap(XtGetTcapResponse),
    XtSmGraphics(XtSmGraphicsRequest),
    XtVersion,
    KittyKeyboardFlags,
    KeyModifierOptions(u16),
}

impl TerminalResponse {
    fn response_bytes(
        self,
        size: TerminalSize,
        terminal: &Terminal,
        modes: &TerminalModeTracker,
        color_state: &TerminalColorState,
    ) -> Vec<u8> {
        match self {
            TerminalResponse::Static(bytes) => bytes.to_vec(),
            TerminalResponse::CursorPosition { private } => {
                let (row, column) = terminal.cursor();
                if private {
                    format!(
                        "\x1b[?{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                } else {
                    format!(
                        "\x1b[{};{}R",
                        row.saturating_add(1),
                        column.saturating_add(1)
                    )
                    .into_bytes()
                }
            }
            TerminalResponse::WindowPixelSize => format!(
                "\x1b[4;{};{}t",
                u32::from(size.rows) * u32::from(TerminalOutputFilter::CELL_HEIGHT_PIXELS),
                u32::from(size.columns) * u32::from(TerminalOutputFilter::CELL_WIDTH_PIXELS)
            )
            .into_bytes(),
            TerminalResponse::CharacterCellSize => format!(
                "\x1b[6;{};{}t",
                TerminalOutputFilter::CELL_HEIGHT_PIXELS,
                TerminalOutputFilter::CELL_WIDTH_PIXELS
            )
            .into_bytes(),
            TerminalResponse::TextAreaSize => {
                format!("\x1b[8;{};{}t", size.rows, size.columns).into_bytes()
            }
            TerminalResponse::WindowTitle => {
                let title = terminal
                    .window_title()
                    .or_else(|| terminal.title())
                    .unwrap_or("");
                format!("\x1b]l{title}\x1b\\").into_bytes()
            }
            TerminalResponse::PrivateModeStatus(mode) => {
                format!("\x1b[?{};{}$y", mode, modes.private_mode_report_value(mode)).into_bytes()
            }
            TerminalResponse::AnsiModeStatus(mode) => {
                format!("\x1b[{};{}$y", mode, modes.ansi_mode_report_value(mode)).into_bytes()
            }
            TerminalResponse::OscColor(query) => color_state.response(query),
            TerminalResponse::ItermReportCellSize => format!(
                "\x1b]1337;ReportCellSize={:.1};{:.1}\x1b\\",
                f32::from(TerminalOutputFilter::CELL_HEIGHT_PIXELS),
                f32::from(TerminalOutputFilter::CELL_WIDTH_PIXELS)
            )
            .into_bytes(),
            TerminalResponse::ChecksumRectangularArea(request) => {
                let checksum = terminal.checksum_rectangle(
                    request.left,
                    request.top,
                    request.right,
                    request.bottom,
                );
                format!("\x1bP{}!~{:04x}\x1b\\", request.request_id, checksum).into_bytes()
            }
            TerminalResponse::Decrqss(query) => query.response(terminal),
            TerminalResponse::XtGetTcap(query) => query.response(),
            TerminalResponse::XtSmGraphics(request) => request.response(size),
            TerminalResponse::XtVersion => xtversion_response(),
            TerminalResponse::KittyKeyboardFlags => {
                format!("\x1b[?{}u", modes.kitty_keyboard_flags()).into_bytes()
            }
            TerminalResponse::KeyModifierOptions(resource) => {
                let value = if resource == 4 {
                    modes.modify_other_keys()
                } else {
                    0
                };
                format!("\x1b[>{resource};{value}m").into_bytes()
            }
        }
    }
}

fn xtversion_response() -> Vec<u8> {
    format!("\x1bP>|R-SSH {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .enumerate()
        .find_map(|(index, window)| {
            (window == needle && !raw_c1_prefix_is_utf8_continuation(haystack, index, needle))
                .then_some(index)
        })
}

fn raw_c1_prefix_is_utf8_continuation(bytes: &[u8], index: usize, prefix: &[u8]) -> bool {
    prefix
        .first()
        .is_some_and(|byte| is_raw_c1_control_byte(*byte))
        && is_utf8_continuation_in_potential_sequence(bytes, index)
}

fn is_raw_c1_control_byte(byte: u8) -> bool {
    (0x80..=0x9f).contains(&byte)
}

fn is_utf8_continuation_in_potential_sequence(bytes: &[u8], index: usize) -> bool {
    if index == 0
        || bytes
            .get(index)
            .is_none_or(|byte| !is_utf8_continuation(*byte))
    {
        return false;
    }

    let mut start = index;
    while start > 0 && is_utf8_continuation(bytes[start]) {
        start -= 1;
    }
    if start == index {
        return false;
    }

    let Some(expected_len) = utf8_sequence_len(bytes[start]) else {
        return false;
    };

    index < start + expected_len
        && bytes[start + 1..=index]
            .iter()
            .all(|byte| is_utf8_continuation(*byte))
}

fn utf8_sequence_len(byte: u8) -> Option<usize> {
    match byte {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

fn is_utf8_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

fn dcs_start_prefixes() -> [(&'static [u8], usize); 3] {
    [
        (b"\x1bP".as_slice(), 2),
        (UTF8_C1_DCS, UTF8_C1_DCS.len()),
        (b"\x90".as_slice(), 1),
    ]
}

fn csi_start_prefixes() -> [(&'static [u8], usize); 3] {
    [
        (b"\x1b[".as_slice(), 2),
        (b"\x9b".as_slice(), 1),
        (UTF8_C1_CSI, UTF8_C1_CSI.len()),
    ]
}

fn find_ignored_static_query(bytes: &[u8], queries: &[&[u8]]) -> Option<StControl> {
    queries
        .iter()
        .filter_map(|query| {
            find_subslice(bytes, query).map(|index| StControl {
                index,
                consumed: query.len(),
            })
        })
        .min_by_key(|control| control.index)
}

fn find_wezterm_window_report_query(bytes: &[u8]) -> Option<WindowReportQuery> {
    csi_start_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            let index = find_subslice(bytes, prefix)?;
            let body_start = index + prefix_len;
            let body = &bytes[body_start..];
            let final_index = body.iter().position(|byte| (0x40..=0x7e).contains(byte))?;
            if body[final_index] != b't' {
                return None;
            }
            let response = wezterm_window_report_response(&body[..final_index])?;
            Some(WindowReportQuery {
                index,
                consumed: prefix_len + final_index + 1,
                response,
            })
        })
        .min_by_key(|query| query.index)
}

fn wezterm_window_report_response(params: &[u8]) -> Option<TerminalResponse> {
    let (first, second) = parse_wezterm_window_params(params)?;
    match first {
        14 if second.is_none() => Some(TerminalResponse::WindowPixelSize),
        16 => Some(TerminalResponse::CharacterCellSize),
        18 => Some(TerminalResponse::TextAreaSize),
        21 if second.is_none() => Some(TerminalResponse::WindowTitle),
        _ => None,
    }
}

fn find_ignored_wezterm_window_query(bytes: &[u8]) -> Option<StControl> {
    csi_start_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            let index = find_subslice(bytes, prefix)?;
            let body_start = index + prefix_len;
            let body = &bytes[body_start..];
            let final_index = body.iter().position(|byte| (0x40..=0x7e).contains(byte))?;
            if body[final_index] != b't' {
                return None;
            }
            if !is_wezterm_ignored_window_query(&body[..final_index]) {
                return None;
            }
            Some(StControl {
                index,
                consumed: prefix_len + final_index + 1,
            })
        })
        .min_by_key(|control| control.index)
}

fn is_wezterm_ignored_window_query(params: &[u8]) -> bool {
    wezterm_window_report_response(params).is_none()
}

fn parse_wezterm_window_params(params: &[u8]) -> Option<(i64, Option<i64>)> {
    let mut parts = params.split(|byte| *byte == b';');
    let first = parts.next().and_then(parse_ascii_i64)?;
    let second = match parts.next() {
        Some([]) | None => None,
        Some(part) => Some(parse_ascii_i64(part)?),
    };
    if parts.any(|part| !part.is_empty() && parse_ascii_i64(part).is_none()) {
        return None;
    }
    Some((first, second))
}

fn parse_ascii_i64(bytes: &[u8]) -> Option<i64> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

struct DecrqcraQuery {
    index: usize,
    consumed: usize,
    request: DecrqcraRequest,
}

#[derive(Clone)]
struct DecrqcraRequest {
    request_id: i64,
    top: u16,
    left: u16,
    bottom: u16,
    right: u16,
}

fn find_decrqcra_query(bytes: &[u8]) -> Option<DecrqcraQuery> {
    csi_start_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            let index = find_subslice(bytes, prefix)?;
            let body_start = index + prefix_len;
            let body = &bytes[body_start..];
            let final_index = body.iter().position(|byte| (0x40..=0x7e).contains(byte))?;
            if body[final_index] != b'y' {
                return None;
            }
            let request = parse_decrqcra_request(&body[..final_index])?;
            Some(DecrqcraQuery {
                index,
                consumed: prefix_len + final_index + 1,
                request,
            })
        })
        .min_by_key(|query| query.index)
}

fn parse_decrqcra_request(params: &[u8]) -> Option<DecrqcraRequest> {
    let params = params.strip_suffix(b"*")?;
    let mut parts = params.split(|byte| *byte == b';');
    let request_id = parts.next().and_then(parse_ascii_i64)?;
    let page_number = parts.next().and_then(parse_ascii_i64)?;
    if request_id < 0 || page_number < 0 {
        return None;
    }

    let top = decrqcra_zero_based_axis(parts.next(), 0)?;
    let left = decrqcra_zero_based_axis(parts.next(), 0)?;
    let bottom = decrqcra_zero_based_axis(parts.next(), i64::from(u16::MAX))?;
    let right = decrqcra_zero_based_axis(parts.next(), i64::from(u16::MAX))?;
    if parts.next().is_some() {
        return None;
    }

    Some(DecrqcraRequest {
        request_id,
        top,
        left,
        bottom,
        right,
    })
}

fn decrqcra_zero_based_axis(part: Option<&[u8]>, default: i64) -> Option<u16> {
    let value = match part {
        Some([]) | None => default,
        Some(part) => parse_ascii_i64(part)?,
    };
    if value <= 0 {
        Some(0)
    } else {
        Some(value.saturating_sub(1).min(i64::from(u16::MAX)) as u16)
    }
}

fn decrqcra_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_decrqcra_query_prefix)
}

fn is_decrqcra_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[")
        .or_else(|| bytes.strip_prefix(b"\x9b"))
        .or_else(|| bytes.strip_prefix(UTF8_C1_CSI))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x9b".starts_with(bytes)
            || UTF8_C1_CSI.starts_with(bytes);
    };

    let mut saw_star = false;
    for byte in rest {
        match *byte {
            b'0'..=b'9' | b';' if !saw_star => {}
            b'*' if !saw_star => saw_star = true,
            b'y' if saw_star => {}
            _ => return false,
        }
    }
    true
}

const UTF8_C1_OSC: &[u8] = b"\xc2\x9d";
const UTF8_C1_CSI: &[u8] = b"\xc2\x9b";
const UTF8_C1_PRIVATE_CSI: &[u8] = b"\xc2\x9b?";
const UTF8_C1_DCS: &[u8] = b"\xc2\x90";
const UTF8_C1_ST: &[u8] = b"\xc2\x9c";
const OSC_START_PREFIXES: &[(&[u8], usize)] = &[
    (b"\x1b]".as_slice(), 2),
    (b"\x9d".as_slice(), 1),
    (UTF8_C1_OSC, UTF8_C1_OSC.len()),
];

struct StControl {
    index: usize,
    consumed: usize,
}

fn find_st_control(bytes: &[u8]) -> Option<StControl> {
    st_control_sequences()
        .into_iter()
        .filter_map(|sequence| {
            find_subslice(bytes, sequence).map(|index| StControl {
                index,
                consumed: sequence.len(),
            })
        })
        .min_by_key(|control| control.index)
}

fn st_control_suffix_len(bytes: &[u8]) -> usize {
    st_control_sequences()
        .into_iter()
        .map(|sequence| suffix_prefix_len(bytes, sequence))
        .max()
        .unwrap_or(0)
}

fn st_control_sequences() -> [&'static [u8]; 3] {
    [b"\x1b\\".as_slice(), b"\x9c".as_slice(), UTF8_C1_ST]
}

fn is_inside_osc_or_st_control_string(bytes: &[u8], index: usize) -> bool {
    is_inside_control_string(bytes, index, find_next_osc_start, find_osc_color_terminator)
        || is_inside_control_string(
            bytes,
            index,
            find_next_st_control_string_start,
            find_xtgettcap_terminator,
        )
}

fn is_inside_control_string(
    bytes: &[u8],
    index: usize,
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<OscColorTerminator>,
) -> bool {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_start, prefix_len)) = find_next_start(&bytes[offset..]) else {
            return false;
        };
        let start = offset + relative_start;
        if start >= index {
            return false;
        }

        let content_start = start + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return true;
        };
        let end = content_start + terminator.index + terminator.length;
        if index < end {
            return true;
        }
        offset = end;
    }

    false
}

fn incomplete_osc_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_control_sequence_start(bytes, find_next_osc_start, find_osc_color_terminator)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_prefix_len(bytes, b"\x1b]"))
        .max(suffix_prefix_len(bytes, UTF8_C1_OSC))
}

fn incomplete_st_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_control_sequence_start(
        bytes,
        find_next_st_control_string_start,
        find_xtgettcap_terminator,
    )
    .map_or(0, |start| bytes.len() - start)
    .max(
        [
            b"\x1bP".as_slice(),
            b"\x1bX".as_slice(),
            b"\x1b^".as_slice(),
            b"\x1b_".as_slice(),
        ]
        .into_iter()
        .map(|prefix| suffix_prefix_len(bytes, prefix))
        .max()
        .unwrap_or(0),
    )
}

fn find_incomplete_control_sequence_start(
    bytes: &[u8],
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<OscColorTerminator>,
) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let Some((relative_index, prefix_len)) = find_next_start(&bytes[offset..]) else {
            break;
        };
        let index = offset + relative_index;
        let content_start = index + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return Some(index);
        };
        offset = content_start + terminator.index + terminator.length;
    }

    None
}

struct DecrqssQuery {
    index: usize,
    consumed: usize,
    response: DecrqssResponse,
}

#[derive(Clone)]
struct DecrqssResponse {
    kind: Option<DecrqssKind>,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum DecrqssKind {
    Sgr,
    CursorShape,
    ScrollRegion,
    ConformanceLevel,
    LeftRightMargins,
}

impl DecrqssResponse {
    fn response(&self, terminal: &Terminal) -> Vec<u8> {
        let mut response = if let Some(kind) = self.kind {
            let mut bytes = b"\x1bP1$r".to_vec();
            match kind {
                DecrqssKind::Sgr => append_sgr_state(terminal.active_style(), &mut bytes),
                DecrqssKind::CursorShape => {
                    append_cursor_shape_state(terminal.cursor_shape(), &mut bytes);
                }
                DecrqssKind::ScrollRegion => {
                    append_scroll_region_state(terminal.scroll_region(), &mut bytes);
                }
                DecrqssKind::ConformanceLevel => bytes.extend_from_slice(b"61;1\"p"),
                DecrqssKind::LeftRightMargins => {
                    append_left_right_margin_state(terminal.left_right_margins(), &mut bytes);
                }
            }
            bytes
        } else {
            b"\x1bP0$r".to_vec()
        };
        response.extend_from_slice(self.terminator.bytes());
        response
    }
}

fn find_decrqss_query(bytes: &[u8]) -> Option<DecrqssQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in dcs_start_prefixes() {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_decrqss_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &DecrqssQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_decrqss_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<DecrqssQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"$q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];

    Some(DecrqssQuery {
        index,
        consumed: prefix_len + b"$q".len() + terminator.index + terminator.length,
        response: DecrqssResponse {
            kind: parse_decrqss_kind(content),
            terminator: terminator.response_terminator,
        },
    })
}

fn parse_decrqss_kind(content: &[u8]) -> Option<DecrqssKind> {
    match content {
        b"m" => Some(DecrqssKind::Sgr),
        b" q" => Some(DecrqssKind::CursorShape),
        b"r" => Some(DecrqssKind::ScrollRegion),
        b"\"p" => Some(DecrqssKind::ConformanceLevel),
        b"s" => Some(DecrqssKind::LeftRightMargins),
        _ => None,
    }
}

fn append_sgr_state(style: &Cell, bytes: &mut Vec<u8>) {
    let mut params = Vec::new();
    if style.bold {
        params.push("1".to_owned());
    }
    if style.faint {
        params.push("2".to_owned());
    }
    if style.italic {
        params.push("3".to_owned());
    }
    append_underline_style_sgr(style, &mut params);
    if style.blink {
        params.push("5".to_owned());
    }
    if style.inverse {
        params.push("7".to_owned());
    }
    if style.conceal {
        params.push("8".to_owned());
    }
    if style.strikethrough {
        params.push("9".to_owned());
    }
    if style.double_underline {
        params.push("21".to_owned());
    }
    if style.overline {
        params.push("53".to_owned());
    }
    match style.vertical_align {
        VerticalAlign::Baseline => {}
        VerticalAlign::Superscript => params.push("73".to_owned()),
        VerticalAlign::Subscript => params.push("74".to_owned()),
    }
    append_color_sgr(58, style.underline_color, &mut params);
    append_color_sgr(38, style.foreground, &mut params);
    append_color_sgr(48, style.background, &mut params);

    if params.is_empty() {
        bytes.push(b'0');
    } else {
        bytes.extend_from_slice(params.join(";").as_bytes());
    }
    bytes.push(b'm');
}

fn append_underline_style_sgr(style: &Cell, params: &mut Vec<String>) {
    match style.underline_style {
        UnderlineStyle::None if style.double_underline => params.push("21".to_owned()),
        UnderlineStyle::None if style.underline => params.push("4".to_owned()),
        UnderlineStyle::None => {}
        UnderlineStyle::Single => params.push("4".to_owned()),
        UnderlineStyle::Double => params.push("21".to_owned()),
        UnderlineStyle::Curly => params.push("4:3".to_owned()),
        UnderlineStyle::Dotted => params.push("4:4".to_owned()),
        UnderlineStyle::Dashed => params.push("4:5".to_owned()),
    }
}

fn append_color_sgr(prefix: u8, color: Color, params: &mut Vec<String>) {
    match color {
        Color::Default => {}
        Color::Indexed(index) => {
            params.push(prefix.to_string());
            params.push("5".to_owned());
            params.push(index.to_string());
        }
        Color::Rgb(red, green, blue) => {
            params.push(prefix.to_string());
            params.push("2".to_owned());
            params.push(red.to_string());
            params.push(green.to_string());
            params.push(blue.to_string());
        }
        Color::Rgba(red, green, blue, alpha) => {
            params.push(prefix.to_string());
            params.push("6".to_owned());
            params.push(red.to_string());
            params.push(green.to_string());
            params.push(blue.to_string());
            params.push(alpha.to_string());
        }
    }
}

fn append_cursor_shape_state(shape: CursorShape, bytes: &mut Vec<u8>) {
    let value = match shape {
        CursorShape::Block => 2,
        CursorShape::Underline => 3,
        CursorShape::Bar => 5,
    };
    bytes.extend_from_slice(value.to_string().as_bytes());
    bytes.extend_from_slice(b" q");
}

fn append_scroll_region_state((top, bottom): (u16, u16), bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(top.saturating_add(1).to_string().as_bytes());
    bytes.push(b';');
    bytes.extend_from_slice(bottom.saturating_add(1).to_string().as_bytes());
    bytes.push(b'r');
}

fn append_left_right_margin_state((left, right): (u16, u16), bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(left.saturating_add(1).to_string().as_bytes());
    bytes.push(b';');
    bytes.extend_from_slice(right.saturating_add(1).to_string().as_bytes());
    bytes.push(b's');
}

fn decrqss_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_decrqss_query_prefix)
}

fn is_decrqss_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(UTF8_C1_DCS))
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes)
            || UTF8_C1_DCS.starts_with(bytes)
            || b"\x90".starts_with(bytes);
    };
    if !b"$q".starts_with(rest) && !rest.starts_with(b"$q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"$q") {
        return [b"m".as_slice(), b" q".as_slice(), b"r".as_slice()]
            .into_iter()
            .any(|target| target.starts_with(body));
    }
    true
}

struct XtGetTcapQuery {
    index: usize,
    consumed: usize,
    response: XtGetTcapResponse,
}

#[derive(Clone)]
struct XtGetTcapResponse {
    entries: Vec<XtGetTcapEntry>,
}

#[derive(Clone)]
struct XtGetTcapEntry {
    name_hex: Vec<u8>,
    value_hex: Option<Vec<u8>>,
}

impl XtGetTcapResponse {
    fn response(&self) -> Vec<u8> {
        let mut response = Vec::new();
        if self.entries.is_empty() {
            response.extend_from_slice(b"\x1bP0+r\x1b\\");
            return response;
        }

        for entry in &self.entries {
            if let Some(value_hex) = &entry.value_hex {
                response.extend_from_slice(b"\x1bP1+r");
                extend_ascii_hex_uppercase(&mut response, &entry.name_hex);
                response.push(b'=');
                extend_ascii_hex_uppercase(&mut response, value_hex);
            } else {
                response.extend_from_slice(b"\x1bP0+r");
                extend_ascii_hex_uppercase(&mut response, &entry.name_hex);
            }
            response.extend_from_slice(b"\x1b\\");
        }
        response
    }
}

fn find_xtgettcap_query(
    bytes: &[u8],
    size: TerminalSize,
    terminal_name: &str,
) -> Option<XtGetTcapQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in dcs_start_prefixes() {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) =
                parse_xtgettcap_query(bytes, index, prefix_len, size, terminal_name)
            {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &XtGetTcapQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_xtgettcap_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
    size: TerminalSize,
    terminal_name: &str,
) -> Option<XtGetTcapQuery> {
    let content_start = index + prefix_len;
    let rest = bytes.get(content_start..)?;
    let body = rest.strip_prefix(b"+q")?;
    let terminator = find_xtgettcap_terminator(body)?;
    let content = &body[..terminator.index];
    let entries = content
        .split(|byte| *byte == b';')
        .map(|entry| parse_xtgettcap_entry(entry, size, terminal_name))
        .collect();

    Some(XtGetTcapQuery {
        index,
        consumed: prefix_len + b"+q".len() + terminator.index + terminator.length,
        response: XtGetTcapResponse { entries },
    })
}

fn find_xtgettcap_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });
    let utf8_c1_st = find_subslice(bytes, UTF8_C1_ST).map(|index| OscColorTerminator {
        index,
        length: UTF8_C1_ST.len(),
        response_terminator: OscResponseTerminator::C1St,
    });

    [st, c1_st, utf8_c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_xtgettcap_entry(
    name_hex: &[u8],
    size: TerminalSize,
    terminal_name: &str,
) -> XtGetTcapEntry {
    let name = decode_ascii_hex(name_hex).map_or_else(
        || String::from_utf8_lossy(name_hex).into_owned().into_bytes(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned().into_bytes(),
    );
    let value_hex = xtgettcap_value_hex(&name, size, terminal_name);
    XtGetTcapEntry {
        name_hex: encode_ascii_hex(&name),
        value_hex,
    }
}
#[allow(clippy::match_same_arms, clippy::too_many_lines)]
fn xtgettcap_value_hex(name: &[u8], size: TerminalSize, terminal_name: &str) -> Option<Vec<u8>> {
    match name {
        b"Co" | b"colors" => Some(b"323536".to_vec()),
        b"TN" | b"name" => Some(encode_ascii_hex(terminal_name.as_bytes())),
        b"RGB" => Some(b"382f382f38".to_vec()),
        b"Tc" => Some(b"31".to_vec()),
        b"am" => Some(b"31".to_vec()),
        b"bce" => Some(b"31".to_vec()),
        b"ccc" => Some(b"31".to_vec()),
        b"hs" => Some(b"31".to_vec()),
        b"km" => Some(b"31".to_vec()),
        b"mc5i" => Some(b"31".to_vec()),
        b"mir" => Some(b"31".to_vec()),
        b"msgr" => Some(b"31".to_vec()),
        b"npc" => Some(b"31".to_vec()),
        b"Su" => Some(b"31".to_vec()),
        b"xenl" => Some(b"31".to_vec()),
        b"Ms" => Some(b"1b5d35323b25703125733b257032257307".to_vec()),
        b"dsl" => Some(encode_ascii_hex(b"\x1b]2;\x1b\\")),
        b"fsl" => Some(encode_ascii_hex(b"\x1b\\")),
        b"tsl" => Some(encode_ascii_hex(b"\x1b]0;")),
        b"initc" => Some(encode_ascii_hex(
            b"\x1b]4;%p1%d;rgb:%p2%{255}%*%{1000}%/%2.2X/%p3%{255}%*%{1000}%/%2.2X/%p4%{255}%*%{1000}%/%2.2X\x1b\\",
        )),
        b"Smulx" => Some(b"1b5b343a25703125646d".to_vec()),
        b"Setulc" => Some(
            b"1b5b35383a323a3a257031257b36353533367d252f25643a257031257b3235367d252f257b3235357d252625643a257031257b3235357d25262564253b6d"
                .to_vec(),
        ),
        b"Cr" => Some(encode_ascii_hex(b"\x1b]112\x07")),
        b"Cs" => Some(encode_ascii_hex(b"\x1b]12;%p1%s\x07")),
        b"Se" => Some(encode_ascii_hex(b"\x1b[2 q")),
        b"Ss" => Some(encode_ascii_hex(b"\x1b[%p1%d q")),
        b"Sync" => Some(encode_ascii_hex(b"\x1b[?2026%?%p1%{1}%-%tl%eh%;")),
        b"sitm" => Some(b"1b5b336d".to_vec()),
        b"ritm" => Some(b"1b5b32336d".to_vec()),
        b"Smol" => Some(encode_ascii_hex(b"\x1b[53m")),
        b"smxx" => Some(encode_ascii_hex(b"\x1b[9m")),
        b"rmxx" => Some(encode_ascii_hex(b"\x1b[29m")),
        b"flash" => Some(encode_ascii_hex(b"\x1b[?5h$<100/>\x1b[?5l")),
        b"op" => Some(encode_ascii_hex(b"\x1b[39;49m")),
        b"oc" => Some(encode_ascii_hex(b"\x1b]104\x07")),
        b"bel" => Some(encode_ascii_hex(b"\x07")),
        b"cr" => Some(encode_ascii_hex(b"\r")),
        b"ind" => Some(encode_ascii_hex(b"\n")),
        b"ri" => Some(encode_ascii_hex(b"\x1bM")),
        b"sc" => Some(encode_ascii_hex(b"\x1b7")),
        b"rc" => Some(encode_ascii_hex(b"\x1b8")),
        b"u6" => Some(encode_ascii_hex(b"\x1b[%i%d;%dR")),
        b"u7" => Some(encode_ascii_hex(b"\x1b[6n")),
        b"u8" => Some(encode_ascii_hex(b"\x1b[?%[;0123456789]c")),
        b"u9" => Some(encode_ascii_hex(b"\x1b[c")),
        b"clear" => Some(encode_ascii_hex(b"\x1b[H\x1b[2J")),
        b"cup" => Some(encode_ascii_hex(b"\x1b[%i%p1%d;%p2%dH")),
        b"home" => Some(encode_ascii_hex(b"\x1b[H")),
        b"el" => Some(encode_ascii_hex(b"\x1b[K")),
        b"ed" => Some(encode_ascii_hex(b"\x1b[J")),
        b"el1" => Some(encode_ascii_hex(b"\x1b[1K")),
        b"dch" => Some(encode_ascii_hex(b"\x1b[%p1%dP")),
        b"dch1" => Some(encode_ascii_hex(b"\x1b[P")),
        b"ich" => Some(encode_ascii_hex(b"\x1b[%p1%d@")),
        b"ich1" => Some(encode_ascii_hex(b"\x1b[@")),
        b"il" => Some(encode_ascii_hex(b"\x1b[%p1%dL")),
        b"il1" => Some(encode_ascii_hex(b"\x1b[L")),
        b"dl" => Some(encode_ascii_hex(b"\x1b[%p1%dM")),
        b"dl1" => Some(encode_ascii_hex(b"\x1b[M")),
        b"cuu" => Some(encode_ascii_hex(b"\x1b[%p1%dA")),
        b"cuu1" => Some(encode_ascii_hex(b"\x1b[A")),
        b"cud" => Some(encode_ascii_hex(b"\x1b[%p1%dB")),
        b"cud1" => Some(encode_ascii_hex(b"\n")),
        b"cub" => Some(encode_ascii_hex(b"\x1b[%p1%dD")),
        b"cub1" => Some(encode_ascii_hex(b"\x08")),
        b"cuf" => Some(encode_ascii_hex(b"\x1b[%p1%dC")),
        b"cuf1" => Some(encode_ascii_hex(b"\x1b[C")),
        b"hpa" => Some(encode_ascii_hex(b"\x1b[%i%p1%dG")),
        b"vpa" => Some(encode_ascii_hex(b"\x1b[%i%p1%dd")),
        b"cbt" => Some(encode_ascii_hex(b"\x1b[Z")),
        b"ht" => Some(encode_ascii_hex(b"\t")),
        b"hts" => Some(encode_ascii_hex(b"\x1bH")),
        b"tbc" => Some(encode_ascii_hex(b"\x1b[3g")),
        b"ech" => Some(encode_ascii_hex(b"\x1b[%p1%dX")),
        b"rep" => Some(encode_ascii_hex(b"%p1%c\x1b[%p2%{1}%-%db")),
        b"csr" => Some(encode_ascii_hex(b"\x1b[%i%p1%d;%p2%dr")),
        b"indn" => Some(encode_ascii_hex(b"\x1b[%p1%dS")),
        b"rin" => Some(encode_ascii_hex(b"\x1b[%p1%dT")),
        b"kmous" => Some(encode_ascii_hex(b"\x1b[<")),
        b"XM" => Some(encode_ascii_hex(
            b"\x1b[?1006;1000%?%p1%{1}%=%th%el%;",
        )),
        b"xm" => Some(encode_ascii_hex(
            b"\x1b[<%i%p3%d;%p1%d;%p2%d;%?%p4%tM%em%;",
        )),
        b"civis" => Some(encode_ascii_hex(b"\x1b[?25l")),
        b"cnorm" => Some(encode_ascii_hex(b"\x1b[?12l\x1b[?25h")),
        b"cvvis" => Some(encode_ascii_hex(b"\x1b[?12;25h")),
        b"smcup" => Some(encode_ascii_hex(b"\x1b[?1049h\x1b[22;0;0t")),
        b"rmcup" => Some(encode_ascii_hex(b"\x1b[?1049l\x1b[23;0;0t")),
        b"is2" | b"rs2" => Some(encode_ascii_hex(b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>")),
        b"rs1" => Some(encode_ascii_hex(b"\x1bc\x1b]104\x07")),
        b"smir" => Some(encode_ascii_hex(b"\x1b[4h")),
        b"rmir" => Some(encode_ascii_hex(b"\x1b[4l")),
        b"smam" => Some(encode_ascii_hex(b"\x1b[?7h")),
        b"rmam" => Some(encode_ascii_hex(b"\x1b[?7l")),
        b"smm" => Some(encode_ascii_hex(b"\x1b[?1034h")),
        b"rmm" => Some(encode_ascii_hex(b"\x1b[?1034l")),
        b"mc0" => Some(encode_ascii_hex(b"\x1b[i")),
        b"mc4" => Some(encode_ascii_hex(b"\x1b[4i")),
        b"mc5" => Some(encode_ascii_hex(b"\x1b[5i")),
        b"meml" => Some(encode_ascii_hex(b"\x1bl")),
        b"memu" => Some(encode_ascii_hex(b"\x1bm")),
        b"smkx" => Some(encode_ascii_hex(b"\x1b[?1h\x1b=")),
        b"rmkx" => Some(encode_ascii_hex(b"\x1b[?1l\x1b>")),
        b"sgr0" => Some(encode_ascii_hex(b"\x1b(B\x1b[m")),
        b"sgr" => Some(encode_ascii_hex(
            b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m",
        )),
        b"bold" => Some(encode_ascii_hex(b"\x1b[1m")),
        b"dim" => Some(encode_ascii_hex(b"\x1b[2m")),
        b"blink" => Some(encode_ascii_hex(b"\x1b[5m")),
        b"rev" => Some(encode_ascii_hex(b"\x1b[7m")),
        b"smso" => Some(encode_ascii_hex(b"\x1b[7m")),
        b"rmso" => Some(encode_ascii_hex(b"\x1b[27m")),
        b"invis" => Some(encode_ascii_hex(b"\x1b[8m")),
        b"smul" => Some(encode_ascii_hex(b"\x1b[4m")),
        b"rmul" => Some(encode_ascii_hex(b"\x1b[24m")),
        b"setaf" => Some(encode_ascii_hex(
            b"\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m",
        )),
        b"setab" => Some(encode_ascii_hex(
            b"\x1b[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m",
        )),
        b"kcuu1" => Some(encode_ascii_hex(b"\x1bOA")),
        b"kcud1" => Some(encode_ascii_hex(b"\x1bOB")),
        b"kcuf1" => Some(encode_ascii_hex(b"\x1bOC")),
        b"kcub1" => Some(encode_ascii_hex(b"\x1bOD")),
        b"kb2" => Some(encode_ascii_hex(b"\x1bOE")),
        b"kbs" => Some(encode_ascii_hex(b"\x7f")),
        b"kcbt" => Some(encode_ascii_hex(b"\x1b[Z")),
        b"khome" => Some(encode_ascii_hex(b"\x1bOH")),
        b"kend" => Some(encode_ascii_hex(b"\x1bOF")),
        b"kich1" => Some(encode_ascii_hex(b"\x1b[2~")),
        b"kdch1" => Some(encode_ascii_hex(b"\x1b[3~")),
        b"kpp" => Some(encode_ascii_hex(b"\x1b[5~")),
        b"knp" => Some(encode_ascii_hex(b"\x1b[6~")),
        b"kHOM" => Some(encode_ascii_hex(b"\x1b[1;2H")),
        b"kEND" => Some(encode_ascii_hex(b"\x1b[1;2F")),
        b"kIC" => Some(encode_ascii_hex(b"\x1b[2;2~")),
        b"kDC" => Some(encode_ascii_hex(b"\x1b[3;2~")),
        b"kPRV" => Some(encode_ascii_hex(b"\x1b[5;2~")),
        b"kNXT" => Some(encode_ascii_hex(b"\x1b[6;2~")),
        b"kLFT" => Some(encode_ascii_hex(b"\x1b[1;2D")),
        b"kRIT" => Some(encode_ascii_hex(b"\x1b[1;2C")),
        b"kri" => Some(encode_ascii_hex(b"\x1b[1;2A")),
        b"kind" => Some(encode_ascii_hex(b"\x1b[1;2B")),
        b"kent" => Some(encode_ascii_hex(b"\x1bOM")),
        b"kf1" => Some(encode_ascii_hex(b"\x1bOP")),
        b"kf2" => Some(encode_ascii_hex(b"\x1bOQ")),
        b"kf3" => Some(encode_ascii_hex(b"\x1bOR")),
        b"kf4" => Some(encode_ascii_hex(b"\x1bOS")),
        b"kf5" => Some(encode_ascii_hex(b"\x1b[15~")),
        b"kf6" => Some(encode_ascii_hex(b"\x1b[17~")),
        b"kf7" => Some(encode_ascii_hex(b"\x1b[18~")),
        b"kf8" => Some(encode_ascii_hex(b"\x1b[19~")),
        b"kf9" => Some(encode_ascii_hex(b"\x1b[20~")),
        b"kf10" => Some(encode_ascii_hex(b"\x1b[21~")),
        b"kf11" => Some(encode_ascii_hex(b"\x1b[23~")),
        b"kf12" => Some(encode_ascii_hex(b"\x1b[24~")),
        name if name.starts_with(b"kf") => xtgettcap_modified_function_key_hex(name),
        b"enacs" => Some(encode_ascii_hex(b"\x1b)0")),
        b"smacs" => Some(encode_ascii_hex(b"\x1b(0")),
        b"rmacs" => Some(encode_ascii_hex(b"\x1b(B")),
        b"acsc" => Some(encode_ascii_hex(
            b"``aaffggiijjkkllmmnnooppqqrrssttuuvvwwxxyyzz{{||}}~~",
        )),
        b"co" | b"cols" => Some(decimal_value_hex(size.columns)),
        b"li" | b"lines" => Some(decimal_value_hex(size.rows)),
        b"it" => Some(decimal_value_hex(8)),
        b"pairs" => Some(decimal_value_hex(0x7fff)),
        _ => None,
    }
}

fn xtgettcap_modified_function_key_hex(name: &[u8]) -> Option<Vec<u8>> {
    let number = parse_ascii_decimal_u8(name.strip_prefix(b"kf")?)?;
    let (function_key, modifier) = match number {
        13..=24 => (number - 12, 2),
        25..=36 => (number - 24, 5),
        37..=48 => (number - 36, 6),
        49..=60 => (number - 48, 3),
        61..=63 => (number - 60, 4),
        _ => return None,
    };

    let sequence = match function_key {
        1 => format!("\x1b[1;{modifier}P"),
        2 => format!("\x1b[1;{modifier}Q"),
        3 => format!("\x1b[1;{modifier}R"),
        4 => format!("\x1b[1;{modifier}S"),
        5 => format!("\x1b[15;{modifier}~"),
        6 => format!("\x1b[17;{modifier}~"),
        7 => format!("\x1b[18;{modifier}~"),
        8 => format!("\x1b[19;{modifier}~"),
        9 => format!("\x1b[20;{modifier}~"),
        10 => format!("\x1b[21;{modifier}~"),
        11 => format!("\x1b[23;{modifier}~"),
        12 => format!("\x1b[24;{modifier}~"),
        _ => return None,
    };

    Some(encode_ascii_hex(sequence.as_bytes()))
}

fn parse_ascii_decimal_u8(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0u8;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(byte - b'0')?;
    }
    Some(value)
}

fn parse_ascii_decimal_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0u64;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
    }
    Some(value)
}

fn decimal_value_hex(value: u16) -> Vec<u8> {
    encode_ascii_hex(value.to_string().as_bytes())
}

fn encode_ascii_hex(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = Vec::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)]);
        encoded.push(HEX[usize::from(byte & 0x0f)]);
    }
    encoded
}

fn extend_ascii_hex_uppercase(output: &mut Vec<u8>, hex: &[u8]) {
    output.extend(hex.iter().map(u8::to_ascii_uppercase));
}

fn decode_ascii_hex(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| Some(parse_hex_digit(pair[0])? * 16 + parse_hex_digit(pair[1])?))
        .collect()
}

fn xtgettcap_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_xtgettcap_query_prefix)
}

fn is_xtgettcap_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(UTF8_C1_DCS))
        .or_else(|| bytes.strip_prefix(b"\x90"))
    else {
        return b"\x1bP".starts_with(bytes)
            || UTF8_C1_DCS.starts_with(bytes)
            || b"\x90".starts_with(bytes);
    };
    if !b"+q".starts_with(rest) && !rest.starts_with(b"+q") {
        return false;
    }
    if let Some(body) = rest.strip_prefix(b"+q") {
        return body
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() || *byte == b';');
    }
    true
}

struct XtSmGraphicsQuery {
    index: usize,
    consumed: usize,
    request: XtSmGraphicsRequest,
}

#[derive(Clone, Copy)]
struct XtSmGraphicsRequest {
    item: u64,
    action: u64,
}

impl XtSmGraphicsRequest {
    const ACTION_READ_ATTRIBUTE: u64 = 1;
    const ACTION_RESET_TO_DEFAULT: u64 = 2;
    const ACTION_READ_MAXIMUM_ALLOWED_VALUE: u64 = 4;
    const ITEM_NUMBER_OF_COLOR_REGISTERS: u64 = 1;
    const ITEM_SIXEL_GRAPHICS_GEOMETRY: u64 = 2;
    const ITEM_REGIS_GRAPHICS_GEOMETRY: u64 = 3;
    const STATUS_SUCCESS: u64 = 0;
    const STATUS_INVALID_ITEM: u64 = 1;
    const STATUS_INVALID_ACTION: u64 = 2;

    fn response(self, size: TerminalSize) -> Vec<u8> {
        let (status, values) = self.status_and_values(size);
        let mut response = format!("\x1b[?{};{}", self.item, status);
        for value in values {
            response.push(';');
            response.push_str(&value.to_string());
        }
        response.push('S');
        response.into_bytes()
    }

    fn status_and_values(self, size: TerminalSize) -> (u64, Vec<u32>) {
        if !matches!(
            self.item,
            Self::ITEM_NUMBER_OF_COLOR_REGISTERS
                | Self::ITEM_SIXEL_GRAPHICS_GEOMETRY
                | Self::ITEM_REGIS_GRAPHICS_GEOMETRY
        ) {
            return (Self::STATUS_INVALID_ITEM, Vec::new());
        }

        match self.action {
            Self::ACTION_READ_ATTRIBUTE | Self::ACTION_READ_MAXIMUM_ALLOWED_VALUE => {
                (Self::STATUS_SUCCESS, self.values(size))
            }
            Self::ACTION_RESET_TO_DEFAULT => (Self::STATUS_SUCCESS, Vec::new()),
            _ => (Self::STATUS_INVALID_ACTION, Vec::new()),
        }
    }

    fn values(self, size: TerminalSize) -> Vec<u32> {
        match self.item {
            Self::ITEM_NUMBER_OF_COLOR_REGISTERS => vec![65_536],
            Self::ITEM_SIXEL_GRAPHICS_GEOMETRY | Self::ITEM_REGIS_GRAPHICS_GEOMETRY => vec![
                u32::from(size.columns) * u32::from(TerminalOutputFilter::CELL_WIDTH_PIXELS),
                u32::from(size.rows) * u32::from(TerminalOutputFilter::CELL_HEIGHT_PIXELS),
            ],
            _ => Vec::new(),
        }
    }
}

fn find_xtsmgraphics_query(bytes: &[u8]) -> Option<XtSmGraphicsQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in csi_start_prefixes() {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_xtsmgraphics_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &XtSmGraphicsQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_xtsmgraphics_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<XtSmGraphicsQuery> {
    let content_start = index + prefix_len;
    let body = bytes.get(content_start..)?.strip_prefix(b"?")?;
    let final_index = body.iter().position(|byte| *byte == b'S')?;
    let content = &body[..final_index];
    let mut parameters = content.split(|byte| *byte == b';');
    let item = parse_ascii_decimal_u64(parameters.next()?)?;
    let action = parse_ascii_decimal_u64(parameters.next()?)?;
    for parameter in parameters {
        parse_ascii_decimal_u64(parameter)?;
    }

    Some(XtSmGraphicsQuery {
        index,
        consumed: prefix_len + b"?".len() + final_index + b"S".len(),
        request: XtSmGraphicsRequest { item, action },
    })
}

fn xtsmgraphics_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_xtsmgraphics_query_prefix)
}

fn is_xtsmgraphics_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[")
        .or_else(|| bytes.strip_prefix(b"\x9b"))
        .or_else(|| bytes.strip_prefix(UTF8_C1_CSI))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x9b".starts_with(bytes)
            || UTF8_C1_CSI.starts_with(bytes);
    };

    if !b"?".starts_with(rest) && !rest.starts_with(b"?") {
        return false;
    }
    let Some(body) = rest.strip_prefix(b"?") else {
        return true;
    };
    !body.contains(&b'S')
        && body
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b';')
}

struct OscColorQuery {
    index: usize,
    consumed: usize,
    query: OscColorResponse,
}

#[derive(Clone)]
struct OscColorResponse {
    kinds: Vec<OscColorKind>,
    terminator: OscResponseTerminator,
}

#[derive(Clone, Copy)]
enum OscColorKind {
    DefaultForeground,
    DefaultBackground,
    Cursor,
    Palette(u8),
}

#[derive(Clone, Copy)]
enum OscResponseTerminator {
    Bel,
    St,
    C1St,
}

struct OscColorTerminator {
    index: usize,
    length: usize,
    response_terminator: OscResponseTerminator,
}

fn find_osc_color_query(bytes: &[u8]) -> Option<OscColorQuery> {
    let mut match_query = None;
    for &(prefix, prefix_len) in OSC_START_PREFIXES {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_osc_color_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &OscColorQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_osc_color_query(bytes: &[u8], index: usize, prefix_len: usize) -> Option<OscColorQuery> {
    let content_start = index + prefix_len;
    let terminator = find_osc_color_terminator(&bytes[content_start..])?;
    let content_end = content_start + terminator.index;
    let kinds = parse_osc_color_query_content(&bytes[content_start..content_end])?;

    Some(OscColorQuery {
        index,
        consumed: content_end + terminator.length - index,
        query: OscColorResponse {
            kinds,
            terminator: terminator.response_terminator,
        },
    })
}

fn find_osc_color_terminator(bytes: &[u8]) -> Option<OscColorTerminator> {
    let bel = bytes
        .iter()
        .position(|byte| *byte == b'\x07')
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::Bel,
        });
    let st = find_subslice(bytes, b"\x1b\\").map(|index| OscColorTerminator {
        index,
        length: 2,
        response_terminator: OscResponseTerminator::St,
    });
    let c1_st = bytes
        .iter()
        .position(|byte| *byte == 0x9c)
        .map(|index| OscColorTerminator {
            index,
            length: 1,
            response_terminator: OscResponseTerminator::C1St,
        });
    let utf8_c1_st = find_subslice(bytes, UTF8_C1_ST).map(|index| OscColorTerminator {
        index,
        length: UTF8_C1_ST.len(),
        response_terminator: OscResponseTerminator::C1St,
    });

    [bel, st, c1_st, utf8_c1_st]
        .into_iter()
        .flatten()
        .min_by_key(|terminator| terminator.index)
}

fn parse_osc_color_query_content(content: &[u8]) -> Option<Vec<OscColorKind>> {
    match content {
        b"10;?" => Some(vec![OscColorKind::DefaultForeground]),
        b"11;?" => Some(vec![OscColorKind::DefaultBackground]),
        b"12;?" => Some(vec![OscColorKind::Cursor]),
        _ => parse_palette_color_query(content),
    }
}

fn parse_palette_color_query(content: &[u8]) -> Option<Vec<OscColorKind>> {
    let rest = content.strip_prefix(b"4;")?;
    let mut parts = rest.split(|byte| *byte == b';');
    let mut kinds = Vec::new();

    while let Some(index) = parts.next() {
        let marker = parts.next()?;
        if marker != b"?" {
            return None;
        }
        kinds.push(OscColorKind::Palette(parse_u8_decimal(index)?));
    }

    (!kinds.is_empty()).then_some(kinds)
}

fn parse_u8_decimal(bytes: &[u8]) -> Option<u8> {
    if bytes.is_empty() {
        return None;
    }
    let mut value = 0u16;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(u16::from(*byte - b'0'));
    }
    u8::try_from(value).ok()
}

fn osc_color_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_osc_color_query_prefix)
}

fn is_osc_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b]")
        .or_else(|| bytes.strip_prefix(b"\x9d"))
        .or_else(|| bytes.strip_prefix(UTF8_C1_OSC))
    else {
        return b"\x1b]".starts_with(bytes)
            || b"\x9d".starts_with(bytes)
            || UTF8_C1_OSC.starts_with(bytes);
    };

    b"10;?".starts_with(rest)
        || b"11;?".starts_with(rest)
        || b"12;?".starts_with(rest)
        || is_palette_color_query_prefix(rest)
}

fn is_palette_color_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"4") else {
        return bytes.is_empty();
    };
    let Some(mut rest) = rest.strip_prefix(b";") else {
        return rest.is_empty();
    };

    loop {
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        if digits == 0 {
            return rest.is_empty();
        }
        rest = &rest[digits..];
        if rest.is_empty() {
            return true;
        }
        let Some(after_separator) = rest.strip_prefix(b";") else {
            return false;
        };
        if after_separator.is_empty() {
            return true;
        }
        let Some(after_query_marker) = after_separator.strip_prefix(b"?") else {
            return false;
        };
        rest = after_query_marker;
        if rest.is_empty() {
            return true;
        }
        let Some(after_next_separator) = rest.strip_prefix(b";") else {
            return false;
        };
        rest = after_next_separator;
    }
}

struct ItermReportCellSizeQuery {
    index: usize,
    consumed: usize,
}

fn find_iterm_report_cell_size_query(bytes: &[u8]) -> Option<ItermReportCellSizeQuery> {
    let mut match_query = None;
    for &(prefix, prefix_len) in OSC_START_PREFIXES {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_iterm_report_cell_size_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &ItermReportCellSizeQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_iterm_report_cell_size_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<ItermReportCellSizeQuery> {
    let content_start = index + prefix_len;
    let terminator = find_osc_terminator(&bytes[content_start..])?;
    let content_end = content_start + terminator.index;

    if &bytes[content_start..content_end] != b"1337;ReportCellSize" {
        return None;
    }

    Some(ItermReportCellSizeQuery {
        index,
        consumed: content_end + terminator.length - index,
    })
}

struct TerminalColorState {
    foreground: DynamicColor,
    background: DynamicColor,
    cursor_override: Option<DynamicColor>,
    palette_overrides: Vec<(u8, [u8; 3])>,
    pending: Vec<u8>,
}

impl Default for TerminalColorState {
    fn default() -> Self {
        Self {
            foreground: DynamicColor::rgb8(DEFAULT_FOREGROUND),
            background: DynamicColor::rgb8(DEFAULT_BACKGROUND),
            cursor_override: None,
            palette_overrides: Vec::new(),
            pending: Vec::new(),
        }
    }
}

impl TerminalColorState {
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some((index, prefix_len)) = find_next_osc_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if is_inside_osc_or_st_control_string(&self.pending, index) {
                self.pending.drain(..index.saturating_add(1));
                continue;
            }
            if index > 0 {
                self.pending.drain(..index);
            }

            let content_start = prefix_len;
            let Some(terminator) = find_osc_color_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            if let Some(change) = parse_osc_color_change(&self.pending[content_start..content_end])
            {
                self.apply(change);
            }
            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn response(&self, query: OscColorResponse) -> Vec<u8> {
        let mut response = Vec::new();
        for kind in query.kinds {
            let mut item = match kind {
                OscColorKind::DefaultForeground => {
                    format!("\x1b]10;{}", color_response(self.foreground)).into_bytes()
                }
                OscColorKind::DefaultBackground => {
                    format!("\x1b]11;{}", color_response(self.background)).into_bytes()
                }
                OscColorKind::Cursor => {
                    format!("\x1b]12;{}", color_response(self.cursor_color())).into_bytes()
                }
                OscColorKind::Palette(index) => format!(
                    "\x1b]4;{};{}",
                    index,
                    palette_color_response(self.palette_color(index))
                )
                .into_bytes(),
            };
            item.extend_from_slice(query.terminator.bytes());
            response.extend(item);
        }
        response
    }

    fn apply(&mut self, change: OscColorChange) {
        match change {
            OscColorChange::DefaultForeground(color) => self.foreground = color,
            OscColorChange::DefaultBackground(color) => self.background = color,
            OscColorChange::Cursor(color) => self.cursor_override = Some(color),
            OscColorChange::ResetDefaultForeground => {
                self.foreground = DynamicColor::rgb8(DEFAULT_FOREGROUND);
            }
            OscColorChange::ResetDefaultBackground => {
                self.background = DynamicColor::rgb8(DEFAULT_BACKGROUND);
            }
            OscColorChange::ResetCursor => self.cursor_override = None,
            OscColorChange::ResetPalette(indices) => self
                .palette_overrides
                .retain(|(palette_index, _)| !indices.contains(palette_index)),
            OscColorChange::ResetPaletteAll => self.palette_overrides.clear(),
            OscColorChange::Palette(changes) => {
                for (index, color) in changes {
                    if let Some((_, existing)) = self
                        .palette_overrides
                        .iter_mut()
                        .find(|(palette_index, _)| *palette_index == index)
                    {
                        *existing = color;
                    } else {
                        self.palette_overrides.push((index, color));
                    }
                }
            }
        }
    }

    fn palette_color(&self, index: u8) -> [u8; 3] {
        self.palette_overrides
            .iter()
            .find_map(|(palette_index, color)| (*palette_index == index).then_some(*color))
            .unwrap_or_else(|| indexed_color(index))
    }

    fn cursor_color(&self) -> DynamicColor {
        self.cursor_override
            .unwrap_or_else(|| DynamicColor::rgb8(DEFAULT_CURSOR))
    }

    fn cursor_override(&self) -> Option<DynamicColor> {
        self.cursor_override
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [b"\x1b]".as_slice(), b"\x9d".as_slice(), UTF8_C1_OSC]
            .into_iter()
            .map(|prefix| suffix_prefix_len(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

#[derive(Clone)]
enum OscColorChange {
    DefaultForeground(DynamicColor),
    DefaultBackground(DynamicColor),
    Cursor(DynamicColor),
    ResetDefaultForeground,
    ResetDefaultBackground,
    ResetCursor,
    ResetPalette(Vec<u8>),
    ResetPaletteAll,
    Palette(Vec<(u8, [u8; 3])>),
}

fn find_next_osc_start(bytes: &[u8]) -> Option<(usize, usize)> {
    OSC_START_PREFIXES
        .iter()
        .copied()
        .filter_map(|(prefix, prefix_len)| {
            find_subslice(bytes, prefix).map(|index| (index, prefix_len))
        })
        .min_by_key(|(index, _)| *index)
}

fn find_next_st_control_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [
        (b"\x1bP".as_slice(), 2),
        (b"\x1bX".as_slice(), 2),
        (b"\x1b^".as_slice(), 2),
        (b"\x1b_".as_slice(), 2),
        (UTF8_C1_DCS, UTF8_C1_DCS.len()),
        (b"\x90".as_slice(), 1),
        (b"\x98".as_slice(), 1),
        (b"\x9e".as_slice(), 1),
        (b"\x9f".as_slice(), 1),
    ]
    .into_iter()
    .filter_map(|(prefix, prefix_len)| {
        find_subslice(bytes, prefix).map(|index| (index, prefix_len))
    })
    .min_by_key(|(index, _)| *index)
}

fn parse_osc_color_change(content: &[u8]) -> Option<OscColorChange> {
    if let Some(color) = content.strip_prefix(b"10;").and_then(parse_color_spec) {
        return Some(OscColorChange::DefaultForeground(color));
    }
    if let Some(color) = content.strip_prefix(b"11;").and_then(parse_color_spec) {
        return Some(OscColorChange::DefaultBackground(color));
    }
    if let Some(color) = content.strip_prefix(b"12;").and_then(parse_color_spec) {
        return Some(OscColorChange::Cursor(color));
    }
    if matches!(content, b"110" | b"110;") {
        return Some(OscColorChange::ResetDefaultForeground);
    }
    if matches!(content, b"111" | b"111;") {
        return Some(OscColorChange::ResetDefaultBackground);
    }
    if matches!(content, b"112" | b"112;") {
        return Some(OscColorChange::ResetCursor);
    }
    if let Some(change) = parse_palette_reset_change(content) {
        return Some(change);
    }
    parse_palette_color_change(content)
}

fn parse_palette_reset_change(content: &[u8]) -> Option<OscColorChange> {
    if matches!(content, b"104" | b"104;") {
        return Some(OscColorChange::ResetPaletteAll);
    }
    let rest = content.strip_prefix(b"104;")?;
    let mut indices = Vec::new();
    for index in rest.split(|byte| *byte == b';') {
        indices.push(parse_u8_decimal(index)?);
    }
    (!indices.is_empty()).then_some(OscColorChange::ResetPalette(indices))
}

fn parse_palette_color_change(content: &[u8]) -> Option<OscColorChange> {
    let rest = content.strip_prefix(b"4;")?;
    let mut changes = Vec::new();
    let mut parts = rest.split(|byte| *byte == b';');

    while let Some(index) = parts.next() {
        let color = parts.next()?;
        changes.push((parse_u8_decimal(index)?, parse_color_spec(color)?.to_rgb8()));
    }

    (!changes.is_empty()).then_some(OscColorChange::Palette(changes))
}

fn parse_color_spec(value: &[u8]) -> Option<DynamicColor> {
    if let Some(hex) = value.strip_prefix(b"#") {
        return parse_hex_color_spec(hex);
    }
    if let Some(rest) = value.strip_prefix(b"rgba:") {
        return parse_slash_rgba_color_spec(rest);
    }
    if value.starts_with(b"rgba(") {
        return parse_function_rgba_color_spec(value);
    }

    let rest = value.strip_prefix(b"rgb:")?;
    let mut components = rest.split(|byte| *byte == b'/');
    let red = parse_rgb_component(components.next()?)?;
    let green = parse_rgb_component(components.next()?)?;
    let blue = parse_rgb_component(components.next()?)?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgb(red, green, blue))
}

fn parse_hex_color_spec(hex: &[u8]) -> Option<DynamicColor> {
    match hex.len() {
        3 => Some(DynamicColor::rgb8([
            parse_hex_digit(hex[0])? * 17,
            parse_hex_digit(hex[1])? * 17,
            parse_hex_digit(hex[2])? * 17,
        ])),
        6 => Some(DynamicColor::rgb8([
            parse_hex_byte(&hex[0..2])?,
            parse_hex_byte(&hex[2..4])?,
            parse_hex_byte(&hex[4..6])?,
        ])),
        _ => None,
    }
}

fn parse_slash_rgba_color_spec(value: &[u8]) -> Option<DynamicColor> {
    let mut components = value.split(|byte| *byte == b'/');
    let red = parse_hex_component16(components.next()?)?;
    let green = parse_hex_component16(components.next()?)?;
    let blue = parse_hex_component16(components.next()?)?;
    let alpha = parse_hex_component16(components.next()?)?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgba(red, green, blue, alpha))
}

fn parse_function_rgba_color_spec(value: &[u8]) -> Option<DynamicColor> {
    let inner = value.strip_prefix(b"rgba(")?.strip_suffix(b")")?;
    let mut components = inner.split(|byte| *byte == b',');
    let red = parse_u8_decimal(components.next()?.trim_ascii())?;
    let green = parse_u8_decimal(components.next()?.trim_ascii())?;
    let blue = parse_u8_decimal(components.next()?.trim_ascii())?;
    let alpha = parse_alpha_float_component(components.next()?.trim_ascii())?;
    components
        .next()
        .is_none()
        .then_some(DynamicColor::rgba8(red, green, blue, alpha))
}

fn parse_rgb_component(component: &[u8]) -> Option<u16> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| u16::from(value) * 0x1111),
        2 => parse_hex_byte(component).map(DynamicColor::expand_byte),
        3 | 4 => parse_hex_component16(component),
        _ => None,
    }
}

fn parse_hex_component16(component: &[u8]) -> Option<u16> {
    match component.len() {
        1 => parse_hex_digit(component[0]).map(|value| u16::from(value) * 0x1111),
        2 => parse_hex_byte(component).map(DynamicColor::expand_byte),
        3 => Some(
            parse_hex_digit(component[0]).map(u16::from)? * 0x1000
                + parse_hex_digit(component[1]).map(u16::from)? * 0x100
                + parse_hex_digit(component[2]).map(u16::from)? * 0x10,
        ),
        4 => Some(
            parse_hex_digit(component[0]).map(u16::from)? * 0x1000
                + parse_hex_digit(component[1]).map(u16::from)? * 0x100
                + parse_hex_digit(component[2]).map(u16::from)? * 0x10
                + parse_hex_digit(component[3]).map(u16::from)?,
        ),
        _ => None,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn parse_alpha_float_component(component: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(component).ok()?;
    let value = text.parse::<f32>().ok()?;
    if !(0.0..=1.0).contains(&value) {
        return None;
    }
    Some((value * f32::from(u16::MAX)).round() as u16)
}

fn parse_hex_byte(bytes: &[u8]) -> Option<u8> {
    Some(parse_hex_digit(bytes[0])? * 16 + parse_hex_digit(bytes[1])?)
}

fn parse_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl OscResponseTerminator {
    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Bel => b"\x07",
            Self::St => b"\x1b\\",
            Self::C1St => b"\x9c",
        }
    }
}

const DEFAULT_FOREGROUND: [u8; 3] = [229, 229, 229];
const DEFAULT_BACKGROUND: [u8; 3] = [12, 12, 12];
const DEFAULT_CURSOR: [u8; 3] = DEFAULT_FOREGROUND;

#[derive(Clone, Copy)]
struct DynamicColor {
    red: u16,
    green: u16,
    blue: u16,
    alpha: Option<u16>,
}

impl DynamicColor {
    const fn rgb8(color: [u8; 3]) -> Self {
        Self::rgb(
            color[0] as u16 * 0x101,
            color[1] as u16 * 0x101,
            color[2] as u16 * 0x101,
        )
    }

    const fn rgb(red: u16, green: u16, blue: u16) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: None,
        }
    }

    const fn rgba(red: u16, green: u16, blue: u16, alpha: u16) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: Some(alpha),
        }
    }

    const fn rgba8(red: u8, green: u8, blue: u8, alpha: u16) -> Self {
        Self::rgba(
            Self::expand_byte(red),
            Self::expand_byte(green),
            Self::expand_byte(blue),
            alpha,
        )
    }

    const fn expand_byte(value: u8) -> u16 {
        value as u16 * 0x101
    }

    const fn to_rgb8(self) -> [u8; 3] {
        [
            (self.red >> 8) as u8,
            (self.green >> 8) as u8,
            (self.blue >> 8) as u8,
        ]
    }

    const fn to_color(self) -> Color {
        let red = (self.red >> 8) as u8;
        let green = (self.green >> 8) as u8;
        let blue = (self.blue >> 8) as u8;
        match self.alpha {
            Some(alpha) => Color::Rgba(red, green, blue, (alpha >> 8) as u8),
            None => Color::Rgb(red, green, blue),
        }
    }
}

fn color_response(color: DynamicColor) -> String {
    match color.alpha {
        Some(alpha) => format!(
            "rgba:{:04x}/{:04x}/{:04x}/{:04x}",
            color.red, color.green, color.blue, alpha
        ),
        None => format!(
            "rgb:{:04x}/{:04x}/{:04x}",
            color.red, color.green, color.blue
        ),
    }
}

fn palette_color_response(color: [u8; 3]) -> String {
    color_response(DynamicColor::rgb8(color))
}

fn indexed_color(index: u8) -> [u8; 3] {
    const ANSI: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 49, 49],
        [13, 188, 121],
        [229, 229, 16],
        [36, 114, 200],
        [188, 63, 188],
        [17, 168, 205],
        [229, 229, 229],
        [102, 102, 102],
        [241, 76, 76],
        [35, 209, 139],
        [245, 245, 67],
        [59, 142, 234],
        [214, 112, 214],
        [41, 184, 219],
        [255, 255, 255],
    ];

    if let Some(color) = ANSI.get(usize::from(index)) {
        return *color;
    }

    if (16..=231).contains(&index) {
        let cube_index = index - 16;
        return [
            xterm_color_cube_intensity(cube_index / 36),
            xterm_color_cube_intensity((cube_index / 6) % 6),
            xterm_color_cube_intensity(cube_index % 6),
        ];
    }

    let level = 8 + (index - 232) * 10;
    [level, level, level]
}

const fn xterm_color_cube_intensity(value: u8) -> u8 {
    if value == 0 { 0 } else { 55 + value * 40 }
}

struct PrivateModeStatusQuery {
    index: usize,
    consumed: usize,
    mode: u16,
}

struct AnsiModeStatusQuery {
    index: usize,
    consumed: usize,
    mode: u16,
}

fn find_private_mode_status_query(bytes: &[u8]) -> Option<PrivateModeStatusQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [
        (b"\x1b[?".as_slice(), b"\x1b[?".len()),
        (b"\x9b?".as_slice(), b"\x9b?".len()),
        (UTF8_C1_PRIVATE_CSI, UTF8_C1_PRIVATE_CSI.len()),
    ] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_private_mode_status_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &PrivateModeStatusQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn find_ansi_mode_status_query(bytes: &[u8]) -> Option<AnsiModeStatusQuery> {
    let mut match_query = None;
    for (prefix, prefix_len) in [
        (b"\x1b[".as_slice(), b"\x1b[".len()),
        (b"\x9b".as_slice(), b"\x9b".len()),
        (UTF8_C1_CSI, UTF8_C1_CSI.len()),
    ] {
        let mut offset = 0;
        while offset < bytes.len() {
            let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                break;
            };
            let index = offset + relative_index;
            if let Some(query) = parse_ansi_mode_status_query(bytes, index, prefix_len) {
                if match_query
                    .as_ref()
                    .is_none_or(|current: &AnsiModeStatusQuery| query.index < current.index)
                {
                    match_query = Some(query);
                }
            }
            offset = index.saturating_add(1);
        }
    }
    match_query
}

fn parse_private_mode_status_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<PrivateModeStatusQuery> {
    let mut cursor = index + prefix_len;
    let start = cursor;
    let mut mode = 0u16;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        mode = mode
            .saturating_mul(10)
            .saturating_add(u16::from(bytes[cursor] - b'0'));
        cursor += 1;
    }
    if cursor == start || bytes.get(cursor..cursor + 2) != Some(b"$p") {
        return None;
    }
    Some(PrivateModeStatusQuery {
        index,
        consumed: cursor + 2 - index,
        mode,
    })
}

fn parse_ansi_mode_status_query(
    bytes: &[u8],
    index: usize,
    prefix_len: usize,
) -> Option<AnsiModeStatusQuery> {
    let mut cursor = index + prefix_len;
    let start = cursor;
    let mut mode = 0u16;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        mode = mode
            .saturating_mul(10)
            .saturating_add(u16::from(bytes[cursor] - b'0'));
        cursor += 1;
    }
    if cursor == start || bytes.get(cursor..cursor + 2) != Some(b"$p") {
        return None;
    }
    Some(AnsiModeStatusQuery {
        index,
        consumed: cursor + 2 - index,
        mode,
    })
}

fn private_mode_status_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_private_mode_status_query_prefix)
}

fn ansi_mode_status_query_suffix_len(bytes: &[u8]) -> usize {
    suffix_len_matching_query_prefix(bytes, is_ansi_mode_status_query_prefix)
}

fn is_private_mode_status_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[?")
        .or_else(|| bytes.strip_prefix(b"\x9b?"))
        .or_else(|| bytes.strip_prefix(UTF8_C1_PRIVATE_CSI))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x1b[?".starts_with(bytes)
            || b"\x9b?".starts_with(bytes)
            || UTF8_C1_PRIVATE_CSI.starts_with(bytes);
    };

    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b"$"
}

fn is_ansi_mode_status_query_prefix(bytes: &[u8]) -> bool {
    let Some(rest) = bytes
        .strip_prefix(b"\x1b[")
        .or_else(|| bytes.strip_prefix(b"\x9b"))
        .or_else(|| bytes.strip_prefix(UTF8_C1_CSI))
    else {
        return b"\x1b[".starts_with(bytes)
            || b"\x9b".starts_with(bytes)
            || UTF8_C1_CSI.starts_with(bytes);
    };

    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    if digits == 0 {
        return rest.is_empty();
    }
    let tail = &rest[digits..];
    tail.is_empty() || tail == b"$"
}

fn suffix_prefix_len(bytes: &[u8], prefix: &[u8]) -> usize {
    let max_len = bytes.len().min(prefix.len().saturating_sub(1));

    (1..=max_len)
        .rev()
        .find(|&length| {
            let suffix_start = bytes.len() - length;
            bytes[suffix_start..] == prefix[..length]
                && !raw_c1_prefix_is_utf8_continuation(bytes, suffix_start, &prefix[..length])
        })
        .unwrap_or(0)
}

fn suffix_len_matching_query_prefix(
    bytes: &[u8],
    mut is_query_prefix: impl FnMut(&[u8]) -> bool,
) -> usize {
    (1..=bytes.len())
        .rev()
        .find(|&length| {
            let suffix_start = bytes.len() - length;
            let suffix = &bytes[suffix_start..];
            is_query_prefix(suffix)
                && !raw_c1_prefix_is_utf8_continuation(bytes, suffix_start, suffix)
        })
        .unwrap_or(0)
}

#[derive(Default)]
struct TerminalClipboardTracker {
    pending: Vec<u8>,
    texts: Vec<String>,
    queries: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardStartKind {
    Osc52,
    ITermCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClipboardStart {
    index: usize,
    prefix_len: usize,
    kind: ClipboardStartKind,
}

impl TerminalClipboardTracker {
    const OSC52_PREFIXES: &'static [&'static [u8]] = &[b"\x1b]52;", b"\x9d52;", b"\xc2\x9d52;"];
    const ITERM_COPY_PREFIXES: &'static [&'static [u8]] = &[
        b"\x1b]1337;Copy=;",
        b"\x9d1337;Copy=;",
        b"\xc2\x9d1337;Copy=;",
    ];
    const ST_TERMINATOR: &'static [u8] = b"\x1b\\";
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some(start) = find_next_clipboard_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if is_inside_osc_or_st_control_string(&self.pending, start.index) {
                self.pending.drain(..start.index.saturating_add(1));
                continue;
            }
            if start.index > 0 {
                self.pending.drain(..start.index);
            }

            let content_start = start.prefix_len;
            let Some(terminator) = find_osc_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            match start.kind {
                ClipboardStartKind::Osc52 => {
                    match parse_osc52_clipboard_content(&self.pending[content_start..content_end]) {
                        Some(ClipboardSequence::Write(text)) => self.texts.push(text),
                        Some(ClipboardSequence::Query(selection)) => self.queries.push(selection),
                        None => {}
                    }
                }
                ClipboardStartKind::ITermCopy => {
                    if let Some(text) = parse_iterm_copy_clipboard_content(
                        &self.pending[content_start..content_end],
                    ) {
                        self.texts.push(text);
                    }
                }
            }

            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = Self::OSC52_PREFIXES
            .iter()
            .chain(Self::ITERM_COPY_PREFIXES.iter())
            .map(|prefix| suffix_prefix_len(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

fn find_next_clipboard_start(bytes: &[u8]) -> Option<ClipboardStart> {
    TerminalClipboardTracker::OSC52_PREFIXES
        .iter()
        .filter_map(|prefix| {
            find_subslice(bytes, prefix).map(|index| ClipboardStart {
                index,
                prefix_len: prefix.len(),
                kind: ClipboardStartKind::Osc52,
            })
        })
        .chain(
            TerminalClipboardTracker::ITERM_COPY_PREFIXES
                .iter()
                .filter_map(|prefix| {
                    find_subslice(bytes, prefix).map(|index| ClipboardStart {
                        index,
                        prefix_len: prefix.len(),
                        kind: ClipboardStartKind::ITermCopy,
                    })
                }),
        )
        .min_by_key(|start| start.index)
}

struct OscTerminator {
    index: usize,
    length: usize,
}

fn find_osc_terminator(bytes: &[u8]) -> Option<OscTerminator> {
    [
        bytes
            .iter()
            .position(|byte| *byte == b'\x07')
            .map(|index| OscTerminator { index, length: 1 }),
        find_subslice(bytes, TerminalClipboardTracker::ST_TERMINATOR).map(|index| OscTerminator {
            index,
            length: TerminalClipboardTracker::ST_TERMINATOR.len(),
        }),
        bytes
            .iter()
            .position(|byte| *byte == 0x9c)
            .map(|index| OscTerminator { index, length: 1 }),
        find_subslice(bytes, UTF8_C1_ST).map(|index| OscTerminator {
            index,
            length: UTF8_C1_ST.len(),
        }),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|terminator| terminator.index)
}

enum ClipboardSequence {
    Write(String),
    Query(String),
}

fn parse_osc52_clipboard_content(content: &[u8]) -> Option<ClipboardSequence> {
    let separator = content.iter().position(|byte| *byte == b';')?;
    let selection = String::from_utf8(content[..separator].to_vec()).ok()?;
    let payload = &content[separator + 1..];
    if payload == b"?" {
        return Some(ClipboardSequence::Query(selection));
    }

    let decoded = STANDARD.decode(payload).ok()?;
    let text = String::from_utf8(decoded).ok()?;

    Some(ClipboardSequence::Write(text))
}

fn parse_iterm_copy_clipboard_content(content: &[u8]) -> Option<String> {
    let decoded = STANDARD.decode(content).ok()?;
    String::from_utf8(decoded).ok()
}

#[derive(Default)]
struct TerminalNotificationTracker {
    pending: Vec<u8>,
    notifications: Vec<TerminalNotification>,
    progress: TerminalProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationStartKind {
    Osc9,
    RxvtNotify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NotificationStart {
    index: usize,
    prefix_len: usize,
    kind: NotificationStartKind,
}

impl TerminalNotificationTracker {
    const OSC9_PREFIXES: &'static [&'static [u8]] = &[b"\x1b]9;", b"\x9d9;", b"\xc2\x9d9;"];
    const RXVT_NOTIFY_PREFIXES: &'static [&'static [u8]] = &[
        b"\x1b]777;notify;",
        b"\x9d777;notify;",
        b"\xc2\x9d777;notify;",
    ];
    const MAX_PENDING: usize = 1024 * 1024;

    fn process(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > Self::MAX_PENDING {
            self.pending.clear();
            return;
        }

        loop {
            let Some(start) = find_next_notification_start(&self.pending) else {
                self.retain_possible_prefix();
                return;
            };
            if is_inside_osc_or_st_control_string(&self.pending, start.index) {
                self.pending.drain(..start.index.saturating_add(1));
                continue;
            }
            if start.index > 0 {
                self.pending.drain(..start.index);
            }

            let content_start = start.prefix_len;
            let Some(terminator) = find_osc_terminator(&self.pending[content_start..]) else {
                return;
            };
            let content_end = content_start + terminator.index;
            let content = &self.pending[content_start..content_end];
            match start.kind {
                NotificationStartKind::Osc9 => {
                    if content.starts_with(b"4;") {
                        if let Some(progress) = parse_osc9_progress_content(content) {
                            self.progress = progress;
                        }
                    } else if let Some(body) = parse_osc9_notification_content(content) {
                        self.notifications
                            .push(TerminalNotification { title: None, body });
                    }
                }
                NotificationStartKind::RxvtNotify => {
                    if let Some(notification) = parse_rxvt_notify_content(content) {
                        self.notifications.push(notification);
                    }
                }
            }

            self.pending.drain(..content_end + terminator.length);
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = Self::OSC9_PREFIXES
            .iter()
            .chain(Self::RXVT_NOTIFY_PREFIXES.iter())
            .map(|prefix| suffix_prefix_len(&self.pending, prefix))
            .max()
            .unwrap_or(0);
        let retained = retained
            .max(incomplete_osc_control_sequence_suffix_len(&self.pending))
            .max(incomplete_st_control_sequence_suffix_len(&self.pending));
        let writable = self.pending.len().saturating_sub(retained);
        if writable > 0 {
            self.pending.drain(..writable);
        }
    }
}

fn find_next_notification_start(bytes: &[u8]) -> Option<NotificationStart> {
    TerminalNotificationTracker::OSC9_PREFIXES
        .iter()
        .filter_map(|prefix| {
            find_subslice(bytes, prefix).map(|index| NotificationStart {
                index,
                prefix_len: prefix.len(),
                kind: NotificationStartKind::Osc9,
            })
        })
        .chain(
            TerminalNotificationTracker::RXVT_NOTIFY_PREFIXES
                .iter()
                .filter_map(|prefix| {
                    find_subslice(bytes, prefix).map(|index| NotificationStart {
                        index,
                        prefix_len: prefix.len(),
                        kind: NotificationStartKind::RxvtNotify,
                    })
                }),
        )
        .min_by_key(|start| start.index)
}

fn parse_osc9_notification_content(content: &[u8]) -> Option<String> {
    String::from_utf8(content.to_vec()).ok()
}

fn parse_osc9_progress_content(content: &[u8]) -> Option<TerminalProgress> {
    let rest = content.strip_prefix(b"4;")?;
    let mut parts = rest.split(|byte| *byte == b';');
    let state = parse_u8_decimal(parts.next()?)?;
    match state {
        0 => Some(TerminalProgress::None),
        1 => Some(TerminalProgress::Percentage(parse_progress_value(
            parts.next()?,
        )?)),
        2 => Some(TerminalProgress::Error(
            parts.next().and_then(parse_progress_value).unwrap_or(0),
        )),
        3 => Some(TerminalProgress::Indeterminate),
        _ => None,
    }
}

fn parse_progress_value(value: &[u8]) -> Option<u8> {
    let value = parse_u8_decimal(value)?;
    (value <= 100).then_some(value)
}

fn parse_rxvt_notify_content(content: &[u8]) -> Option<TerminalNotification> {
    let separator = content.iter().position(|byte| *byte == b';')?;
    let title = String::from_utf8(content[..separator].to_vec()).ok()?;
    let body = String::from_utf8(content[separator + 1..].to_vec()).ok()?;
    Some(TerminalNotification {
        title: Some(title),
        body,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use rssh_core::TerminalSize;

    use crate::terminal_modes::{MouseInputMode, MouseProtocolMode, MouseReportingMode};

    use super::{TerminalNotification, TerminalProgress, TerminalRuntime};

    #[test]
    fn feeds_plain_pty_output_into_terminal_grid() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let responses = runtime.feed_pty_output(b"abc");

        assert!(responses.is_empty());
        assert_eq!(runtime.terminal().grid().get(0, 0).unwrap().ch, 'a');
        assert_eq!(runtime.terminal().grid().get(0, 1).unwrap().ch, 'b');
        assert_eq!(runtime.terminal().grid().get(0, 2).unwrap().ch, 'c');
    }

    #[test]
    fn feeds_emoji_prefixed_pty_output_into_terminal_grid() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(24, 2));

        let output = runtime.feed_pty_output_with_display("👍 Process".as_bytes());

        assert!(output.responses.is_empty());
        assert!(
            terminal_text(&runtime).contains("Process"),
            "display={:?} text={:?}",
            String::from_utf8_lossy(&output.display),
            terminal_text(&runtime)
        );
    }

    #[test]
    fn does_not_match_raw_c1_inside_utf8_text() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(24, 2));

        let output = runtime.feed_pty_output_with_display(b"before \xc3\x9b6n after");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"before \xc3\x9b6n after");
        assert!(terminal_text(&runtime).contains("before Û6n after"));
    }

    #[test]
    fn does_not_retain_raw_c1_prefix_inside_utf8_text() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(24, 2));

        let output = runtime.feed_pty_output_with_display(b"before \xc3\x9b");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"before \xc3\x9b");
        assert!(terminal_text(&runtime).contains("before Û"));
    }

    #[test]
    fn reports_bell_events_without_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let output = runtime.feed_pty_output_with_display(b"ab\x07cd\x07");

        assert!(output.responses.is_empty());
        assert_eq!(output.bells, 2);
        assert_eq!(output.display, b"abcd");
        assert_eq!(terminal_text(&runtime), "abcd                ");
    }

    #[test]
    fn reports_damage_regions_from_terminal_feed() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let output = runtime.feed_pty_output_with_display(b"abc");

        assert_eq!(
            output.damage,
            vec![rssh_core::DamageRegion::new(0, 0, 3, 1)]
        );
    }

    #[test]
    fn omits_osc_title_from_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b]0;ops\x07after");

        assert!(output.responses.is_empty());
        assert_eq!(runtime.terminal().title(), Some("ops"));
        assert_eq!(output.display, b"beforeafter");
        assert!(terminal_text(&runtime).contains("beforeafter"));
    }

    #[test]
    fn omits_split_osc_title_from_display_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1b]0;op");
        let second = runtime.feed_pty_output_with_display(b"s\x07after");

        assert_eq!(first.display, b"before");
        assert_eq!(second.display, b"after");
        assert_eq!(runtime.terminal().title(), Some("ops"));
        assert!(terminal_text(&runtime).contains("beforeafter"));
    }

    #[test]
    fn consumes_wezterm_no_response_window_title_stack_controls_without_side_effects() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]0;main\x07\x1b[22;0t middle\x1b]0;alternate\x07\x1b[23;2tafter",
        );

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"before middleafter");
        assert_eq!(runtime.terminal().title(), Some("alternate"));
        assert!(terminal_text(&runtime).contains("before middleafter"));
    }

    #[test]
    fn malformed_window_title_stack_controls_do_not_fall_through_to_terminal_parser() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]0;main\x07\x1b[22;?t middle\x1b]0;alternate\x07\x1b[23;?tafter",
        );

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"before middleafter");
        assert_eq!(runtime.terminal().title(), Some("alternate"));
        assert!(terminal_text(&runtime).contains("before middleafter"));
    }

    #[test]
    fn tracks_c1_osc8_hyperlinks_without_displaying_control_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output =
            runtime.feed_pty_output_with_display(b"a\x9d8;;https://example.com\x9cbc\x9d8;;\x9cd");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"abcd");
        assert_eq!(
            runtime
                .terminal()
                .grid()
                .get(0, 1)
                .unwrap()
                .hyperlink
                .as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            runtime
                .terminal()
                .grid()
                .get(0, 2)
                .unwrap()
                .hyperlink
                .as_deref(),
            Some("https://example.com")
        );
        assert_eq!(runtime.terminal().grid().get(0, 0).unwrap().hyperlink, None);
        assert_eq!(runtime.terminal().grid().get(0, 3).unwrap().hyperlink, None);
    }

    #[test]
    fn omits_st_controls_without_displaying_control_bytes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"ab\x1b\\cd\x9cef");
        let second = runtime.feed_pty_output_with_display("gh\u{9c}ij".as_bytes());
        let third = runtime.feed_pty_output_with_display(b"kl\x1b");
        let fourth = runtime.feed_pty_output_with_display(b"\\mn");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"abcdef");
        assert!(second.responses.is_empty());
        assert_eq!(second.display, b"ghij");
        assert!(third.responses.is_empty());
        assert_eq!(third.display, b"kl");
        assert!(fourth.responses.is_empty());
        assert_eq!(fourth.display, b"mn");
        assert!(terminal_text(&runtime).contains("abcdefghijklmn"));
    }

    #[test]
    fn ignores_queries_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b]0;title \x1b[6n\x07after");

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn holds_split_queries_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1b]0;title \x1b[");
        let second = runtime.feed_pty_output_with_display(b"6n\x07after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert!(second.responses.is_empty());
        assert_eq!(second.display, b"after");
    }

    #[test]
    fn holds_split_queries_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1bPpayload \x1b[");
        let second = runtime.feed_pty_output_with_display(b"6n\x1b\\after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert!(second.responses.is_empty());
        assert_eq!(second.display, b"after");
    }

    #[test]
    fn answers_cursor_position_query_without_feeding_it_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output(b"before\x1b[");
        let second = runtime.feed_pty_output(b"6nafter");

        assert!(first.is_empty());
        assert_eq!(second, vec![b"\x1b[1;7R".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[6n"));
    }

    #[test]
    fn answers_cursor_position_query_with_current_cursor() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"abc\x1b[6n");

        assert_eq!(responses, vec![b"\x1b[1;4R".to_vec()]);
        assert!(terminal_text(&runtime).contains("abc"));
    }

    #[test]
    fn answers_c1_cursor_position_query_without_feeding_it_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"abc\x9b6n");

        assert_eq!(responses, vec![b"\x1b[1;4R".to_vec()]);
        assert!(terminal_text(&runtime).contains("abc"));
        assert!(!terminal_text(&runtime).contains("6n"));
    }

    #[test]
    fn consumes_private_cursor_position_queries_like_wezterm() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        let csi = '\u{9b}';
        let mut input = b"abc\x1b[?6n def".to_vec();
        input.extend_from_slice(b"\x9b?6n ghi");
        input.extend_from_slice(format!("{csi}?6n").as_bytes());

        let responses = runtime.feed_pty_output(&input);

        assert!(responses.is_empty());
        let text = terminal_text(&runtime);
        assert!(text.contains("abc def ghi"));
        assert!(!text.contains("?6n"));
    }

    #[test]
    fn answers_device_and_status_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime
            .feed_pty_output(b"a\x1b[c b\x1b[0c c\x1b[>c d\x1b[>0c e\x1b[=c f\x1b[=0c g\x1b[5n h");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1b[0n".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d e f g h"));
        assert!(!text.contains("[0c"));
        assert!(!text.contains("[>c"));
        assert!(!text.contains("[>0c"));
        assert!(!text.contains("[=c"));
        assert!(!text.contains("[=0c"));
        assert!(!text.contains("[5n"));
    }

    #[test]
    fn answers_c1_device_and_status_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses =
            runtime.feed_pty_output(b"a\x9bc b\x9b0c c\x9b>c d\x9b>0c e\x9b=c f\x9b=0c g\x9b5n h");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1b[0n".to_vec()
            ]
        );
        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d e f g h"));
        assert!(!text.contains("0c"));
        assert!(!text.contains("=c"));
    }

    #[test]
    fn answers_utf8_c1_device_and_status_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        let csi = '\u{9b}';
        let input = format!("a{csi}c b{csi}0c c{csi}>c d{csi}>0c e{csi}=c f{csi}=0c g{csi}5n h");

        let responses = runtime.feed_pty_output(input.as_bytes());

        assert_eq!(
            responses,
            vec![
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[?65;4;6;18;22;52c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1b[>1;277;0c".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1bP!|00000000\x1b\\".to_vec(),
                b"\x1b[0n".to_vec()
            ]
        );
        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d e f g h"));
        assert!(!text.contains("0c"));
        assert!(!text.contains("=c"));
    }

    #[test]
    fn answers_terminal_parameter_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"a\x1b[x b\x1b[0x c\x1b[1x d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[3;1;1;128;128;1;0x".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("[x"));
        assert!(!text.contains("[0x"));
        assert!(!text.contains("[1x"));
    }

    #[test]
    fn answers_c1_terminal_parameter_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let responses = runtime.feed_pty_output(b"a\x9bx b\x9b0x c\x9b1x d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[3;1;1;128;128;1;0x".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("0x"));
        assert!(!text.contains("1x"));
    }

    #[test]
    fn answers_utf8_c1_terminal_parameter_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        let csi = '\u{9b}';
        let input = format!("a{csi}x b{csi}0x c{csi}1x d");

        let responses = runtime.feed_pty_output(input.as_bytes());

        assert_eq!(
            responses,
            vec![
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[2;1;1;128;128;1;0x".to_vec(),
                b"\x1b[3;1;1;128;128;1;0x".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("0x"));
        assert!(!text.contains("1x"));
    }

    #[test]
    fn answers_xtsmgraphics_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(
            b"a\x1b[?1;1S b\x1b[?1;4S c\x1b[?2;1S d\x1b[?2;4S e\x1b[?3;1S f\x1b[?3;4S g\x1b[?2;2S h\x1b[?9;1S i\x1b[?1;3;10S j",
        );

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;0;65536S".to_vec(),
                b"\x1b[?1;0;65536S".to_vec(),
                b"\x1b[?2;0;1056;688S".to_vec(),
                b"\x1b[?2;0;1056;688S".to_vec(),
                b"\x1b[?3;0;1056;688S".to_vec(),
                b"\x1b[?3;0;1056;688S".to_vec(),
                b"\x1b[?2;0S".to_vec(),
                b"\x1b[?9;1S".to_vec(),
                b"\x1b[?1;2S".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d e f g h i j"));
        assert!(!text.contains("?1;1S"));
        assert!(!text.contains("?2;4S"));
        assert!(!text.contains("?9;1S"));
    }

    #[test]
    fn answers_xtsmgraphics_large_numeric_parameters_like_wezterm() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses =
            runtime.feed_pty_output(b"a\x1b[?70000;1S b\x1b[?1;70000S c\x1b[?1;1;70000S d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?70000;1S".to_vec(),
                b"\x1b[?1;2S".to_vec(),
                b"\x1b[?1;0;65536S".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("70000"));
    }

    #[test]
    fn answers_c1_xtsmgraphics_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"a\x9b?1;1S b\x9b?2;4S c\x9b?9;1S d");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;0;65536S".to_vec(),
                b"\x1b[?2;0;1056;688S".to_vec(),
                b"\x1b[?9;1S".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("?1;1S"));
    }

    #[test]
    fn answers_utf8_c1_xtsmgraphics_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));
        let csi = '\u{9b}';
        let input = format!("a{csi}?1;1S b{csi}?2;4S c{csi}?9;1S d");

        let responses = runtime.feed_pty_output(input.as_bytes());

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;0;65536S".to_vec(),
                b"\x1b[?2;0;1056;688S".to_vec(),
                b"\x1b[?9;1S".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("a b c d"));
        assert!(!text.contains("?1;1S"));
    }

    #[test]
    fn answers_text_area_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[18tafter");

        assert_eq!(responses, vec![b"\x1b[8;43;132t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[18t"));
    }

    #[test]
    fn answers_window_pixel_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[14tafter");

        assert_eq!(responses, vec![b"\x1b[4;688;1056t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[14t"));
    }

    #[test]
    fn consumes_window_position_query_without_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[13tafter");

        assert!(responses.is_empty());

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[13t"));
    }

    #[test]
    fn consumes_screen_pixel_size_query_without_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[15tafter");

        assert!(responses.is_empty());

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[15t"));
    }

    #[test]
    fn answers_character_cell_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[16tafter");

        assert_eq!(responses, vec![b"\x1b[6;16;8t".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[16t"));
    }

    #[test]
    fn answers_wezterm_window_reports_with_empty_and_extra_parameters() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses =
            runtime.feed_pty_output(b"before\x1b[14;t middle\x1b[16;0;99t after\x1b[18;1t");

        assert_eq!(
            responses,
            vec![
                b"\x1b[4;688;1056t".to_vec(),
                b"\x1b[6;16;8t".to_vec(),
                b"\x1b[8;43;132t".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle after"));
        assert!(!text.contains("[14;"));
        assert!(!text.contains("[16;"));
        assert!(!text.contains("[18;"));
    }

    #[test]
    fn answers_iterm_report_cell_size_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b]1337;ReportCellSize\x07after");

        assert_eq!(
            responses,
            vec![b"\x1b]1337;ReportCellSize=16.0;8.0\x1b\\".to_vec()]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("ReportCellSize"));
    }

    #[test]
    fn consumes_screen_size_query_without_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[19tafter");

        assert!(responses.is_empty());

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[19t"));
    }

    #[test]
    fn consumes_window_state_query_without_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses = runtime.feed_pty_output(b"before\x1b[11tafter");

        assert!(responses.is_empty());

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[11t"));
    }

    #[test]
    fn consumes_window_title_queries_without_response_by_default() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let responses =
            runtime.feed_pty_output(b"\x1b]0;ops\x07before\x1b[20t middle\x1b[21tafter");

        assert!(responses.is_empty());

        let text = terminal_text(&runtime);
        assert!(text.contains("before middleafter"));
        assert!(!text.contains("[20t"));
        assert!(!text.contains("[21t"));
    }

    #[test]
    fn answers_window_title_query_when_title_reporting_enabled() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));
        runtime.set_enable_title_reporting(true);

        let responses = runtime.feed_pty_output(b"\x1b]0;ops\x07before\x1b[21tafter");

        assert_eq!(responses, vec![b"\x1b]lops\x1b\\".to_vec()]);
        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[21t"));
    }

    #[test]
    fn consumes_decrqcra_without_response_by_default() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));

        let responses = runtime.feed_pty_output(b"ABC\x1b[7;1;1;1;1;3*yDEF");

        assert!(responses.is_empty());
        let text = terminal_text(&runtime);
        assert!(text.contains("ABCDEF"));
        assert!(!text.contains("[7;"));
    }

    #[test]
    fn answers_decrqcra_when_checksum_rectangular_area_enabled() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(10, 2));
        runtime.set_enable_checksum_rectangular_area(true);

        let responses = runtime.feed_pty_output(b"ABC\x1b[7;1;1;1;1;3*y");

        assert_eq!(responses, vec![b"\x1bP7!~00c6\x1b\\".to_vec()]);
        assert!(terminal_text(&runtime).starts_with("ABC"));
    }

    #[test]
    fn answers_c1_terminal_size_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let text_area = runtime.feed_pty_output(b"\x9b18t");
        let screen = runtime.feed_pty_output(b"\x9b19t");

        assert_eq!(text_area, vec![b"\x1b[8;43;132t".to_vec()]);
        assert!(screen.is_empty());
    }

    #[test]
    fn answers_c1_window_pixel_and_cell_size_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let window_pixels = runtime.feed_pty_output(b"\x9b14t");
        let cell_pixels = runtime.feed_pty_output(b"\x9b16t");

        assert_eq!(window_pixels, vec![b"\x1b[4;688;1056t".to_vec()]);
        assert_eq!(cell_pixels, vec![b"\x1b[6;16;8t".to_vec()]);
    }

    #[test]
    fn consumes_c1_window_position_and_screen_pixel_size_queries_without_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let window_position = runtime.feed_pty_output(b"\x9b13t");
        let screen_pixels = runtime.feed_pty_output(b"\x9b15t");

        assert!(window_position.is_empty());
        assert!(screen_pixels.is_empty());
    }

    #[test]
    fn consumes_c1_window_state_and_title_queries_without_response_by_default() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        runtime.feed_pty_output(b"\x1b]0;ops\x07");
        let state = runtime.feed_pty_output(b"\x9b11t");
        let icon = runtime.feed_pty_output(b"\x9b20t");
        let title = runtime.feed_pty_output(b"\x9b21t");

        assert!(state.is_empty());
        assert!(icon.is_empty());
        assert!(title.is_empty());
    }

    #[test]
    fn answers_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime
            .feed_pty_output(b"before\x1b[?1h\x1b[?1$p middle\x1b[?1004$p after\x1b[?9999$p");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;1$y".to_vec(),
                b"\x1b[?1004;2$y".to_vec(),
                b"\x1b[?9999;0$y".to_vec()
            ]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle after"));
        assert!(!text.contains("$p"));
    }

    #[test]
    fn answers_display_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(
            b"\x1b[?1034$p \x1b[?1034h\x1b[?1034$p\x1b[?1034l\x1b[?1034$p \
              \x1b[?12$p \x1b[?12h\x1b[?12$p\x1b[?12l\x1b[?12$p \
              \x1b[?7$p \x1b[?7l\x1b[?7$p \
              \x1b[?25$p \x1b[?25l\x1b[?25$p \
              \x1b[?45$p \x1b[?45h\x1b[?45$p\x1b[?45l\x1b[?45$p \
              \x1b[?6$p \x1b[?6h\x1b[?6$p \
              \x1b[?80$p \x1b[?80h\x1b[?80$p\x1b[?80l\x1b[?80$p \
              \x1b[?8452$p \x1b[?8452h\x1b[?8452$p\x1b[?8452l\x1b[?8452$p \
              \x1b[?47$p \x1b[?47h\x1b[?47$p\x1b[?47l\x1b[?47$p \
              \x1b[?1048$p \x1b[?1048h\x1b[?1048$p\x1b[?1048l\x1b[?1048$p \
              \x1b[?1047$p \x1b[?1047h\x1b[?1047$p\x1b[?1047l\x1b[?1047$p \
              \x1b[?1049$p \x1b[?1049h\x1b[?1049$p\x1b[?1049l\x1b[?1049$p",
        );

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1034;2$y".to_vec(),
                b"\x1b[?1034;1$y".to_vec(),
                b"\x1b[?1034;2$y".to_vec(),
                b"\x1b[?12;2$y".to_vec(),
                b"\x1b[?12;1$y".to_vec(),
                b"\x1b[?12;2$y".to_vec(),
                b"\x1b[?7;1$y".to_vec(),
                b"\x1b[?7;2$y".to_vec(),
                b"\x1b[?25;1$y".to_vec(),
                b"\x1b[?25;2$y".to_vec(),
                b"\x1b[?45;2$y".to_vec(),
                b"\x1b[?45;1$y".to_vec(),
                b"\x1b[?45;2$y".to_vec(),
                b"\x1b[?6;2$y".to_vec(),
                b"\x1b[?6;1$y".to_vec(),
                b"\x1b[?80;2$y".to_vec(),
                b"\x1b[?80;1$y".to_vec(),
                b"\x1b[?80;2$y".to_vec(),
                b"\x1b[?8452;2$y".to_vec(),
                b"\x1b[?8452;1$y".to_vec(),
                b"\x1b[?8452;2$y".to_vec(),
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1047;0$y".to_vec(),
                b"\x1b[?1047;0$y".to_vec(),
                b"\x1b[?1047;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
            ]
        );
    }

    #[test]
    fn reports_wezterm_unknown_alternate_screen_private_mode_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(
            b"\x1b[?47$p\x1b[?47h\x1b[?47$p\
              \x1b[?1047$p\x1b[?1047h\x1b[?1047$p\
              \x1b[?1048$p\x1b[?1048h\x1b[?1048$p\
              \x1b[?1049$p\x1b[?1049h\x1b[?1049$p",
        );

        assert_eq!(
            responses,
            vec![
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?1047;0$y".to_vec(),
                b"\x1b[?1047;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_declrmm_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses =
            runtime.feed_pty_output(b"\x1b[?69$p\x1b[?69h\x1b[?69$p\x1b[?69l\x1b[?69$p");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?69;2$y".to_vec(),
                b"\x1b[?69;1$y".to_vec(),
                b"\x1b[?69;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_wezterm_private_mode_reports() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(
            b"\x1b[?3$p \x1b[?2027$p \x1b[?2027l\x1b[?2027$p \
              \x1b[?1070$p \x1b[?1070h\x1b[?1070$p\x1b[?1070l\x1b[?1070$p",
        );

        assert_eq!(
            responses,
            vec![
                b"\x1b[?3;2$y".to_vec(),
                b"\x1b[?2027;3$y".to_vec(),
                b"\x1b[?2027;3$y".to_vec(),
                b"\x1b[?1070;2$y".to_vec(),
                b"\x1b[?1070;1$y".to_vec(),
                b"\x1b[?1070;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_dec_ansi_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[?2$p\x1b[?2h\x1b[?2$p\x1b[?2l\x1b[?2$p");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?2;2$y".to_vec(),
                b"\x1b[?2;1$y".to_vec(),
                b"\x1b[?2;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_screen_reverse_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"\x1b[?5$p\x1b[?5h\x1b[?5$p\x1b[?5l\x1b[?5$p");

        assert_eq!(
            output.responses,
            vec![
                b"\x1b[?5;2$y".to_vec(),
                b"\x1b[?5;1$y".to_vec(),
                b"\x1b[?5;2$y".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn answers_private_mode_status_defaults_after_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(
            b"\x1b[?1;6;25;47;1048;1049;1000;1006;1004;2004;2026h\x1b[?7l\x1b=\x1bc\
              \x1b[?1$p\x1b[?6$p\x1b[?7$p\x1b[?25$p\x1b[?47$p\x1b[?1048$p\
              \x1b[?1049$p\x1b[?1000$p\x1b[?1006$p\x1b[?1004$p\x1b[?2004$p\x1b[?2026$p",
        );

        assert_eq!(
            responses,
            vec![
                b"\x1b[?1;2$y".to_vec(),
                b"\x1b[?6;2$y".to_vec(),
                b"\x1b[?7;1$y".to_vec(),
                b"\x1b[?25;1$y".to_vec(),
                b"\x1b[?47;0$y".to_vec(),
                b"\x1b[?1048;0$y".to_vec(),
                b"\x1b[?1049;0$y".to_vec(),
                b"\x1b[?1000;2$y".to_vec(),
                b"\x1b[?1006;2$y".to_vec(),
                b"\x1b[?1004;2$y".to_vec(),
                b"\x1b[?2004;2$y".to_vec(),
                b"\x1b[?2026;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_ansi_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b[4$p \x1b[4h\x1b[4$p \x1b[4l\x1b[4$p \x1b[999$p",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b[4;2$y".to_vec(),
                b"\x1b[4;1$y".to_vec(),
                b"\x1b[4;2$y".to_vec(),
                b"\x1b[999;0$y".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before   ");
        assert!(!terminal_text(&runtime).contains("$p"));
    }

    #[test]
    fn answers_automatic_newline_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime
            .feed_pty_output_with_display(b"before\x1b[20$p \x1b[20h\x1b[20$p \x1b[20l\x1b[20$p");

        assert_eq!(
            output.responses,
            vec![
                b"\x1b[20;2$y".to_vec(),
                b"\x1b[20;1$y".to_vec(),
                b"\x1b[20;2$y".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before  ");
        assert!(!terminal_text(&runtime).contains("$p"));
    }

    #[test]
    fn answers_bidirectional_support_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"before\x1b[8$p \x1b[8h\x1b[8$p \x1b[8l\x1b[8$p");

        assert_eq!(
            output.responses,
            vec![
                b"\x1b[8;2$y".to_vec(),
                b"\x1b[8;1$y".to_vec(),
                b"\x1b[8;2$y".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before  ");
        assert!(!terminal_text(&runtime).contains("$p"));
    }

    #[test]
    fn answers_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses =
            runtime.feed_pty_output(b"\x1b[?6h\x1b[4h\x1b[?6$p\x1b[4$p\x1b[!p\x1b[?6$p\x1b[4$p");

        assert_eq!(
            responses,
            vec![
                b"\x1b[?6;1$y".to_vec(),
                b"\x1b[4;1$y".to_vec(),
                b"\x1b[?6;2$y".to_vec(),
                b"\x1b[4;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn answers_bidirectional_support_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[8h\x1b[8$p\x1b[!p\x1b[8$p");

        assert_eq!(
            responses,
            vec![b"\x1b[8;1$y".to_vec(), b"\x1b[8;2$y".to_vec()]
        );
    }

    #[test]
    fn answers_auto_wrap_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[?7l\x1b[?7$p\x1b[!p\x1b[?7$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?7;2$y".to_vec(), b"\x1b[?7;1$y".to_vec()]
        );
    }

    #[test]
    fn answers_cursor_visibility_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[?25l\x1b[?25$p\x1b[!p\x1b[?25$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?25;2$y".to_vec(), b"\x1b[?25;1$y".to_vec()]
        );
    }

    #[test]
    fn answers_reverse_wrap_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[?45h\x1b[?45$p\x1b[!p\x1b[?45$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?45;1$y".to_vec(), b"\x1b[?45;2$y".to_vec()]
        );
    }

    #[test]
    fn answers_screen_reverse_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x1b[?5h\x1b[?5$p\x1b[!p\x1b[?5$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?5;1$y".to_vec(), b"\x1b[?5;2$y".to_vec()]
        );
    }

    #[test]
    fn restores_wezterm_input_modes_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x1b[?1h\x1b=\x1b[>4;2m");
        assert!(runtime.application_cursor_keys());
        assert!(runtime.application_keypad());
        assert_eq!(runtime.modify_other_keys(), 2);

        runtime.feed_pty_output(b"\x1b[!p");

        assert!(!runtime.application_cursor_keys());
        assert!(!runtime.application_keypad());
        assert_eq!(runtime.modify_other_keys(), 0);
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?1$p\x1b[?4m"),
            vec![b"\x1b[?1;2$y".to_vec(), b"\x1b[>4;0m".to_vec()]
        );
    }

    #[test]
    fn answers_c1_mode_status_after_soft_terminal_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9b?6h\x9b4h\x9b!p\x9b?6$p\x9b4$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?6;2$y".to_vec(), b"\x1b[4;2$y".to_vec()]
        );
    }

    #[test]
    fn answers_c1_cursor_blink_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9b?12$p\x9b?12h\x9b?12$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?12;2$y".to_vec(), b"\x1b[?12;1$y".to_vec()]
        );
    }

    #[test]
    fn answers_c1_meta_key_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9b?1034$p\x9b?1034h\x9b?1034$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?1034;2$y".to_vec(), b"\x1b[?1034;1$y".to_vec()]
        );
    }

    #[test]
    fn answers_c1_ansi_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b4h");
        let responses = runtime.feed_pty_output(b"\x9b4$p");

        assert_eq!(responses, vec![b"\x1b[4;1$y".to_vec()]);
    }

    #[test]
    fn answers_c1_automatic_newline_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b20h");
        let responses = runtime.feed_pty_output(b"\x9b20$p");

        assert_eq!(responses, vec![b"\x1b[20;1$y".to_vec()]);
    }

    #[test]
    fn answers_c1_bidirectional_support_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b8h");
        let responses = runtime.feed_pty_output(b"\x9b8$p");

        assert_eq!(responses, vec![b"\x1b[8;1$y".to_vec()]);
    }

    #[test]
    fn answers_osc_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]10;?\x07 middle\x1b]11;?\x1b\\ after\x1b]4;1;?\x07done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec(),
                b"\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\".to_vec(),
                b"\x1b]4;1;rgb:cdcd/3131/3131\x07".to_vec()
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle afterdone"));
        assert!(!text.contains("10;?"));
        assert!(!text.contains("11;?"));
        assert!(!text.contains("4;1;?"));
    }

    #[test]
    fn answers_c1_osc_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9d4;196;?\x9c");

        assert_eq!(
            responses,
            vec![b"\x1b]4;196;rgb:ffff/0000/0000\x9c".to_vec()]
        );
    }

    #[test]
    fn answers_utf8_c1_osc_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display("\u{9d}4;196;?\u{9c}".as_bytes());

        assert_eq!(
            output.responses,
            vec![b"\x1b]4;196;rgb:ffff/0000/0000\x9c".to_vec()]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn answers_cursor_color_queries_after_changes_and_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]12;rgb:aa/bb/cc\x07 middle\x1b]12;?\x07 after\x1b]112\x07 reset\x1b]12;?\x1b\\done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]12;rgb:aaaa/bbbb/cccc\x07".to_vec(),
                b"\x1b]12;rgb:e5e5/e5e5/e5e5\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle after resetdone");
    }

    #[test]
    fn answers_c1_cursor_color_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(b"\x9d12;rgb:01/02/03\x9c\x9d12;?\x9c");

        assert_eq!(
            output.responses,
            vec![b"\x1b]12;rgb:0101/0202/0303\x9c".to_vec()]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn answers_osc_color_queries_after_color_changes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]10;rgb:11/22/33\x07 middle\x1b]10;?\x07 after\x1b]4;1;rgb:01/02/03\x1b\\ done\x1b]4;1;?\x1b\\",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:1111/2222/3333\x07".to_vec(),
                b"\x1b]4;1;rgb:0101/0202/0303\x1b\\".to_vec()
            ]
        );
        assert_eq!(output.display, b"before middle after done");
    }

    #[test]
    fn applies_hex_osc_color_changes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]10;#112233\x07\x1b]4;2;#445566\x07\x1b]10;?\x07\x1b]4;2;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:1111/2222/3333\x07".to_vec(),
                b"\x1b]4;2;rgb:4444/5555/6666\x07".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn applies_rgba_osc_dynamic_color_changes() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]10;rgba(127,127,127,0.4)\x07\
              \x1b]11;rgba:efff/ecff/f4ff/d000\x1b\\\
              \x1b]12;rgba(1,2,3,1)\x07\
              \x1b]10;?\x07\x1b]11;?\x1b\\\x1b]12;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgba:7f7f/7f7f/7f7f/6666\x07".to_vec(),
                b"\x1b]11;rgba:efff/ecff/f4ff/d000\x1b\\".to_vec(),
                b"\x1b]12;rgba:0101/0202/0303/ffff\x07".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn applies_multiple_palette_color_changes_from_one_osc4_sequence() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07\
              \x1b]4;1;?\x07\x1b]4;2;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]4;1;rgb:0101/0202/0303\x07".to_vec(),
                b"\x1b]4;2;rgb:0404/0505/0606\x07".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn answers_multiple_palette_color_queries_from_one_osc4_sequence() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]4;1;rgb:01/02/03;2;rgb:04/05/06\x07\
              \x1b]4;1;?;2;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![b"\x1b]4;1;rgb:0101/0202/0303\x07\x1b]4;2;rgb:0404/0505/0606\x07".to_vec()]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn resets_dynamic_and_palette_colors() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\
              \x1b]10;rgb:11/22/33\x07\x1b]11;rgb:44/55/66\x07\
              \x1b]4;1;rgb:01/02/03\x07\
              \x1b]10;?\x07\x1b]11;?\x07\x1b]4;1;?\x07\
              \x1b]110\x07\x1b]111\x07\x1b]104;1\x07\
              \x1b]10;?\x07\x1b]11;?\x07\x1b]4;1;?\x07after",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]10;rgb:1111/2222/3333\x07".to_vec(),
                b"\x1b]11;rgb:4444/5555/6666\x07".to_vec(),
                b"\x1b]4;1;rgb:0101/0202/0303\x07".to_vec(),
                b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec(),
                b"\x1b]11;rgb:0c0c/0c0c/0c0c\x07".to_vec(),
                b"\x1b]4;1;rgb:cdcd/3131/3131\x07".to_vec(),
            ]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn resets_all_palette_colors() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]4;1;rgb:01/02/03\x07\x1b]4;2;rgb:04/05/06\x07\
              \x1b]104\x07\x1b]4;1;?\x07\x1b]4;2;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]4;1;rgb:cdcd/3131/3131\x07".to_vec(),
                b"\x1b]4;2;rgb:0d0d/bcbc/7979\x07".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn resets_multiple_palette_colors() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1b]4;1;rgb:01/02/03\x07\x1b]4;2;rgb:04/05/06\x07\x1b]4;3;rgb:07/08/09\x07\
              \x1b]104;1;2\x07\x1b]4;1;?\x07\x1b]4;2;?\x07\x1b]4;3;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1b]4;1;rgb:cdcd/3131/3131\x07".to_vec(),
                b"\x1b]4;2;rgb:0d0d/bcbc/7979\x07".to_vec(),
                b"\x1b]4;3;rgb:0707/0808/0909\x07".to_vec(),
            ]
        );
        assert!(output.display.is_empty());
    }

    #[test]
    fn ignores_osc_color_changes_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"\x1bPpayload \x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07",
        );

        assert_eq!(
            output.responses,
            vec![b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec()]
        );
        assert_eq!(output.display, b" after");
    }

    #[test]
    fn ignores_split_osc_color_changes_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let first = runtime.feed_pty_output_with_display(b"\x1bPpayload ");
        let second =
            runtime.feed_pty_output_with_display(b"\x1b]10;rgb:11/22/33\x1b\\ after\x1b]10;?\x07");

        assert!(first.responses.is_empty());
        assert!(first.display.is_empty());
        assert_eq!(
            second.responses,
            vec![b"\x1b]10;rgb:e5e5/e5e5/e5e5\x07".to_vec()]
        );
        assert_eq!(second.display, b" after");
    }

    #[test]
    fn answers_xtgettcap_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1bP+q436f\x1b\\ middle\x90+q544e;524742;6e616d65\x9c after\x1bP+q666f6f\x1b\\done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1+r436F=323536\x1b\\".to_vec(),
                b"\x1bP1+r544E=787465726D2D323536636F6C6F72\x1b\\\x1bP1+r524742=382F382F38\x1b\\\x1bP1+r6E616D65=787465726D2D323536636F6C6F72\x1b\\".to_vec(),
                b"\x1bP0+r666F6F\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");

        let text = terminal_text(&runtime);
        assert!(text.contains("before middle afterdone"));
        assert!(!text.contains("+q"));
    }

    #[test]
    fn answers_xtgettcap_invalid_hex_names_like_wezterm() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let first = runtime.feed_pty_output_with_display(b"before\x1bP+qZZ;544e");
        let second = runtime.feed_pty_output_with_display(b";5\x1b\\after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert_eq!(
            second.responses,
            vec![
                b"\x1bP0+r5A5A\x1b\\\x1bP1+r544E=787465726D2D323536636F6C6F72\x1b\\\x1bP0+r35\x1b\\"
                    .to_vec()
            ]
        );
        assert_eq!(second.display, b"after");
    }

    #[test]
    fn answers_xtgettcap_non_utf8_hex_names_like_wezterm() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(b"before\x1bP+qff;436f\x1b\\after");

        assert_eq!(
            output.responses,
            vec![b"\x1bP0+rEFBFBD\x1b\\\x1bP1+r436F=323536\x1b\\".to_vec()]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_utf8_c1_dcs_xtgettcap_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let dcs = '\u{90}';
        let st = '\u{9c}';
        let input = format!("before{dcs}+q436f{st}after");

        let output = runtime.feed_pty_output_with_display(input.as_bytes());

        assert_eq!(
            output.responses,
            vec![b"\x1bP1+r436F=323536\x1b\\".to_vec()]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_size_queries_from_current_terminal_size() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));

        let output = runtime.feed_pty_output_with_display(b"before\x1bP+q636f;6c69\x1b\\after");

        assert_eq!(
            output.responses,
            vec![b"\x1bP1+r636F=313332\x1b\\\x1bP1+r6C69=3433\x1b\\".to_vec()]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_official_numeric_capability_names() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(132, 43));
        let query = xtgettcap_query(&[
            b"cols".as_slice(),
            b"lines".as_slice(),
            b"it".as_slice(),
            b"pairs".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"cols".as_slice(), b"132".as_slice()),
                (b"lines".as_slice(), b"43".as_slice()),
                (b"it".as_slice(), b"8".as_slice()),
                (b"pairs".as_slice(), b"32767".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_modern_style_and_color_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"Tc".as_slice(),
            b"Smulx".as_slice(),
            b"Setulc".as_slice(),
            b"sitm".as_slice(),
            b"ritm".as_slice(),
            b"Smol".as_slice(),
            b"smxx".as_slice(),
            b"rmxx".as_slice(),
            b"op".as_slice(),
            b"oc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"Tc".as_slice(), b"1".as_slice()),
                (b"Smulx".as_slice(), b"\x1b[4:%p1%dm".as_slice()),
                (
                    b"Setulc".as_slice(),
                    b"\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%d%;m".as_slice()
                ),
                (b"sitm".as_slice(), b"\x1b[3m".as_slice()),
                (b"ritm".as_slice(), b"\x1b[23m".as_slice()),
                (b"Smol".as_slice(), b"\x1b[53m".as_slice()),
                (b"smxx".as_slice(), b"\x1b[9m".as_slice()),
                (b"rmxx".as_slice(), b"\x1b[29m".as_slice()),
                (b"op".as_slice(), b"\x1b[39;49m".as_slice()),
                (b"oc".as_slice(), b"\x1b]104\x07".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_official_boolean_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"am".as_slice(),
            b"bce".as_slice(),
            b"ccc".as_slice(),
            b"hs".as_slice(),
            b"mc5i".as_slice(),
            b"mir".as_slice(),
            b"msgr".as_slice(),
            b"npc".as_slice(),
            b"Su".as_slice(),
            b"xenl".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"am".as_slice(), b"1".as_slice()),
                (b"bce".as_slice(), b"1".as_slice()),
                (b"ccc".as_slice(), b"1".as_slice()),
                (b"hs".as_slice(), b"1".as_slice()),
                (b"mc5i".as_slice(), b"1".as_slice()),
                (b"mir".as_slice(), b"1".as_slice()),
                (b"msgr".as_slice(), b"1".as_slice()),
                (b"npc".as_slice(), b"1".as_slice()),
                (b"Su".as_slice(), b"1".as_slice()),
                (b"xenl".as_slice(), b"1".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_official_printer_memory_and_reset_templates() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"flash".as_slice(),
            b"mc0".as_slice(),
            b"mc4".as_slice(),
            b"mc5".as_slice(),
            b"meml".as_slice(),
            b"memu".as_slice(),
            b"rs1".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"flash".as_slice(), b"\x1b[?5h$<100/>\x1b[?5l".as_slice()),
                (b"mc0".as_slice(), b"\x1b[i".as_slice()),
                (b"mc4".as_slice(), b"\x1b[4i".as_slice()),
                (b"mc5".as_slice(), b"\x1b[5i".as_slice()),
                (b"meml".as_slice(), b"\x1bl".as_slice()),
                (b"memu".as_slice(), b"\x1bm".as_slice()),
                (b"rs1".as_slice(), b"\x1bc\x1b]104\x07".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_title_and_palette_templates() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"dsl".as_slice(),
            b"fsl".as_slice(),
            b"tsl".as_slice(),
            b"initc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"dsl".as_slice(), b"\x1b]2;\x1b\\".as_slice()),
                (b"fsl".as_slice(), b"\x1b\\".as_slice()),
                (b"tsl".as_slice(), b"\x1b]0;".as_slice()),
                (
                    b"initc".as_slice(),
                    b"\x1b]4;%p1%d;rgb:%p2%{255}%*%{1000}%/%2.2X/%p3%{255}%*%{1000}%/%2.2X/%p4%{255}%*%{1000}%/%2.2X\x1b\\".as_slice()
                ),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_tmux_cursor_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"Cr".as_slice(),
            b"Cs".as_slice(),
            b"Se".as_slice(),
            b"Ss".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"Cr".as_slice(), b"\x1b]112\x07".as_slice()),
                (b"Cs".as_slice(), b"\x1b]12;%p1%s\x07".as_slice()),
                (b"Se".as_slice(), b"\x1b[2 q".as_slice()),
                (b"Ss".as_slice(), b"\x1b[%p1%d q".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_synchronized_output_capability() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[b"Sync".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[(
                b"Sync".as_slice(),
                b"\x1b[?2026%?%p1%{1}%-%tl%eh%;".as_slice()
            )])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_mouse_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[b"kmous".as_slice(), b"XM".as_slice(), b"xm".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"kmous".as_slice(), b"\x1b[<".as_slice()),
                (
                    b"XM".as_slice(),
                    b"\x1b[?1006;1000%?%p1%{1}%=%th%el%;".as_slice()
                ),
                (
                    b"xm".as_slice(),
                    b"\x1b[<%i%p3%d;%p1%d;%p2%d;%?%p4%tM%em%;".as_slice()
                ),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_foundational_terminal_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"clear".as_slice(),
            b"cup".as_slice(),
            b"home".as_slice(),
            b"civis".as_slice(),
            b"cnorm".as_slice(),
            b"cvvis".as_slice(),
            b"smcup".as_slice(),
            b"rmcup".as_slice(),
            b"sgr0".as_slice(),
            b"sgr".as_slice(),
            b"bold".as_slice(),
            b"dim".as_slice(),
            b"blink".as_slice(),
            b"rev".as_slice(),
            b"smso".as_slice(),
            b"rmso".as_slice(),
            b"invis".as_slice(),
            b"smul".as_slice(),
            b"rmul".as_slice(),
            b"setaf".as_slice(),
            b"setab".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"clear".as_slice(), b"\x1b[H\x1b[2J".as_slice()),
                (b"cup".as_slice(), b"\x1b[%i%p1%d;%p2%dH".as_slice()),
                (b"home".as_slice(), b"\x1b[H".as_slice()),
                (b"civis".as_slice(), b"\x1b[?25l".as_slice()),
                (b"cnorm".as_slice(), b"\x1b[?12l\x1b[?25h".as_slice()),
                (b"cvvis".as_slice(), b"\x1b[?12;25h".as_slice()),
                (b"smcup".as_slice(), b"\x1b[?1049h\x1b[22;0;0t".as_slice()),
                (b"rmcup".as_slice(), b"\x1b[?1049l\x1b[23;0;0t".as_slice()),
                (b"sgr0".as_slice(), b"\x1b(B\x1b[m".as_slice()),
                (
                    b"sgr".as_slice(),
                    b"%?%p9%t\x1b(0%e\x1b(B%;\x1b[0%?%p6%t;1%;%?%p5%t;2%;%?%p2%t;4%;%?%p1%p3%|%t;7%;%?%p4%t;5%;%?%p7%t;8%;m".as_slice()
                ),
                (b"bold".as_slice(), b"\x1b[1m".as_slice()),
                (b"dim".as_slice(), b"\x1b[2m".as_slice()),
                (b"blink".as_slice(), b"\x1b[5m".as_slice()),
                (b"rev".as_slice(), b"\x1b[7m".as_slice()),
                (b"smso".as_slice(), b"\x1b[7m".as_slice()),
                (b"rmso".as_slice(), b"\x1b[27m".as_slice()),
                (b"invis".as_slice(), b"\x1b[8m".as_slice()),
                (b"smul".as_slice(), b"\x1b[4m".as_slice()),
                (b"rmul".as_slice(), b"\x1b[24m".as_slice()),
                (
                    b"setaf".as_slice(),
                    b"\x1b[%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m".as_slice()
                ),
                (
                    b"setab".as_slice(),
                    b"\x1b[%?%p1%{8}%<%t4%p1%d%e%p1%{16}%<%t10%p1%{8}%-%d%e48;5;%p1%d%;m".as_slice()
                ),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_common_control_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"el".as_slice(),
            b"ed".as_slice(),
            b"el1".as_slice(),
            b"dch1".as_slice(),
            b"ich1".as_slice(),
            b"il1".as_slice(),
            b"dl1".as_slice(),
            b"cuu".as_slice(),
            b"cud".as_slice(),
            b"cub".as_slice(),
            b"cuf".as_slice(),
            b"hpa".as_slice(),
            b"vpa".as_slice(),
            b"cbt".as_slice(),
            b"ht".as_slice(),
            b"hts".as_slice(),
            b"tbc".as_slice(),
            b"ech".as_slice(),
            b"rep".as_slice(),
            b"csr".as_slice(),
            b"indn".as_slice(),
            b"rin".as_slice(),
            b"smir".as_slice(),
            b"rmir".as_slice(),
            b"smam".as_slice(),
            b"rmam".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"el".as_slice(), b"\x1b[K".as_slice()),
                (b"ed".as_slice(), b"\x1b[J".as_slice()),
                (b"el1".as_slice(), b"\x1b[1K".as_slice()),
                (b"dch1".as_slice(), b"\x1b[P".as_slice()),
                (b"ich1".as_slice(), b"\x1b[@".as_slice()),
                (b"il1".as_slice(), b"\x1b[L".as_slice()),
                (b"dl1".as_slice(), b"\x1b[M".as_slice()),
                (b"cuu".as_slice(), b"\x1b[%p1%dA".as_slice()),
                (b"cud".as_slice(), b"\x1b[%p1%dB".as_slice()),
                (b"cub".as_slice(), b"\x1b[%p1%dD".as_slice()),
                (b"cuf".as_slice(), b"\x1b[%p1%dC".as_slice()),
                (b"hpa".as_slice(), b"\x1b[%i%p1%dG".as_slice()),
                (b"vpa".as_slice(), b"\x1b[%i%p1%dd".as_slice()),
                (b"cbt".as_slice(), b"\x1b[Z".as_slice()),
                (b"ht".as_slice(), b"\t".as_slice()),
                (b"hts".as_slice(), b"\x1bH".as_slice()),
                (b"tbc".as_slice(), b"\x1b[3g".as_slice()),
                (b"ech".as_slice(), b"\x1b[%p1%dX".as_slice()),
                (b"rep".as_slice(), b"%p1%c\x1b[%p2%{1}%-%db".as_slice()),
                (b"csr".as_slice(), b"\x1b[%i%p1%d;%p2%dr".as_slice()),
                (b"indn".as_slice(), b"\x1b[%p1%dS".as_slice()),
                (b"rin".as_slice(), b"\x1b[%p1%dT".as_slice()),
                (b"smir".as_slice(), b"\x1b[4h".as_slice()),
                (b"rmir".as_slice(), b"\x1b[4l".as_slice()),
                (b"smam".as_slice(), b"\x1b[?7h".as_slice()),
                (b"rmam".as_slice(), b"\x1b[?7l".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_common_key_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"kcuu1".as_slice(),
            b"kcud1".as_slice(),
            b"kcuf1".as_slice(),
            b"kcub1".as_slice(),
            b"kb2".as_slice(),
            b"kbs".as_slice(),
            b"kcbt".as_slice(),
            b"khome".as_slice(),
            b"kend".as_slice(),
            b"kich1".as_slice(),
            b"kdch1".as_slice(),
            b"kpp".as_slice(),
            b"knp".as_slice(),
            b"kHOM".as_slice(),
            b"kEND".as_slice(),
            b"kIC".as_slice(),
            b"kDC".as_slice(),
            b"kPRV".as_slice(),
            b"kNXT".as_slice(),
            b"kLFT".as_slice(),
            b"kRIT".as_slice(),
            b"kri".as_slice(),
            b"kind".as_slice(),
            b"kent".as_slice(),
            b"kf1".as_slice(),
            b"kf2".as_slice(),
            b"kf3".as_slice(),
            b"kf4".as_slice(),
            b"kf5".as_slice(),
            b"kf6".as_slice(),
            b"kf7".as_slice(),
            b"kf8".as_slice(),
            b"kf9".as_slice(),
            b"kf10".as_slice(),
            b"kf11".as_slice(),
            b"kf12".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"kcuu1".as_slice(), b"\x1bOA".as_slice()),
                (b"kcud1".as_slice(), b"\x1bOB".as_slice()),
                (b"kcuf1".as_slice(), b"\x1bOC".as_slice()),
                (b"kcub1".as_slice(), b"\x1bOD".as_slice()),
                (b"kb2".as_slice(), b"\x1bOE".as_slice()),
                (b"kbs".as_slice(), b"\x7f".as_slice()),
                (b"kcbt".as_slice(), b"\x1b[Z".as_slice()),
                (b"khome".as_slice(), b"\x1bOH".as_slice()),
                (b"kend".as_slice(), b"\x1bOF".as_slice()),
                (b"kich1".as_slice(), b"\x1b[2~".as_slice()),
                (b"kdch1".as_slice(), b"\x1b[3~".as_slice()),
                (b"kpp".as_slice(), b"\x1b[5~".as_slice()),
                (b"knp".as_slice(), b"\x1b[6~".as_slice()),
                (b"kHOM".as_slice(), b"\x1b[1;2H".as_slice()),
                (b"kEND".as_slice(), b"\x1b[1;2F".as_slice()),
                (b"kIC".as_slice(), b"\x1b[2;2~".as_slice()),
                (b"kDC".as_slice(), b"\x1b[3;2~".as_slice()),
                (b"kPRV".as_slice(), b"\x1b[5;2~".as_slice()),
                (b"kNXT".as_slice(), b"\x1b[6;2~".as_slice()),
                (b"kLFT".as_slice(), b"\x1b[1;2D".as_slice()),
                (b"kRIT".as_slice(), b"\x1b[1;2C".as_slice()),
                (b"kri".as_slice(), b"\x1b[1;2A".as_slice()),
                (b"kind".as_slice(), b"\x1b[1;2B".as_slice()),
                (b"kent".as_slice(), b"\x1bOM".as_slice()),
                (b"kf1".as_slice(), b"\x1bOP".as_slice()),
                (b"kf2".as_slice(), b"\x1bOQ".as_slice()),
                (b"kf3".as_slice(), b"\x1bOR".as_slice()),
                (b"kf4".as_slice(), b"\x1bOS".as_slice()),
                (b"kf5".as_slice(), b"\x1b[15~".as_slice()),
                (b"kf6".as_slice(), b"\x1b[17~".as_slice()),
                (b"kf7".as_slice(), b"\x1b[18~".as_slice()),
                (b"kf8".as_slice(), b"\x1b[19~".as_slice()),
                (b"kf9".as_slice(), b"\x1b[20~".as_slice()),
                (b"kf10".as_slice(), b"\x1b[21~".as_slice()),
                (b"kf11".as_slice(), b"\x1b[23~".as_slice()),
                (b"kf12".as_slice(), b"\x1b[24~".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_keypad_transmit_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[b"smkx".as_slice(), b"rmkx".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"smkx".as_slice(), b"\x1b[?1h\x1b=".as_slice()),
                (b"rmkx".as_slice(), b"\x1b[?1l\x1b>".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_modified_function_key_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let entries: &[(&[u8], &[u8])] = &[
            (b"kf13".as_slice(), b"\x1b[1;2P".as_slice()),
            (b"kf14".as_slice(), b"\x1b[1;2Q".as_slice()),
            (b"kf15".as_slice(), b"\x1b[1;2R".as_slice()),
            (b"kf16".as_slice(), b"\x1b[1;2S".as_slice()),
            (b"kf17".as_slice(), b"\x1b[15;2~".as_slice()),
            (b"kf18".as_slice(), b"\x1b[17;2~".as_slice()),
            (b"kf19".as_slice(), b"\x1b[18;2~".as_slice()),
            (b"kf20".as_slice(), b"\x1b[19;2~".as_slice()),
            (b"kf21".as_slice(), b"\x1b[20;2~".as_slice()),
            (b"kf22".as_slice(), b"\x1b[21;2~".as_slice()),
            (b"kf23".as_slice(), b"\x1b[23;2~".as_slice()),
            (b"kf24".as_slice(), b"\x1b[24;2~".as_slice()),
            (b"kf25".as_slice(), b"\x1b[1;5P".as_slice()),
            (b"kf26".as_slice(), b"\x1b[1;5Q".as_slice()),
            (b"kf27".as_slice(), b"\x1b[1;5R".as_slice()),
            (b"kf28".as_slice(), b"\x1b[1;5S".as_slice()),
            (b"kf29".as_slice(), b"\x1b[15;5~".as_slice()),
            (b"kf30".as_slice(), b"\x1b[17;5~".as_slice()),
            (b"kf31".as_slice(), b"\x1b[18;5~".as_slice()),
            (b"kf32".as_slice(), b"\x1b[19;5~".as_slice()),
            (b"kf33".as_slice(), b"\x1b[20;5~".as_slice()),
            (b"kf34".as_slice(), b"\x1b[21;5~".as_slice()),
            (b"kf35".as_slice(), b"\x1b[23;5~".as_slice()),
            (b"kf36".as_slice(), b"\x1b[24;5~".as_slice()),
            (b"kf37".as_slice(), b"\x1b[1;6P".as_slice()),
            (b"kf38".as_slice(), b"\x1b[1;6Q".as_slice()),
            (b"kf39".as_slice(), b"\x1b[1;6R".as_slice()),
            (b"kf40".as_slice(), b"\x1b[1;6S".as_slice()),
            (b"kf41".as_slice(), b"\x1b[15;6~".as_slice()),
            (b"kf42".as_slice(), b"\x1b[17;6~".as_slice()),
            (b"kf43".as_slice(), b"\x1b[18;6~".as_slice()),
            (b"kf44".as_slice(), b"\x1b[19;6~".as_slice()),
            (b"kf45".as_slice(), b"\x1b[20;6~".as_slice()),
            (b"kf46".as_slice(), b"\x1b[21;6~".as_slice()),
            (b"kf47".as_slice(), b"\x1b[23;6~".as_slice()),
            (b"kf48".as_slice(), b"\x1b[24;6~".as_slice()),
            (b"kf49".as_slice(), b"\x1b[1;3P".as_slice()),
            (b"kf50".as_slice(), b"\x1b[1;3Q".as_slice()),
            (b"kf51".as_slice(), b"\x1b[1;3R".as_slice()),
            (b"kf52".as_slice(), b"\x1b[1;3S".as_slice()),
            (b"kf53".as_slice(), b"\x1b[15;3~".as_slice()),
            (b"kf54".as_slice(), b"\x1b[17;3~".as_slice()),
            (b"kf55".as_slice(), b"\x1b[18;3~".as_slice()),
            (b"kf56".as_slice(), b"\x1b[19;3~".as_slice()),
            (b"kf57".as_slice(), b"\x1b[20;3~".as_slice()),
            (b"kf58".as_slice(), b"\x1b[21;3~".as_slice()),
            (b"kf59".as_slice(), b"\x1b[23;3~".as_slice()),
            (b"kf60".as_slice(), b"\x1b[24;3~".as_slice()),
            (b"kf61".as_slice(), b"\x1b[1;4P".as_slice()),
            (b"kf62".as_slice(), b"\x1b[1;4Q".as_slice()),
            (b"kf63".as_slice(), b"\x1b[1;4R".as_slice()),
        ];
        let names: Vec<&[u8]> = entries.iter().map(|(name, _)| *name).collect();
        let query = xtgettcap_query(&names);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(output.responses, vec![xtgettcap_response(entries)]);
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_acs_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"enacs".as_slice(),
            b"smacs".as_slice(),
            b"rmacs".as_slice(),
            b"acsc".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"enacs".as_slice(), b"\x1b)0".as_slice()),
                (b"smacs".as_slice(), b"\x1b(0".as_slice()),
                (b"rmacs".as_slice(), b"\x1b(B".as_slice()),
                (
                    b"acsc".as_slice(),
                    b"``aaffggiijjkkllmmnnooppqqrrssttuuvvwwxxyyzz{{||}}~~".as_slice()
                ),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_control_sequence_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"bel".as_slice(),
            b"cr".as_slice(),
            b"ind".as_slice(),
            b"ri".as_slice(),
            b"sc".as_slice(),
            b"rc".as_slice(),
            b"cuu1".as_slice(),
            b"cud1".as_slice(),
            b"cuf1".as_slice(),
            b"cub1".as_slice(),
            b"dch".as_slice(),
            b"ich".as_slice(),
            b"dl".as_slice(),
            b"il".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"bel".as_slice(), b"\x07".as_slice()),
                (b"cr".as_slice(), b"\r".as_slice()),
                (b"ind".as_slice(), b"\n".as_slice()),
                (b"ri".as_slice(), b"\x1bM".as_slice()),
                (b"sc".as_slice(), b"\x1b7".as_slice()),
                (b"rc".as_slice(), b"\x1b8".as_slice()),
                (b"cuu1".as_slice(), b"\x1b[A".as_slice()),
                (b"cud1".as_slice(), b"\n".as_slice()),
                (b"cuf1".as_slice(), b"\x1b[C".as_slice()),
                (b"cub1".as_slice(), b"\x08".as_slice()),
                (b"dch".as_slice(), b"\x1b[%p1%dP".as_slice()),
                (b"ich".as_slice(), b"\x1b[%p1%d@".as_slice()),
                (b"dl".as_slice(), b"\x1b[%p1%dM".as_slice()),
                (b"il".as_slice(), b"\x1b[%p1%dL".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_meta_key_capabilities() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[b"km".as_slice(), b"smm".as_slice(), b"rmm".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"km".as_slice(), b"1".as_slice()),
                (b"smm".as_slice(), b"\x1b[?1034h".as_slice()),
                (b"rmm".as_slice(), b"\x1b[?1034l".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_reset_templates() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[b"is2".as_slice(), b"rs2".as_slice()]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (
                    b"is2".as_slice(),
                    b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>".as_slice()
                ),
                (
                    b"rs2".as_slice(),
                    b"\x1b[!p\x1b[?3;4l\x1b[4l\x1b>".as_slice()
                ),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtgettcap_wezterm_query_templates() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let query = xtgettcap_query(&[
            b"u6".as_slice(),
            b"u7".as_slice(),
            b"u8".as_slice(),
            b"u9".as_slice(),
        ]);
        let mut input = b"before".to_vec();
        input.extend_from_slice(&query);
        input.extend_from_slice(b"after");

        let output = runtime.feed_pty_output_with_display(&input);

        assert_eq!(
            output.responses,
            vec![xtgettcap_response(&[
                (b"u6".as_slice(), b"\x1b[%i%d;%dR".as_slice()),
                (b"u7".as_slice(), b"\x1b[6n".as_slice()),
                (b"u8".as_slice(), b"\x1b[?%[;0123456789]c".as_slice()),
                (b"u9".as_slice(), b"\x1b[c".as_slice()),
            ])]
        );
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_decrqss_state_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b[1;2;4:3;5;8;9;53;73;58;5;34;38;6;4;5;6;7;48;2;1;2;3m\x1bP$qm\x1b\\ middle\x1b[5 q\x90$q q\x9c after\x1b[2;5r\x1bP$qr\x1b\\done",
        );

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1$r1;2;4:3;5;8;9;53;73;58;5;34;38;6;4;5;6;7;48;2;1;2;3m\x1b\\".to_vec(),
                b"\x1bP1$r5 q\x9c".to_vec(),
                b"\x1bP1$r2;5r\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle afterdone");
        assert!(!String::from_utf8_lossy(&output.display).contains("$q"));
    }

    #[test]
    fn answers_wezterm_decrqss_conformance_and_left_right_margin_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"before\x1bP$q\"p\x1b\\ middle\x90$qs\x9c after");

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1$r61;1\"p\x1b\\".to_vec(),
                b"\x1bP1$r1;80s\x9c".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle after");
    }

    #[test]
    fn answers_utf8_c1_dcs_decrqss_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let dcs = '\u{90}';
        let st = '\u{9c}';
        let input = format!("before{dcs}$q\"p{st} middle{dcs}$qs{st} after");

        let output = runtime.feed_pty_output_with_display(input.as_bytes());

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP1$r61;1\"p\x9c".to_vec(),
                b"\x1bP1$r1;80s\x9c".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle after");
    }

    #[test]
    fn answers_split_utf8_c1_dcs_decrqss_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let first = runtime.feed_pty_output_with_display(b"before\xc2");
        let second = runtime.feed_pty_output_with_display(b"\x90$q\"p\xc2");
        let third = runtime.feed_pty_output_with_display(b"\x9cafter");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert!(second.responses.is_empty());
        assert!(second.display.is_empty());
        assert_eq!(third.responses, vec![b"\x1bP1$r61;1\"p\x9c".to_vec()]);
        assert_eq!(third.display, b"after");
    }

    #[test]
    fn ignores_queries_inside_utf8_c1_dcs_payload() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let dcs = '\u{90}';
        let st = '\u{9c}';
        let input = format!("before{dcs}payload \x1b[6n{st}after");

        let output = runtime.feed_pty_output_with_display(input.as_bytes());

        assert!(output.responses.is_empty());
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_split_wezterm_decrqss_conformance_and_left_right_margin_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let first = runtime.feed_pty_output_with_display(b"before\x1bP$q\"");
        let second = runtime.feed_pty_output_with_display(b"p\x1b\\ middle\x90$q");
        let third = runtime.feed_pty_output_with_display(b"s\x9c after");

        assert!(first.responses.is_empty());
        assert_eq!(first.display, b"before");
        assert_eq!(second.responses, vec![b"\x1bP1$r61;1\"p\x1b\\".to_vec()]);
        assert_eq!(second.display, b" middle");
        assert_eq!(third.responses, vec![b"\x1bP1$r1;80s\x9c".to_vec()]);
        assert_eq!(third.display, b" after");
    }

    #[test]
    fn answers_decrqss_left_right_margin_query_from_declrmm_state() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"before\x1b[?69h\x1b[3;6s\x1bP$qs\x1b\\after");

        assert_eq!(output.responses, vec![b"\x1bP1$r3;6s\x1b\\".to_vec()]);
        assert_eq!(output.display, b"beforeafter");
    }

    #[test]
    fn answers_xtversion_queries_without_feeding_them_to_terminal() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let output =
            runtime.feed_pty_output_with_display(b"before\x1b[>q middle\x1b[>0q after\x9b>q done");

        assert_eq!(
            output.responses,
            vec![
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
                b"\x1bP>|R-SSH 0.1.0\x1b\\".to_vec(),
            ]
        );
        assert_eq!(output.display, b"before middle after done");
    }

    #[test]
    fn answers_c1_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b?1000;1006h\x1b[?2004h\x1b[?2026h");
        let normal_mouse = runtime.feed_pty_output(b"\x9b?1000$p");
        let sgr_mouse = runtime.feed_pty_output(b"\x9b?1006$p");
        let bracketed_paste = runtime.feed_pty_output(b"\x9b?2004$p");
        let synchronized_output = runtime.feed_pty_output(b"\x1b[?2026$p");

        assert_eq!(normal_mouse, vec![b"\x1b[?1000;1$y".to_vec()]);
        assert_eq!(sgr_mouse, vec![b"\x1b[?1006;1$y".to_vec()]);
        assert_eq!(bracketed_paste, vec![b"\x1b[?2004;1$y".to_vec()]);
        assert_eq!(synchronized_output, vec![b"\x1b[?2026;1$y".to_vec()]);
    }

    #[test]
    fn answers_c1_wezterm_private_mode_reports() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        let responses = runtime.feed_pty_output(b"\x9b?2027$p\x9b?1070h\x9b?1070$p");

        assert_eq!(
            responses,
            vec![b"\x1b[?2027;3$y".to_vec(), b"\x1b[?1070;1$y".to_vec()]
        );
    }

    #[test]
    fn answers_c1_dec_ansi_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));

        runtime.feed_pty_output(b"\x9b?2h");
        let responses = runtime.feed_pty_output(b"\x9b?2$p");

        assert_eq!(responses, vec![b"\x1b[?2;1$y".to_vec()]);
    }

    #[test]
    fn answers_utf8_c1_private_mode_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(80, 24));
        let csi = '\u{9b}';

        let initial = runtime.feed_pty_output(format!("{csi}?45$p").as_bytes());
        runtime.feed_pty_output(format!("{csi}?45h").as_bytes());
        let enabled = runtime.feed_pty_output(format!("{csi}?45$p").as_bytes());

        assert_eq!(initial, vec![b"\x1b[?45;2$y".to_vec()]);
        assert_eq!(enabled, vec![b"\x1b[?45;1$y".to_vec()]);
    }

    #[test]
    fn tracks_application_cursor_key_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.application_cursor_keys());

        runtime.feed_pty_output(b"\x1b[?1h");
        assert!(runtime.application_cursor_keys());

        runtime.feed_pty_output(b"\x1b[?1l");
        assert!(!runtime.application_cursor_keys());
    }

    #[test]
    fn tracks_split_application_cursor_key_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?");
        assert!(!runtime.application_cursor_keys());

        runtime.feed_pty_output(b"1h");
        assert!(runtime.application_cursor_keys());
    }

    #[test]
    fn tracks_focus_reporting_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.focus_reporting());

        runtime.feed_pty_output(b"\x1b[?1004h");
        assert!(runtime.focus_reporting());

        runtime.feed_pty_output(b"\x1b[?1004l");
        assert!(!runtime.focus_reporting());
    }

    #[test]
    fn tracks_bracketed_paste_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.bracketed_paste());

        runtime.feed_pty_output(b"\x1b[?2004h");
        assert!(runtime.bracketed_paste());

        runtime.feed_pty_output(b"\x1b[?2004l");
        assert!(!runtime.bracketed_paste());
    }

    #[test]
    fn tracks_synchronized_output_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.synchronized_output());
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?2026$p"),
            vec![b"\x1b[?2026;2$y".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[?2026h");
        assert!(runtime.synchronized_output());
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?2026$p"),
            vec![b"\x1b[?2026;1$y".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[?2026l");
        assert!(!runtime.synchronized_output());
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?2026$p"),
            vec![b"\x1b[?2026;2$y".to_vec()]
        );
    }

    #[test]
    fn delays_synchronized_output_damage_until_mode_resets() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output_with_display(b"before\x1b[?2026hmid");

        assert_eq!(first.display, b"beforemid");
        assert_eq!(first.damage, vec![rssh_core::DamageRegion::new(0, 0, 6, 1)]);
        assert!(runtime.synchronized_output());
        assert!(terminal_text(&runtime).contains("beforemid"));

        let buffered = runtime.feed_pty_output_with_display(b"after\x1b[?2026$p");

        assert_eq!(buffered.display, b"after");
        assert!(buffered.damage.is_empty());
        assert_eq!(buffered.responses, vec![b"\x1b[?2026;1$y".to_vec()]);
        assert!(terminal_text(&runtime).contains("beforemidafter"));

        let flushed = runtime.feed_pty_output_with_display(b"\x1b[?2026l done");

        assert_eq!(flushed.display, b" done");
        assert!(!flushed.damage.is_empty());
        assert!(!runtime.synchronized_output());
        assert!(terminal_text(&runtime).contains("beforemidafter done"));
    }

    #[test]
    fn ignores_private_input_modes_inside_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]0;title \x1b[?1004h\x07\x1bPpayload \x1b[?2004h\x1b\\");

        assert!(!runtime.focus_reporting());
        assert!(!runtime.bracketed_paste());
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?1004$p\x1b[?2004$p"),
            vec![b"\x1b[?1004;2$y".to_vec(), b"\x1b[?2004;2$y".to_vec()]
        );
    }

    #[test]
    fn extracts_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;Y29weQ==\x07");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_iterm_copy_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]1337;Copy=;Y29weQ==\x07");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_c1_iterm_copy_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d1337;Copy=;Y29weQ==\x9c");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_utf8_c1_iterm_copy_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output("\u{9d}1337;Copy=;Y29weQ==\u{9c}".as_bytes());

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_wezterm_osc9_and_osc777_notifications_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(
            b"before\x1b]9;build done\x07 middle\x9d777;notify;Build;failed\x9c after",
        );

        assert_eq!(output.display, b"before middle after");
        assert_eq!(
            runtime.take_notifications(),
            vec![
                TerminalNotification {
                    title: None,
                    body: "build done".to_owned(),
                },
                TerminalNotification {
                    title: Some("Build".to_owned()),
                    body: "failed".to_owned(),
                },
            ]
        );
        assert!(runtime.take_notifications().is_empty());
    }

    #[test]
    fn extracts_utf8_c1_osc9_and_osc777_notifications_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(
            "before\u{9d}9;build done\u{9c} middle\u{9d}777;notify;Build;failed\u{9c} after"
                .as_bytes(),
        );

        assert_eq!(output.display, b"before middle after");
        assert_eq!(
            runtime.take_notifications(),
            vec![
                TerminalNotification {
                    title: None,
                    body: "build done".to_owned(),
                },
                TerminalNotification {
                    title: Some("Build".to_owned()),
                    body: "failed".to_owned(),
                },
            ]
        );
        assert!(runtime.take_notifications().is_empty());
    }

    #[test]
    fn tracks_conemu_progress_osc9_without_notification() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime
            .feed_pty_output_with_display(b"before\x1b]9;4;1;42\x07 middle\x9d9;4;2\x9c after");

        assert_eq!(output.display, b"before middle after");
        assert_eq!(runtime.progress(), TerminalProgress::Error(0));
        assert!(runtime.take_notifications().is_empty());

        runtime.feed_pty_output(b"\x1b]9;4;3\x07");
        assert_eq!(runtime.progress(), TerminalProgress::Indeterminate);

        runtime.feed_pty_output(b"\x1b]9;4;0\x07");
        assert_eq!(runtime.progress(), TerminalProgress::None);
    }

    #[test]
    fn tracks_utf8_c1_conemu_progress_osc9_without_notification() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(
            "before\u{9d}9;4;1;42\u{9c} middle\u{9d}9;4;2\u{9c} after".as_bytes(),
        );

        assert_eq!(output.display, b"before middle after");
        assert_eq!(runtime.progress(), TerminalProgress::Error(0));
        assert!(runtime.take_notifications().is_empty());
    }

    #[test]
    fn extracts_c1_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;Y29weQ==\x9c");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn extracts_utf8_c1_osc52_clipboard_text_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output("\u{9d}52;c;Y29weQ==\u{9c}".as_bytes());

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
        assert!(runtime.take_clipboard_texts().is_empty());
    }

    #[test]
    fn ignores_osc52_clipboard_text_inside_osc_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]0;title \x1b]52;c;Y29weQ==\x07");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn ignores_osc52_clipboard_text_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1bPpayload \x1b]52;c;Y29weQ==\x1b\\");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn ignores_split_osc52_clipboard_text_inside_st_control_strings() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1bPpayload ");
        runtime.feed_pty_output(b"\x1b]52;c;Y29weQ==\x1b\\");

        assert!(runtime.take_clipboard_texts().is_empty());
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn extracts_split_osc52_clipboard_text_with_st_terminator() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;Y2");
        assert!(runtime.take_clipboard_texts().is_empty());

        runtime.feed_pty_output(b"9weQ==\x1b\\");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
    }

    #[test]
    fn extracts_split_c1_osc52_clipboard_text() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;Y2");
        assert!(runtime.take_clipboard_texts().is_empty());

        runtime.feed_pty_output(b"9weQ==\x9c");

        assert_eq!(runtime.take_clipboard_texts(), vec!["copy".to_owned()]);
    }

    #[test]
    fn extracts_osc52_clipboard_queries_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b]52;c;?\x07");

        assert_eq!(runtime.take_clipboard_queries(), vec!["c".to_owned()]);
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn extracts_c1_osc52_clipboard_queries_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9d52;c;?\x9c");

        assert_eq!(runtime.take_clipboard_queries(), vec!["c".to_owned()]);
        assert!(runtime.take_clipboard_queries().is_empty());
    }

    #[test]
    fn tracks_combined_private_input_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?1;1004;2004h");

        assert!(runtime.application_cursor_keys());
        assert!(runtime.focus_reporting());
        assert!(runtime.bracketed_paste());
    }

    #[test]
    fn tracks_c1_private_input_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x9b?1;1004;2004h");

        assert!(runtime.application_cursor_keys());
        assert!(runtime.focus_reporting());
        assert!(runtime.bracketed_paste());
    }

    #[test]
    fn tracks_mouse_reporting_modes_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert_eq!(runtime.mouse_input_mode(), MouseInputMode::default());

        runtime.feed_pty_output(b"\x1b[?1000;1006h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr)
        );

        runtime.feed_pty_output(b"\x1b[?1002h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::Sgr)
        );

        runtime.feed_pty_output(b"\x1b[?1006l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::ButtonEvent, MouseProtocolMode::X10)
        );

        runtime.feed_pty_output(b"\x1b[?1002;1000l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::None, MouseProtocolMode::X10)
        );
    }

    #[test]
    fn resets_extended_mouse_protocols_to_x10_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?1000;1005;1015;1006;1016h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::SgrPixels)
        );

        runtime.feed_pty_output(b"\x1b[?1016l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::X10)
        );

        runtime.feed_pty_output(b"\x1b[?1006h");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::Sgr)
        );

        runtime.feed_pty_output(b"\x1b[?1006l");
        assert_eq!(
            runtime.mouse_input_mode(),
            MouseInputMode::new(MouseReportingMode::Normal, MouseProtocolMode::X10)
        );
    }

    #[test]
    fn answers_extended_mouse_protocol_status_queries() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[?1005;1015;1016h");

        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?1005$p\x1b[?1015$p\x1b[?1016$p"),
            vec![
                b"\x1b[?1005;2$y".to_vec(),
                b"\x1b[?1015;2$y".to_vec(),
                b"\x1b[?1016;1$y".to_vec(),
            ]
        );

        runtime.feed_pty_output(b"\x1b[?1005;1015;1016l");

        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?1005$p\x1b[?1015$p\x1b[?1016$p"),
            vec![
                b"\x1b[?1005;2$y".to_vec(),
                b"\x1b[?1015;2$y".to_vec(),
                b"\x1b[?1016;2$y".to_vec(),
            ]
        );
    }

    #[test]
    fn tracks_application_keypad_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.application_keypad());

        runtime.feed_pty_output(b"\x1b=");
        assert!(runtime.application_keypad());

        runtime.feed_pty_output(b"\x1b>");
        assert!(!runtime.application_keypad());
    }

    #[test]
    fn tracks_split_application_keypad_mode_from_pty_output() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b");
        assert!(!runtime.application_keypad());

        runtime.feed_pty_output(b"=");
        assert!(runtime.application_keypad());
    }

    #[test]
    fn answers_kitty_keyboard_protocol_flags_queries_and_tracks_push_pop() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        runtime.set_enable_kitty_keyboard(true);

        let output = runtime.feed_pty_output_with_display(b"before\x1b[?u");

        assert_eq!(output.display, b"before");
        assert_eq!(output.responses, vec![b"\x1b[?0u".to_vec()]);

        runtime.feed_pty_output(b"\x1b[>1u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?1u".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[>9u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?9u".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[<u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?1u".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[<1u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?0u".to_vec()]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before"));
        assert!(!text.contains("[?u"));
        assert!(!text.contains("[>"));
        assert!(!text.contains("[<"));
    }

    #[test]
    fn answers_kitty_keyboard_protocol_flags_queries_and_tracks_set_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        runtime.set_enable_kitty_keyboard(true);

        runtime.feed_pty_output(b"\x1b[=1u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?1u".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[=8;2u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?9u".to_vec()]
        );

        runtime.feed_pty_output(b"\x1b[=1;3u");
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?u"),
            vec![b"\x1b[?8u".to_vec()]
        );

        let text = terminal_text(&runtime);
        assert!(!text.contains("[="));
    }

    #[test]
    fn ignores_kitty_keyboard_protocol_when_disabled() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        runtime.feed_pty_output(b"\x1b[=1u");
        let output = runtime.feed_pty_output_with_display(b"\x1b[?u");

        assert!(output.responses.is_empty());
        assert_eq!(runtime.kitty_keyboard_flags(), 0);
    }

    #[test]
    fn tracks_win32_input_mode_when_allowed() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        assert!(!runtime.win32_input_mode());

        runtime.feed_pty_output(b"\x1b[?9001h");
        assert!(runtime.win32_input_mode());

        runtime.feed_pty_output(b"\x1b[?9001l");
        assert!(!runtime.win32_input_mode());
    }

    #[test]
    fn ignores_win32_input_mode_when_disallowed() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        runtime.set_allow_win32_input_mode(false);

        runtime.feed_pty_output(b"\x1b[?9001h");

        assert!(!runtime.win32_input_mode());
    }

    #[test]
    fn answers_kitty_graphics_query_for_supported_direct_rgb_payload() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime
            .feed_pty_output_with_display(b"before\x1b_Ga=q,i=31,t=d,f=24,s=1,v=1;/wAA\x1b\\after");

        assert_eq!(output.display, b"beforeafter");
        assert_eq!(output.responses, vec![b"\x1b_Gi=31;OK\x1b\\".to_vec()]);
        assert!(runtime.terminal().inline_images().is_empty());
        assert!(!terminal_text(&runtime).contains("_G"));
    }

    #[test]
    fn answers_kitty_graphics_query_for_supported_file_rgb_payload() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));
        let file = KittyTestFile::new(&[255, 0, 0]);
        let encoded_path = STANDARD.encode(file.path.as_os_str().to_string_lossy().as_bytes());
        let query =
            format!("before\x1b_Ga=q,i=32,t=f,f=24,s=1,v=1,c=1,r=1;{encoded_path}\x1b\\after");

        let output = runtime.feed_pty_output_with_display(query.as_bytes());

        assert_eq!(output.display, b"beforeafter");
        assert_eq!(output.responses, vec![b"\x1b_Gi=32;OK\x1b\\".to_vec()]);
        assert!(runtime.terminal().inline_images().is_empty());
        assert!(!terminal_text(&runtime).contains("_G"));
    }

    #[test]
    fn answers_kitty_graphics_placement_query_for_missing_image() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b_Ga=p,i=404,p=2\x1b\\after");

        assert_eq!(output.display, b"beforeafter");
        assert_eq!(
            output.responses,
            vec![b"\x1b_Gi=404,p=2;ENOENT:No image with id 404\x1b\\".to_vec()]
        );
        assert!(runtime.terminal().inline_images().is_empty());
        assert!(!terminal_text(&runtime).contains("_G"));
    }

    #[test]
    fn answers_kitty_graphics_placement_query_for_existing_image() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let store =
            runtime.feed_pty_output_with_display(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,c=1,r=1;/wAA\x1b\\");
        assert_eq!(store.responses, vec![b"\x1b_Gi=7;OK\x1b\\".to_vec()]);

        let output = runtime.feed_pty_output_with_display(b"before\x1b_Ga=p,i=7,p=2\x1b\\after");

        assert_eq!(output.display, b"beforeafter");
        assert_eq!(output.responses, vec![b"\x1b_Gi=7,p=2;OK\x1b\\".to_vec()]);
        assert_eq!(runtime.terminal().inline_images().len(), 1);
        assert!(!terminal_text(&runtime).contains("_G"));
    }

    #[test]
    fn answers_modify_other_keys_queries_and_tracks_set_reset() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let output = runtime.feed_pty_output_with_display(b"before\x1b[?4m");

        assert_eq!(output.display, b"before");
        assert_eq!(output.responses, vec![b"\x1b[>4;0m".to_vec()]);

        runtime.feed_pty_output(b"\x1b[>4;2m");
        assert_eq!(runtime.modify_other_keys(), 2);
        assert_eq!(
            runtime.feed_pty_output(b"\x1b[?4m"),
            vec![b"\x1b[>4;2m".to_vec()]
        );

        let text = terminal_text(&runtime);
        assert!(text.contains("before"));
        assert!(!text.contains("[>4"));
        assert!(!text.contains("[?4"));
    }

    #[test]
    fn resize_updates_terminal_grid_and_size_query_response() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(4, 2));
        runtime.feed_pty_output(b"abcd\r\nef");

        runtime.resize(TerminalSize::new(6, 3));
        let responses = runtime.feed_pty_output(b"\x1b[18t");

        assert_eq!(runtime.terminal().grid().size(), TerminalSize::new(6, 3));
        assert_eq!(responses, vec![b"\x1b[8;3;6t".to_vec()]);
    }

    #[test]
    fn answers_split_device_attribute_query() {
        let mut runtime = TerminalRuntime::new(TerminalSize::new(20, 2));

        let first = runtime.feed_pty_output(b"before\x1b[");
        let second = runtime.feed_pty_output(b">cafter");

        assert!(first.is_empty());
        assert_eq!(second, vec![b"\x1b[>1;277;0c".to_vec()]);

        let text = terminal_text(&runtime);
        assert!(text.contains("beforeafter"));
        assert!(!text.contains("[>c"));
    }

    fn terminal_text(runtime: &TerminalRuntime) -> String {
        let grid = runtime.terminal().grid();
        let size = grid.size();
        let mut text = String::new();

        for row in 0..size.rows {
            for column in 0..size.columns {
                text.push(grid.get(row, column).unwrap().ch);
            }
        }

        text
    }

    fn xtgettcap_query(names: &[&[u8]]) -> Vec<u8> {
        let mut query = b"\x1bP+q".to_vec();
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                query.push(b';');
            }
            query.extend_from_slice(&super::encode_ascii_hex(name));
        }
        query.extend_from_slice(b"\x1b\\");
        query
    }

    fn xtgettcap_response(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut response = Vec::new();
        for (name, value) in entries {
            response.extend_from_slice(b"\x1bP1+r");
            response.extend_from_slice(&encode_ascii_hex_upper(name));
            response.push(b'=');
            response.extend_from_slice(&encode_ascii_hex_upper(value));
            response.extend_from_slice(b"\x1b\\");
        }
        response
    }

    fn encode_ascii_hex_upper(bytes: &[u8]) -> Vec<u8> {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut encoded = Vec::with_capacity(bytes.len() * 2);
        for byte in bytes {
            encoded.push(HEX[usize::from(byte >> 4)]);
            encoded.push(HEX[usize::from(byte & 0x0f)]);
        }
        encoded
    }

    struct KittyTestFile {
        path: PathBuf,
    }

    impl KittyTestFile {
        fn new(data: &[u8]) -> Self {
            static NEXT_TEST_FILE_ID: AtomicUsize = AtomicUsize::new(0);

            let suffix = NEXT_TEST_FILE_ID.fetch_add(1, Ordering::Relaxed);
            let mut path = std::env::temp_dir();
            path.push(format!(
                "rssh-runtime-kitty-file-query-{}-{suffix}.rgb",
                std::process::id()
            ));
            fs::write(&path, data).expect("write runtime kitty query test image file");
            Self { path }
        }
    }

    impl Drop for KittyTestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
