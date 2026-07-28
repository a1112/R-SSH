#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DcsTerminator {
    SevenBit,
    EightBit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecrqssKind {
    Sgr,
    CursorShape,
    ScrollRegion,
    ConformanceLevel,
    LeftRightMargins,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DecrqssRequest {
    pub(crate) kind: DecrqssKind,
    pub(crate) terminator: DcsTerminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XtGetTcapName {
    /// The name exactly as encoded in the request.
    pub(crate) encoded: Vec<u8>,
    /// The decoded name, or `None` when the request contains invalid ASCII hex.
    pub(crate) decoded: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XtGetTcapRequest {
    pub(crate) names: Vec<XtGetTcapName>,
    pub(crate) terminator: DcsTerminator,
}

pub(crate) fn parse_decrqss_request(bytes: &[u8]) -> Option<DecrqssRequest> {
    let (body, terminator) = parse_exact_dcs_frame(bytes)?;
    let content = body.strip_prefix(b"$q")?;
    let kind = match content {
        b"m" => DecrqssKind::Sgr,
        b" q" => DecrqssKind::CursorShape,
        b"r" => DecrqssKind::ScrollRegion,
        b"\"p" => DecrqssKind::ConformanceLevel,
        b"s" => DecrqssKind::LeftRightMargins,
        _ => DecrqssKind::Unknown,
    };
    Some(DecrqssRequest { kind, terminator })
}

pub(crate) fn parse_xtgettcap_request(bytes: &[u8]) -> Option<XtGetTcapRequest> {
    let (body, terminator) = parse_exact_dcs_frame(bytes)?;
    let content = body.strip_prefix(b"+q")?;
    let names = content
        .split(|byte| *byte == b';')
        .map(|encoded| XtGetTcapName {
            encoded: encoded.to_vec(),
            decoded: decode_ascii_hex(encoded),
        })
        .collect();
    Some(XtGetTcapRequest { names, terminator })
}

fn parse_exact_dcs_frame(bytes: &[u8]) -> Option<(&[u8], DcsTerminator)> {
    let body = bytes
        .strip_prefix(b"\x1bP")
        .or_else(|| bytes.strip_prefix(b"\x90"))
        .or_else(|| bytes.strip_prefix(b"\xc2\x90"))?;
    let (content, terminator) = if let Some(content) = body.strip_suffix(b"\x1b\\") {
        (content, DcsTerminator::SevenBit)
    } else if let Some(content) = body.strip_suffix(b"\xc2\x9c") {
        (content, DcsTerminator::EightBit)
    } else if let Some(content) = body.strip_suffix(b"\x9c") {
        (content, DcsTerminator::EightBit)
    } else {
        return None;
    };
    (!contains_st(content)).then_some((content, terminator))
}

fn contains_st(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|window| window == b"\x1b\\")
        || bytes.windows(2).any(|window| window == b"\xc2\x9c")
        || bytes.contains(&0x9c)
}

fn decode_ascii_hex(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            parse_hex_digit(pair[0])?
                .checked_mul(16)?
                .checked_add(parse_hex_digit(pair[1])?)
        })
        .collect()
}

const fn parse_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decrqss_kinds_from_all_dcs_and_st_encodings() {
        let cases: &[(&[u8], DecrqssKind, DcsTerminator)] = &[
            (b"\x1bP$qm\x1b\\", DecrqssKind::Sgr, DcsTerminator::SevenBit),
            (
                b"\x90$q q\x9c",
                DecrqssKind::CursorShape,
                DcsTerminator::EightBit,
            ),
            (
                b"\xc2\x90$qr\xc2\x9c",
                DecrqssKind::ScrollRegion,
                DcsTerminator::EightBit,
            ),
            (
                b"\x1bP$q\"p\x1b\\",
                DecrqssKind::ConformanceLevel,
                DcsTerminator::SevenBit,
            ),
            (
                b"\x1bP$qs\x1b\\",
                DecrqssKind::LeftRightMargins,
                DcsTerminator::SevenBit,
            ),
        ];

        for &(bytes, kind, terminator) in cases {
            assert_eq!(
                parse_decrqss_request(bytes),
                Some(DecrqssRequest { kind, terminator })
            );
        }
    }

    #[test]
    fn preserves_unknown_decrqss_request_for_failure_response() {
        assert_eq!(
            parse_decrqss_request(b"\x1bP$qwat\x1b\\"),
            Some(DecrqssRequest {
                kind: DecrqssKind::Unknown,
                terminator: DcsTerminator::SevenBit,
            })
        );
    }

    #[test]
    fn decrqss_parser_is_anchored_and_requires_exact_consumption() {
        for bytes in [
            b"x\x1bP$qm\x1b\\".as_slice(),
            b"\x1bP$qm\x1b\\x".as_slice(),
            b"\x1bP$qm".as_slice(),
            b"\x1bP+q6b75\x1b\\".as_slice(),
            b"\x1bP$qm\x07".as_slice(),
        ] {
            assert_eq!(parse_decrqss_request(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn parses_xtgettcap_hex_names_and_preserves_terminator() {
        assert_eq!(
            parse_xtgettcap_request(b"\x1bP+q544e;436f;6b6d6f7573\x1b\\"),
            Some(XtGetTcapRequest {
                names: vec![
                    XtGetTcapName {
                        encoded: b"544e".to_vec(),
                        decoded: Some(b"TN".to_vec()),
                    },
                    XtGetTcapName {
                        encoded: b"436f".to_vec(),
                        decoded: Some(b"Co".to_vec()),
                    },
                    XtGetTcapName {
                        encoded: b"6b6d6f7573".to_vec(),
                        decoded: Some(b"kmous".to_vec()),
                    },
                ],
                terminator: DcsTerminator::SevenBit,
            })
        );
        assert_eq!(
            parse_xtgettcap_request(b"\xc2\x90+q544e\xc2\x9c").map(|request| request.terminator),
            Some(DcsTerminator::EightBit)
        );
        assert_eq!(
            parse_xtgettcap_request(b"\x90+q544e\x9c").map(|request| request.terminator),
            Some(DcsTerminator::EightBit)
        );
    }

    #[test]
    fn xtgettcap_hex_is_checked_without_losing_invalid_names() {
        assert_eq!(
            parse_xtgettcap_request(b"\x1bP+q4;GG;ff\x1b\\"),
            Some(XtGetTcapRequest {
                names: vec![
                    XtGetTcapName {
                        encoded: b"4".to_vec(),
                        decoded: None,
                    },
                    XtGetTcapName {
                        encoded: b"GG".to_vec(),
                        decoded: None,
                    },
                    XtGetTcapName {
                        encoded: b"ff".to_vec(),
                        decoded: Some(vec![0xff]),
                    },
                ],
                terminator: DcsTerminator::SevenBit,
            })
        );
    }

    #[test]
    fn xtgettcap_parser_is_anchored_and_requires_exact_consumption() {
        for bytes in [
            b"x\x1bP+q544e\x1b\\".as_slice(),
            b"\x1bP+q544e\x1b\\x".as_slice(),
            b"\x1bP+q544e".as_slice(),
            b"\x1bP$q544e\x1b\\".as_slice(),
            b"\x1bP+q544e\x07".as_slice(),
        ] {
            assert_eq!(parse_xtgettcap_request(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn embedded_st_cannot_hide_trailing_data() {
        assert_eq!(
            parse_xtgettcap_request(b"\x1bP+q544e\x1b\\+q436f\x1b\\"),
            None
        );
        assert_eq!(parse_decrqss_request(b"\x1bP$qm\x9cignored"), None);
    }
}
