# 2026-09-19 Windows 本机验证

基线：`7c203f01f23e4bf5cf77b4aeceb1ec1845d94e01`（`main`）。
已执行所有远端分支及标签的 fetch，补全浅克隆历史与按需对象，并清理已删除分支的远端跟踪引用。
远端现存 15 个分支；验证对象是主分支，不是分别验证这 15 个历史分支。

## 环境与范围

- Windows 11 专业版，`10.0.26200`，`x86_64-pc-windows-msvc`。
- Rust/Cargo `1.89.0`；最终 Web 检查使用 Node `24.19.0`。
- 系统 OpenSSH `9.5p2`；应用版本报告 PTY 后端为 `windows-conpty`。
- 工具终端初始 PATH 缺少系统目录、Windows PowerShell、OpenSSH 与 Cargo。验证进程补齐这些路径，并仅为本次子进程设置 PowerShell 执行策略。
- 原始证据：`evidence/windows-20260919/`（忽略提交）；可复用环境脚本：该目录下的 `env.ps1`。
- 此记录是本机开发验证，不构成 Stage 7 受保护任务产品认证、跨平台准入或正式拆仓批准。

## 已修复的 Windows 测试问题

`rssh-test-support` 的标记辅助程序直接写 UTF-8 字节，却没有设置控制台输出代码页。
在本机中文 Windows 的 ConPTY 中，多语言标记被错误解码，原生十帧测试无法匹配标记并在 120 秒后超时。

对照实验 `marker-probe.log` 中，原脚本 `marker_preserved=false`，显式设置 UTF-8 后为 `true`。
本次修改在写入前设置 `Console.OutputEncoding`，并新增真实 ConPTY 回归测试，覆盖中文、阿拉伯文、印地文、希伯来文和 emoji。
修改不涉及产品逻辑，也没有放宽测试阈值。

## 已通过

| 检查 | 证据 |
| --- | --- |
| Rust 格式、工作区 Clippy（`-D warnings`）、架构约束 | `fmt-final.log`、`clippy-final.log`、`architecture-final.log` |
| Task 10 来源、R-Term 发布契约、功能测试目录和 hermeticity | `provenance.log`、`release-contract.log`、`matrix-catalog.log`、`hermeticity.log` |
| 核心 Python CI 检查器测试，41 项 | `ci-core-tests.log` |
| Web lint、3 项单元测试、生产构建（Node 24） | `web-*-node24.log` |
| Chromium 到真实 PTY 的端到端测试 | `playwright.log`、`web.playwright.json` |
| Windows 控制台功能分片，11 个场景全部通过 | `console-shard-0.log`、`console-shard-1.log`、`functional-console/` |
| ConPTY 多语言回归测试 | `marker-regression.log` |
| Windows 原生专用 runner：SSH、真实 PTY 十帧、100%/125%/150%/200% DPI、会话日志 | `native-runner.log` |
| Release 构建与生产观察器隔离 | `release-build.log`、`release-isolation.log` |
| 未签名 ZIP 打包、启动、SSH 与真实 PTY 窗口冒烟 | `package-smoke.log` |

构建产物：`dist/rssh-windows-20260919-unsigned.zip`，SHA-256 见 `package-sha256.json`。
该包来自默认功能的 Release 构建，是本机验证产物，不是签名发行包或最小 GUI 产品认证包。

## 全量测试中的未解决项

1. `window::tests::pane_pty_stop_reaps_real_child_and_joins_real_reader`：全量并发运行时超过既有 2 秒清理期限，转交 reaper；单独运行通过。保留并发失败，不以单项通过替代全量结果。见 `workspace-final.log`、`pty-stop-recheck.log`。
2. `local_app_drains_output_after_fast_child_exit`：进程归属检查只匹配 PID/父 PID，误把本轮开始前已存在的进程算作测试遗留。报告中的 `cmd.exe` PID 26004 创建于 12:38:15，而失败发生在约 14:01；其父 PID 16460 被后续测试复用。见 `pid-reuse-evidence.json`。未终止这个无关进程；该测试单独运行曾通过，见 `local-pty-recheck.log`。

首轮 `workspace-tests.log` 使用不完整环境且包含修复前代码，之后已被最终轮替代；其环境失败不能当成最终产品缺陷，也不能当作通过。
最终 `cargo test --locked --workspace --all-targets --no-fail-fast` 返回 101。
日志汇总为 **6076 项通过、2 项失败、12 项忽略**；失败目标为 `rssh-app` 主程序测试和 `local_pty` 集成测试。
忽略项沿用仓库标记，其中四档 DPI 与会话日志场景已由专用原生 runner 另外执行并通过。
没有把未执行的 release 性能探针算作通过。

额外的全量 Python `unittest discover` 在约 53 分钟后仍未汇总，且已输出失败/错误标记；本轮将其停止，不计为通过，保留 `python-tests.log`。
可确认通过的是单独执行的 41 项核心检查器测试。其余历史证明脚本的完整 Windows 结果仍未取得。

**结论：本机主要构建、传输、浏览器、原生渲染和打包检查通过，但全量测试未全绿。**
上述验证在提交前的工作区执行；未放宽剩余失败用例的断言。

## 复现命令

```powershell
. ./evidence/windows-20260919/env.ps1
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace --all-targets --no-fail-fast
./scripts/ci/run-native-window.ps1 -Profile debug -ExpectedTarget windows-x86_64 -ExpectedPtyBackend windows-conpty
```
