# Codex 状态栏实现技术报告

## 设计目标
- 提供可选的“自定义状态栏”外观，同时保持与上游 TUI 的兼容与低侵入集成。
- 将渲染、状态收集、背景刷新与布局解耦，便于后续合并和定制。
- 支持运行状态、队列预览、上下文/Token 消耗、Git/K8s/AWS/主机等环境信息的动态展示，并在终端宽度不足时逐步降级。

## 功能开关与配置
- 配置项：`tui_custom_statusline`（默认开启）。
  - 定义位置：`codex-rs/core/src/config/types.rs`（`Tui` 结构新增 `custom_statusline`，默认 true），在 `config/mod.rs` 读取并传播。
- 导出模块：`codex-rs/tui/src/statusline/*`；当开关为 false 或高度不足/存在底部弹窗时，回退为上游默认布局。

## 关键文件与模块
- 核心渲染与数据结构：`codex-rs/tui/src/statusline/mod.rs`
  - `StatusLineSnapshot` 及子结构：cwd、model、tokens/context、run_state、git、environment（devspace/hostname/aws/k8s）。
  - `StatusLineRenderer` trait；默认皮肤与自定义皮肤（`skins/mod.rs`）共享渲染逻辑。
  - 自适应降级策略：按序隐藏/简化队列预览 → 中断提示 → 计时器 → 运行标签 → git/tokens/context/path 等，直至宽度适配；最终回退为 `codex | model | branch*`。
  - 运行胶囊（run pill）与状态行分离渲染；计时器支持暂停/恢复，Label 默认 “Ready when you are”。
  - 上下文条：彩色进度（绿/黄/橙/红）+ 可切换紧凑文本；Tokens 支持 Minimal/Compact/Full 三档。
- Palette 与图标：`statusline/palette.rs` 定义 Catppuccin 风格颜色与图标（model、git、AWS、K8s、hostname、devspace）。
- Overlay 与布局：`statusline/overlay.rs`
  - 布局：顶部空行 → run pill → 间距 → pane → 间距 → 底部状态行。保留空白 Margin，避免覆盖底部内容。
  - 背景刷新：异步 Git（porcelain v2 解析 dirty/ahead/behind）、Kube context（kubeconfig current-context）、Devspace（TMUX_DEVSPACE）、Hostname、AWS Profile。
  - 暴露 API：更新 git/kube、model、tokens、队列、运行标签、计时器、任务开始/完成、中断提示。
- 状态管理：`statusline/state.rs`
  - 构建 Snapshot，维护 RunTimer（计时与 spinner 起点），序列化 token/context。
  - 依据 FrameRequester 安排重绘（计时器活动时 48ms tick）。
- 皮肤：`statusline/skins/mod.rs` 自定义皮肤实现 Renderer，逻辑与默认皮肤一致，保证行为一致性。
- 集成点：`codex-rs/tui/src/chatwidget.rs`
  - 构造时创建 `StatusLineOverlay`（在配置允许且高度满足时生效）。
  - 事件流：任务开始/结束、Exec/Tool/Approval 开始、Git/Kube 更新（`AppEvent::StatusLineGit/StatusLineKubeContext`）、Token 更新、队列变更。
  - ESC 在任务运行时：调用 `halt_running_task` 中断、清理 Run 状态与提示；底部 Pane 隐藏原有状态指示器以让自定义状态栏接管。
  - 将底部 Pane 包装为 `BottomPaneWithOverlay`，在渲染/高度/光标计算时考虑 Overlay。
- 底部 Pane/Composer 改动：`tui/src/bottom_pane/*`, `tui/src/bottom_pane/chat_composer.rs`
  - 新布局常量与空白填充，光标对齐行校验，保留顶部/底部填充与透明 Margin，确保状态栏与输入区分离。
- App 事件：`tui/src/app_event.rs` 新增 StatusLineGit/Kube 事件；`app.rs` 处理事件并驱动 ChatWidget。
- 环境/持久化：
  - Workspace 状态（模型与 MCP 启用）持久化：`core/src/workspace_state.rs`。
  - 历史文件锁定：`core/src/message_history.rs` 使用 `fs2` 读/写锁，防止竞争。
- CLI 版本附带 commit SHA：`codex-rs/cli/build.rs` + `main.rs` 中 `CLI_VERSION`。

## 数据来源与处理
- Git：`collect_git_info` 获取分支；`git status --porcelain=2 --branch` 解析 dirty/ahead/behind。
- K8s：读取 `KUBECONFIG` 路径列表，解析 `current-context`，裁剪 ARN/gke 前缀。
- Devspace：`TMUX_DEVSPACE`。
- Hostname：`HOSTNAME` 环境或系统 hostname。
- AWS Profile：`AWS_PROFILE` / `AWS_VAULT`。
- Token/Context：来自 `TokenUsageInfo`，基于上下文窗口计算剩余百分比（预留 baseline 以降低噪声）。

## 降级策略（宽度优先级）
1. 隐藏队列预览
2. 隐藏中断提示
3. 隐藏计时器
4. 缩短运行标签 → 隐藏运行标签
5. 简化 Git → 隐藏 Git
6. Tokens：Full → Compact → Minimal → 隐藏
7. Context：Bar → Compact → 隐藏
8. 路径：Full → Basename → 隐藏
9. 逐项隐藏环境信息（Devspace/K8s/AWS/Hostname）

## 布局与交互
- 高度不足或底部存在视图（弹窗/审批等）时不启用 Overlay。
- Run pill 和 Status Line 保留透明 Margin，不覆盖底部行；底部 Pane 光标与提示保持对齐。
- ESC 行为：运行中触发中断，清空任务状态，Run pill 恢复默认提示。

## 测试与快照
- Statusline 快照：`tui/src/statusline/snapshots/*.snap`，覆盖宽/窄、Run pill、降级路径。
- ChatWidget/BottomPane 快照：验证 composer 填充/光标行、Overlay 与 Pane 组合、审批/队列/运行态的整体布局。
- 覆盖点：
  - 自定义皮肤与默认 Renderer 输出一致性。
  - 计时器/队列预览/中断提示展示。
  - ESC 中断后状态重置。
  - 上下文/Token 计算与色阶。
  - Git/K8s/环境段落的显示与裁剪。
- 其他：历史锁定单元测试、Workspace 持久化测试、CLI 版本包含 SHA 测试。

## 典型路径（运行时）
1. 会话配置完成 → `ChatWidget` 初始化 Overlay，传入配置/Token/队列。
2. 任务开始 → Run pill 显示计时器/Label/队列预览；底部原状态指示隐藏。
3. 收到 Tokens/Context 更新 → Overlay 更新快照重绘。
4. 后台任务刷新 Git/Kube/Env → 通过 AppEvent 驱动 Overlay 更新。
5. 任务完成或 ESC → 计时器暂停，状态重置为 “Ready when you are”，队列同步。
6. 终端宽度变化 → Renderer 重新降级/扩展组成，保证不截断。

## 风险与注意事项
- 高度要求：Overlay 需要预留行数，过窄/有弹窗时自动回退，不影响交互。
- 环境探测依赖外部命令/文件（git/kube）；失败时安全降级为无信息。
- Token/Context 依赖后端上报；缺失时隐藏相关段落。
- 上游合并：定制主要集中在 `statusline/` 与 `chatwidget` 底部接入层，避免侵入上游组件；配置开关默认开启但可回退。

## 文件参考
- `codex-rs/tui/src/statusline/mod.rs`
- `codex-rs/tui/src/statusline/overlay.rs`
- `codex-rs/tui/src/statusline/palette.rs`
- `codex-rs/tui/src/statusline/skins/mod.rs`
- `codex-rs/tui/src/statusline/state.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/bottom_pane/mod.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/core/src/config/{mod.rs,types.rs}`
- `codex-rs/core/src/workspace_state.rs`
- `codex-rs/core/src/message_history.rs`
- `codex-rs/cli/build.rs`, `codex-rs/cli/src/main.rs`

如需进一步针对特定场景（如仅展示运行胶囊、不展示环境信息）的定制建议或风险评估，可告知。
