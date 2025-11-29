# Codex TUI 自定义状态栏实现说明

本说明基于 PR `zcg/codex#1`（合并日期 2025‑11‑25，基准提交 `7573812e` 及最终合并 `9107fb2`）的实现梳理，帮助快速理解并维护 codex TUI 的状态栏定制层。

## 总体架构
- **开关**：`Config.tui_custom_statusline`（默认开启）。关闭后回退到上游原始状态栏。
- **入口**：`tui/src/chatwidget.rs` 在构造 `ChatWidget` 时创建 `StatusLineOverlay`，并在生命周期内把 git/k8s/环境/任务事件转发给它。
- **分层**：
  - `statusline/overlay.rs`：协调层，负责环境探测、异步刷新、布局计算以及向 `AppEventSender` 发事件。
  - `statusline/state.rs`：状态存储与快照生成，集中更新 cwd/模型/Token/上下文/环境/运行状态等，并触发重绘。
  - `statusline/mod.rs`：渲染核心，定义快照数据结构、降级策略和最终的行渲染。
  - `statusline/skins/`：定制渲染器（`CustomStatusLineRenderer`）及调色板（`palette.rs`），可替换默认渲染器。

## 关键数据流
1) **初始化**：`StatusLineOverlay::bootstrap` 根据配置填充模型、初始 Token 用量、排队消息，并启动 Git/K8s 刷新。
2) **环境探测**（同步）：DevSpace(`TMUX_DEVSPACE`)、主机名(`HOSTNAME`→系统 fallback)、AWS 配置(`AWS_PROFILE` / `AWS_VAULT`)。
3) **后台任务**（Tokio）：
   - `collect_git_info` + 自行调用 `git status --porcelain=2 --branch` 解析 dirty/ahead/behind。
   - 读取 kubeconfig 的 `current-context`，并截取末段简化显示。
4) **事件回传**：刷新结果通过 `AppEvent::StatusLineGit` / `StatusLineKubeContext` 送回 `ChatWidget`，再写入 `StatusLineState`。
5) **重绘**：任何状态更新都会调用 `FrameRequester` 请求下一帧。

## 渲染与降级逻辑（`mod.rs`）
- **主状态行**与**运行胶囊**分开渲染。先尝试完整内容，若超过目标宽度按序降级直至适配。
- **降级顺序（高→低保真）**：队列预览 → 中断提示 → 计时器 → 运行标签缩短/隐藏 → 路径简化/隐藏 → Token 简化/隐藏 → Context 简化/隐藏 → Git 简化/隐藏 → 依次移除 DevSpace/K8s/AWS/主机名 → 最后隐藏路径。
- **视觉风格**：Catppuccin 配色（`BASE/LAVENDER/SKY/PEACH` 等）+ powerline 分隔符（` ` 等）。模型、环境、Git 片段采用前景/背景渐变；状态旋转器默认弱化为 `◦`。
- **运行胶囊**：固定隐藏路径/Token/Context/Git，只呈现运行标签、计时、队列预览和中断提示，并有独立降级序列。
- **时间与 Token 辅助**：紧凑耗时格式化（秒/分/时），Token 统计会排除缓存输入；上下文剩余百分比用于进度条。

## 布局（`overlay.rs`）
- 预留高度：1 行运行胶囊 + 1 行状态栏，顶部/中部/底部各 1 行间距，共 5 行保留。底部区域高度不足或存在“活跃视图”时不渲染，避免遮挡。
- 运行胶囊贴近底部上方，状态栏固定在最底行；内容区位于两者之间。

## 环境与安全
- Git 命令未设超时（PR 讨论曾建议 5s timeout，可视需要补充）；失败则静默返回 `None`。
- K8s 解析允许多路径 `KUBECONFIG`，取首个包含 `current-context` 的配置。

## 测试与回归防护
- `statusline/tests.rs` 与 `chatwidget/tests.rs` 增加大量 insta 快照，覆盖窄/宽屏、排队消息、运行/空闲、环境组合等情境。
- `custom_renderer_matches_default_run_pill` 等用例确保定制渲染与默认逻辑在语义上保持一致，差异仅限外观。

## 维护要点
- 变更 API/渲染时同步更新快照；若修改降级策略，确认窄宽度场景的稳定性。
- 保持定制层隔离：颜色/符号仅在 `skins` 模块内引用；上游同步时主要关注 `overlay.rs` 钩子与 `ChatWidget` 连接点。
- 可选参考 PR 附带的 `customization-plan.md` 工作流（拉取上游→重放补丁→`just fmt`→`just fix -p codex-tui`→`cargo test -p codex-tui`）。

## 快速定位
- 状态管理：`tui/src/statusline/state.rs`
- 渲染逻辑：`tui/src/statusline/mod.rs`
- 异步刷新与布局：`tui/src/statusline/overlay.rs`
- 调色板与皮肤：`tui/src/statusline/palette.rs`, `tui/src/statusline/skins/mod.rs`
- 配置开关：`core/src/config/types.rs` (`tui_custom_statusline`)

