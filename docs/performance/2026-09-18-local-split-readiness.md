# 2026-09-18 本机验证与拆仓准入审计

当前结论：**NO-GO，尚不可正式拆仓**。本轮不修改准入合同、性能阈值或平台要求。
基线主分支为 `22233263695c576b4e5642a5b8d26c0714be49ae`。

## 本轮已落实

- continuation 测试逐批等待发布，隔离传输通知与后续唤醒；负向验证能检出被禁用的唤醒。
- 输入测试覆盖 Shift 特殊字符、中文标点、IME 组合取消与一次提交、粘贴；断言写入字节。
- 真实 PTY 输出和 native SSH 断开/重连场景增加 `()（）!@#$%^&*，。！？`，验证传输后的内容。
- 修正 macOS 测试的窗口焦点前提；使用实际渲染布局定位第三个标签的关闭按钮。
  产品吞掉聚焦点击的默认行为不变，生产代码未改动。
- 本机专项结果：runtime composition 6、IME 15、paste 37、shortcut 70、reload 35、close 128 项通过。
  过滤器可能重叠，这些数字不代表独立测试总数。
- OpenSSH 集成 7 项、PTY 集成 6 项通过；OpenSSH 强制要求工具可用，native SSH 执行已安装 Release。
  PTY 集成执行当前 debug 构建，不能冒充 Release 安装验收。
- 现有 `22233263` unsigned Release 安装与启动证据继续保留。本次都是测试/文档变化，不替换用户安装。

原始日志位置：`/Users/a1-6/.codex/backups/R-SSH-split-readiness-20260918`。
输入法真机操作仍只有用户此前的完成确认；本轮自动测试不等于真实输入法端到端采集。
睡眠唤醒、真实网络切换未执行。

## 正式拆仓的剩余门槛

| 门槛 | 当前事实 | 所需下一步 |
| --- | --- | --- |
| 本次修复集成 | 本机验证通过，PR/精确提交 CI 待记录 | PR 通过后合并，单独检查 main CI |
| Task 12 Windows 产品认证 | 仓库 runner API 返回 `total_count=0`；`run-stage7-product-gates.ps1` 不存在 | 实施 Windows 协调器，在固定机器取得原始性能、功能、秘密扫描和来源证据 |
| Task 12 Linux/macOS 正式认证 | 现有 shell 入口调用 macOS-only local runner；旧本机采样不是 protected-job provenance | 实施正式跨平台协调和受保护任务，并按精确候选/LKG 采样 |
| cross-platform-go | 尚无完整不可变证据清单 | 由现有 checker 判定通过，不能用 CI 绿色替代 |
| Task 13–15 提取准备 | release contract v2、bootstrap 检查器/模板、提取工具尚未实施 | 按原计划完成边界隔离、历史映射、Task10 来源、独立 workspace 和发布性校验 |
| Task 16 extraction-ready | checker 返回 blocked/NO-GO：缺少证据清单 | 汇集 prior 与提取各阶段证据，再由 checker 判定 |

核验命令：

```sh
python3 scripts/ci/check-stage7-split-gate.py \
  --contract scripts/ci/stage7-split-contract.json \
  --requested-state extraction-ready
gh api repos/a1112/R-SSH/actions/runners
```

无清单的检查用于确认 fail-closed 行为及当前没有提交正式证据，不能证明历史上不存在其他外部证据。
正式准入规则见 [Stage 7 任务计划](../plans/2026-08-23-stage7-split-readiness.md)。
本机开发顺序见 [下一阶段计划](../plans/2026-09-18-macos-local-development-next-stage.md)。
不创建远端 R-Term 仓库、不切换消费来源、不删除本地包，直至相应准入证据具备。
