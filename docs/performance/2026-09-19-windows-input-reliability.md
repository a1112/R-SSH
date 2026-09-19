# Windows 下一阶段：输入验收与会话可靠性

本阶段承接 [Windows 本机验证](2026-09-19-windows-validation.md)，将现有回归用例整理为可重复执行的输入和会话验收矩阵。
基线为 `7c203f01f23e4bf5cf77b4aeceb1ec1845d94e01` 加当前工作区补丁。

本文保留首轮 19 项验收的历史记录；后续矩阵扩展与失败修复见 [Windows 认证复核](2026-09-19-windows-certification-followup.md)。

## 自动化入口

使用 PowerShell 7，确保 `cargo`、`rustc`、`git`、`ssh.exe`、`powershell.exe`、`cmd.exe` 可从 PATH 找到。
子进程需要能够执行仓库 PowerShell 脚本；本机验证仅使用进程级执行策略。

```powershell
./scripts/ci/run-windows-input-reliability.ps1 -SelfTest
./scripts/ci/run-windows-input-reliability.ps1 -PlanOnly
./scripts/ci/run-windows-input-reliability.ps1 -Rounds 3 -OutputDirectory evidence/windows-input-reliability
```

- 矩阵位于 `scripts/ci/windows-input-reliability.json`，19 个固定用例，默认 3 轮共 57 次执行。
- 从本次 Cargo JSON 构建结果提取测试和应用路径，记录提交、工作区补丁哈希、Rust 工具链、OS、矩阵与二进制哈希；结束时复核二进制未被替换。
- 每次使用精确测试名，必须有且仅有一个通过且未忽略的测试。零匹配、忽略、失败、多匹配均不能算通过。
- 复用 Windows Job Object 进程树框架：构建上限 1200 秒，每个用例上限 120 秒；无语义失败重试。
- 输出目录必须为空。每次保存 stdout/stderr，异常写入 `report.json`，最终报告原子替换。
- 成功状态为 `automated_passed_manual_pending`；计划预览不产生成功验收报告。

## 覆盖与证据边界

| 范围 | 本轮自动验证内容 | 尚需真机操作 |
| --- | --- | --- |
| Shift 与特殊字符 | 向窗口键盘处理器提交事件，断言写入字节 | 物理键盘布局、实际 Shift 组合键 |
| IME | 合成 preedit/commit/cancel 调用，检查不提前写入、取消不写入、提交一次 | 系统输入法、候选选择、组合期间焦点切换 |
| 粘贴与快捷键 | UTF-8/括号粘贴编码、重载快捷键行为 | 真实剪贴板、焦点目标、PTY/SSH 各自接收文本 |
| 会话可靠性 | 持续输出唤醒、关闭前排空、多 pane 所有权、重开标签、配置重载 | 实际多窗口交互、真实休眠唤醒 |
| ConPTY | 真实子进程输出的多语言与特殊字符经过终端解析后完整保留 | 物理输入到本地 shell 的完整交互 |
| SSH | 系统 SSH 回环命令、原生 SSH 断开重连及生命周期 | 物理输入到 SSH 会话、真实网络切换 |

事件处理器/编码测试使用捕获写入或模拟传输；不能替代操作系统输入法和真实键盘端到端证据。
实际 ConPTY 与 SSH 用例验证传输、输出及生命周期，不把它们描述为真实键盘输入验收。

## 本机执行记录

最终证据目录：`evidence/windows-next-stage/acceptance-final/`。
2026-09-19 14:41:49–14:42:09（北京时间），19 个用例连续执行 3 轮，**57/57 通过**。
报告状态为 `automated_passed_manual_pending`；测试程序与实际应用的结束哈希复核通过。
程序和测试哈希、每项结果、日志对应顺序、未完成手工项均保存在 `report.json`。
入口结果解析自检覆盖零测试、忽略测试、失败、多测试、缺失和重复汇总的拒绝行为。

## 真机验收记录要求

每次记录 Windows 版本、键盘布局、输入法名称及版本、应用完整路径与 SHA-256、配置文件路径及哈希、PTY/SSH 类型、执行时间。
先确认配置加载成功，再操作；不要把自动 Unicode 注入当成真实输入法操作。

| 项目 | 操作和预期 | 当前状态 |
| --- | --- | --- |
| 英文 Shift 标点 | 物理输入 `()!@#$%^&*`，核对实际收到文本及 UTF-8 字节 | 待真机验收 |
| 中文全角标点 | 输入法提交 `（） ，。！？`，不丢失或重复 | 待真机验收 |
| 组合与焦点 | 组合期间不发送；取消不发送；提交仅一次；切换焦点不串 pane | 待真机验收 |
| 粘贴和快捷键 | PTY 与 SSH 分别粘贴 `()（）!@#$%^&*，。！？`，快捷键不误写文本 | 待真机验收 |
| 休眠与网络 | 独立记录真实休眠/唤醒、网络切换后的连接、输出和关闭结果 | 待真机验收 |

保存预期文本、实际收到的原始文本/字节与日志；仅有截图或主观确认时注明证据限制。
所有手工项初始为待办，不以本轮自动化通过自动改成完成。

## 保留问题

上一轮的 PTY 并发清理期限和 PID 复用误报继续保留，列在矩阵的 `known_open_checks`，本轮不声明它们已修复。
本阶段没有更新完整工作区全绿结论，也没有执行性能对比、签名发行或 Stage 7 正式跨平台认证。

## 拆仓准入复核

2026-09-19 执行 `check-stage7-split-gate.py --contract scripts/ci/stage7-split-contract.json --requested-state extraction-ready`，未提供正式证据清单，返回 `NO-GO`、`state=blocked`，原因为 `extraction-ready requires exactly one evidence manifest`。
本轮 57 项通过不能替代 Stage 7 所需的不可变跨平台原始证据；当前记录不足以批准正式拆仓。
