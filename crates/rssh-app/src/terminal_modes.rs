use crossterm::event::MouseEventKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalModeChange {
    ApplicationCursorKeys(bool),
    ApplicationKeypad(bool),
    BracketedPaste(bool),
    Mouse(MouseInputMode),
    Focus(bool),
    SynchronizedOutput(bool),
    KittyKeyboardFlags(u16),
    ModifyOtherKeys(u8),
}

pub(crate) const KITTY_KEYBOARD_DISAMBIGUATE: u16 = 1;
pub(crate) const KITTY_KEYBOARD_REPORT_EVENTS: u16 = 1 << 1;
pub(crate) const KITTY_KEYBOARD_ALTERNATE_KEYS: u16 = 1 << 2;
pub(crate) const KITTY_KEYBOARD_REPORT_ALL: u16 = 1 << 3;
pub(crate) const KITTY_KEYBOARD_ASSOCIATED_TEXT: u16 = 1 << 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MouseProtocolMode {
    #[default]
    X10,
    Utf8,
    Sgr,
    Urxvt,
}

impl MouseProtocolMode {
    const fn bits(self) -> u8 {
        match self {
            Self::X10 => 0,
            Self::Utf8 => 1,
            Self::Sgr => 2,
            Self::Urxvt => 3,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Utf8,
            2 => Self::Sgr,
            3 => Self::Urxvt,
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
            protocol: MouseProtocolMode::from_bits((bits >> Self::PROTOCOL_SHIFT) & 0b11),
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
    kitty_keyboard_modes: KittyKeyboardModes,
    modify_other_keys: u8,
    tracked_modes: TrackedTerminalModes,
}

pub(crate) struct SynchronizedOutputModeSequence {
    pub(crate) index: usize,
    pub(crate) consumed: usize,
    pub(crate) enabled: bool,
}

pub(crate) struct KittyKeyboardModeSequence {
    pub(crate) index: usize,
    pub(crate) consumed: usize,
}

pub(crate) struct KittyKeyboardFlagsQuery {
    pub(crate) index: usize,
    pub(crate) consumed: usize,
}

pub(crate) struct KeyModifierOptionsSequence {
    pub(crate) index: usize,
    pub(crate) consumed: usize,
}

pub(crate) struct KeyModifierOptionsQuery {
    pub(crate) index: usize,
    pub(crate) consumed: usize,
    pub(crate) resource: u16,
}

impl TerminalModeTracker {
    const APPLICATION_KEYPAD_PREFIX: &'static [u8] = b"\x1b=";
    const CSI_MODE_PREFIX: &'static [u8] = b"\x1b[";
    const CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x1b[?";
    const CSI_KITTY_KEYBOARD_PUSH_PREFIX: &'static [u8] = b"\x1b[>";
    const CSI_KITTY_KEYBOARD_POP_PREFIX: &'static [u8] = b"\x1b[<";
    const CSI_KITTY_KEYBOARD_SET_PREFIX: &'static [u8] = b"\x1b[=";
    const C1_CSI_MODE_PREFIX: &'static [u8] = b"\x9b";
    const C1_CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\x9b?";
    const C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX: &'static [u8] = b"\x9b>";
    const C1_CSI_KITTY_KEYBOARD_POP_PREFIX: &'static [u8] = b"\x9b<";
    const C1_CSI_KITTY_KEYBOARD_SET_PREFIX: &'static [u8] = b"\x9b=";
    const NUMERIC_KEYPAD_PREFIX: &'static [u8] = b"\x1b>";
    const RESET_PREFIX: &'static [u8] = b"\x1bc";
    const SOFT_RESET_PREFIX: &'static [u8] = b"\x1b[!p";
    const C1_SOFT_RESET_PREFIX: &'static [u8] = b"\x9b!p";

    #[allow(clippy::too_many_lines)]
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
                ModeSequence::Reset => {
                    self.reset(&mut emit);
                    self.pending.drain(..2);
                }
                ModeSequence::SoftReset { prefix_len } => {
                    self.soft_reset();
                    self.pending.drain(..prefix_len);
                }
                ModeSequence::KittyKeyboard {
                    prefix_len,
                    operation,
                } => match Self::parse_kitty_keyboard_mode_sequence(
                    &self.pending,
                    prefix_len,
                    operation,
                ) {
                    ModeParse::KittyKeyboard {
                        value,
                        apply_mode,
                        consumed,
                    } => {
                        self.apply_kitty_keyboard_mode(operation, value, apply_mode, &mut emit);
                        self.pending.drain(..consumed);
                    }
                    ModeParse::Incomplete => return,
                    ModeParse::Invalid => {
                        self.pending.drain(..1);
                    }
                    ModeParse::Complete { .. } | ModeParse::KeyModifierOptions { .. } => {
                        unreachable!()
                    }
                },
                ModeSequence::KeyModifierOptions { prefix_len } => {
                    match Self::parse_kitty_keyboard_mode_sequence(
                        &self.pending,
                        prefix_len,
                        KittyKeyboardOperation::Push,
                    ) {
                        ModeParse::KittyKeyboard {
                            value,
                            apply_mode,
                            consumed,
                        } => {
                            self.apply_kitty_keyboard_mode(
                                KittyKeyboardOperation::Push,
                                value,
                                apply_mode,
                                &mut emit,
                            );
                            self.pending.drain(..consumed);
                        }
                        ModeParse::Incomplete => return,
                        ModeParse::Invalid => match Self::parse_key_modifier_options_sequence(
                            &self.pending,
                            prefix_len,
                        ) {
                            ModeParse::KeyModifierOptions {
                                resource,
                                value,
                                consumed,
                            } => {
                                self.apply_key_modifier_options(resource, value, &mut emit);
                                self.pending.drain(..consumed);
                            }
                            ModeParse::Incomplete => return,
                            ModeParse::Invalid => {
                                self.pending.drain(..1);
                            }
                            ModeParse::Complete { .. } | ModeParse::KittyKeyboard { .. } => {
                                unreachable!()
                            }
                        },
                        ModeParse::Complete { .. } | ModeParse::KeyModifierOptions { .. } => {
                            unreachable!()
                        }
                    }
                }
                ModeSequence::CsiMode { prefix_len } => {
                    match Self::parse_mode_sequence(&self.pending, prefix_len) {
                        ModeParse::Complete {
                            modes,
                            enabled,
                            consumed,
                        } => {
                            for mode in modes {
                                self.apply_ansi_mode(mode, enabled);
                            }
                            self.pending.drain(..consumed);
                        }
                        ModeParse::Incomplete => return,
                        ModeParse::Invalid => {
                            self.pending.drain(..1);
                        }
                        ModeParse::KittyKeyboard { .. } | ModeParse::KeyModifierOptions { .. } => {
                            unreachable!()
                        }
                    }
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
                        ModeParse::KittyKeyboard { .. } | ModeParse::KeyModifierOptions { .. } => {
                            unreachable!()
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
                Self::CSI_KITTY_KEYBOARD_PUSH_PREFIX,
                ModeSequence::KeyModifierOptions {
                    prefix_len: Self::CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
                },
            ),
            (
                Self::CSI_KITTY_KEYBOARD_POP_PREFIX,
                ModeSequence::KittyKeyboard {
                    prefix_len: Self::CSI_KITTY_KEYBOARD_POP_PREFIX.len(),
                    operation: KittyKeyboardOperation::Pop,
                },
            ),
            (
                Self::CSI_KITTY_KEYBOARD_SET_PREFIX,
                ModeSequence::KittyKeyboard {
                    prefix_len: Self::CSI_KITTY_KEYBOARD_SET_PREFIX.len(),
                    operation: KittyKeyboardOperation::Apply,
                },
            ),
            (
                Self::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
                ModeSequence::KeyModifierOptions {
                    prefix_len: Self::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
                },
            ),
            (
                Self::C1_CSI_KITTY_KEYBOARD_POP_PREFIX,
                ModeSequence::KittyKeyboard {
                    prefix_len: Self::C1_CSI_KITTY_KEYBOARD_POP_PREFIX.len(),
                    operation: KittyKeyboardOperation::Pop,
                },
            ),
            (
                Self::C1_CSI_KITTY_KEYBOARD_SET_PREFIX,
                ModeSequence::KittyKeyboard {
                    prefix_len: Self::C1_CSI_KITTY_KEYBOARD_SET_PREFIX.len(),
                    operation: KittyKeyboardOperation::Apply,
                },
            ),
            (
                Self::SOFT_RESET_PREFIX,
                ModeSequence::SoftReset {
                    prefix_len: Self::SOFT_RESET_PREFIX.len(),
                },
            ),
            (
                Self::C1_SOFT_RESET_PREFIX,
                ModeSequence::SoftReset {
                    prefix_len: Self::C1_SOFT_RESET_PREFIX.len(),
                },
            ),
            (
                Self::CSI_MODE_PREFIX,
                ModeSequence::CsiMode {
                    prefix_len: Self::CSI_MODE_PREFIX.len(),
                },
            ),
            (
                Self::C1_CSI_MODE_PREFIX,
                ModeSequence::CsiMode {
                    prefix_len: Self::C1_CSI_MODE_PREFIX.len(),
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
            (Self::RESET_PREFIX, ModeSequence::Reset),
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

    fn parse_kitty_keyboard_mode_sequence(
        bytes: &[u8],
        prefix_len: usize,
        operation: KittyKeyboardOperation,
    ) -> ModeParse {
        let mut cursor = prefix_len;
        let mut value = 0u16;
        let value_start = cursor;

        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            value = value
                .saturating_mul(10)
                .saturating_add(u16::from(bytes[cursor] - b'0'));
            cursor += 1;
        }

        if cursor >= bytes.len() {
            return ModeParse::Incomplete;
        }

        if matches!(operation, KittyKeyboardOperation::Apply) && cursor == value_start {
            return ModeParse::Invalid;
        }

        let apply_mode =
            if matches!(operation, KittyKeyboardOperation::Apply) && bytes[cursor] == b';' {
                cursor += 1;
                if cursor >= bytes.len() {
                    return ModeParse::Incomplete;
                }
                let mode_start = cursor;
                let mut mode = 0u16;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    mode = mode
                        .saturating_mul(10)
                        .saturating_add(u16::from(bytes[cursor] - b'0'));
                    cursor += 1;
                }
                if cursor == mode_start {
                    return ModeParse::Invalid;
                }
                match mode {
                    1 => KittyKeyboardApplyMode::Replace,
                    2 => KittyKeyboardApplyMode::Set,
                    3 => KittyKeyboardApplyMode::Reset,
                    _ => return ModeParse::Invalid,
                }
            } else {
                KittyKeyboardApplyMode::Replace
            };

        if bytes[cursor] != b'u' {
            return ModeParse::Invalid;
        }

        ModeParse::KittyKeyboard {
            value,
            apply_mode,
            consumed: cursor + 1,
        }
    }

    fn parse_key_modifier_options_sequence(bytes: &[u8], prefix_len: usize) -> ModeParse {
        let mut cursor = prefix_len;
        if cursor >= bytes.len() {
            return ModeParse::Incomplete;
        }

        if bytes[cursor] == b'm' {
            return ModeParse::KeyModifierOptions {
                resource: None,
                value: None,
                consumed: cursor + 1,
            };
        }

        let resource_start = cursor;
        let mut resource = 0u16;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            resource = resource
                .saturating_mul(10)
                .saturating_add(u16::from(bytes[cursor] - b'0'));
            cursor += 1;
        }

        if cursor == resource_start {
            return ModeParse::Invalid;
        }
        if cursor >= bytes.len() {
            return ModeParse::Incomplete;
        }

        let value = match bytes[cursor] {
            b'm' => None,
            b';' => {
                cursor += 1;
                if cursor >= bytes.len() {
                    return ModeParse::Incomplete;
                }
                let value_start = cursor;
                let mut value = 0u16;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    value = value
                        .saturating_mul(10)
                        .saturating_add(u16::from(bytes[cursor] - b'0'));
                    cursor += 1;
                }
                if cursor == value_start {
                    return ModeParse::Invalid;
                }
                if cursor >= bytes.len() {
                    return ModeParse::Incomplete;
                }
                if bytes[cursor] != b'm' {
                    return ModeParse::Invalid;
                }
                Some(value)
            }
            _ => return ModeParse::Invalid,
        };

        ModeParse::KeyModifierOptions {
            resource: Some(resource),
            value,
            consumed: cursor + 1,
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
            6 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::ORIGIN_MODE, enabled);
            }
            7 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::AUTO_WRAP, enabled);
            }
            12 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::CURSOR_BLINKING, enabled);
            }
            25 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::CURSOR_VISIBLE, enabled);
            }
            69 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::LEFT_RIGHT_MARGIN_MODE, enabled);
            }
            80 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::SIXEL_SCROLLING, enabled);
            }
            1034 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::META_KEY, enabled);
            }
            47 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::ALTERNATE_SCREEN_47, enabled);
            }
            1048 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::PRIVATE_CURSOR_SAVE, enabled);
            }
            1047 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::ALTERNATE_SCREEN_1047, enabled);
            }
            1049 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::ALTERNATE_SCREEN_1049, enabled);
            }
            _ => {}
        }
    }

    fn apply_kitty_keyboard_mode(
        &mut self,
        operation: KittyKeyboardOperation,
        value: u16,
        apply_mode: KittyKeyboardApplyMode,
        emit: &mut impl FnMut(TerminalModeChange),
    ) {
        let before = self.kitty_keyboard_flags();
        match operation {
            KittyKeyboardOperation::Push => self.kitty_keyboard_modes.push(value),
            KittyKeyboardOperation::Pop => self.kitty_keyboard_modes.pop(value),
            KittyKeyboardOperation::Apply => self.kitty_keyboard_modes.apply(value, apply_mode),
        }
        let after = self.kitty_keyboard_flags();
        if before != after {
            emit(TerminalModeChange::KittyKeyboardFlags(after));
        }
    }

    fn apply_key_modifier_options(
        &mut self,
        resource: Option<u16>,
        value: Option<u16>,
        emit: &mut impl FnMut(TerminalModeChange),
    ) {
        if resource.is_none() || resource == Some(4) {
            let next = value
                .and_then(|value| u8::try_from(value).ok())
                .filter(|value| *value <= 2)
                .unwrap_or(0);
            if self.modify_other_keys != next {
                self.modify_other_keys = next;
                emit(TerminalModeChange::ModifyOtherKeys(next));
            }
        }
    }

    fn apply_ansi_mode(&mut self, mode: u16, enabled: bool) {
        if mode == 4 {
            self.tracked_modes
                .set(TrackedTerminalModes::INSERT_MODE, enabled);
        }
    }

    fn soft_reset(&mut self) {
        self.tracked_modes
            .set(TrackedTerminalModes::ORIGIN_MODE, false);
        self.tracked_modes
            .set(TrackedTerminalModes::INSERT_MODE, false);
        self.tracked_modes
            .set(TrackedTerminalModes::LEFT_RIGHT_MARGIN_MODE, false);
    }

    fn reset(&mut self, emit: &mut impl FnMut(TerminalModeChange)) {
        let application_cursor_keys = self.application_cursor_keys();
        let application_keypad = self.application_keypad();
        let bracketed_paste = self.bracketed_paste();
        let mouse_input_mode = self.mouse_input_mode();
        let focus_reporting = self.focus_reporting();
        let synchronized_output = self.synchronized_output();
        let kitty_keyboard_flags = self.kitty_keyboard_flags();
        let modify_other_keys = self.modify_other_keys();

        self.mouse_modes = MouseModes::default();
        self.kitty_keyboard_modes = KittyKeyboardModes::default();
        self.modify_other_keys = 0;
        self.tracked_modes = TrackedTerminalModes::default();

        if application_cursor_keys {
            emit(TerminalModeChange::ApplicationCursorKeys(false));
        }
        if application_keypad {
            emit(TerminalModeChange::ApplicationKeypad(false));
        }
        if bracketed_paste {
            emit(TerminalModeChange::BracketedPaste(false));
        }
        if mouse_input_mode != MouseInputMode::default() {
            emit(TerminalModeChange::Mouse(MouseInputMode::default()));
        }
        if focus_reporting {
            emit(TerminalModeChange::Focus(false));
        }
        if synchronized_output {
            emit(TerminalModeChange::SynchronizedOutput(false));
        }
        if kitty_keyboard_flags != 0 {
            emit(TerminalModeChange::KittyKeyboardFlags(0));
        }
        if modify_other_keys != 0 {
            emit(TerminalModeChange::ModifyOtherKeys(0));
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

    pub(crate) fn kitty_keyboard_flags(&self) -> u16 {
        self.kitty_keyboard_modes.flags()
    }

    pub(crate) const fn modify_other_keys(&self) -> u8 {
        self.modify_other_keys
    }

    pub(crate) fn private_mode_report_value(&self, mode: u16) -> u8 {
        match mode {
            1 => mode_report_value(self.application_cursor_keys()),
            6 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::ORIGIN_MODE),
            ),
            7 => mode_report_value(self.tracked_modes.enabled(TrackedTerminalModes::AUTO_WRAP)),
            12 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::CURSOR_BLINKING),
            ),
            25 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::CURSOR_VISIBLE),
            ),
            47 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::ALTERNATE_SCREEN_47),
            ),
            69 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::LEFT_RIGHT_MARGIN_MODE),
            ),
            80 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::SIXEL_SCROLLING),
            ),
            1000 | 1002 | 1003 | 1005 | 1006 | 1015 => {
                self.mouse_modes.report_value(mode).unwrap_or(0)
            }
            1004 => mode_report_value(self.focus_reporting()),
            1034 => mode_report_value(self.tracked_modes.enabled(TrackedTerminalModes::META_KEY)),
            1048 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::PRIVATE_CURSOR_SAVE),
            ),
            1047 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::ALTERNATE_SCREEN_1047),
            ),
            1049 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::ALTERNATE_SCREEN_1049),
            ),
            2004 => mode_report_value(self.bracketed_paste()),
            2026 => mode_report_value(self.synchronized_output()),
            _ => 0,
        }
    }

    pub(crate) fn ansi_mode_report_value(&self, mode: u16) -> u8 {
        match mode {
            4 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::INSERT_MODE),
            ),
            _ => 0,
        }
    }

    fn retain_possible_prefix(&mut self) {
        let retained = [
            Self::CSI_MODE_PREFIX,
            Self::CSI_PRIVATE_MODE_PREFIX,
            Self::CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            Self::CSI_KITTY_KEYBOARD_POP_PREFIX,
            Self::CSI_KITTY_KEYBOARD_SET_PREFIX,
            Self::C1_CSI_MODE_PREFIX,
            Self::C1_CSI_PRIVATE_MODE_PREFIX,
            Self::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            Self::C1_CSI_KITTY_KEYBOARD_POP_PREFIX,
            Self::C1_CSI_KITTY_KEYBOARD_SET_PREFIX,
            Self::APPLICATION_KEYPAD_PREFIX,
            Self::NUMERIC_KEYPAD_PREFIX,
            Self::RESET_PREFIX,
            Self::SOFT_RESET_PREFIX,
            Self::C1_SOFT_RESET_PREFIX,
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

#[derive(Clone, Copy)]
enum KittyKeyboardOperation {
    Push,
    Pop,
    Apply,
}

#[derive(Clone, Copy)]
enum KittyKeyboardApplyMode {
    Replace,
    Set,
    Reset,
}

#[derive(Default)]
struct KittyKeyboardModes {
    flags: u16,
    stack: Vec<u16>,
}

impl KittyKeyboardModes {
    const MAX_STACK: usize = 32;

    fn flags(&self) -> u16 {
        self.flags
    }

    fn push(&mut self, flags: u16) {
        if self.stack.len() == Self::MAX_STACK {
            self.stack.remove(0);
        }
        self.stack.push(self.flags);
        self.flags = flags;
    }

    fn pop(&mut self, count: u16) {
        let count = if count == 0 { 1 } else { count };
        for _ in 0..count {
            let Some(flags) = self.stack.pop() else {
                self.flags = 0;
                return;
            };
            self.flags = flags;
        }
    }

    fn apply(&mut self, flags: u16, mode: KittyKeyboardApplyMode) {
        match mode {
            KittyKeyboardApplyMode::Replace => self.flags = flags,
            KittyKeyboardApplyMode::Set => self.flags |= flags,
            KittyKeyboardApplyMode::Reset => self.flags &= !flags,
        }
    }
}

#[derive(Default)]
struct MouseModes(u8);

impl MouseModes {
    const NORMAL: u8 = 1;
    const BUTTON_EVENT: u8 = 1 << 1;
    const ANY_EVENT: u8 = 1 << 2;
    const UTF8_PROTOCOL: u8 = 1 << 3;
    const SGR_PROTOCOL: u8 = 1 << 4;
    const URXVT_PROTOCOL: u8 = 1 << 5;

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
        } else if self.0 & Self::URXVT_PROTOCOL != 0 {
            MouseProtocolMode::Urxvt
        } else if self.0 & Self::UTF8_PROTOCOL != 0 {
            MouseProtocolMode::Utf8
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
            1005 => Some(Self::UTF8_PROTOCOL),
            1006 => Some(Self::SGR_PROTOCOL),
            1015 => Some(Self::URXVT_PROTOCOL),
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

#[derive(Clone, Copy)]
struct TrackedTerminalModes(u32);

impl TrackedTerminalModes {
    const APPLICATION_CURSOR_KEYS: u32 = 1;
    const APPLICATION_KEYPAD: u32 = 1 << 1;
    const BRACKETED_PASTE: u32 = 1 << 2;
    const FOCUS: u32 = 1 << 3;
    const SYNCHRONIZED_OUTPUT: u32 = 1 << 4;
    const ORIGIN_MODE: u32 = 1 << 5;
    const AUTO_WRAP: u32 = 1 << 6;
    const CURSOR_VISIBLE: u32 = 1 << 7;
    const CURSOR_BLINKING: u32 = 1 << 8;
    const ALTERNATE_SCREEN_47: u32 = 1 << 9;
    const ALTERNATE_SCREEN_1047: u32 = 1 << 10;
    const ALTERNATE_SCREEN_1049: u32 = 1 << 11;
    const PRIVATE_CURSOR_SAVE: u32 = 1 << 12;
    const INSERT_MODE: u32 = 1 << 13;
    const META_KEY: u32 = 1 << 14;
    const LEFT_RIGHT_MARGIN_MODE: u32 = 1 << 15;
    const SIXEL_SCROLLING: u32 = 1 << 16;

    fn set(&mut self, mode: u32, enabled: bool) -> bool {
        let before = self.0;
        if enabled {
            self.0 |= mode;
        } else {
            self.0 &= !mode;
        }
        self.0 != before
    }

    const fn enabled(self, mode: u32) -> bool {
        self.0 & mode != 0
    }
}

impl Default for TrackedTerminalModes {
    fn default() -> Self {
        Self(Self::AUTO_WRAP | Self::CURSOR_VISIBLE | Self::SIXEL_SCROLLING)
    }
}

pub(crate) fn find_synchronized_output_mode_sequence(
    bytes: &[u8],
) -> Option<SynchronizedOutputModeSequence> {
    synchronized_output_private_mode_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            let mut offset = 0;
            let mut match_sequence = None;

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && let ModeParse::Complete {
                        modes,
                        enabled,
                        consumed,
                    } = TerminalModeTracker::parse_mode_sequence(&bytes[index..], prefix_len)
                    && modes.contains(&2026)
                {
                    match_sequence = Some(SynchronizedOutputModeSequence {
                        index,
                        consumed,
                        enabled,
                    });
                    break;
                }
                offset = index.saturating_add(1);
            }

            match_sequence
        })
        .min_by_key(|sequence| sequence.index)
}

pub(crate) fn synchronized_output_mode_sequence_suffix_len(bytes: &[u8]) -> usize {
    synchronized_output_private_mode_prefixes()
        .into_iter()
        .map(|(prefix, prefix_len)| {
            let mut offset = 0;
            let mut retained = suffix_len_matching_prefix(bytes, prefix);

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && matches!(
                        TerminalModeTracker::parse_mode_sequence(&bytes[index..], prefix_len),
                        ModeParse::Incomplete
                    )
                {
                    retained = retained.max(bytes.len() - index);
                }
                offset = index.saturating_add(1);
            }

            retained
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn find_kitty_keyboard_mode_sequence(bytes: &[u8]) -> Option<KittyKeyboardModeSequence> {
    kitty_keyboard_mode_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len, operation)| {
            let mut offset = 0;
            let mut match_sequence = None;

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && let ModeParse::KittyKeyboard { consumed, .. } =
                        TerminalModeTracker::parse_kitty_keyboard_mode_sequence(
                            &bytes[index..],
                            prefix_len,
                            operation,
                        )
                {
                    match_sequence = Some(KittyKeyboardModeSequence { index, consumed });
                    break;
                }
                offset = index.saturating_add(1);
            }

            match_sequence
        })
        .min_by_key(|sequence| sequence.index)
}

pub(crate) fn kitty_keyboard_mode_sequence_suffix_len(bytes: &[u8]) -> usize {
    kitty_keyboard_mode_prefixes()
        .into_iter()
        .map(|(prefix, prefix_len, operation)| {
            let mut offset = 0;
            let mut retained = suffix_len_matching_prefix(bytes, prefix);

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && matches!(
                        TerminalModeTracker::parse_kitty_keyboard_mode_sequence(
                            &bytes[index..],
                            prefix_len,
                            operation,
                        ),
                        ModeParse::Incomplete
                    )
                {
                    retained = retained.max(bytes.len() - index);
                }
                offset = index.saturating_add(1);
            }

            retained
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn find_key_modifier_options_sequence(
    bytes: &[u8],
) -> Option<KeyModifierOptionsSequence> {
    key_modifier_options_prefixes()
        .into_iter()
        .filter_map(|(prefix, prefix_len)| {
            let mut offset = 0;
            let mut match_sequence = None;

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && let ModeParse::KeyModifierOptions { consumed, .. } =
                        TerminalModeTracker::parse_key_modifier_options_sequence(
                            &bytes[index..],
                            prefix_len,
                        )
                {
                    match_sequence = Some(KeyModifierOptionsSequence { index, consumed });
                    break;
                }
                offset = index.saturating_add(1);
            }

            match_sequence
        })
        .min_by_key(|sequence| sequence.index)
}

pub(crate) fn key_modifier_options_sequence_suffix_len(bytes: &[u8]) -> usize {
    key_modifier_options_prefixes()
        .into_iter()
        .map(|(prefix, prefix_len)| {
            let mut offset = 0;
            let mut retained = suffix_len_matching_prefix(bytes, prefix);

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if !is_inside_osc_or_st_control_string(bytes, index)
                    && matches!(
                        TerminalModeTracker::parse_key_modifier_options_sequence(
                            &bytes[index..],
                            prefix_len,
                        ),
                        ModeParse::Incomplete
                    )
                {
                    retained = retained.max(bytes.len() - index);
                }
                offset = index.saturating_add(1);
            }

            retained
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn find_kitty_keyboard_flags_query(bytes: &[u8]) -> Option<KittyKeyboardFlagsQuery> {
    kitty_keyboard_flags_query_prefixes()
        .into_iter()
        .filter_map(|prefix| {
            let mut offset = 0;
            let mut match_query = None;

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                let consumed = prefix.len() + 1;
                if !is_inside_osc_or_st_control_string(bytes, index) {
                    if bytes.len() < index + consumed {
                        return None;
                    }
                    if bytes[index + prefix.len()] == b'u' {
                        match_query = Some(KittyKeyboardFlagsQuery { index, consumed });
                        break;
                    }
                }
                offset = index.saturating_add(1);
            }

            match_query
        })
        .min_by_key(|query| query.index)
}

pub(crate) fn kitty_keyboard_flags_query_suffix_len(bytes: &[u8]) -> usize {
    kitty_keyboard_flags_query_prefixes()
        .into_iter()
        .map(|prefix| {
            let prefix_suffix = suffix_len_matching_prefix(bytes, prefix);
            let incomplete_query = find_subslice(bytes, prefix)
                .filter(|index| bytes.len() < index + prefix.len() + 1)
                .map_or(0, |index| bytes.len() - index);
            prefix_suffix.max(incomplete_query)
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn find_key_modifier_options_query(bytes: &[u8]) -> Option<KeyModifierOptionsQuery> {
    kitty_keyboard_flags_query_prefixes()
        .into_iter()
        .filter_map(|prefix| {
            let mut offset = 0;
            let mut match_query = None;

            while offset < bytes.len() {
                let Some(relative_index) = find_subslice(&bytes[offset..], prefix) else {
                    break;
                };
                let index = offset + relative_index;
                if is_inside_osc_or_st_control_string(bytes, index) {
                    offset = index.saturating_add(1);
                    continue;
                }

                let mut cursor = index + prefix.len();
                if cursor >= bytes.len() {
                    return None;
                }
                let resource_start = cursor;
                let mut resource = 0u16;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    resource = resource
                        .saturating_mul(10)
                        .saturating_add(u16::from(bytes[cursor] - b'0'));
                    cursor += 1;
                }
                if cursor == resource_start {
                    offset = index.saturating_add(1);
                    continue;
                }
                if cursor >= bytes.len() {
                    return None;
                }
                if bytes[cursor] == b'm' {
                    match_query = Some(KeyModifierOptionsQuery {
                        index,
                        consumed: cursor + 1 - index,
                        resource,
                    });
                    break;
                }
                offset = index.saturating_add(1);
            }

            match_query
        })
        .min_by_key(|query| query.index)
}

pub(crate) fn key_modifier_options_query_suffix_len(bytes: &[u8]) -> usize {
    kitty_keyboard_flags_query_prefixes()
        .into_iter()
        .map(|prefix| {
            let prefix_suffix = suffix_len_matching_prefix(bytes, prefix);
            let incomplete_query = find_subslice(bytes, prefix)
                .filter(|index| {
                    let mut cursor = index + prefix.len();
                    if cursor >= bytes.len() {
                        return true;
                    }
                    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                        cursor += 1;
                    }
                    cursor >= bytes.len()
                })
                .map_or(0, |index| bytes.len() - index);
            prefix_suffix.max(incomplete_query)
        })
        .max()
        .unwrap_or(0)
}

fn synchronized_output_private_mode_prefixes() -> [(&'static [u8], usize); 2] {
    [
        (
            TerminalModeTracker::CSI_PRIVATE_MODE_PREFIX,
            TerminalModeTracker::CSI_PRIVATE_MODE_PREFIX.len(),
        ),
        (
            TerminalModeTracker::C1_CSI_PRIVATE_MODE_PREFIX,
            TerminalModeTracker::C1_CSI_PRIVATE_MODE_PREFIX.len(),
        ),
    ]
}

fn kitty_keyboard_mode_prefixes() -> [(&'static [u8], usize, KittyKeyboardOperation); 6] {
    [
        (
            TerminalModeTracker::CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            TerminalModeTracker::CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
            KittyKeyboardOperation::Push,
        ),
        (
            TerminalModeTracker::CSI_KITTY_KEYBOARD_POP_PREFIX,
            TerminalModeTracker::CSI_KITTY_KEYBOARD_POP_PREFIX.len(),
            KittyKeyboardOperation::Pop,
        ),
        (
            TerminalModeTracker::CSI_KITTY_KEYBOARD_SET_PREFIX,
            TerminalModeTracker::CSI_KITTY_KEYBOARD_SET_PREFIX.len(),
            KittyKeyboardOperation::Apply,
        ),
        (
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
            KittyKeyboardOperation::Push,
        ),
        (
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_POP_PREFIX,
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_POP_PREFIX.len(),
            KittyKeyboardOperation::Pop,
        ),
        (
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_SET_PREFIX,
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_SET_PREFIX.len(),
            KittyKeyboardOperation::Apply,
        ),
    ]
}

fn key_modifier_options_prefixes() -> [(&'static [u8], usize); 2] {
    [
        (
            TerminalModeTracker::CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            TerminalModeTracker::CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
        ),
        (
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            TerminalModeTracker::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
        ),
    ]
}

fn kitty_keyboard_flags_query_prefixes() -> [&'static [u8]; 2] {
    [
        b"\x1b[?".as_slice(),
        TerminalModeTracker::C1_CSI_PRIVATE_MODE_PREFIX,
    ]
}

enum ModeParse {
    Complete {
        modes: Vec<u16>,
        enabled: bool,
        consumed: usize,
    },
    KittyKeyboard {
        value: u16,
        apply_mode: KittyKeyboardApplyMode,
        consumed: usize,
    },
    KeyModifierOptions {
        resource: Option<u16>,
        value: Option<u16>,
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
    CsiPrivateMode {
        prefix_len: usize,
    },
    CsiMode {
        prefix_len: usize,
    },
    KittyKeyboard {
        prefix_len: usize,
        operation: KittyKeyboardOperation,
    },
    KeyModifierOptions {
        prefix_len: usize,
    },
    ApplicationKeypad(bool),
    Reset,
    SoftReset {
        prefix_len: usize,
    },
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

#[cfg(test)]
mod tests {
    use super::{TerminalModeChange, TerminalModeTracker};

    #[test]
    fn tracks_kitty_keyboard_protocol_push_pop_flags() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        assert_eq!(tracker.kitty_keyboard_flags(), 0);

        tracker.process(b"\x1b[>1u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 1);

        tracker.process(b"\x1b[>9u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 9);

        tracker.process(b"\x1b[<u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 1);

        tracker.process(b"\x1b[<1u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 0);
        assert_eq!(
            changes,
            vec![
                TerminalModeChange::KittyKeyboardFlags(1),
                TerminalModeChange::KittyKeyboardFlags(9),
                TerminalModeChange::KittyKeyboardFlags(1),
                TerminalModeChange::KittyKeyboardFlags(0)
            ]
        );
    }

    #[test]
    fn tracks_kitty_keyboard_protocol_set_reset_flags() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        tracker.process(b"\x1b[=1u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 1);

        tracker.process(b"\x1b[=8;2u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 9);

        tracker.process(b"\x1b[=1;3u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 8);

        tracker.process(b"\x1b[=8;1u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 8);

        tracker.process(b"\x1b[=0u", |change| changes.push(change));
        assert_eq!(tracker.kitty_keyboard_flags(), 0);
        assert_eq!(
            changes,
            vec![
                TerminalModeChange::KittyKeyboardFlags(1),
                TerminalModeChange::KittyKeyboardFlags(9),
                TerminalModeChange::KittyKeyboardFlags(8),
                TerminalModeChange::KittyKeyboardFlags(0)
            ]
        );
    }

    #[test]
    fn tracks_split_and_c1_kitty_keyboard_protocol_flags() {
        let mut tracker = TerminalModeTracker::default();

        tracker.process(b"\x1b[>", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 0);

        tracker.process(b"17u", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 17);

        tracker.process(b"\x9b>3u", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 3);

        tracker.process(b"\x9b<2u", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 0);
    }

    #[test]
    fn tracks_split_and_c1_kitty_keyboard_protocol_set_flags() {
        let mut tracker = TerminalModeTracker::default();

        tracker.process(b"\x1b[=", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 0);

        tracker.process(b"17u", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 17);

        tracker.process(b"\x9b=1;3u", |_| {});
        assert_eq!(tracker.kitty_keyboard_flags(), 16);
    }

    #[test]
    fn tracks_xterm_modify_other_keys_mode() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        assert_eq!(tracker.modify_other_keys(), 0);

        tracker.process(b"\x1b[>4;2m", |change| changes.push(change));
        assert_eq!(tracker.modify_other_keys(), 2);

        tracker.process(b"\x9b>4;1m", |change| changes.push(change));
        assert_eq!(tracker.modify_other_keys(), 1);

        tracker.process(b"\x1b[>4;0m", |change| changes.push(change));
        assert_eq!(tracker.modify_other_keys(), 0);
        assert_eq!(
            changes,
            vec![
                TerminalModeChange::ModifyOtherKeys(2),
                TerminalModeChange::ModifyOtherKeys(1),
                TerminalModeChange::ModifyOtherKeys(0)
            ]
        );
    }
}
