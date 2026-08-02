# R-SSH 现代深色终端视觉设计

## 目标

将 R-SSH 的默认视觉从当前高密度、低层次的灰黑界面调整为现代深色终端风格，同时保持现有 WezTerm 配置兼容、Unicode shaping、字体 fallback 和 GPU 渲染行为。

成功标准：

- 英文等宽字符清晰、列对齐稳定，中文和其他脚本继续由系统字体 fallback。
- 终端正文、标签栏、光标、选区和 ANSI 颜色形成明确层级。
- 长输出、OpenCode、Claude Code、Codex、Git 彩色日志和 Unicode 混排均无重影、裁切或 tofu。
- 用户配置的字体、颜色、字号和 padding 仍能覆盖默认值。

## 视觉方向

采用现代深海蓝方案：

- 主字体：Cascadia Mono；不可用时依次使用 Source Code Pro、Consolas 和现有 Noto Sans fixture。
- 字体 fallback：保留 Noto Sans SC/JP、Microsoft YaHei、Meiryo、Malgun Gothic、Segoe UI、Nirmala UI 和 emoji 字体。
- 终端背景：`#0B1220`。
- 默认前景：`#D8E2F0`。
- 弱化前景：`#8492A6`。
- 光标：`#67E8F9`，光标文字使用深色背景色。
- 选区：蓝灰色半透明效果，对比清晰但不刺眼。
- 标签栏背景：`#080D18`；活动标签：`#172033`；非活动标签：`#101827`。
- 状态和焦点强调色：`#38BDF8`。

ANSI 调色板避免纯黑和高饱和荧光色，保持日志、编译器诊断和 AI CLI 语义颜色易辨识。红色用于错误，黄色用于警告，绿色用于成功，蓝青色用于链接、提示和活动状态。

## 字体与网格

GPU 文本后端优先加载平台等宽字体，不将窗口 DPI 再次叠加到物理像素网格。默认字体大小和网格必须成对调整，保证字形不会跨越行边界。

初始目标为 15px Cascadia Mono、9px 单元格宽度和 18px 单元格高度。若真实 GPU 截图显示基线或宽字符对齐异常，则保持字体 15px，并仅在 8–10px 宽、17–19px 高范围内校正网格。

中文宽字符必须占两个逻辑单元格；字体 fallback 只能改变字形来源，不能改变终端模型的列宽。

## 布局

默认窗口内容增加 8px 左右 padding 和 6px 上下 padding。padding 参与内容区域计算，但不改变 PTY 报告的单元格尺寸。

标签栏保持单行，使用更深的背景与清晰的活动标签。标题过长时继续按现有宽度规则裁切，不能通过压缩字形塞入标签。

Windows 和其他现代默认窗口在真实渲染路径中增加非交互的 `R-SSH` 品牌段；workspace
名称使用弱化前景，默认 tab 标签隐藏 pane/index 诊断元数据并保留 `×` 关闭标记与 `+▾`
新建入口。显式 `tab_bar_style` 或标签配置仍优先，继续使用原始 WezTerm 标签格式。

滚动条默认行为不变。光标、下划线、分隔线和选区颜色使用新调色板，但不改变交互语义。

## 配置优先级与降级

新字体和颜色只作为默认值。Lua 配置、内置配色方案、外部配色文件和命令行覆盖保持现有优先级。

若平台字体文件不存在，加载器跳过该候选并继续后续字体；最终始终保留内置 fixture，避免启动失败。单个系统字体损坏不得阻止窗口创建。

Windows 优先 Cascadia Mono；macOS 优先 Menlo/Monaco；Linux 优先 Noto Sans Mono/DejaVu Sans Mono。跨平台可以存在细微字形差异，但颜色、间距和终端列对齐必须一致。

## 效果图与实现验证

先生成一张 16:10 控制台效果图，包含标签栏、PowerShell 提示符、Git 状态、中文、ANSI 色块以及一段 AI CLI 输出，用于确认整体比例和颜色关系。

实现后以真实 R-SSH GPU 窗口截图替代概念稿进行最终判断，覆盖：

- 中文、日文、韩文、阿拉伯文、印地文和 emoji。
- OpenCode、Claude Code、Codex 的版本和帮助输出。
- Git 彩色日志、编译器诊断、ANSI 16 色和 35 行滚动输出。
- 100%、125% 和 150% Windows 缩放下的行距与基线。

自动验证包括字体选择/fallback、默认调色板、padding、损坏区域与完整重绘一致性、GPU 文本、原生窗口 E2E 和全 workspace 测试。
