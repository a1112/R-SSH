use rterm_types::{DamageRegion, SessionId, TerminalSize};
use std::collections::HashSet;

#[test]
fn terminal_value_types_preserve_the_stage_zero_contract() {
    let mut sessions = HashSet::new();
    sessions.insert(SessionId::new(42));
    assert!(sessions.contains(&SessionId::new(42)));
    assert_eq!(SessionId::new(42).get(), 42);
    assert_eq!(TerminalSize::new(120, 30).cells(), 3_600);
    assert!(DamageRegion::new(0, 0, 0, 1).is_empty());
    assert_eq!(DamageRegion::new(u16::MAX, 0, 2, 1).right(), u16::MAX);
}
