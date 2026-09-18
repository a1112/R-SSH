# Task 5 packaged functional runner checkpoint

Continue PR #29 from `91bbdb5137963f6e9235db12244dcb0d84e94f0c`. Its R-Term
candidate/rollback archive job passed; quality was still running at the initial
check. This checkpoint adds execution requirements, not a claim that the new
product scenarios have already passed.

## Integration

- Require the existing native-SSH reconnect, real-PTY ten-frame window and
  100%-scale GPU-text tests for both modern and frozen packages.
- Force the executable override to each verified package payload and require
  OpenSSH tools. Use the same production GUI/transfer features as the build.
- Run one exact scenario per command, explicitly enabling the ignored GPU
  scenario, and reject results other than one passed/zero failed/zero ignored.
- Retain command results and the tested executable hash; revalidate archive,
  payload, original executable and source identities after testing.
- Provision Xvfb, OpenSSH client and Vulkan dependencies in the existing Linux
  rollback job. No baseline, performance threshold or original consumer command
  is changed. The nine Task 12 drafts remain excluded.

## Test scope

The orchestration regressions use real temporary Git repositories, Cargo
builds, native package scripts and compiled Rust fixture tests that check the
package executable override and mandatory environment. They are deliberately
not GPU or SSH implementations, and cannot certify actual product behavior.
The package selection and zero-test regressions were observed failing before
implementation; the focused cases passed afterwards.
Review additionally required archive permission binding: a real tar with its
binary execute bits stripped first passed incorrectly, then failed after mode
validation was added. Unix payload/archive regular-file modes now match, and
binary/launcher execute bits are mandatory. The workflow test asserts the exact
`rterm-consumer-contract` job boundary rather than matching the whole workflow.

## Independent CI failure

While this change was in progress, push run 34923075757 for the previous commit
failed quality at `local_app_drains_output_after_fast_child_exit`: its separate
PowerShell/CIM owned-process probe hit a 10-second deadline at local_pty.rs:402.
The archive job passed. No Rust code in that test changed between 702a06b7 and
91bbdb51. An exact local rerun passed (20.70 seconds total test time), which is
not evidence that the CI failure is fixed. No timeout or assertion was relaxed;
the remaining CI/Task 6 investigation must retain this distinction.
The complete local `local_pty` integration target also passed all six tests
(16.24 seconds). This still does not reproduce or resolve the hosted timeout.

## Remaining acceptance

Inspect the real committed-SHA CI evidence for all three scenarios in both
profiles before closing this Task 5 acceptance slice. Hosted software GPU
results are not hardware performance evidence. Task 6 and the original
fixed-runner/extraction gates remain separate; no physical split is authorized
or performed by this integration.

## Follow-up: integration-test build isolation

For `5781429b`, both CI runs (34925524032 and 34925526601) rejected the
candidate at the final original-artifact identity check. The retained command
evidence shows all three candidate package scenarios passed. However, Cargo
implicitly rebuilt `rssh-app` for its integration tests; dev-dependency features
can change that executable even with identical explicit production features.
The rollback profile was not reached, so this is not dual-profile acceptance.

A real Cargo fixture with a feature-changing dev dependency reproduced the
same `artifact changed after its build command` failure. Package test builds
now use a separate `packaged-functional` target beneath the existing target,
with link/reparse and containment checks before and after directory creation.
The test target is recorded in the receipt. Tests still execute the attested
payload, and original executable, archive, payload and source checks are kept.
The regression executes both binaries and verifies that the packaged one has
production behavior while Cargo's implicit test binary has dev-feature behavior.

Functional run 34925526682 separately lost the text input in the nested
Wayland/foot host-terminal scenario: driver commands completed, but the fixture
recorded an empty line instead of the required marker. The retained evidence
does not establish why the text was lost. A failed-job-only rerun was requested;
no input assertion or timeout was relaxed, and the build-isolation change is
not a claimed fix for that independent failure.
The unchanged-SHA failed-job rerun subsequently passed, and Functional run
34925526682 completed successfully. This establishes a successful rerun, not
that the intermittent input-loss cause has been identified or eliminated.
