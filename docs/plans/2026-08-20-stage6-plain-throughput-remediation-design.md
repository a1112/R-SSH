# Stage 6 Plain-Throughput Remediation Design

## Status and scope

The Stage 6 release contract is merged, but the protected Windows release run
cannot complete because the production plain-scroll benchmark records about
4.59 MiB/s against the existing 5 MiB/s absolute floor. The candidate is not
slower than the immutable same-machine last-known-good revision, so this is not
a Stage 6 regression. It is nevertheless a release blocker: the absolute gate
is part of the approved production contract and must not be weakened.

This remediation changes only terminal feed processing. It does not change the
benchmark workload, thresholds, runner configuration, terminal protocol,
snapshot format, public API, or the authorization boundary for the later
physical repository split.

## Evidence and root cause

The plain workload is emitted in 8 KiB chunks and contains printable ASCII plus
CR/LF. `Terminal::feed_at_current_seqno` currently copies every chunk into a
UTF-8 scratch buffer, decodes the complete prefix into a second `Vec<char>`,
then visits those characters again to apply printable and C0 behavior. The
decode staging is required for arbitrary Unicode and split control sequences,
but it is redundant for the dominant plain-scroll input.

Same-machine measurements exclude a scheduler-only explanation: high-priority
P-core runs remain below the absolute floor. Candidate and last-known-good
revisions are close (approximately 4.59 versus 4.56 MiB/s), which localizes the
gap to established terminal processing rather than the Stage 6 release
contract or GUI hybrid renderer.

## Chosen design

Add a deliberately narrow direct-ASCII path before the existing decode path.
It is eligible only when all of the following are true:

- no incomplete UTF-8 bytes are pending;
- no incomplete terminal control sequence is pending;
- NFC normalization is disabled;
- every input byte is ASCII; and
- the chunk contains no ESC byte.

The direct path consumes printable ASCII and ordinary C0 controls using the
same terminal state transitions as the decoded path, without materializing the
intermediate byte and character vectors. Printable runs remain semantically
ordered with controls. If any eligibility condition is false, the entire chunk
uses the existing UTF-8/control parser.

ESC is intentionally excluded even though it is ASCII. Keeping escape, CSI,
OSC, DCS, APC, character-set selection, and split-sequence handling on the
existing path avoids a second control parser and makes fallback behavior easy
to audit. Unicode and NFC likewise remain unchanged.

## Alternatives considered

- Lowering the 5 MiB/s threshold was rejected because it would weaken the
  approved release contract and conceal a production bottleneck.
- Changing the benchmark record size or chunking was rejected because it would
  invalidate the fixed-runner baseline rather than improve the product.
- Rewriting all parser states around byte slices was rejected as too broad for
  a release-blocking remediation.
- Adding parallel parsing was rejected because terminal mutations are ordered
  and the coordination cost would add risk without addressing the redundant
  allocation directly.

## Correctness and performance contracts

Tests compare the direct path with the legacy decoded path across printable
text, C0 controls, wrapping, scrollback, chunk boundaries, alternate terminal
sizes, and representative randomized ASCII streams. Separate eligibility tests
prove that ESC, Unicode, pending UTF-8, pending control state, and NFC always
fall back. Existing frozen parser traces and workspace tests remain required.

The fixed Windows machine remains authoritative. The remediation is acceptable
only if the unchanged plain-scroll gate is at least 5,242,880 bytes/s and the
query, chunk latency, render latency, idle CPU, RSS, startup, and Private Bytes
contracts do not regress. The candidate-versus-LKG 5+40 comparison and package
smoke must pass before merge.

## Rollback

The change is one parser optimization commit behind a stable eligibility
predicate. Reverting that commit restores the prior decoded path without data
migration or configuration changes. The immutable Stage 6 LKG remains
`0e8ebd5de22758275cbb6a849c19c032268d7fac`.
