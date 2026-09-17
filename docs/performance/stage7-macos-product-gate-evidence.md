# macOS 本机产品验收记录

日期：2026-09-17。状态：macOS ARM64 本机产品验收通过，完整回归通过。

本次执行源于将下一阶段验收改为当前 Mac 本机执行的要求。该记录只覆盖本机架构与运行条件，不授予 `windows-memory-go`、`cross-platform-go` 或仓库拆分准入。

## 固定版本与产物

- 产品候选：`ba31959d421051350035bfe71acc705b61f702c4`。
- 产品性能基线：`4cbee13c591ded5ccfc6b0aec68f2b33143528c1`，来自原 Stage 7 合同。
- 冻结 R-Term 回退：`0e8ebd5de22758275cbb6a849c19c032268d7fac`，来自发布合同。
- 本机：MacBook Pro / Apple M3 Max / 36 GB，macOS 26.6.2（25G83），原生 aarch64；Retina 面板 3456×2234。采样固定 80×24、100% 缩放；显示与电源原始信息保留在采样目录。
- 证据根目录：`/Users/a1-6/.codex/backups/R-SSH-macos-20260917/`。
- 实际候选 Release 包：`final-candidate/rssh-candidate-macos-aarch64-unsigned.tar.gz`。
- 包内程序：`final-candidate/payload/R-SSH.app/Contents/MacOS/rssh-app`。
- 总体验收索引：`local-acceptance.json`，关联所有主要回执和最终回归日志的 SHA-256。
- 二进制、归档及源提交身份校验：`release-package-identities.json`。
- 采样脚本、合同和构建日志副本：`implementation-and-checks/`；脚本哈希单独记录，不把本地协调脚本误称为产品提交的一部分。

## 本机发现并修复的问题

1. 演练器创建的系统临时目录带 `/var` 别名，触发路径安全校验。只规范化新创建的自有临时目录，并保留对外部传入链接路径的拒绝。
2. macOS 不支持 interprocess 在 socket 绑定前执行 `fchmod`。改为在已有 0700 私有目录内绑定，再将 socket 设置为 0600；增加 socket 权限断言。
3. BSD tar 自动附加 AppleDouble `._` 元数据，导致归档与 payload 清单不一致。打包时设置 `COPYFILE_DISABLE=1`，继续完整校验归档内容和权限。
4. 原诊断启动器仅保留失败输出尾部。新增可选原始 stdout/stderr/SSH 日志保留，对实际 fixture secret 执行全流扫描，并对缺失日志、读取/写入失败和泄漏返回不完整证据。最后补强为拒绝复用已有证据目录，实际验证旧回执保持不变；最终采集器另行通过真实 SSH 原始日志冒烟检查。
5. 仓库补齐历史后仍带 promisor/blob:none 配置，测试中的不存在提交会触发反复网络请求。确认 22,881 个可达对象全部存在后，备份本地 Git 配置并移除残留部分克隆配置；缺失对象检查恢复为本地快速失败。

## 已完成的功能验证

`rehearsal-final/candidate.json` 和 `rollback.json` 均为 `ok: true`。两种配置完整运行原发布合同的六条消费命令；分别验证实际包的原生 SSH 断开/重连、真实 PTY 十帧和 100% 缩放 GPU 文本。每条必需场景都要求恰好一项通过、零失败、零忽略。

发布合同现有消费包采用 Debug 构建，因此另对真正的 Release `.app` 执行上述三项场景，记录于 `release-functional.json`。执行前后程序哈希一致。Release 候选和性能 LKG 的归档均已校验有效载荷、执行权限和源提交身份。

冻结回退配置另在独立目录构建 Release，并再次通过相同三项原生场景。`frozen-release/receipt.json` 同时记录冻结源与候选消费者 SHA、原始构建哈希、归档及包测试结果；执行后重新验证准备目录、锁文件、源树和所有包身份。外部复跑助手首次后验检查缺少 Python 导入路径，修正助手后仅重跑身份检查通过，未重建或重试已通过的场景。

手动通过 CUA 检查实际 `.app` 的原生窗口、粘贴与 Return 执行，得到 `RSSH_MACOS_NATIVE_ACCEPTANCE_OK` 回显；检查活动中文输入法下的输入与 Control-C，最终 Command-Q 退出后确认进程消失。快速 `typeText` 在活动输入法下没有可靠输入完整 ASCII 命令，未将该尝试列为通过；粘贴执行单独验证通过。截图保留在本任务对话，文字记录在 `manual-ui-evidence.json`。

## 性能协议与结果

正式采样目录为 `sampling-final/`。`sampling-initial/` 是有并发构建、旧归档元数据的探索批次，已标记 interrupted，不用于认证。

正式运行期间暂停本任务的完整 Python 回归，构建全部结束后再采样。启动、空窗口及单 SSH 场景分别在候选与 LKG 间逐轮交错：5 次预热 + 30 次独立冷进程；每进程 60 秒期限。驻留等待 owner-ready 后稳定 5 秒，以 100 ms 间隔取 10 点；先取各进程 nearest-rank 中位数，再算跨进程 p50/p95，max 使用全部 300 点。

产品使用 `auto`，驻留验证 GPU 与 Apple M3 Max 适配器身份、SSH connected 状态和原生 `macos_phys_footprint_bytes`。Windows 的 Private Bytes、Working Set 和显存门槛不移植到 macOS。macOS 首帧 stderr 中同名 `first_frame_private_bytes` 字段不作为本轮内存指标。

| 首次呈现（ms） | p50 | p95 | max |
| --- | ---: | ---: | ---: |
| 候选 | 167 | 181 | 184 |
| 产品 LKG | 171 | 186 | 188 |

启动数据仅作诊断；本轮 footprint 相对门槛为 p50/p95/max 均不超过同机 LKG 的 1.05 倍。驻留结果如下（MiB）：

| 场景 / 版本 | p50 | p95 | max |
| --- | ---: | ---: | ---: |
| 空窗口 / 候选 | 379.11 | 380.69 | 388.81 |
| 空窗口 / 产品 LKG | 1025.64 | 1028.44 | 1036.22 |
| 单 SSH / 候选 | 379.63 | 380.36 | 380.39 |
| 单 SSH / 产品 LKG | 1025.78 | 1028.28 | 1028.42 |

两组全部比值均低于 1.05，约为基线的 0.37，采样报告为 `passed`。210 次进程记录（含预热）全部齐备；每组每版本各 30 次正式采样。140 份驻留原始日志扫描完整、秘密命中数为零；逐文件哈希复核通过。

另外执行了长时 SSH 可视检查：连接状态可见，窗口放大与恢复后连接仍正常，截图中未见测试秘密。该交互批次保存在 `visual-ssh.json` 和 `visual-ssh-capture/`，400 点数据不混入上述固定协议。字体清单另存 `font-inventory-after-sampling.json`，明确为采样后的环境记录。

## 验证记录

- `rssh-functional-tests`：157 通过，零失败、零忽略。
- `rssh-diagnostics` 最终常规测试：91 通过；默认忽略的 macOS live sampler 和两个真实诊断 GUI 探针均已显式运行通过（另计 3 项）。通用诊断 GUI 首次指向产品包时因缺少 diagnostic-tools 被拒绝；随后改用独立诊断构建通过，未向产品包启用诊断特性。必需产品路径由真实包与完整产品采样覆盖。
- 候选/回退演练 Python 测试：28 通过。
- 新协调器确定性测试：6 通过，覆盖统计分层、指标/后端/退出状态拒绝、超时保留及包路径校验。
- 修改模块 Clippy、Rust 格式及 shell 语法检查已通过。
- 完整 Python 回归：216 项全部通过。首轮因浅克隆缺少完整历史被来源门禁拒绝，补齐历史后完整重跑通过；最终日志为 `implementation-and-checks/rssh-macos-python-full-history.log`。运行期间为性能采样暂停过进程，日志总耗时包含该暂停。

## 复跑

先用固定提交构建 Release 候选和 LKG，按 `package-native.sh` 的 `--runtime-target macos-aarch64 --pty-backend unix-pty --unsigned` 打包；设置 `GITHUB_SHA` 为对应源提交。旧 LKG 的原打包脚本还需在环境设置 `COPYFILE_DISABLE=1`。用既有 `verify_archive` 核验归档与 payload 后运行：

```sh
caffeinate -i bash scripts/ci/run-stage7-product-gates.sh \
  --candidate-ref ba31959d421051350035bfe71acc705b61f702c4 \
  --candidate-package /absolute/path/to/candidate/payload \
  --lkg-package /absolute/path/to/product-lkg/payload \
  --launcher /absolute/path/to/rssh-bench-launcher \
  --output-dir /absolute/path/to/new-evidence-directory
```

入口拒绝覆盖已有证据目录。其 `passed` 仅表示本地性能采样通过；总体产品结论还必须结合原生功能、回退、包身份及人工界面记录，不能从一个采样字段推导跨平台 GO。

## 版本与验收结论

性能与包验收绑定不可变候选 `ba31959d421051350035bfe71acc705b61f702c4`。后续收尾改动为采集目录拒绝复用、协调器校验与文件哈希记录、对应测试和文档；其中正式采样实际使用的协调器副本及哈希已保存在证据目录。不能把这些记录重新标成另一提交的产品测量。

本机 ARM64 的候选与冻结回退 Release 原生功能、实际 GUI 交互、包身份和固定协议性能均通过。此结论只针对上述硬件、产物和采样条件；不改变原 Stage 7 跨平台或拆分状态。

改动保存在本地分支 `codex/macos-product-acceptance`，未推送；本记录不声称收尾提交已通过远端 CI。
