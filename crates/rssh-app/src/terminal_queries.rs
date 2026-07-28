#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlFamily {
    Csi,
    Osc,
    Dcs,
    Enq,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScannedSegment {
    Bytes(Vec<u8>),
    Control {
        family: ControlFamily,
        bytes: Vec<u8>,
    },
}

impl ScannedSegment {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) | Self::Control { bytes, .. } => bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ScanState {
    #[default]
    Ground,
    Escape,
    Utf8C1,
    Utf8Text(u8),
    Csi,
    Osc,
    OscEscape,
    OscUtf8C1,
    Dcs,
    DcsEscape,
    DcsUtf8C1,
    String,
    StringEscape,
    StringUtf8C1,
}

#[derive(Default)]
pub(crate) struct TerminalQueryScanner {
    pending: Vec<u8>,
    cursor: usize,
    candidate_start: Option<usize>,
    state: ScanState,
    inspected_bytes: u64,
    record_work: bool,
}

impl TerminalQueryScanner {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn new_with_work_counter() -> Self {
        Self {
            record_work: true,
            ..Self::default()
        }
    }

    #[must_use]
    pub(crate) const fn inspected_bytes(&self) -> u64 {
        self.inspected_bytes
    }

    pub(crate) fn discard_incomplete(&mut self) {
        self.pending.clear();
        self.cursor = 0;
        self.candidate_start = None;
        self.state = ScanState::Ground;
    }

    pub(crate) fn process(&mut self, bytes: &[u8]) -> Vec<ScannedSegment> {
        if self.record_work {
            self.process_inner::<true>(bytes)
        } else {
            self.process_inner::<false>(bytes)
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the streaming state transition table is clearest as one exhaustive match"
    )]
    fn process_inner<const RECORD_WORK: bool>(&mut self, bytes: &[u8]) -> Vec<ScannedSegment> {
        self.pending.extend_from_slice(bytes);
        let mut segments = Vec::new();
        let mut emitted_end = 0;

        while self.cursor < self.pending.len() {
            let byte = self.pending[self.cursor];
            if RECORD_WORK {
                self.inspected_bytes = self.inspected_bytes.saturating_add(1);
            }

            match self.state {
                ScanState::Ground => match byte {
                    0x05 => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        let end = self.cursor + 1;
                        Self::push_control(
                            &self.pending,
                            &mut segments,
                            self.cursor,
                            end,
                            ControlFamily::Enq,
                        );
                        emitted_end = end;
                        self.cursor = end;
                    }
                    0x1b => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::Escape;
                        self.cursor += 1;
                    }
                    0x9b => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::Csi;
                        self.cursor += 1;
                    }
                    0x9d => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::Osc;
                        self.cursor += 1;
                    }
                    0x90 => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::Dcs;
                        self.cursor += 1;
                    }
                    0x98 | 0x9e | 0x9f => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::String;
                        self.cursor += 1;
                    }
                    0x9c => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        let end = self.cursor + 1;
                        Self::push_control(
                            &self.pending,
                            &mut segments,
                            self.cursor,
                            end,
                            ControlFamily::Other,
                        );
                        emitted_end = end;
                        self.cursor = end;
                    }
                    0xc2 => {
                        Self::push_bytes(&self.pending, &mut segments, emitted_end, self.cursor);
                        self.candidate_start = Some(self.cursor);
                        self.state = ScanState::Utf8C1;
                        self.cursor += 1;
                    }
                    0xc3..=0xdf => {
                        self.state = ScanState::Utf8Text(1);
                        self.cursor += 1;
                    }
                    0xe0..=0xef => {
                        self.state = ScanState::Utf8Text(2);
                        self.cursor += 1;
                    }
                    0xf0..=0xf7 => {
                        self.state = ScanState::Utf8Text(3);
                        self.cursor += 1;
                    }
                    _ => self.cursor += 1,
                },
                ScanState::Escape => {
                    self.cursor += 1;
                    match byte {
                        b'[' => self.state = ScanState::Csi,
                        b']' => self.state = ScanState::Osc,
                        b'P' => self.state = ScanState::Dcs,
                        b'X' | b'^' | b'_' => self.state = ScanState::String,
                        _ => {
                            emitted_end = self.finish_control(
                                &mut segments,
                                self.cursor,
                                ControlFamily::Other,
                            );
                        }
                    }
                }
                ScanState::Utf8C1 => {
                    self.cursor += 1;
                    match byte {
                        0x9b => self.state = ScanState::Csi,
                        0x9d => self.state = ScanState::Osc,
                        0x90 => self.state = ScanState::Dcs,
                        0x98 | 0x9e | 0x9f => self.state = ScanState::String,
                        0x9c => {
                            emitted_end = self.finish_control(
                                &mut segments,
                                self.cursor,
                                ControlFamily::Other,
                            );
                        }
                        _ => {
                            self.state = ScanState::Ground;
                            self.candidate_start = None;
                        }
                    }
                }
                ScanState::Utf8Text(remaining) => {
                    self.cursor += 1;
                    self.state = if remaining == 1 {
                        ScanState::Ground
                    } else {
                        ScanState::Utf8Text(remaining - 1)
                    };
                }
                ScanState::Csi => {
                    self.cursor += 1;
                    if (0x40..=0x7e).contains(&byte) {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Csi);
                    }
                }
                ScanState::Osc => {
                    self.cursor += 1;
                    match byte {
                        0x07 | 0x9c => {
                            emitted_end =
                                self.finish_control(&mut segments, self.cursor, ControlFamily::Osc);
                        }
                        0x1b => self.state = ScanState::OscEscape,
                        0xc2 => self.state = ScanState::OscUtf8C1,
                        _ => {}
                    }
                }
                ScanState::OscEscape => {
                    self.cursor += 1;
                    if byte == b'\\' {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Osc);
                    } else {
                        self.state = ScanState::Osc;
                    }
                }
                ScanState::OscUtf8C1 => {
                    self.cursor += 1;
                    if byte == 0x9c {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Osc);
                    } else {
                        self.state = ScanState::Osc;
                    }
                }
                ScanState::Dcs => {
                    self.cursor += 1;
                    match byte {
                        0x9c => {
                            emitted_end =
                                self.finish_control(&mut segments, self.cursor, ControlFamily::Dcs);
                        }
                        0x1b => self.state = ScanState::DcsEscape,
                        0xc2 => self.state = ScanState::DcsUtf8C1,
                        _ => {}
                    }
                }
                ScanState::DcsEscape => {
                    self.cursor += 1;
                    if byte == b'\\' {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Dcs);
                    } else {
                        self.state = ScanState::Dcs;
                    }
                }
                ScanState::DcsUtf8C1 => {
                    self.cursor += 1;
                    if byte == 0x9c {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Dcs);
                    } else {
                        self.state = ScanState::Dcs;
                    }
                }
                ScanState::String => {
                    self.cursor += 1;
                    match byte {
                        0x9c => {
                            emitted_end = self.finish_control(
                                &mut segments,
                                self.cursor,
                                ControlFamily::Other,
                            );
                        }
                        0x1b => self.state = ScanState::StringEscape,
                        0xc2 => self.state = ScanState::StringUtf8C1,
                        _ => {}
                    }
                }
                ScanState::StringEscape => {
                    self.cursor += 1;
                    if byte == b'\\' {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Other);
                    } else {
                        self.state = ScanState::String;
                    }
                }
                ScanState::StringUtf8C1 => {
                    self.cursor += 1;
                    if byte == 0x9c {
                        emitted_end =
                            self.finish_control(&mut segments, self.cursor, ControlFamily::Other);
                    } else {
                        self.state = ScanState::String;
                    }
                }
            }
        }

        if self.state == ScanState::Ground {
            Self::push_bytes(
                &self.pending,
                &mut segments,
                emitted_end,
                self.pending.len(),
            );
            emitted_end = self.pending.len();
        }

        if emitted_end > 0 {
            self.pending.drain(..emitted_end);
            self.cursor = self.cursor.saturating_sub(emitted_end);
            self.candidate_start = self
                .candidate_start
                .map(|start| start.saturating_sub(emitted_end));
        }

        segments
    }

    fn finish_control(
        &mut self,
        segments: &mut Vec<ScannedSegment>,
        end: usize,
        family: ControlFamily,
    ) -> usize {
        let start = self
            .candidate_start
            .take()
            .expect("control sequence must have a start");
        Self::push_control(&self.pending, segments, start, end, family);
        self.state = ScanState::Ground;
        end
    }

    fn push_bytes(pending: &[u8], segments: &mut Vec<ScannedSegment>, start: usize, end: usize) {
        if start < end {
            segments.push(ScannedSegment::Bytes(pending[start..end].to_vec()));
        }
    }

    fn push_control(
        pending: &[u8],
        segments: &mut Vec<ScannedSegment>,
        start: usize,
        end: usize,
        family: ControlFamily,
    ) {
        segments.push(ScannedSegment::Control {
            family,
            bytes: pending[start..end].to_vec(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlFamily, ScannedSegment, TerminalQueryScanner};

    fn scan_in_chunks(input: &[u8], chunk_size: usize) -> (Vec<ScannedSegment>, u64) {
        let mut scanner = TerminalQueryScanner::new_with_work_counter();
        let mut segments = Vec::new();
        for chunk in input.chunks(chunk_size) {
            segments.extend(scanner.process(chunk));
        }
        (segments, scanner.inspected_bytes())
    }

    fn flattened(segments: &[ScannedSegment]) -> Vec<u8> {
        segments
            .iter()
            .flat_map(|segment| segment.bytes().iter().copied())
            .collect()
    }

    fn controls(segments: &[ScannedSegment]) -> Vec<(ControlFamily, Vec<u8>)> {
        segments
            .iter()
            .filter_map(|segment| match segment {
                ScannedSegment::Control { family, bytes } => Some((*family, bytes.clone())),
                ScannedSegment::Bytes(_) => None,
            })
            .collect()
    }

    #[test]
    fn terminal_queries_frames_every_existing_fixed_and_dynamic_family() {
        let cases: &[(&[u8], ControlFamily)] = &[
            (b"\x1b[6n", ControlFamily::Csi),
            (b"\x1b[c", ControlFamily::Csi),
            (b"\x1b[>c", ControlFamily::Csi),
            (b"\x1b[=c", ControlFamily::Csi),
            (b"\x1b[x", ControlFamily::Csi),
            (b"\x1b[>q", ControlFamily::Csi),
            (b"\x1b[5n", ControlFamily::Csi),
            (b"\x1b[14t", ControlFamily::Csi),
            (b"\x1b[16t", ControlFamily::Csi),
            (b"\x1b[18t", ControlFamily::Csi),
            (b"\x1b[?25$p", ControlFamily::Csi),
            (b"\x1b[4$p", ControlFamily::Csi),
            (b"\x1b[?2026h", ControlFamily::Csi),
            (b"\x1b[?u", ControlFamily::Csi),
            (b"\x1b[>4m", ControlFamily::Csi),
            (b"\x1b[1;1;1;1;1;1*y", ControlFamily::Csi),
            (b"\x1b]4;1;?\x07", ControlFamily::Osc),
            (b"\x1b]1337;ReportCellSize\x1b\\", ControlFamily::Osc),
            (b"\x1b]52;c;?\x07", ControlFamily::Osc),
            (b"\x1b]8;;https://example.test\x1b\\", ControlFamily::Osc),
            (b"\x1bP$qm\x1b\\", ControlFamily::Dcs),
            (b"\x1bP+q544e\x1b\\", ControlFamily::Dcs),
            (b"\x1bP?q\x1b\\", ControlFamily::Dcs),
            (b"\x05", ControlFamily::Enq),
        ];

        for &(query, expected_family) in cases {
            let (segments, _) = scan_in_chunks(query, query.len());
            assert_eq!(flattened(&segments), query);
            assert_eq!(
                controls(&segments),
                vec![(expected_family, query.to_vec())],
                "query {query:?}"
            );
        }
    }

    #[test]
    fn terminal_queries_frames_all_48_fixed_query_forms() {
        let fixed: &[&[u8]] = &[
            b"\x1b[6n",
            b"\x9b6n",
            b"\xc2\x9b6n",
            b"\x1b[c",
            b"\x1b[0c",
            b"\x9bc",
            b"\xc2\x9bc",
            b"\x9b0c",
            b"\xc2\x9b0c",
            b"\x1b[>c",
            b"\x1b[>0c",
            b"\x9b>c",
            b"\x9b>0c",
            b"\xc2\x9b>c",
            b"\xc2\x9b>0c",
            b"\x1b[=c",
            b"\x1b[=0c",
            b"\x9b=c",
            b"\x9b=0c",
            b"\xc2\x9b=c",
            b"\xc2\x9b=0c",
            b"\x1b[x",
            b"\x1b[0x",
            b"\x1b[1x",
            b"\x9bx",
            b"\x9b0x",
            b"\x9b1x",
            b"\xc2\x9bx",
            b"\xc2\x9b0x",
            b"\xc2\x9b1x",
            b"\x1b[>q",
            b"\x1b[>0q",
            b"\x9b>q",
            b"\xc2\x9b>q",
            b"\x9b>0q",
            b"\xc2\x9b>0q",
            b"\x1b[5n",
            b"\x9b5n",
            b"\xc2\x9b5n",
            b"\x1b[14t",
            b"\x9b14t",
            b"\xc2\x9b14t",
            b"\x1b[16t",
            b"\x9b16t",
            b"\xc2\x9b16t",
            b"\x1b[18t",
            b"\x9b18t",
            b"\xc2\x9b18t",
        ];
        assert_eq!(fixed.len(), 48);
        for &query in fixed {
            let (segments, _) = scan_in_chunks(query, 1);
            assert_eq!(
                controls(&segments),
                vec![(ControlFamily::Csi, query.to_vec())],
                "query {query:?}"
            );
        }
    }

    #[test]
    fn terminal_queries_preserves_every_byte_boundary_split() {
        let queries: &[&[u8]] = &[
            b"\x1b[?25$p",
            b"\x1b]4;1;?\x1b\\",
            b"\x1bP+q544e\x1b\\",
            b"\xc2\x9b6n",
            b"\xc2\x9d4;1;?\xc2\x9c",
            b"\xc2\x90$qm\xc2\x9c",
        ];

        for &query in queries {
            for split in 1..query.len() {
                let mut scanner = TerminalQueryScanner::new_with_work_counter();
                let mut segments = scanner.process(&query[..split]);
                segments.extend(scanner.process(&query[split..]));
                assert_eq!(
                    flattened(&segments),
                    query,
                    "query {query:?}, split {split}"
                );
                assert_eq!(
                    controls(&segments).len(),
                    1,
                    "query {query:?}, split {split}"
                );
            }
        }
    }

    #[test]
    fn terminal_queries_frames_multiple_queries_in_one_chunk() {
        let input = b"before\x1b[6nbetween\x1b]4;1;?\x07after";
        let (segments, _) = scan_in_chunks(input, input.len());
        assert_eq!(flattened(&segments), input);
        assert_eq!(
            controls(&segments),
            vec![
                (ControlFamily::Csi, b"\x1b[6n".to_vec()),
                (ControlFamily::Osc, b"\x1b]4;1;?\x07".to_vec()),
            ]
        );
    }

    #[test]
    fn terminal_queries_finds_valid_query_after_unknown_same_family_control() {
        let input = b"\x1b[999z\x1b[6n\x1b]777;unknown\x07\x1b]4;1;?\x07";
        let (segments, _) = scan_in_chunks(input, input.len());
        assert_eq!(
            controls(&segments),
            vec![
                (ControlFamily::Csi, b"\x1b[999z".to_vec()),
                (ControlFamily::Csi, b"\x1b[6n".to_vec()),
                (ControlFamily::Osc, b"\x1b]777;unknown\x07".to_vec()),
                (ControlFamily::Osc, b"\x1b]4;1;?\x07".to_vec()),
            ]
        );
    }

    #[test]
    fn terminal_queries_keeps_embedded_queries_inside_control_strings() {
        let strings: &[&[u8]] = &[
            b"\x1b]777;\x1b[6n\x07",
            b"\x1bPpayload\x1b[6n\x1b\\",
            b"\x1bXpayload\x1b[6n\x1b\\",
            b"\x1b^payload\x1b[6n\x1b\\",
            b"\x1b_payload\x1b[6n\x1b\\",
            b"\x98payload\x1b[6n\x9c",
            b"\x9epayload\x1b[6n\x9c",
            b"\x9fpayload\x1b[6n\x9c",
        ];
        for &control_string in strings {
            let (segments, _) = scan_in_chunks(control_string, 1);
            let framed = controls(&segments);
            assert_eq!(flattened(&segments), control_string);
            assert_eq!(
                framed.len(),
                1,
                "embedded query escaped its control string {control_string:?}"
            );
            assert_ne!(framed[0].0, ControlFamily::Csi);
        }
    }

    #[test]
    fn terminal_queries_does_not_treat_utf8_continuations_as_raw_c1() {
        let input = b"\xc3\x9b6n\xc3\x9d4;1;?\xc3\x90$qm\xc3\x9c";
        let (segments, _) = scan_in_chunks(input, 1);
        assert_eq!(flattened(&segments), input);
        assert!(controls(&segments).is_empty());
    }

    #[test]
    fn terminal_queries_releases_c2_when_the_next_byte_is_not_c1() {
        let input = b"before\xc2\xa9after";
        let (segments, _) = scan_in_chunks(input, 1);
        assert_eq!(flattened(&segments), input);
        assert!(controls(&segments).is_empty());
    }

    #[test]
    fn terminal_queries_frames_standalone_string_terminators() {
        for terminator in [b"\x1b\\".as_slice(), b"\x9c", b"\xc2\x9c"] {
            let (segments, _) = scan_in_chunks(terminator, 1);
            assert_eq!(
                controls(&segments),
                vec![(ControlFamily::Other, terminator.to_vec())]
            );
        }
    }

    #[test]
    fn terminal_queries_passes_unknown_csi_osc_and_dcs_without_loss() {
        let input = b"a\x1b[999zb\x1b]777;unknown\x07c\x1bP+zunknown\x1b\\d";
        let (segments, _) = scan_in_chunks(input, 3);
        assert_eq!(flattened(&segments), input);
        assert_eq!(controls(&segments).len(), 3);
    }

    #[test]
    fn terminal_queries_supports_raw_and_utf8_c1_forms() {
        let input = b"\x9b6n\xc2\x9b5n\x9d4;1;?\x9c\xc2\x90$qm\xc2\x9c";
        let (segments, _) = scan_in_chunks(input, 1);
        assert_eq!(flattened(&segments), input);
        assert_eq!(
            controls(&segments)
                .into_iter()
                .map(|(family, _)| family)
                .collect::<Vec<_>>(),
            vec![
                ControlFamily::Csi,
                ControlFamily::Csi,
                ControlFamily::Osc,
                ControlFamily::Dcs,
            ]
        );
    }

    #[test]
    fn terminal_queries_inspects_no_more_than_four_times_the_input() {
        let input = b"plain output ".repeat(4096);
        let (_, inspected) = scan_in_chunks(&input, 512);
        assert!(
            inspected <= (input.len() as u64).saturating_mul(4),
            "inspected {inspected} bytes for {} input bytes",
            input.len()
        );
    }

    #[test]
    fn terminal_queries_chunk_size_work_ratio_is_bounded() {
        let input = b"plain\x1b[6n\x1b]4;1;?\x07\x1bP$qm\x1b\\".repeat(4096);
        let (_, small_chunk_work) = scan_in_chunks(&input, 512);
        let (_, large_chunk_work) = scan_in_chunks(&input, 16 * 1024);
        let (larger, smaller) = if small_chunk_work >= large_chunk_work {
            (small_chunk_work, large_chunk_work)
        } else {
            (large_chunk_work, small_chunk_work)
        };
        assert!(
            u128::from(larger).saturating_mul(4) <= u128::from(smaller).saturating_mul(5),
            "512-byte work {small_chunk_work}, 16-KiB work {large_chunk_work}"
        );
    }

    #[test]
    fn terminal_queries_work_counter_is_disabled_by_default_and_saturates() {
        let mut normal = TerminalQueryScanner::new();
        let _ = normal.process(b"plain\x1b[6n");
        assert_eq!(normal.inspected_bytes(), 0);

        let mut measured = TerminalQueryScanner::new_with_work_counter();
        measured.inspected_bytes = u64::MAX - 1;
        let _ = measured.process(b"abc");
        assert_eq!(measured.inspected_bytes(), u64::MAX);
    }
}
