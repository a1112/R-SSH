#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKey {
    Text(char),
    Control(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Delete,
    Insert,
    PageUp,
    PageDown,
}

pub fn encode_terminal_key(key: TerminalKey) -> Option<Vec<u8>> {
    match key {
        TerminalKey::Text(character) => {
            let mut bytes = Vec::new();
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
            Some(bytes)
        }
        TerminalKey::Control(character) => encode_control_char(character),
        TerminalKey::Enter => Some(b"\r".to_vec()),
        TerminalKey::Backspace => Some(vec![0x7f]),
        TerminalKey::Tab => Some(b"\t".to_vec()),
        TerminalKey::Escape => Some(vec![0x1b]),
        TerminalKey::Left => Some(b"\x1b[D".to_vec()),
        TerminalKey::Right => Some(b"\x1b[C".to_vec()),
        TerminalKey::Up => Some(b"\x1b[A".to_vec()),
        TerminalKey::Down => Some(b"\x1b[B".to_vec()),
        TerminalKey::Home => Some(b"\x1b[H".to_vec()),
        TerminalKey::End => Some(b"\x1b[F".to_vec()),
        TerminalKey::Delete => Some(b"\x1b[3~".to_vec()),
        TerminalKey::Insert => Some(b"\x1b[2~".to_vec()),
        TerminalKey::PageUp => Some(b"\x1b[5~".to_vec()),
        TerminalKey::PageDown => Some(b"\x1b[6~".to_vec()),
    }
}

fn encode_control_char(character: char) -> Option<Vec<u8>> {
    let lower = character.to_ascii_lowercase();
    if !lower.is_ascii_lowercase() {
        return None;
    }

    Some(vec![lower as u8 - b'a' + 1])
}

#[cfg(test)]
mod tests {
    use super::{TerminalKey, encode_terminal_key};

    #[test]
    fn encodes_text_as_utf8() {
        assert_eq!(
            encode_terminal_key(TerminalKey::Text('中')).unwrap(),
            "中".as_bytes()
        );
    }

    #[test]
    fn encodes_control_c() {
        assert_eq!(
            encode_terminal_key(TerminalKey::Control('c')).unwrap(),
            vec![3]
        );
    }

    #[test]
    fn encodes_navigation_keys() {
        assert_eq!(encode_terminal_key(TerminalKey::Enter).unwrap(), b"\r");
        assert_eq!(encode_terminal_key(TerminalKey::Backspace).unwrap(), [0x7f]);
        assert_eq!(encode_terminal_key(TerminalKey::Up).unwrap(), b"\x1b[A");
        assert_eq!(
            encode_terminal_key(TerminalKey::Delete).unwrap(),
            b"\x1b[3~"
        );
    }
}
