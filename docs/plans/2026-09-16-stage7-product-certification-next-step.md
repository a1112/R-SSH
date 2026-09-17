# Stage 7 下一步：macOS 本机产品验收

创建：2026-09-16；修订：2026-09-17。状态：macOS 协调脚本已实施，固定候选、冻结回退 Release 原生验收和同机性能采样均通过；完整脚本回归 216 项通过，本机验收完成。执行证据见 [本机产品验收记录](../performance/stage7-macos-product-gate-evidence.md)。

本阶段在当前 Mac 上完成候选版本、冻结回退版本的打包、原生 GUI/PTY/SSH/GPU 功能及同机性能验收，交付可复现脚本和原始证据报告。Windows 性能机及 Linux 环境不再是本阶段的前置依赖。

交付结论限定为“macOS 本机验收通过/失败/受阻”，并记录具体硬件与架构。完整 Task 12 的跨平台认证和 R-Term 拆分准入另行保留；现有机器合同不因调整执行平台而自动改变。

## 1. 起点与平台调整

下表远端 CI 与 runner 状态为 2026-09-16 的已核实快照；实施时刷新候选提交及相关产物，本次修订不重新宣称远端状态。

| 项目 | 当前证据 | 对下一步的含义 |
| --- | --- | --- |
| 主分支 | `5a7d7ed70611b443f0fabff6764573a569bc0b11`；设计前工作区干净，与 origin/main 对齐 | 本设计的起点；后续认证绑定实际候选的完整提交 SHA |
| 合并后 CI | [35101058760](https://github.com/a1112/R-SSH/actions/runs/35101058760) 成功 | 可以开始下一个增量 |
| 合并后 CodeQL | [35101058622](https://github.com/a1112/R-SSH/actions/runs/35101058622) 成功 | 当前安全扫描已通过 |
| 候选与回退合同 | 上述 CI 的 “R-Term candidate and rollback contract” 成功，job `104810307426` | 本轮尚未下载审计完整原始产物，先补齐产物审计 |
| 历史 Gate 0 | `0e190289e24bc12c6d621e47f1560f9afaf5bb9d` 的 attribution-ready 证据 | 是归因与来源证明，不能直接授予当前候选产品 GO |
| 产品协调入口 | `run-stage7-product-gates.ps1`、`.sh` 及对应 runner 测试尚不存在 | 下一步需要实现的主要缺口 |
| 性能机 | 仓库级 runner `rssh-stage7-i9-14900k`：offline；标签 Windows/X64/rssh-performance | 仅影响后续 Windows 认证，不阻塞本轮 macOS 验收 |
| Unix 性能环境 | 本次仓库级 runner 列表未见 Linux/macOS 节点 | 本轮直接使用本机图形会话；本地记录不能冒充受保护 CI 来源 |

生产 lazyfont/GPU 适配和双版本消费路径已有实现，本阶段在这些成果上补齐认证。旧计划提到另一 Windows 工作区中的九个 Task 12 草稿；当前 checkout 中没有这些协调文件。若找回草稿，先审查来源与现有合同兼容性，再选择性复用。

## 2. 基线与状态边界

所有基线从当前合同读取，拒绝以浮动分支名替代。三个用途分别为：

| 合同字段 | 固定值 | 用途 |
| --- | --- | --- |
| `stage7-split-contract.json:lkg_rssh_ref` | `21dd01b3d73dd9c9241ac10e7a25d92cb2bcfea6` | 来源历史与提取边界 |
| `stage7-split-contract.json:product_lkg_ref` | `4cbee13c591ded5ccfc6b0aec68f2b33143528c1` | 产品延迟、内存的同机比较基线 |
| `rterm-release-contract.json:last_known_good_rterm_ref` | `0e8ebd5de22758275cbb6a849c19c032268d7fac` | 冻结 R-Term 消费与回退验证 |

早期任务文本若把产品比值指向 `lkg_rssh_ref`，以当前合同的 `product_lkg_ref` 为准。

沿用现有状态机：`blocked → attribution-ready → windows-memory-go → cross-platform-go → extraction-ready → dual-source-verified → split-complete`。

macOS 本地验收结果是阶段报告结论，不向现有状态机新增 `macos-go`，也不直接晋级 `cross-platform-go`。每次正式晋级由现有 `check-stage7-split-gate.py` 对完整不可变证据判定；新增协调器只负责执行、记录和组装，不创建另一套 GO 判定规则。

## 3. 协调器设计

执行链：固定合同与 SHA → 环境预检 → 隔离构建及包身份记录 → 原生功能验证 → 同机交错采样 → 原始证据归档 → 现有证据组装与门禁检查 → 产品验收报告。

拟新增的交付物：

- `scripts/ci/run-stage7-product-gates.sh`：本轮首先实现 macOS 原生入口，不依赖 PowerShell；Linux 后续扩展。
- Windows `.ps1` 协调入口移至后续平台工作，不列入本轮交付。
- `scripts/ci/tests/test_stage7_product_runners.py`：入口协议、执行顺序、失败保留和证据绑定的确定性测试。
- `docs/performance/stage7-macos-product-gate-evidence.md`：记录候选、本机硬件/架构、原始产物位置、本地验收结论及后续平台缺口。

优先复用现有 Python 证据模型和校验器。仅在两种入口确有重复的数据处理时提取共享模块，避免在 shell 中维护两套统计算法。这里列出的新增文件为实施目标，不代表已经存在。

每次执行使用独立输出目录，记录合同哈希、候选与基线 SHA、构建角色、二进制哈希、runner/字体/驱动指纹、命令及退出结果。生产、诊断、候选和回退构建隔离 target 目录，防止测试构建覆盖已记录身份的产品。

包验证读取产物清单中的实际 executable；macOS 校验包内二进制，不能拿启动包装脚本代替。构建产物、归档有效载荷和实测进程身份必须相符，同时验证解包后的执行权限。清单采用原子写入；失败仍保留原始证据，存放位置独立于会被清理的临时工作区。

拒绝路径逃逸、符号链接替换、证据篡改、缺失采样、零测试成功和必需后端不支持。哈希用于完整性与关联验证，不描述为密码学签名。

## 4. macOS 验收范围与采样

| 项目 | 本阶段通过条件 |
| --- | --- |
| 环境与身份 | 原生架构运行，记录 macOS、芯片、内存、工具链、显示缩放、字体和实际 GPU 后端；候选及基线固定完整 SHA |
| 构建与打包 | Release 构建及现有必需检查通过；验证 `.app` 内二进制、清单、归档有效载荷与实际执行身份一致 |
| GUI | 从实际包启动可见窗口；记录首帧、文本渲染、输入、缩放和关闭行为；自动断言配合必要的窗口证据 |
| PTY/SSH | 原生 PTY 十帧、loopback SSH 连接/重连及进程清理通过；必需测试不能静默跳过 |
| GPU | 产品 `auto` 路径成功，记录实际后端；驻留场景证明实际 GPU 渲染，不能以 CPU 回退替代 |
| 候选与冻结回退 | 两种消费配置分别构建、打包并运行原生功能，证明切换与回退后可用 |
| 内存 | 采集原生 `physical footprint`；合同要求的 p50/p95/max 相对同机 `product_lkg_ref` 比值均 ≤ 1.05 |
| 启动 | 保存首次呈现时间及原始分布用于诊断；现有 Windows 的 400/500 ms 门槛不移植为 macOS 已批准门槛 |
| 证据 | 原始样本完整、身份可核验、秘密扫描零命中且覆盖完整；必需项失败或跳过不能判定通过 |

Windows 的 Private Bytes/Working Set 绝对门槛继续保留在原合同，不能套用到 macOS physical footprint。当前不为 macOS 发明新的绝对内存或启动预算；需要增设预算时单独修改合同并说明依据。统一内存环境的 GPU 内存诊断同样不能直接等同 Windows 显存门槛。

每组 5 次预热、30 次独立冷进程，单进程超时 60 秒。候选和产品 LKG 在同机逐轮交错运行，保持电源模式、窗口几何、字体、后端及后台负载条件一致，避免休眠和并发性能任务。记录运行环境变化，变化影响可比性时重跑整组。

启动场景使用 `--benchmark-startup`，每进程取一次 marker，不增加五秒稳定等待；结束于 CPU bootstrap，不附带 GPU backend identity。驻留场景等待 owner-ready marker 后稳定 5 秒，再以 100 ms 间隔采集 10 个样本。每进程取 nearest-rank 中位数，对 30 个代表值计算 p50/p95；max 取全部 300 个原始样本，禁止混合展开计算百分位。

macOS footprint 采集接口及所需权限先做最小探测，读数不可用时标记受阻，禁止用 RSS 替代。Apple Silicon 与 Intel 结果按实际架构分别标注，不用 Rosetta 或交叉编译推导另一架构已验收。

## 5. 分阶段实施与退出条件

### PR 1：macOS 预检、基线和功能证据

固定候选 SHA，并审计现有成功 CI 的候选与冻结回退产物；缺失证据在本机对应版本重新构建执行。记录锁文件、构建、包和二进制身份。复用 `package-native.sh`、`package-smoke.sh`、`run-native-window.sh` 及既有候选/回退演练能力；测试构建与待认证 Release 包隔离，避免原生测试入口重新构建覆盖已记录身份的包。

预检本机原生 Rust target、工具链、GUI 会话、GPU、OpenSSH fixture 和 footprint 读取能力。只有相关交互测试确需时检查辅助功能/输入权限，不擅自更改系统隐私设置。使用隔离 SSH fixture，不依赖修改用户系统 SSH 服务。

退出条件：候选、产品性能 LKG 和冻结 R-Term 回退三种角色清晰；本机原生窗口、PTY、SSH、GPU 和包运行证据齐全。失败留下原始日志和明确原因。

### PR 2：macOS 产品协调器与测试

实现 `.sh` 原生入口，完成预检、隔离构建、同机交错采样、身份绑定、失败归档和 macOS 报告。复用现有证据格式与校验函数；本地报告明确区分指标通过、功能通过和完整跨平台门禁状态。

确定性测试覆盖 5+30 运行次数、采样顺序、统计分层、超时清理、缺失 marker、错误 renderer、错误架构、包身份不符、footprint 不可用和部分执行失败。构造超阈值或缺失原始证据的用例必须失败，不能只测试成功路径。

退出条件：新增协调测试、受影响的既有消费演练与门禁测试通过；脚本能完整运行 macOS 验收，并在条件不足时明确失败或受阻。

### 本机验收批次：完成 macOS 验收

选择包含协调实现且必需检查通过的完整候选 SHA，在当前 Mac 上运行全部功能与性能批次。从实际 `.app` 验证交互和生命周期，保存原始采样及候选/LKG 比较，核验冻结回退版本功能。

秘密扫描覆盖 stdout/stderr、markers、指标、会话日志及可见内容快照；按实际 fixture secret 检查零命中和覆盖完整，不在公开证据存储秘密或其摘要。

退出条件：本节所有必需功能、footprint 相对门槛和证据校验通过，生成 `stage7-macos-product-gate-evidence.md`，包含可复跑命令、提交、硬件、架构、包路径和哈希。本轮可以独立结项，无需等待 Windows/Linux。报告注明本地来源，不伪造 protected-job provenance。

若超标或基线无法原生运行，保留失败/受阻结论，按证据定向修复后重新完整验收；不改用另一基线或降低门槛。历史 Windows access violation 作为后续平台问题保留，不成为本机结项依赖，也不声明已解决。

### 后续平台与拆分工作

Windows 固定硬件认证、Linux PSS 验收及 macOS 受保护任务来源属于后续完整 Task 12 工作。届时刷新各平台候选与证据兼容性，由现有校验器判定 `windows-memory-go` / `cross-platform-go`。本地 macOS 通过不等同这两个状态。

完整跨平台 GO 后再推进 Task 13 的 release contract v2、bootstrap 和本地提取演练。远端发布、消费来源切换及本地包删除仍按各自状态门槛执行。

## 6. 验证与交付边界

本轮交付为 macOS 协调脚本、有效测试、实际 Release 包及本机验收报告。报告将代码检查、打包原生功能、性能样本和全平台状态分别列出，确保结论与证据范围一致。

按改动运行对应 Python 测试与 shell 检查；修改共享证据模型时运行完整 Python CI 测试和合同检查。涉及 Rust 产品行为时补充对应 Rust 测试及必需 CI。设计修订本身只检查文档与引用，本次不宣称已完成真实验收。

## 7. 依据

- [Stage 7 原始设计](2026-08-23-stage7-split-readiness-design.md)
- [Stage 7 任务计划](2026-08-23-stage7-split-readiness.md)
- [双 LKG 检查点](2026-09-05-stage7-dual-lkg-checkpoint.md)
- [双适配器计划](2026-09-09-rterm-dual-adapter.md)
- [打包功能检查点](2026-09-15-rterm-packaged-functional-checkpoint.md)
- [已验证演练集成检查点](2026-09-15-verified-rehearsal-integration-checkpoint.md)
- [历史 Gate 0 证据](../performance/stage7-gate0-evidence.md)
- [Stage 7 当前合同](../../scripts/ci/stage7-split-contract.json)
- [R-Term 当前发布合同](../../scripts/ci/rterm-release-contract.json)
