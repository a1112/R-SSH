#[derive(Default)]
pub(crate) struct TerminalVisibleOutputFilter {
    state: VisibleOutputState,
}

#[derive(Default)]
enum VisibleOutputState {
    #[default]
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
    StString,
    StStringEscape,
}

impl TerminalVisibleOutputFilter {
    pub(crate) fn process(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut visible = Vec::new();

        for byte in bytes {
            self.process_byte(*byte, &mut visible);
        }

        visible
    }

    fn process_byte(&mut self, byte: u8, visible: &mut Vec<u8>) {
        match self.state {
            VisibleOutputState::Ground => self.process_ground_byte(byte, visible),
            VisibleOutputState::Escape => self.process_escape_byte(byte),
            VisibleOutputState::Csi => self.process_csi_byte(byte),
            VisibleOutputState::Osc => self.process_osc_byte(byte),
            VisibleOutputState::OscEscape => self.process_osc_escape_byte(byte),
            VisibleOutputState::StString => self.process_st_string_byte(byte),
            VisibleOutputState::StStringEscape => self.process_st_string_escape_byte(byte),
        }
    }

    fn process_ground_byte(&mut self, byte: u8, visible: &mut Vec<u8>) {
        match byte {
            b'\x07' | b'\x00' | b'\x18' | b'\x1a' => {}
            b'\x1b' => self.state = VisibleOutputState::Escape,
            0x90 | 0x98 | 0x9e | 0x9f => self.state = VisibleOutputState::StString,
            0x9b => self.state = VisibleOutputState::Csi,
            0x9d => self.state = VisibleOutputState::Osc,
            _ => visible.push(byte),
        }
    }

    fn process_escape_byte(&mut self, byte: u8) {
        self.state = match byte {
            b'[' => VisibleOutputState::Csi,
            b']' => VisibleOutputState::Osc,
            b'P' | b'X' | b'^' | b'_' => VisibleOutputState::StString,
            _ => VisibleOutputState::Ground,
        };
    }

    fn process_csi_byte(&mut self, byte: u8) {
        if byte == b'\x18' || byte == b'\x1a' || (0x40..=0x7e).contains(&byte) {
            self.state = VisibleOutputState::Ground;
        }
    }

    fn process_osc_byte(&mut self, byte: u8) {
        match byte {
            b'\x07' | 0x9c | b'\x18' | b'\x1a' => self.state = VisibleOutputState::Ground,
            b'\x1b' => self.state = VisibleOutputState::OscEscape,
            _ => {}
        }
    }

    fn process_osc_escape_byte(&mut self, byte: u8) {
        self.state = if byte == b'\\' {
            VisibleOutputState::Ground
        } else {
            VisibleOutputState::Osc
        };
    }

    fn process_st_string_byte(&mut self, byte: u8) {
        match byte {
            0x9c | b'\x18' | b'\x1a' => self.state = VisibleOutputState::Ground,
            b'\x1b' => self.state = VisibleOutputState::StStringEscape,
            _ => {}
        }
    }

    fn process_st_string_escape_byte(&mut self, byte: u8) {
        self.state = if byte == b'\\' {
            VisibleOutputState::Ground
        } else {
            VisibleOutputState::StString
        };
    }
}
