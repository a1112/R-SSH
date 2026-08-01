# Production Parity Verification

Date: 2026-08-02 (Asia/Shanghai)
Code baseline: `f02acff606d51fe779b94507c97c6776c22af9e5`
Documentation task: Task 26 of the [production-parity implementation plan](plans/2026-07-28-production-parity-implementation.md)

## Status vocabulary

This report uses four evidence states literally:

- **verified locally on Windows x64**: the command ran in this local session on
  Windows x64 and its result was inspected.
- **defined in hosted workflow but not run in this local session**: the workflow
  or contract exists in the repository, but this session produced no hosted run
  for the exact commit.
- **requires protected/self-hosted environment**: repository code alone cannot
  produce the certification evidence; protected credentials, reviewers, rules,
  hardware, or a fixed runner are required.
- **not yet evidenced**: neither this local session nor a linked external run
  supplies the required evidence.

These states deliberately do not imply six-platform certification, production
signing, notarization, attestation, protected performance certification, hardware
certification, or 100% WezTerm parity.

## Verification environment

| Field | Value |
| --- | --- |
| Operating system | Windows 11 Pro for Workstations Insider Preview, build `10.0.26300`, 64-bit |
| CPU | Intel Core i5-14600KF, 20 logical processors |
| Rust | `rustc 1.89.0 (29483883e 2025-08-04)` |
| Build concurrency | `CARGO_BUILD_JOBS=2` |
| Temporary storage | `E:\temp\rssh-task26` |
| Hard deadlines | `scripts/ci/process-harness.ps1`, Windows Job Object process-tree cleanup |

The code commits through `f02acff6` are local and were not pushed during this
session. Consequently, no GitHub Actions result can exist for this exact local
tip until it is pushed and a workflow runs.

## Platform and certification snapshot

| Scope | Evidence state | Evidence and remaining boundary |
| --- | --- | --- |
| Windows x64 application, ConPTY, native window, loopback SSH, and unsigned package | **verified locally on Windows x64** | Debug and release native-window tests passed on code baseline `83ade73a`. A package assembled while that code baseline was checked out passed fresh-extracted smoke, but its manifest records `source_commit=local`; the package is not exact-clean-checkout or exact-commit provenance. Loopback services are not an external production SSH server. |
| Windows ARM64 | **defined in hosted workflow but not run in this local session** | Six-target release matrix and package contract exist; no Windows ARM64 runtime result is linked. |
| Linux x64 and ARM64 | **defined in hosted workflow but not run in this local session** | Commit `74f78bab` guards pull-request and nightly native installs; `f02acff6` applies the same canonical guard to unsigned build/package smoke and protected signed-package smoke. YAML, shell, Rust contract, and cleanup semantics were verified locally; no exact-commit hosted Linux result is linked. |
| macOS x64 and ARM64 | **defined in hosted workflow but not run in this local session** | Native/package/notary contracts exist; no exact-commit macOS runtime result is linked. |
| Deterministic hosted performance job | **defined in hosted workflow but not run in this local session** | The workflow contains work-counter and chunk-ratio gates; this report does not substitute a hosted run. |
| Absolute fixed-machine performance | **requires protected/self-hosted environment** | No protected baseline fingerprint or successful exact-commit run is linked. Local numbers below are observations, not certification. |
| Authenticode, macOS signing/notary/staple, SBOM, provenance, and publication | **requires protected/self-hosted environment** | The protected release DAG is defined; no signed/notarized/attested artifact was produced in this session. |
| Real IME, GPU-vendor, RDP, DPI/Retina, and hardware matrix | **not yet evidenced** | No repository workflow and result set provides this certification. |
| General or 100% WezTerm parity | **not yet evidenced** | The target remains a bounded compatibility subset. Arbitrary Lua, mux/domain runtime, and broader protocol/configuration behavior remain outside this claim. |

## Requirement-to-evidence checklist

The commit column identifies the implementation boundary. A commit proves that
code exists; the test/job column determines whether behavior was actually
observed in this session.

| Requirement | Implementation commit | Test, metric, or job evidence | State | Remaining risk |
| --- | --- | --- | --- | --- |
| Rust 1.89 MSRV and locked workspace | `ef3cefb0a39051a04161e5c08272c029f0420806` | Rust 1.89 fmt, clippy, workspace tests, and release build | **verified locally on Windows x64** | Other toolchain/OS combinations need hosted runs. |
| Hard deadlines and process-tree cleanup | `40bd34eb326844daa764efe5bdb4006a59e46412`, hardened by `83ade73a9d11e165dc66e82e8f6ca1b910c2946c` | Exact-handle identity test, wrong-StartTime rejection, still-running deadline rejection, 20/20 harness stress, Task 24 debug/release | **verified locally on Windows x64** | Unix process-group implementation still needs hosted execution. |
| Debug GUI stack-overflow reproduction and fix | RED `2ed62144c39827939024a58f29205e3ab8be6c16`, GREEN `7f8f1398cda9902a85f83538b5c3345dc3e1d598` | Full workspace and native ten-frame tests complete without `overflowed its stack` | **verified locally on Windows x64** | Other window systems and drivers need hosted/hardware evidence. |
| Bounded ANSI query scanning | `4f106ad3dfe0bfd11d6c2334607553eff44a3762`, `4eb87f96cf3601ea662c906c378fc3278e4a8553` | Query benchmark inspected `1,499,241` bytes for a 1 MiB input; no threshold violations | **verified locally on Windows x64** | Absolute latency is not certified. |
| Scrollback/grid work proportionality | `59447293dc51da22d266f76083f3761d9bb6d295`, `acef9d82eda3c6bd66d268da9ef8501dbc53649d` | Both local 1 MiB workloads report `scrolled_survivor_cell_clones=0` and `history_row_relocations=0` | **verified locally on Windows x64** | Work counters do not prove GPU-present latency. |
| Hosted deterministic performance gate | `48b34f3186c153ca7ddf4eceb26e9d4964fc1b0b` | `.github/workflows/ci.yml` work-counter and chunk-ratio contract | **defined in hosted workflow but not run in this local session** | No exact-commit hosted result is linked. |
| Protected absolute performance gate | `62a0984c1904eb5373b972aa9d5ad56b44635882` | `.github/workflows/release.yml` fixed-runner baseline contract | **requires protected/self-hosted environment** | No protected runner fingerprint, baseline, or exact-commit result is linked. |
| Grapheme, font fixtures, shaping, and fallback foundation | `c3698e939d4be82b92e1ed74c771bfd4a5a409db`, `707dbb35bd6ae371a6eeaa75a23d230c3711784d`, `14708b7e0c67d4cc51700dea51609d86a54846c7`, `3a76a42d4a2aa5f3316b9f39778531a003bb856b`, `c7f64565dd25964a5213c79267b4d5cd4828a531` | Workspace tests and native multilingual text specimen assertions | **verified locally on Windows x64** | Real IME and broader font/configuration parity are **not yet evidenced**. |
| Shaping caches and renderer data flow | `05901951755f69d033f6e74871a92f9542f54abb`, `323ad0168f6919756d082ca3a6eff538aa992e8c` | Workspace tests and benchmark render proxy | **verified locally on Windows x64** | Cache behavior across long-lived multi-window workloads needs profiling. |
| Direct GPU terminal renderer and glyph atlas | `025c467ca0d811be12ce3f6bc8edb42061a967d7`, `0de48ea462be2dfd75296f122807bc4ea23c41e6`, `12c71872da09d8af002207e8bc50718a67189790`, `29afb927f7770242910a5fde6979ff7394e20f77` | Debug and release ten-frame native-window tests assert direct GPU text, zero compatibility uploads, and clean device state | **verified locally on Windows x64** | GPU vendor, software adapter, RDP, recovery, and input-to-present budgets are **not yet evidenced**. |
| Native SSH full-duplex session lifecycle | `dda474a5ae5bf0aade03f405fdb337b81f97d8c4` | Workspace and native loopback tests | **verified locally on Windows x64** | External server/vendor interoperability remains outside loopback evidence. |
| Task 20: SSH forwarding lifecycle | `2d70078532d557947daf862027b8db6fce291309` | Focused lifecycle tests exercise startup, cancellation, child drain, disconnect, and total deadlines | **verified locally on Windows x64** | Network failure modes on independent servers need external E2E. |
| Task 21: hermetic SSH fixtures | `afa3df11d142856b93b1af86bc01c64dc54634ef` | Native loopback fixture used by Task 22 and package smoke | **verified locally on Windows x64** | Hermetic fixture is not an independent OpenSSH server. |
| Task 22: native SSH, system OpenSSH, SFTP, and SCP | `93cc2ec267057684d88b0bd99bc1357900b04b4e` | Native loopback `8/8`; system OpenSSH `6/6`; transfer `3/3` | **verified locally on Windows x64** | The Linux-only isolated real-`sshd` test was not run locally. Commit `74f78babf962b161e732bdf651bae869d3d95a40` supplies its hosted dependency and service guard, but no hosted result is linked. |
| Linux hosted OpenSSH dependency and service guard | `74f78babf962b161e732bdf651bae869d3d95a40` | Both native jobs install client/server packages without starting the system service; static job-boundary, ordering, YAML, shell, cleanup-semantics, fmt, Clippy, and focused-test gates passed | **defined in hosted workflow but not run in this local session** | The required isolated real-`sshd` test still needs an exact-commit hosted Linux x64/ARM64 run. |
| Linux release OpenSSH service guard | `f02acff606d51fe779b94507c97c6776c22af9e5` | Unsigned build-package and protected signed-package-smoke jobs use the same CI guard; job-scoped and decoy-negative contracts, YAML/Bash, fmt, Clippy, and focused tests passed | **defined in hosted workflow but not run in this local session** | Unsigned and protected Linux package jobs still need exact-commit hosted execution. |
| Task 23: required platform PTY coverage | `3bb2dd3dd2f2b8595da996a290ced5ea2022e950` | Default one-group run completed 100 no-retry attempts in 19.56 s; p99/stage budgets and 3 s CIM survivor assertion passed | **verified locally on Windows x64** | Successful output does not expose the internal p99 value, so no number is invented here; Unix PTY needs hosted evidence. |
| Task 24: native terminal E2E matrix | `7efb6886289201087e78bb7f6a2945fea98a2c56` | Windows x64 debug passed in 18.54 s and release in 111.52 s, including native SSH, system OpenSSH, and ten-frame GPU/PTY linkage assertions | **verified locally on Windows x64** | Other five release targets are **defined in hosted workflow but not run in this local session**. PR native E2E covers three representative targets, not all six release artifacts. |
| Task 25: six native artifact contracts | `5744013ea3d6e329dc4b480819c11167e1e8a162` | Fresh Windows x64 unsigned ZIP extraction and package smoke while code HEAD was `83ade73a`; archive and binary hashes recorded below | **verified locally on Windows x64** | The manifest records `source_commit=local`, so this is not exact-clean-checkout or exact-commit provenance. Five targets and the protected publish chain have no runtime result in this session. |

## Local command results

All commands used Rust 1.89 where a Rust command was involved and ran with hard
outer deadlines for long-lived child processes.

| Gate | Result |
| --- | --- |
| `cargo +1.89.0 fmt --all -- --check` | Passed. |
| `cargo +1.89.0 clippy --locked --workspace --all-targets -- -D warnings` | Passed. |
| `cargo +1.89.0 test --locked --workspace --all-targets` | Passed at `83ade73a` in 118.16 s and again after `74f78bab` in 128.9 s; the main application target reported `4200/4200` in both runs. The later `f02acff6` release-contract slice passed its focused `7/7` suite plus workspace Clippy. |
| `cargo +1.89.0 build --locked --release -p rssh-app` | Passed in 6 min 59 s. |
| Process harness | TDD RED/GREEN confirmed; 20/20 consecutive self-tests passed, 2.647–3.119 s each. |
| Task 22 native loopback | `8/8` passed in 9.30 s. |
| Task 22 system OpenSSH | `6/6` passed in 2.59 s. |
| Task 22 SFTP/SCP | `3/3` passed in 2.10 s. |
| Task 23 default quick-exit | `1/1` test; 100 attempts, one group, zero retries, budget assertions passed in 19.56 s. |
| Task 24 native window | Debug and release profiles passed; each included native SSH, required system OpenSSH, version/backend checks, and real-PTY ten-frame presentation. |
| Task 25 package | Fresh-extracted unsigned Windows x64 package passed version, doctor, self-test, benchmark, launcher, packaged OpenSSH, and packaged native-window smoke. |

### Local benchmark observations

Command shape: release `rssh-app bench --json`, 1 MiB, 8 KiB chunks, 120x30,
30 proxy render frames, and a 200 ms idle sample.

| Workload | Runtime | Throughput | Chunk p95 | Render p95 | RSS | Deterministic work |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| `plain-scroll` | 282 ms | 3,705,635 B/s | 3,096 us | 527 us | 51,994,624 B | query bytes 0; survivor clones 0; row relocations 0 |
| `ansi-scroll-query` | 257 ms | 4,065,596 B/s | 2,996 us | 361 us | 52,727,808 B | inspected bytes 1,499,241; survivor clones 0; row relocations 0 |

Both reports contained `ok=true` and an empty `threshold_violations` array for
the invoked command. This is not an absolute performance certification:

- local plain throughput is below the design target of 5 MiB/s;
- local query chunk p95 is above the later 2 ms stabilization target;
- render p95 measures the CPU/offscreen `PixelRenderer` benchmark proxy, not
  native GPU presentation or input-to-present latency;
- no protected fixed-machine baseline run exists for this exact commit.

### Fresh Windows x64 package evidence

The following paths are ephemeral local evidence, not release downloads:

- archive: `E:\temp\rssh-task26\package-postfix-4b4fc10cfbd1464aabe7c5bb570a81fb\assembly\R-SSH-windows-x64-unsigned.zip`
- archive SHA-256: `7977fd5dd7598e89e13bbab5d82a8e3c053dc95e609b787d705ceeba11c47ec7`
- extracted binary: `E:\temp\rssh-task26\package-postfix-4b4fc10cfbd1464aabe7c5bb570a81fb\fresh-extract\R-SSH-windows-x64-unsigned\rssh-app.exe`
- extracted binary SHA-256: `2f5df6e3f2503792c317105f74f15dbc4438737e695e2d8fa049fe893e2ac561`
- extracted and workspace binary paths were distinct;
- manifest: version `0.1.0`, Rust target `x86_64-pc-windows-msvc`, runtime target
  `windows-x86_64`, PTY backend `windows-conpty`, signing state `unsigned`,
  source commit `local`. The package is not a publishable provenance artifact.

The smoke benchmark reported `ok=true`, no threshold violations, query
throughput 4,196,519 B/s, chunk p95 2,874 us, and zero survivor clones or row
relocations. Packaged system-OpenSSH and native ten-frame GUI focused tests each
passed `1/1`.

All package build, assembly, extraction, and smoke phases completed before the
outer reporting expression attempted to call the newer .NET
`SHA256.HashData` API, which Windows PowerShell 5.1 does not provide. That final
reporting expression returned nonzero; the archive and extracted-binary hashes
above were then recovered with PowerShell 5.1-compatible `Get-FileHash` from the
same unique extraction. The unsupported reporting API was not part of the
package or product scripts and did not mask a smoke-phase failure.

## Open evidence gaps

1. Push the exact commit and collect linked hosted runs for every intended OS,
   architecture, display backend, PTY backend, and the Linux independent
   OpenSSH server test.
2. Run the protected fixed-performance job with installed baseline variables
   and record the runner fingerprint. Local benchmark data cannot replace it.
3. Run protected Authenticode and macOS signing/notary/staple jobs, then verify
   the SBOM, provenance/attestation, final package smoke, and publish DAG.
4. Add and execute real IME, GPU-vendor, RDP, DPI/Retina, and hardware recovery
   certification. The current workflows do not provide this evidence.
5. Keep WezTerm compatibility claims bounded to behaviors listed in the
   [gap tracker](research/wezterm-parity-gap.md); do not infer general parity
   from the production foundation.
