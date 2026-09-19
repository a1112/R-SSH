# Windows 失败修复与拆仓认证复核

日期：2026-09-19。基于 `220900e91ea2109add9921d4bdf82126a2788dfd` 的后续修复。

## 修复范围

- 快速退出测试原先只按 PID/父 PID 匹配残留进程。Windows 会复用 PID，因此早于测试创建的无关 `cmd.exe` 也可能被误判。现在每次启动记录 UTC 时间范围，同时匹配进程的创建时间；早于该次启动或晚于该次结束的 PID 新旧实例不归属该次测试。新增回归覆盖应用、shell、两类控制台进程的真实残留、旧实例与后续复用实例。
- 真实 PTY 关闭测试现在先回答 ConPTY 启动时的光标位置查询，再等待子进程输出就绪标记，确认读取线程已读到标记后测量关闭。原来的 `io::copy` 消费者没有实现终端查询应答；只加入就绪标记的中间版本因此出现启动超时，已保留失败日志。最终测试将启动协议与运行中子进程关闭分开，保留原有 2 秒关闭期限、读取线程完成、无 reaper 转交与子进程消失断言；不修改生产关闭逻辑。
- Windows 关闭用例使用 `cmd.exe /D /Q /C` 的内建输入等待，避免托管运行时或额外后代进程影响关闭用例；保留真实 ConPTY 和读写句柄。
- 全量运行另外暴露了动画时钟竞态：颜色选择和半程插值断言执行前，真实时间继续推进，导致颜色表示或舍入结果变化。加入仅测试编译启用的时钟，固定这些用例的采样时刻，保留原动画配置与精确色值断言；生产版本继续使用真实时间。
- 进程框架自检原来先发就绪信号再写输出，改为写入并刷新两条流后再发布就绪。超时与输出排空断言保持不变，辅助 PowerShell 进程以隐藏窗口启动。
- `cargo -j1` 仅限制编译并行度。本机默认 16 路测试并发仍触发原生关闭超时，因此 Windows CI 的 quality 任务显式设置 `RUST_TEST_THREADS=4`；本轮最终完整套件使用同样设置。未删除测试、放宽期限或把无限制并发失败改写成通过。
- 将原失败测试及新回归纳入 Windows 验收矩阵，矩阵由 19 项扩展至 25 项，三轮共 75 次。物理键盘、真实输入法、剪贴板、休眠及网络切换仍保留待验收。

## 执行与证据

本轮原始日志位于 `evidence/windows-certification-20260919/`。构建输出使用 `D:/codex-builds/R-SSH-windows-certification-20260919`，避免原工作区盘空间不足影响验证。

结果以本轮最终日志及验收报告为准；此前 `windows-validation` 与 `windows-input-reliability` 文档保留各自历史批次结果。

| 最终检查 | 结果 | 原始记录 |
| --- | --- | --- |
| 完整 Rust 工作区，4 路测试并发 | 6079 通过、0 失败、12 忽略，退出码 0 | `workspace-bounded.log`、`workspace-result.json` |
| 全工作区 Clippy，warnings 为错误 | 通过 | `clippy-complete.log` |
| 架构检查 | 通过 | `architecture-final.json` |
| 核心 Python 与针对性门禁回归 | 43 项通过 | `python-focused.log` |
| Windows 输入、生命周期、进程框架与真实 PTY/SSH | 25 项 × 3 轮，75/75 通过 | `acceptance-final/report.json` 及逐项日志 |
| Rust 格式检查 | 通过 | `format-final.log` |

完整 Rust 日志含 107 份测试汇总。应用、主程序测试及真实 PTY 测试二进制在专项验收重建前的哈希保存在 `workspace-binaries.json`。
专项验收于北京时间 16:17:19–16:20:02 执行，前后应用及测试二进制哈希一致，报告状态为 `automated_passed_manual_pending`。五项手工验收类别仍为待办。
本轮代码、合同、最终日志及验收报告的 SHA-256 索引为 `local-validation-receipt.json`；它是本地复核记录，不是可晋级的 Stage 7 正式证据清单。

全量 Python discovery 已输出 64 项通过，但历史证明全套仍未结束；为单独复核有时限的原生测试，本轮停止该并发负载，保留 `python-first-failure.log` 与 `python-incomplete.txt`，不算完整 Python 套件通过。Windows 正式确定性认证仍缺这项完整结果。

仓库已非浅克隆，禁用懒加载后核对 22,973 个可达对象均完整。备份本地 Git 配置后移除遗留 `remote.origin.promisor` 和 `partialclonefilter`，避免负向 Git 对象测试访问远端。未改变提交历史或远端地址。

本地完整套件复跑时保持与 Windows CI 相同的测试并发设置，并单独执行有时间期限的原生检查：

```powershell
$env:CARGO_TARGET_DIR = 'D:/codex-builds/R-SSH-windows-certification-20260919'
$env:RUST_TEST_THREADS = '4'
cargo test --workspace --all-targets --locked -j1
cargo clippy --locked --workspace --all-targets -- -D warnings
./scripts/ci/run-windows-input-reliability.ps1 -Rounds 3 -OutputDirectory <新的空证据目录>
```

环境仍需 Rust 1.89.0、PowerShell 7、Windows PowerShell、Git、OpenSSH 及完整系统 PATH；脚本策略仅使用进程级设置。

## 正式拆仓状态

现有检查器对 `attribution-ready`、`windows-memory-go`、`cross-platform-go`、`extraction-ready` 均返回 **NO-GO / blocked**。这些调用未提供正式清单，明确返回相应状态需要证据清单；不能据此声称全部性能项目已实际执行并失败。

本轮用户确认暂无可用 runner。普通本地测试日志不能替代受保护任务来源，仓库中的历史 macOS 本地报告也不能直接证明本轮候选通过跨平台认证。

按冻结合同，逐阶段新增证据为：

| 状态 | 新增证据类型数 | 尚需完成 |
| --- | ---: | --- |
| attribution-ready | 8 | 字体所有权、累计 GPU 归因、硬件/字体指纹、真实双 bare 仓库消费、确定性验证及完整原始记录 |
| windows-memory-go | 11 | Release 来源、首帧/驻留/SSH/GPU 原始采样、同机固定基线比较、包/原生功能/秘密扫描/完整确定性套件 |
| cross-platform-go | 14 | Linux PSS 与 macOS physical footprint，同平台 LKG 比较、原生功能、包及受保护任务来源 |
| extraction-ready | 9 | 发布合同 v2、提取清单与历史映射、历史安全扫描、SBOM、独立 CI、双仓 Task 10、外部消费及回退证明 |

这是累计 42 类证据，不是 42 个普通测试。完整类型名与本轮门禁输出分别保存在 `required-artifacts.json` 和 `split-gate-decisions.json`。

## 恢复认证的入口

1. 配置 `.github/workflows/release.yml` 已定义的 `[self-hosted, Windows, X64, rssh-performance]` runner 与 `performance` 受保护环境，使用固定候选 SHA 执行 Stage 7 attribution 任务。该任务的 `stage7_gate_only` 入口只提供相应阶段证据，不能单独代表 extraction-ready。
2. 补齐 Windows 产品认证协调及固定基线采样；为相同候选提供 Linux/macOS 原生环境、采样与受保护任务证据。macOS 本地采集可复用 `scripts/ci/run-stage7-product-gates.sh`，其输出范围仍是本地报告。
3. 跨平台 GO 后执行 Task 13 的本地提取证明，以 `assemble-stage7-evidence.py` 组装真实片段与前序清单，再交给原检查器验证。

```powershell
python scripts/ci/check-stage7-split-gate.py `
  --contract scripts/ci/stage7-split-contract.json `
  --requested-state extraction-ready `
  --evidence-manifest <完整证据清单路径>
```

未降低门槛、修改固定基线或将合成测试证据当成认证证据。
