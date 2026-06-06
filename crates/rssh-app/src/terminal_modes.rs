use crossterm::event::MouseEventKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalModeChange {
    ApplicationCursorKeys(bool),
    ApplicationKeypad(bool),
    BracketedPaste(bool),
    Mouse(MouseInputMode),
    Focus(bool),
    SynchronizedOutput(bool),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MouseProtocolMode {
    #[default]
    X10,
    Sgr,
}

impl MouseProtocolMode {
    const fn bits(self) -> u8 {
        match self {
            Self::X10 => 0,
            Self::Sgr => 1,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Sgr,
            _ => Self::X10,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MouseReportingMode {
    #[default]
    None,
    Normal,
    ButtonEvent,
    AnyEvent,
}

impl MouseReportingMode {
    const fn bits(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Normal => 1,
            Self::ButtonEvent => 2,
            Self::AnyEvent => 3,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Normal,
            2 => Self::ButtonEvent,
            3 => Self::AnyEvent,
            _ => Self::None,
        }
    }

    pub(crate) const fn is_enabled(self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) const fn allows(self, event_kind: MouseEventKind) -> bool {
        match self {
            Self::None => false,
            Self::Normal => matches!(
                event_kind,
                MouseEventKind::Down(_)
                    | MouseEventKind::Up(_)
                    | MouseEventKind::ScrollUp
                    | MouseEventKind::ScrollDown
                    | MouseEventKind::ScrollLeft
                    | MouseEventKind::ScrollRight
            ),
            Self::ButtonEvent => !matches!(event_kind, MouseEventKind::Moved),
            Self::AnyEvent => true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MouseInputMode {
    reporting: MouseReportingMode,
    protocol: MouseProtocolMode,
}

impl MouseInputMode {
    const PROTOCOL_SHIFT: u8 = 2;

    pub(crate) const fn new(reporting: MouseReportingMode, protocol: MouseProtocolMode) -> Self {
        Self {
            reporting,
            protocol,
        }
    }

    pub(crate) const fn bits(self) -> u8 {
        self.reporting.bits() | (self.protocol.bits() << Self::PROTOCOL_SHIFT)
    }

    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self {
            reporting: MouseReportingMode::from_bits(bits & 0b11),
            protocol: MouseProtocolMode::from_bits((bits >> Self::PROTOCOL_SHIFT) & 1),
        }
    }

    pub(crate) const fn reporting(self) -> MouseReportingMode {
        self.reporting
    }

    pub(crate) const fn protocol(self) -> MouseProtocolMode {
        self.protocol
    }

    pub(crate) const fn reporting_enabled(self) -> bool {
        self.reporting.is_enabled()
    }

    pub(crate) const fn allows(self, event_kind: MouseEventKind) -> bool {
        self.reporting.allows(event_kind)
    }

    pub(crate) const fn with_reporting(self, reporting: MouseReportingMode) -> Self {
        Self { reporting, ..self }
    }
}

#[derive(Default)]
pub(crate) struct TerminalModeTracker {
    pending: Vec<u8>,
    mouse_modes: MouseModes,
    tracked_modes: TrackedTerminalModes,
}

impl TerminalModeTracker {
    const APPLICATION_KEYPAD_PREFIX: &'static [u8] = b"\x1b=";
    const CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x1b[?";
    const C1_CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x9b?";
    const NUMERIC_KEYPAD_PREFIX: &'static [u8] = b"\x1b>";

    pub(crate) fn process(&mut self, bytes: &[u8], mut emit: impl FnMut(TerminalModeChange)) {
        self.pending.extend_from_slice(bytes);

        loop {
            let Some(start) = Self::find_next_mode_start(&self.pending) else {
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

            match start.sequence {
                ModeSequence::ApplicationKeypad(enabled) => {
                    self.set_application_keypad(enabled, &mut emit);
                    self.pending.drain(..2);
                }
                ModeSequence::CsiPrivateMode { prefix_len } => {
                    match Self::parse_mode_sequence(&self.pending, prefix_len) {
                        ModeParse::Complete {
                            modes,
                            enabled,
                            consumed,
                        } => {
                            let before_mouse = self.mouse_input_mode();
                            let mut saw_mouse_mode = false;
                            for mode in modes {
                                if self.mouse_modes.set(mode, enabled) {
                                    saw_mouse_mode = true;
                                } else {
                                    self.apply_mode(mode, enabled, &mut emit);
                                }
                            }
                            if saw_mouse_mode {
                                self.emit_mouse_change(before_mouse, &mut emit);
                            }
                            self.pending.drain(..consumed);
                        }
                        ModeParse::Incomplete => return,
                        ModeParse::Invalid => {
                            self.pending.drain(..1);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn process_without_emitting(&mut self, bytes: &[u8]) {
        self.process(bytes, |_| {});
    }

    fn find_next_mode_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        [
            (
                Self::CSI_PRIVATE_MODE_PREFIX,
                ModeSequence::CsiPrivateMode {
                    prefix_len: Self::CSI_PRIVATE_MODE_PREFIX.len(),
                },
            ),
            (
                Self::C1_CSI_PRIVATE_MODE_PREFIX,
                ModeSequence::CsiPrivateMode {
                    prefix_len: Self::C1_CSI_PRIVATE_MODE_PREFIX.len(),
                },
            ),
            (
                Self::APPLICATION_KEYPAD_PREFIX,
                ModeSequence::ApplicationKeypad(true),
            ),
            (
                Self::NUMERIC_KEYPAD_PREFIX,
                ModeSequence::ApplicationKeypad(false),
            ),
        ]
        .into_iter()
        .filter_map(|(prefix, sequence)| {
            find_subslice(bytes, prefix).map(|index| ModeSequenceStart { index, sequence })
        })
        .min_by_key(|start| start.index)
    }

    fn parse_mode_sequence(bytes: &[u8], prefix_len: usize) -> ModeParse {
        let mut cursor = prefix_len;
        let mut modes = Vec::new();

        loop {
            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            let start = cursor;
            let mut mode = 0u16;
            while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                mode = mode
                    .saturating_mul(10)
                    .saturating_add(u16::from(bytes[cursor] - b'0'));
                cursor += 1;
            }

            if cursor == start {
                return ModeParse::Invalid;
            }
            modes.push(mode);

            if cursor >= bytes.len() {
                return ModeParse::Incomplete;
            }

            match bytes[cursor] {
                b';' => cursor += 1,
                b'h' | b'l' => {
                    return ModeParse::Complete {
                        modes,
                        enabled: bytes[cursor] == b'h',
                        consumed: cursor + 1,
                    };
                }
                _ => return ModeParse::Invalid,
            }
        }
    }

    fn apply_mode(&mut self, mode: u16, enabled: bool, emit: &mut impl FnMut(TerminalModeChange)) {
        match mode {
            1 => {
                if self
                    .tracked_modes
                    .set(TrackedTerminalModes::APPLICATION_CURSOR_KEYS, enabled)
                {
                    emit(TerminalModeChange::ApplicationCursorKeys(enabled));
                }
            }
            1004 => {
                if self.tracked_modes.set(TrackedTerminalModes::FOCUS, enabled) {
                    emit(TerminalModeChange::Focus(enabled));
                }
            }
            2004 => {
                if self
                    .tracked_modes
                    .set(TrackedTerminalModes::BRACKETED_PASTE, enabled)
                {
                    emit(TerminalModeChange::BracketedPaste(enabled));
                }
            }
            2026 => {
                if self
                    .tracked_modes
                    .set(TrackedTerminalModes::SYNCHRONIZED_OUTPUT, enabled)
                {
                    emit(TerminalModeChange::SynchronizedOutput(enabled));
                }
            }
            _ => {}
        }
    }

    fn set_application_keypad(&mut self, enabled: bool, emit: &mut impl FnMut(TerminalModeChange)) {
        if self
            .tracked_modes
            .set(TrackedTerminalModes::APPLICATION_KEYPAD, enabled)
        {
            emit(TerminalModeChange::ApplicationKeypad(enabled));
        }
    }

    fn emit_mouse_change(&self, before: MouseInputMode, emit: &mut impl FnMut(TerminalModeChange)) {
        let after = self.mouse_input_mode();
        if before != after && (before.reporting_enabled() || after.reporting_enabled()) {
            emit(TerminalModeChange::Mouse(after));
        }
    }

    pub(crate) fn application_cursor_keys(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::APPLICATION_CURSOR_KEYS)
    }

    pub(crate) fn application_keypad(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::APPLICATION_KEYPAD)
    }

    pub(crate) fn focus_reporting(&self) -> bool {
        self.tracked_modes.enabled(TrackedTerminalModes::FOCUS)
    }

    pub(crate) fn bracketed_paste(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::BRACKETED_PASTE)
    }

    pub(crate) fn synchronized_output(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::SYNCHRONIZED_OUTPUT)
    }

    pub(crate) fn mouse_input_mode(&self) -> MouseInputMode {
        self.mouse_modes.input_mode()
    }

    pub(crate) fn private_mode_report_value(&self, mode: u16) -> u8 {
        match mode {
            1 => mode_report_value(self.application_cursor_keys()),
            1000 | 1002 | 1003 | 1006 => self.mouse_modes.report_value(mode).unwrap_or(0),
            1004 => mode_report_value(self.focus_reporting()),
            2004 => mode_report_value(self.bracketed_paste()),
            2026 => mode_report_value(self.synchronized_output()),
            _ => 0,
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [
            Self::CSI_PRIVATE_MODE_PREFIX,
            Self::C1_CSI_PRIVATE_MODE_PREFIX,
            Self::APPLICATION_KEYPAD_PREFIX,
            Self::NUMERIC_KEYPAD_PREFIX,
        ]
        .into_iter()
        .map(|prefix| suffix_len_matching_prefix(&self.pending, prefix))
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

#[derive(Default)]
struct MouseModes(u8);

impl MouseModes {
    const NORMAL: u8 = 1;
    const BUTTON_EVENT: u8 = 1 << 1;
    const ANY_EVENT: u8 = 1 << 2;
    const SGR_PROTOCOL: u8 = 1 << 3;

    fn set(&mut self, mode: u16, enabled: bool) -> bool {
        let Some(mask) = Self::mask(mode) else {
            return false;
        };

        if enabled {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }

        true
    }

    fn input_mode(&self) -> MouseInputMode {
        let reporting = if self.0 & Self::ANY_EVENT != 0 {
            MouseReportingMode::AnyEvent
        } else if self.0 & Self::BUTTON_EVENT != 0 {
            MouseReportingMode::ButtonEvent
        } else if self.0 & Self::NORMAL != 0 {
            MouseReportingMode::Normal
        } else {
            MouseReportingMode::None
        };
        let protocol = if self.0 & Self::SGR_PROTOCOL != 0 {
            MouseProtocolMode::Sgr
        } else {
            MouseProtocolMode::X10
        };

        MouseInputMode::new(reporting, protocol)
    }

    const fn mask(mode: u16) -> Option<u8> {
        match mode {
            1000 => Some(Self::NORMAL),
            1002 => Some(Self::BUTTON_EVENT),
            1003 => Some(Self::ANY_EVENT),
            1006 => Some(Self::SGR_PROTOCOL),
            _ => None,
        }
    }

    fn report_value(&self, mode: u16) -> Option<u8> {
        Self::mask(mode).map(|mask| mode_report_value(self.0 & mask != 0))
    }
}

const fn mode_report_value(enabled: bool) -> u8 {
    if enabled { 1 } else { 2 }
}

#[derive(Clone, Copy, Default)]
struct TrackedTerminalModes(u8);

impl TrackedTerminalModes {
    const APPLICATION_CURSOR_KEYS: u8 = 1;
    const APPLICATION_KEYPAD: u8 = 1 << 1;
    const BRACKETED_PASTE: u8 = 1 << 2;
    const FOCUS: u8 = 1 << 3;
    const SYNCHRONIZED_OUTPUT: u8 = 1 << 4;

    fn set(&mut self, mode: u8, enabled: bool) -> bool {
        let before = self.0;
        if enabled {
            self.0 |= mode;
        } else {
            self.0 &= !mode;
        }
        self.0 != before
    }

    const fn enabled(self, mode: u8) -> bool {
        self.0 & mode != 0
    }
}

enum ModeParse {
    Complete {
        modes: Vec<u16>,
        enabled: bool,
        consumed: usize,
    },
    Incomplete,
    Invalid,
}

#[derive(Clone, Copy)]
struct ModeSequenceStart {
    index: usize,
    sequence: ModeSequence,
}

#[derive(Clone, Copy)]
enum ModeSequence {
    CsiPrivateMode { prefix_len: usize },
    ApplicationKeypad(bool),
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_inside_osc_or_st_control_string(bytes: &[u8], index: usize) -> bool {
    is_inside_control_string(bytes, index, find_next_osc_start, find_osc_terminator)
        || is_inside_control_string(
            bytes,
            index,
            find_next_st_control_string_start,
            find_st_terminator,
        )
}

fn is_inside_control_string(
    bytes: &[u8],
    index: usize,
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<ControlStringTerminator>,
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
    find_incomplete_control_sequence_start(bytes, find_next_osc_start, find_osc_terminator)
        .map_or(0, |start| bytes.len() - start)
        .max(suffix_len_matching_prefix(bytes, b"\x1b]"))
}

fn incomplete_st_control_sequence_suffix_len(bytes: &[u8]) -> usize {
    find_incomplete_control_sequence_start(
        bytes,
        find_next_st_control_string_start,
        find_st_terminator,
    )
    .map_or(0, |start| bytes.len() - start)
    .max(suffix_len_matching_prefix(bytes, b"\x1bP"))
    .max(suffix_len_matching_prefix(bytes, b"\x1bX"))
    .max(suffix_len_matching_prefix(bytes, b"\x1b^"))
    .max(suffix_len_matching_prefix(bytes, b"\x1b_"))
}

fn find_incomplete_control_sequence_start(
    bytes: &[u8],
    mut find_next_start: impl FnMut(&[u8]) -> Option<(usize, usize)>,
    mut find_terminator: impl FnMut(&[u8]) -> Option<ControlStringTerminator>,
) -> Option<usize> {
    let mut offset = 0;
    while offset < bytes.len() {
        let (relative_start, prefix_len) = find_next_start(&bytes[offset..])?;
        let start = offset + relative_start;
        let content_start = start + prefix_len;
        let Some(terminator) = find_terminator(&bytes[content_start..]) else {
            return Some(start);
        };
        offset = content_start + terminator.index + terminator.length;
    }

    None
}

fn find_next_osc_start(bytes: &[u8]) -> Option<(usize, usize)> {
    [(b"\x1b]".as_slice(), 2), (b"\x9d".as_slice(), 1)]
        .into_iter()
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

fn find_osc_terminator(bytes: &[u8]) -> Option<ControlStringTerminator> {
    [
        (find_subslice(bytes, b"\x1b\\"), 2),
        (find_subslice(bytes, b"\x9c"), 1),
        (find_subslice(bytes, b"\x07"), 1),
    ]
    .into_iter()
    .filter_map(|(index, length)| index.map(|index| ControlStringTerminator { index, length }))
    .min_by_key(|terminator| terminator.index)
}

fn find_st_terminator(bytes: &[u8]) -> Option<ControlStringTerminator> {
    [
        (find_subslice(bytes, b"\x1b\\"), 2),
        (find_subslice(bytes, b"\x9c"), 1),
    ]
    .into_iter()
    .filter_map(|(index, length)| index.map(|index| ControlStringTerminator { index, length }))
    .min_by_key(|terminator| terminator.index)
}

struct ControlStringTerminator {
    index: usize,
    length: usize,
}

fn suffix_len_matching_prefix(haystack: &[u8], needle: &[u8]) -> usize {
    let max = haystack.len().min(needle.len().saturating_sub(1));
    (1..=max)
        .rev()
        .find(|&length| haystack[haystack.len() - length..] == needle[..length])
        .unwrap_or(0)
}
