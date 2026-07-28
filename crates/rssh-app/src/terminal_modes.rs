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
    Win32InputMode(bool),
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
    SgrPixels,
}

impl MouseProtocolMode {
    const fn bits(self) -> u8 {
        match self {
            Self::X10 => 0,
            Self::Utf8 => 1,
            Self::Sgr => 2,
            Self::Urxvt => 3,
            Self::SgrPixels => 4,
        }
    }

    const fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Utf8,
            2 => Self::Sgr,
            3 => Self::Urxvt,
            4 => Self::SgrPixels,
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
    const PROTOCOL_MASK: u8 = 0b111;

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
            protocol: MouseProtocolMode::from_bits(
                (bits >> Self::PROTOCOL_SHIFT) & Self::PROTOCOL_MASK,
            ),
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

pub(crate) struct TerminalModeTracker {
    pending: Vec<u8>,
    mouse_modes: MouseModes,
    kitty_keyboard_modes: KittyKeyboardModes,
    modify_other_keys: u8,
    allow_win32_input_mode: bool,
    tracked_modes: TrackedTerminalModes,
}

impl Default for TerminalModeTracker {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            mouse_modes: MouseModes::default(),
            kitty_keyboard_modes: KittyKeyboardModes::default(),
            modify_other_keys: 0,
            allow_win32_input_mode: true,
            tracked_modes: TrackedTerminalModes::default(),
        }
    }
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
    const UTF8_C1_CSI_MODE_PREFIX: &'static [u8] = b"\xc2\x9b";
    const UTF8_C1_CSI_PRIVATE_MODE_PREFIX: &'static [u8] = b"\xc2\x9b?";
    const UTF8_C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX: &'static [u8] = b"\xc2\x9b>";
    const UTF8_C1_CSI_KITTY_KEYBOARD_POP_PREFIX: &'static [u8] = b"\xc2\x9b<";
    const UTF8_C1_CSI_KITTY_KEYBOARD_SET_PREFIX: &'static [u8] = b"\xc2\x9b=";
    const NUMERIC_KEYPAD_PREFIX: &'static [u8] = b"\x1b>";
    const RESET_PREFIX: &'static [u8] = b"\x1bc";
    const SOFT_RESET_PREFIX: &'static [u8] = b"\x1b[!p";
    const C1_SOFT_RESET_PREFIX: &'static [u8] = b"\x9b!p";
    const UTF8_C1_SOFT_RESET_PREFIX: &'static [u8] = b"\xc2\x9b!p";

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
                    self.soft_reset(&mut emit);
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

    pub(crate) fn clear_kitty_keyboard_flags(&mut self) {
        self.kitty_keyboard_modes = KittyKeyboardModes::default();
    }

    pub(crate) fn set_allow_win32_input_mode(&mut self, allowed: bool) {
        self.allow_win32_input_mode = allowed;
        if !allowed {
            self.tracked_modes
                .set(TrackedTerminalModes::WIN32_INPUT_MODE, false);
        }
    }

    fn find_next_mode_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        [
            Self::find_csi_private_mode_start(bytes),
            Self::find_key_modifier_options_start(bytes),
            Self::find_kitty_keyboard_start(bytes),
            Self::find_soft_reset_start(bytes),
            Self::find_csi_mode_start(bytes),
            Self::find_simple_escape_start(bytes),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|start| start.index)
    }

    fn find_csi_private_mode_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
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
                    Self::UTF8_C1_CSI_PRIVATE_MODE_PREFIX,
                    ModeSequence::CsiPrivateMode {
                        prefix_len: Self::UTF8_C1_CSI_PRIVATE_MODE_PREFIX.len(),
                    },
                ),
            ],
        )
    }

    fn find_key_modifier_options_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
            [
                (
                    Self::CSI_KITTY_KEYBOARD_PUSH_PREFIX,
                    ModeSequence::KeyModifierOptions {
                        prefix_len: Self::CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
                    },
                ),
                (
                    Self::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
                    ModeSequence::KeyModifierOptions {
                        prefix_len: Self::C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
                    },
                ),
                (
                    Self::UTF8_C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
                    ModeSequence::KeyModifierOptions {
                        prefix_len: Self::UTF8_C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX.len(),
                    },
                ),
            ],
        )
    }

    fn find_kitty_keyboard_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
            [
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
                    Self::UTF8_C1_CSI_KITTY_KEYBOARD_POP_PREFIX,
                    ModeSequence::KittyKeyboard {
                        prefix_len: Self::UTF8_C1_CSI_KITTY_KEYBOARD_POP_PREFIX.len(),
                        operation: KittyKeyboardOperation::Pop,
                    },
                ),
                (
                    Self::UTF8_C1_CSI_KITTY_KEYBOARD_SET_PREFIX,
                    ModeSequence::KittyKeyboard {
                        prefix_len: Self::UTF8_C1_CSI_KITTY_KEYBOARD_SET_PREFIX.len(),
                        operation: KittyKeyboardOperation::Apply,
                    },
                ),
            ],
        )
    }

    fn find_soft_reset_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
            [
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
                    Self::UTF8_C1_SOFT_RESET_PREFIX,
                    ModeSequence::SoftReset {
                        prefix_len: Self::UTF8_C1_SOFT_RESET_PREFIX.len(),
                    },
                ),
            ],
        )
    }

    fn find_csi_mode_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
            [
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
                    Self::UTF8_C1_CSI_MODE_PREFIX,
                    ModeSequence::CsiMode {
                        prefix_len: Self::UTF8_C1_CSI_MODE_PREFIX.len(),
                    },
                ),
            ],
        )
    }

    fn find_simple_escape_start(bytes: &[u8]) -> Option<ModeSequenceStart> {
        Self::find_mode_start_with_prefixes(
            bytes,
            [
                (
                    Self::APPLICATION_KEYPAD_PREFIX,
                    ModeSequence::ApplicationKeypad(true),
                ),
                (
                    Self::NUMERIC_KEYPAD_PREFIX,
                    ModeSequence::ApplicationKeypad(false),
                ),
                (Self::RESET_PREFIX, ModeSequence::Reset),
            ],
        )
    }

    fn find_mode_start_with_prefixes<const N: usize>(
        bytes: &[u8],
        prefixes: [(&'static [u8], ModeSequence); N],
    ) -> Option<ModeSequenceStart> {
        prefixes
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
        if let Some(tracked_mode) = TrackedTerminalModes::private_mode_bit(mode) {
            self.tracked_modes.set(tracked_mode, enabled);
            return;
        }

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
            9001 => {
                if self.allow_win32_input_mode
                    && self
                        .tracked_modes
                        .set(TrackedTerminalModes::WIN32_INPUT_MODE, enabled)
                {
                    emit(TerminalModeChange::Win32InputMode(enabled));
                }
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
        match mode {
            4 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::INSERT_MODE, enabled);
            }
            8 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::BIDIRECTIONAL_SUPPORT, enabled);
            }
            20 => {
                self.tracked_modes
                    .set(TrackedTerminalModes::AUTOMATIC_NEWLINE, enabled);
            }
            _ => {}
        }
    }

    fn soft_reset(&mut self, emit: &mut impl FnMut(TerminalModeChange)) {
        if self
            .tracked_modes
            .set(TrackedTerminalModes::APPLICATION_CURSOR_KEYS, false)
        {
            emit(TerminalModeChange::ApplicationCursorKeys(false));
        }
        if self.modify_other_keys != 0 {
            self.modify_other_keys = 0;
            emit(TerminalModeChange::ModifyOtherKeys(0));
        }
        if self
            .tracked_modes
            .set(TrackedTerminalModes::APPLICATION_KEYPAD, false)
        {
            emit(TerminalModeChange::ApplicationKeypad(false));
        }
        self.tracked_modes
            .set(TrackedTerminalModes::ORIGIN_MODE, false);
        self.tracked_modes
            .set(TrackedTerminalModes::AUTO_WRAP, true);
        self.tracked_modes
            .set(TrackedTerminalModes::CURSOR_VISIBLE, true);
        self.tracked_modes
            .set(TrackedTerminalModes::INSERT_MODE, false);
        self.tracked_modes
            .set(TrackedTerminalModes::LEFT_RIGHT_MARGIN_MODE, false);
        self.tracked_modes
            .set(TrackedTerminalModes::BIDIRECTIONAL_SUPPORT, false);
        self.tracked_modes
            .set(TrackedTerminalModes::REVERSE_WRAP, false);
        self.tracked_modes
            .set(TrackedTerminalModes::SCREEN_REVERSE, false);
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
        let win32_input_mode = self.win32_input_mode();

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
        if win32_input_mode {
            emit(TerminalModeChange::Win32InputMode(false));
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

    pub(crate) fn win32_input_mode(&self) -> bool {
        self.tracked_modes
            .enabled(TrackedTerminalModes::WIN32_INPUT_MODE)
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
            2 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::DEC_ANSI_MODE),
            ),
            3 => mode_report_value(false),
            5 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::SCREEN_REVERSE),
            ),
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
            45 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::REVERSE_WRAP),
            ),
            69 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::LEFT_RIGHT_MARGIN_MODE),
            ),
            80 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::SIXEL_DISPLAY_MODE),
            ),
            8452 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::SIXEL_SCROLLS_RIGHT),
            ),
            1000 | 1002 | 1003 | 1005 | 1006 | 1015 | 1016 => {
                self.mouse_modes.report_value(mode).unwrap_or(0)
            }
            1004 => mode_report_value(self.focus_reporting()),
            1034 => mode_report_value(self.tracked_modes.enabled(TrackedTerminalModes::META_KEY)),
            1070 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::PRIVATE_COLOR_REGISTERS),
            ),
            9001 => mode_report_value(self.win32_input_mode()),
            2004 => mode_report_value(self.bracketed_paste()),
            2026 => mode_report_value(self.synchronized_output()),
            2027 => 3,
            _ => 0,
        }
    }

    pub(crate) fn ansi_mode_report_value(&self, mode: u16) -> u8 {
        match mode {
            4 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::INSERT_MODE),
            ),
            8 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::BIDIRECTIONAL_SUPPORT),
            ),
            20 => mode_report_value(
                self.tracked_modes
                    .enabled(TrackedTerminalModes::AUTOMATIC_NEWLINE),
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
            Self::UTF8_C1_CSI_MODE_PREFIX,
            Self::UTF8_C1_CSI_PRIVATE_MODE_PREFIX,
            Self::UTF8_C1_CSI_KITTY_KEYBOARD_PUSH_PREFIX,
            Self::UTF8_C1_CSI_KITTY_KEYBOARD_POP_PREFIX,
            Self::UTF8_C1_CSI_KITTY_KEYBOARD_SET_PREFIX,
            Self::APPLICATION_KEYPAD_PREFIX,
            Self::NUMERIC_KEYPAD_PREFIX,
            Self::RESET_PREFIX,
            Self::SOFT_RESET_PREFIX,
            Self::C1_SOFT_RESET_PREFIX,
            Self::UTF8_C1_SOFT_RESET_PREFIX,
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
    const SGR_PIXELS_PROTOCOL: u8 = 1 << 6;
    const PROTOCOL_MASK: u8 =
        Self::UTF8_PROTOCOL | Self::SGR_PROTOCOL | Self::URXVT_PROTOCOL | Self::SGR_PIXELS_PROTOCOL;

    fn set(&mut self, mode: u16, enabled: bool) -> bool {
        let Some(mask) = Self::mask(mode) else {
            return false;
        };

        if Self::is_protocol_mode(mode) {
            self.0 &= !Self::PROTOCOL_MASK;
            if enabled {
                self.0 |= mask;
            }
        } else if enabled {
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
        let protocol = if self.0 & Self::SGR_PIXELS_PROTOCOL != 0 {
            MouseProtocolMode::SgrPixels
        } else if self.0 & Self::SGR_PROTOCOL != 0 {
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
            1016 => Some(Self::SGR_PIXELS_PROTOCOL),
            _ => None,
        }
    }

    const fn is_protocol_mode(mode: u16) -> bool {
        matches!(mode, 1005 | 1006 | 1015 | 1016)
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
    const SIXEL_DISPLAY_MODE: u32 = 1 << 16;
    const SIXEL_SCROLLS_RIGHT: u32 = 1 << 17;
    const REVERSE_WRAP: u32 = 1 << 18;
    const SCREEN_REVERSE: u32 = 1 << 19;
    const WIN32_INPUT_MODE: u32 = 1 << 20;
    const AUTOMATIC_NEWLINE: u32 = 1 << 21;
    const BIDIRECTIONAL_SUPPORT: u32 = 1 << 22;
    const PRIVATE_COLOR_REGISTERS: u32 = 1 << 23;
    const DEC_ANSI_MODE: u32 = 1 << 24;

    fn private_mode_bit(mode: u16) -> Option<u32> {
        match mode {
            2 => Some(Self::DEC_ANSI_MODE),
            5 => Some(Self::SCREEN_REVERSE),
            6 => Some(Self::ORIGIN_MODE),
            7 => Some(Self::AUTO_WRAP),
            12 => Some(Self::CURSOR_BLINKING),
            25 => Some(Self::CURSOR_VISIBLE),
            45 => Some(Self::REVERSE_WRAP),
            47 => Some(Self::ALTERNATE_SCREEN_47),
            69 => Some(Self::LEFT_RIGHT_MARGIN_MODE),
            80 => Some(Self::SIXEL_DISPLAY_MODE),
            1034 => Some(Self::META_KEY),
            1047 => Some(Self::ALTERNATE_SCREEN_1047),
            1048 => Some(Self::PRIVATE_CURSOR_SAVE),
            1049 => Some(Self::ALTERNATE_SCREEN_1049),
            1070 => Some(Self::PRIVATE_COLOR_REGISTERS),
            8452 => Some(Self::SIXEL_SCROLLS_RIGHT),
            _ => None,
        }
    }

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
        Self(Self::AUTO_WRAP | Self::CURSOR_VISIBLE)
    }
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
        .find(|&length| {
            let suffix_start = haystack.len() - length;
            haystack[suffix_start..] == needle[..length]
                && !raw_c1_prefix_is_utf8_continuation(haystack, suffix_start, &needle[..length])
        })
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
    fn tracks_reverse_wraparound_private_mode_status() {
        let mut tracker = TerminalModeTracker::default();

        assert_eq!(tracker.private_mode_report_value(45), 2);

        tracker.process(b"\x1b[?45h", |_| {});
        assert_eq!(tracker.private_mode_report_value(45), 1);

        tracker.process(b"\x9b?45l", |_| {});
        assert_eq!(tracker.private_mode_report_value(45), 2);
    }

    #[test]
    fn tracks_screen_reverse_private_mode_status() {
        let mut tracker = TerminalModeTracker::default();

        assert_eq!(tracker.private_mode_report_value(5), 2);

        tracker.process(b"\x1b[?5h", |_| {});
        assert_eq!(tracker.private_mode_report_value(5), 1);

        tracker.process(b"\x9b?5l", |_| {});
        assert_eq!(tracker.private_mode_report_value(5), 2);
    }

    #[test]
    fn tracks_wezterm_private_mode_reports() {
        let mut tracker = TerminalModeTracker::default();

        assert_eq!(tracker.private_mode_report_value(3), 2);
        assert_eq!(tracker.private_mode_report_value(2027), 3);
        assert_eq!(tracker.private_mode_report_value(1070), 2);

        tracker.process(b"\x1b[?2027l\x1b[?1070h", |_| {});
        assert_eq!(tracker.private_mode_report_value(2027), 3);
        assert_eq!(tracker.private_mode_report_value(1070), 1);

        tracker.process(b"\x9b?1070l", |_| {});
        assert_eq!(tracker.private_mode_report_value(1070), 2);
    }

    #[test]
    fn tracks_dec_ansi_private_mode_status() {
        let mut tracker = TerminalModeTracker::default();

        assert_eq!(tracker.private_mode_report_value(2), 2);

        tracker.process(b"\x1b[?2h", |_| {});
        assert_eq!(tracker.private_mode_report_value(2), 1);

        tracker.process(b"\x9b?2l", |_| {});
        assert_eq!(tracker.private_mode_report_value(2), 2);
    }

    #[test]
    fn tracks_utf8_c1_private_mode_status() {
        let mut tracker = TerminalModeTracker::default();
        let csi = '\u{9b}';

        assert_eq!(tracker.private_mode_report_value(45), 2);

        tracker.process(format!("{csi}?45h").as_bytes(), |_| {});
        assert_eq!(tracker.private_mode_report_value(45), 1);

        tracker.process(format!("{csi}?45l").as_bytes(), |_| {});
        assert_eq!(tracker.private_mode_report_value(45), 2);
    }

    #[test]
    fn soft_reset_restores_cursor_visibility_mode_status() {
        let mut tracker = TerminalModeTracker::default();

        tracker.process(b"\x1b[?25l", |_| {});
        assert_eq!(tracker.private_mode_report_value(25), 2);

        tracker.process(b"\x1b[!p", |_| {});

        assert_eq!(tracker.private_mode_report_value(25), 1);
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

    #[test]
    fn tracks_win32_input_private_mode() {
        let mut tracker = TerminalModeTracker::default();
        let mut changes = Vec::new();

        assert!(!tracker.win32_input_mode());

        tracker.process(b"\x1b[?9001h", |change| changes.push(change));
        assert!(tracker.win32_input_mode());

        tracker.process(b"\x9b?9001l", |change| changes.push(change));
        assert!(!tracker.win32_input_mode());

        assert_eq!(
            changes,
            vec![
                TerminalModeChange::Win32InputMode(true),
                TerminalModeChange::Win32InputMode(false)
            ]
        );
    }
}
