# Stage 7 Gate 0 evidence

## Decision

Gate 0 is **GO** at commit
`0e190289e24bc12c6d621e47f1560f9afaf5bb9d`. The certified state is
`attribution-ready`.

This decision proves that the font-ownership attribution, eight-stage GPU
attribution, deterministic attribution suite, and immutable local two-bare-Git
source proof are bound to one source commit and intact raw evidence. It does
not claim `windows-memory-go`, authorize extraction, publish R-Term, or permit
deleting the in-tree R-Term packages.

The frozen validator was rerun against the downloaded evidence and returned:

```json
{"artifact_count":967,"decision":"GO","go":true,"ok":true,"reason":"Immutable raw evidence proves attribution-ready.","state":"attribution-ready","violations":[]}
```

## CI and immutable identity

- Workflow: [Release run 33405866286](https://github.com/lcxinc/R-SSH/actions/runs/33405866286), attempt 1
- Job: [Stage 7 cumulative GPU attribution matrix](https://github.com/lcxinc/R-SSH/actions/runs/33405866286/job/99533199371), job `99533199371`
- Result: `success`
- Source branch: `codex/stage7-split-readiness`
- Certified source SHA: `0e190289e24bc12c6d621e47f1560f9afaf5bb9d`
- Product executable SHA-256: `99998c8534c40a8e121b86c9dd03c2e351dbf41d2b5caffba2aaa9e720099738`
- Launcher executable SHA-256: `66e1b094aa0050c27737118fd74d012b83caa3bd0acc96538de977b1edd856cd`
- Runner fingerprint SHA-256: `a7a9650282b9ea32b5e5fd329383d968e76df160afd1934fe7f0ea6855f17f2d`
- Font inventory fingerprint SHA-256: `27b20fbac87aaca51174c643427bf3409c274ffea95c5a5dbaf478c642d9d125`

The runner fingerprint, without host name or machine-local paths, records:

- Windows `10.0.26300`, build revision `9032`, `x86_64`
- GPU vendor ID `4318`, device ID `11524`, driver `32.0.16.2002`, WDDM `3.2`
- Physical memory `68003237888` bytes and manual pagefile mode
- One primary `2560 x 1440` display at `96 x 96` DPI
- Local session, `zh-CN` culture/UI/system locale
- Power plan `8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c`
- Process-cold starts with no explicit OS file-cache flush

## Evidence integrity

The downloaded evidence contains 972 physical files totalling 106,576,826
bytes. Five files are the final manifest plus its four input fragments; the
remaining 967 files are manifest entries. An independent read-only audit
recomputed every entry's size and SHA-256 with zero mismatches.

- Frozen contract SHA-256: `9836ca68f212c641596fbe1571b19caf86f506fb112948f7903d398e18136923`
- Final manifest SHA-256: `9ae7f25cb1677f1e2c6e608a8987f4941678c3cbf882b3ba5f4896f19ed4cd40`
- External-source fragment SHA-256: `c1666fe7ff5a9b92a105ab58c48f89918b97fd4159996fd51479b457e8df22b7`
- Font fragment SHA-256: `4bcf553b328c6a89155547e78a44cb156696fe56ef13441e5924a4c3840786bc`
- Stage fragment SHA-256: `969ed2e5df078bbb74c138d85022122ea6098e2de7f1845541a863c761d4de64`
- Deterministic-test fragment SHA-256: `4ca220bfe6e8d5dcabeb6c0c8560db099dc55d0e071d28dd082f816814cf451e`

Manifest entry counts are:

| Artifact type | Count |
| --- | ---: |
| Attribution matrix raw | 960 |
| Attribution matrix aggregate | 1 |
| Font ownership raw | 1 |
| Font ownership aggregate | 1 |
| Font catalog fingerprint | 1 |
| Runner fingerprint | 1 |
| Deterministic attribution tests | 1 |
| Local two-bare-Git source proof | 1 |

## Font-ownership proof

Each mode used five warmups and 30 process-cold measured processes. Each
measured process contributed the nearest-rank median of ten residence samples;
cross-process p50/p95 use nearest rank and maximum uses all 300 raw samples.
The independent audit recomputed all 90 process representatives and all three
aggregate rows with zero mismatches.

| Mode/specimen | p50 bytes (MiB) | p95 bytes (MiB) | Raw max bytes (MiB) |
| --- | ---: | ---: | ---: |
| `current-copied/ascii` | 311361536 (296.938) | 312954880 (298.457) | 313856000 (299.316) |
| `shared-all/ascii` | 220266496 (210.062) | 222072832 (211.785) | 222568448 (212.258) |
| `lazy/ascii` | 130355200 (124.316) | 132661248 (126.516) | 133165056 (126.996) |

The recomputed p50 reductions are:

- Current copied to shared: `91095040` bytes (`86.875 MiB`), above the required `64 MiB`
- Shared to lazy: `89911296` bytes (`85.746 MiB`), above the required `32 MiB`

All six CJK/emoji functional specimens used the same actual Vulkan backend,
rendered with zero tofu, used one catalog generation per frame, and retained
the same source bytes after simulated device recovery. The lazy mode's first
fallback activation latency is report-only: `17.939 ms` for CJK and `12.726 ms`
for emoji.

## Eight-stage attribution proof

The matrix contains four requested backends, eight ordered stages, and 30
process-cold measurements per backend/stage: 960 raw process files and 9,600
raw memory samples. The independent audit recomputed every process
representative, every p50/p95/raw maximum, and every aggregate representative
set. It found zero representative, statistic, identity, or file-integrity
mismatches and zero failure classifications.

Values below are Private Working Set MiB in `p50 / p95 / raw max` order.

| Requested backend/stage | p50 | p95 | Raw max |
| --- | ---: | ---: | ---: |
| `auto/cpu-window` | 2.293 | 2.340 | 2.363 |
| `auto/instance-surface` | 29.027 | 29.082 | 29.086 |
| `auto/adapter-device` | 88.309 | 89.836 | 89.902 |
| `auto/configured-surface-clear` | 101.969 | 103.723 | 104.086 |
| `auto/layer-pipelines` | 102.961 | 104.781 | 104.859 |
| `auto/fixture-font-text` | 104.359 | 106.863 | 108.031 |
| `auto/platform-font-index` | 104.105 | 106.703 | 107.148 |
| `auto/full-frame` | 104.707 | 106.832 | 106.867 |
| `dx12/cpu-window` | 2.293 | 2.324 | 2.355 |
| `dx12/instance-surface` | 2.629 | 2.680 | 2.688 |
| `dx12/adapter-device` | 65.336 | 65.824 | 65.871 |
| `dx12/configured-surface-clear` | 66.762 | 67.383 | 68.035 |
| `dx12/layer-pipelines` | 68.090 | 68.379 | 68.676 |
| `dx12/fixture-font-text` | 69.133 | 69.578 | 70.078 |
| `dx12/platform-font-index` | 68.980 | 69.496 | 70.566 |
| `dx12/full-frame` | 70.277 | 72.059 | 72.473 |
| `vulkan/cpu-window` | 2.297 | 2.359 | 2.359 |
| `vulkan/instance-surface` | 10.230 | 10.320 | 10.340 |
| `vulkan/adapter-device` | 32.070 | 32.156 | 32.168 |
| `vulkan/configured-surface-clear` | 70.176 | 70.309 | 70.363 |
| `vulkan/layer-pipelines` | 70.699 | 70.812 | 70.863 |
| `vulkan/fixture-font-text` | 72.809 | 72.906 | 72.984 |
| `vulkan/platform-font-index` | 72.738 | 72.910 | 72.918 |
| `vulkan/full-frame` | 73.043 | 73.398 | 73.465 |
| `gl/cpu-window` | 2.293 | 2.320 | 2.344 |
| `gl/instance-surface` | 28.375 | 28.434 | 28.445 |
| `gl/adapter-device` | 31.012 | 31.082 | 31.090 |
| `gl/configured-surface-clear` | 31.902 | 31.965 | 32.000 |
| `gl/layer-pipelines` | 32.066 | 32.145 | 32.156 |
| `gl/fixture-font-text` | 32.668 | 32.805 | 32.812 |
| `gl/platform-font-index` | 32.684 | 32.840 | 32.867 |
| `gl/full-frame` | 33.711 | 33.871 | 33.918 |

`auto` selected Vulkan. From `adapter-device` onward, the actual identities
were stable within every backend: `auto` and `vulkan` used adapter digest
`bf4644668e23a0b251550103e503541e9cd849ceadced0fcff6257518c234e9d`,
DX12 used `381a5d32affdbcbcc53192af6845855b854bbc2557e9cea72a1e880540520e3a`,
and GL used `b3aa90660f58afe22ddd2173284db250dbb9ee4a1214d59aa50331eb308b46e0`.
The pre-adapter stages correctly have no actual adapter identity.

## Deterministic and external-source proofs

The `stage7-attribution-deterministic-v1` suite passed and is bound to the same
source, binaries, platform, and runner fingerprint.

The external-source proof created two independent bare repositories with no
alternates and proved immutable full-SHA consumption. All seven R-Term packages
resolved from synthesized candidate commit
`c27e06c406b3d5d3d9fabeef4a63c5a4a5206ab3`; the synthesized rollback commit
was `e04611d0b6304b14c6bb9805fb3caa9df8200c5a`. The source proof maps the R-SSH
candidate `0e190289e24bc12c6d621e47f1560f9afaf5bb9d` and LKG
`21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6` into those independent object
stores.

The vendored exceptions resolved only through normalized relative paths
`glyphon-0.12.0/Cargo.toml` and `gpu-allocator-0.28.0/Cargo.toml`, both with no
registry/Git source. Both lockfile generations, locked metadata, and locked
consumer check returned zero. The proof records no post-commit lockfile
regeneration. Before this evidence-summary edit, an independent status check at
the certified source SHA reported an empty worktree and index.

## Next state

The next permitted transition is `windows-memory-go`. It requires promoting the
lazy font path to production, lazily materializing image-only GPU resources,
and then producing a new exact-commit Windows product evidence chain. Gate 0
evidence remains an immutable historical predecessor; it is not reused as if it
certified a later implementation commit.
