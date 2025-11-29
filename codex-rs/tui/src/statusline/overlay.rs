use std::env;
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::statusline::StatusLine88CodeSnapshot;
use crate::statusline::StatusLineGitSnapshot;
use crate::statusline::StatusLineRenderer;
use crate::statusline::code88_api::fetch_88code_usage;
use crate::statusline::state::StatusLineState;
use crate::text_formatting::truncate_text;
use codex_core::config::Config;
use codex_core::git_info::collect_git_info;
use codex_core::protocol::McpInvocation;
use codex_core::protocol::TokenUsageInfo;
use hostname::get as get_hostname;
#[cfg(test)]
use lazy_static::lazy_static;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget as _;
use tokio::process::Command;
use tokio::runtime::Handle;
use tokio::task::spawn_blocking;

use super::CustomStatusLineRenderer;

#[derive(Debug, Clone, Copy)]
pub(crate) struct StatusLineLayout {
    pub pane_area: Rect,
    pub run_pill_area: Rect,
    pub status_line_area: Rect,
}

#[derive(Debug)]
pub(crate) struct StatusLineOverlay {
    state: StatusLineState,
    app_event_tx: AppEventSender,
    cwd: PathBuf,
    code88_api_key: Option<String>,
}

impl StatusLineOverlay {
    const MARGIN_ABOVE_PILL: u16 = 1;
    const MARGIN_ABOVE_PANE: u16 = 1;
    const MARGIN_BELOW_PANE: u16 = 1;
    const RUN_PILL_HEIGHT: u16 = 1;
    const STATUS_LINE_HEIGHT: u16 = 1;
    // Minimum pane content reduced by 1 since BottomPane no longer adds TOP_MARGIN
    const MIN_PANE_CONTENT_HEIGHT: u16 = 3;
    const RESERVED_ROWS: u16 = Self::MARGIN_ABOVE_PILL
        + Self::RUN_PILL_HEIGHT
        + Self::MARGIN_ABOVE_PANE
        + Self::MARGIN_BELOW_PANE
        + Self::STATUS_LINE_HEIGHT;
    pub(crate) fn new(
        config: &Config,
        frame_requester: crate::tui::FrameRequester,
        app_event_tx: AppEventSender,
        renderer: Option<Box<dyn StatusLineRenderer>>,
    ) -> Option<Self> {
        if !config.tui_custom_statusline {
            return None;
        }
        let renderer = renderer.unwrap_or_else(|| Box::new(CustomStatusLineRenderer));
        let state = StatusLineState::with_renderer(config, frame_requester, renderer);
        Some(Self {
            state,
            app_event_tx,
            cwd: config.cwd.clone(),
            code88_api_key: config.tui_code88_api_key.clone(),
        })
    }

    pub(crate) fn bootstrap(
        &mut self,
        config: &Config,
        initial_tokens: Option<TokenUsageInfo>,
        queued_messages: Vec<String>,
    ) {
        self.sync_model(config);
        self.state.update_tokens(initial_tokens);
        self.refresh_environment();
        self.state.set_queued_messages(queued_messages);
        self.spawn_git_refresh();
        self.spawn_kube_refresh();
        // Initialize 88code with loading state if API key is configured
        if self.code88_api_key.is_some() {
            self.state.set_88code_info(Some(StatusLine88CodeSnapshot {
                is_error: false,
                ..Default::default()
            }));
        }
        self.spawn_88code_refresh();
    }

    pub(crate) fn sync_model(&mut self, config: &Config) {
        self.state
            .update_model(config.model.clone(), config.model_reasoning_effort);
    }

    pub(crate) fn refresh_environment(&mut self) {
        self.state.set_devspace(detect_devspace());
        self.state.set_hostname(detect_hostname());
        self.state.set_aws_profile(detect_aws_profile());
    }

    pub(crate) fn spawn_background_tasks(&self) {
        self.spawn_git_refresh();
        self.spawn_kube_refresh();
        self.spawn_88code_refresh();
    }

    pub(crate) fn refresh_git(&self) {
        self.spawn_git_refresh();
    }

    fn spawn_git_refresh(&self) {
        let Ok(handle) = Handle::try_current() else {
            return;
        };
        let cwd = self.cwd.clone();
        let tx = self.app_event_tx.clone();
        handle.spawn(async move {
            let snapshot = collect_status_line_git_snapshot(cwd).await;
            tx.send(AppEvent::StatusLineGit(snapshot));
        });
    }

    fn spawn_kube_refresh(&self) {
        let Ok(handle) = Handle::try_current() else {
            return;
        };
        let tx = self.app_event_tx.clone();
        handle.spawn(async move {
            let context = detect_kube_context_async().await;
            tx.send(AppEvent::StatusLineKubeContext(context));
        });
    }

    fn spawn_88code_refresh(&self) {
        let Some(api_key) = self.code88_api_key.clone() else {
            return; // No API key configured, skip refresh
        };
        let Ok(handle) = Handle::try_current() else {
            return;
        };
        let tx = self.app_event_tx.clone();
        handle.spawn(async move {
            let snapshot = match fetch_88code_usage(&api_key).await {
                Ok(data) => Some(StatusLine88CodeSnapshot {
                    subscription_name: data.subscription_name,
                    credit_limit: data.credit_limit,
                    current_credits: data.current_credits,
                    is_error: false,
                    error_msg: None,
                }),
                Err(e) => Some(StatusLine88CodeSnapshot {
                    is_error: true,
                    error_msg: Some(e.to_string()),
                    ..Default::default()
                }),
            };
            tx.send(AppEvent::StatusLine88Code(snapshot));
        });
    }

    pub(crate) fn update_git(&mut self, git: Option<StatusLineGitSnapshot>) {
        self.state.set_git_info(git);
    }

    pub(crate) fn update_kube_context(&mut self, context: Option<String>) {
        self.state.set_kubernetes_context(context);
    }

    pub(crate) fn update_88code(&mut self, data: Option<StatusLine88CodeSnapshot>) {
        self.state.set_88code_info(data);
    }

    pub(crate) fn set_renderer(&mut self, renderer: Box<dyn StatusLineRenderer>) {
        self.state.set_renderer(renderer);
    }

    #[cfg(test)]
    pub(crate) fn state_mut(&mut self) -> &mut StatusLineState {
        &mut self.state
    }

    pub(crate) fn set_session_id(&mut self, session_id: Option<String>) {
        self.state.set_session_id(session_id);
    }

    pub(crate) fn set_run_header(&mut self, header: &str) {
        self.state.update_run_header(header);
    }

    pub(crate) fn set_interrupt_hint_visible(&mut self, visible: bool) {
        self.state.set_interrupt_hint_visible(visible);
    }

    pub(crate) fn start_task(&mut self, label: &str) {
        self.state.start_task(label);
    }

    pub(crate) fn complete_task(&mut self) {
        self.state.complete_task();
    }

    pub(crate) fn resume_timer(&mut self) {
        self.state.resume_timer();
    }

    pub(crate) fn update_tokens(&mut self, info: Option<TokenUsageInfo>) {
        self.state.update_tokens(info);
    }

    pub(crate) fn set_queued_messages(&mut self, messages: Vec<String>) {
        self.state.set_queued_messages(messages);
    }

    pub(crate) const fn reserved_rows() -> u16 {
        Self::RESERVED_ROWS
    }

    pub(crate) fn layout(
        &self,
        bottom_pane_area: Rect,
        has_active_view: bool,
    ) -> Option<StatusLineLayout> {
        let reserved_height = Self::RESERVED_ROWS;
        let minimum_height = reserved_height + Self::MIN_PANE_CONTENT_HEIGHT;
        if has_active_view || bottom_pane_area.height < minimum_height {
            return None;
        }

        let mut y_cursor = bottom_pane_area.y.saturating_add(Self::MARGIN_ABOVE_PILL);
        let run_pill_area = Rect {
            x: bottom_pane_area.x,
            y: y_cursor,
            width: bottom_pane_area.width,
            height: Self::RUN_PILL_HEIGHT,
        };

        y_cursor = y_cursor
            .saturating_add(Self::RUN_PILL_HEIGHT)
            .saturating_add(Self::MARGIN_ABOVE_PANE);
        let pane_height = bottom_pane_area.height.saturating_sub(reserved_height);
        let pane_area = Rect {
            x: bottom_pane_area.x,
            y: y_cursor,
            width: bottom_pane_area.width,
            height: pane_height,
        };

        let status_line_area = Rect {
            x: bottom_pane_area.x,
            y: bottom_pane_area
                .y
                .saturating_add(bottom_pane_area.height)
                .saturating_sub(Self::STATUS_LINE_HEIGHT),
            width: bottom_pane_area.width,
            height: Self::STATUS_LINE_HEIGHT,
        };

        Some(StatusLineLayout {
            pane_area,
            run_pill_area,
            status_line_area,
        })
    }

    pub(crate) fn render_run_pill(&self, area: Rect, buf: &mut Buffer) {
        let line = self.state.render_run_pill(area.width);
        line.render(area, buf);
    }

    pub(crate) fn render_status_line(&self, area: Rect, buf: &mut Buffer) {
        let line = self.state.render_line(area.width);
        line.render(area, buf);
    }

    pub(crate) fn exec_status_label(command: &[String]) -> String {
        if command.is_empty() {
            return "Running command".to_string();
        }
        let joined = command.join(" ");
        let summary = truncate_text(&joined, 40);
        format!("Running {summary}")
    }

    pub(crate) fn tool_status_label(invocation: &McpInvocation) -> String {
        let label = if invocation.server.is_empty() {
            invocation.tool.clone()
        } else {
            format!("{}:{}", invocation.server, invocation.tool)
        };
        format!("Running tool {}", truncate_text(&label, 40))
    }

    pub(crate) fn approval_status_label(subject: &str) -> String {
        format!("Awaiting approval for {subject}")
    }
}

fn detect_devspace() -> Option<String> {
    #[cfg(test)]
    if let Some(override_value) = DEVSPACE_OVERRIDE.lock().unwrap().clone() {
        return override_value;
    }

    env::var("TMUX_DEVSPACE")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn detect_aws_profile() -> Option<String> {
    env::var("AWS_PROFILE")
        .or_else(|_| env::var("AWS_VAULT"))
        .ok()
        .map(|profile| {
            profile
                .trim()
                .trim_start_matches("export AWS_PROFILE=")
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

fn detect_hostname() -> Option<String> {
    if let Ok(host) = env::var("HOSTNAME")
        && !host.trim().is_empty()
    {
        return Some(host);
    }
    get_hostname().ok().and_then(|os| os.into_string().ok())
}

async fn collect_status_line_git_snapshot(cwd: PathBuf) -> Option<StatusLineGitSnapshot> {
    let info = collect_git_info(&cwd).await?;
    let (dirty, ahead, behind) = git_status_porcelain(&cwd)
        .await
        .unwrap_or((false, None, None));
    Some(StatusLineGitSnapshot {
        branch: info.branch,
        dirty,
        ahead,
        behind,
    })
}

async fn git_status_porcelain(cwd: &Path) -> Option<(bool, Option<i64>, Option<i64>)> {
    let output = Command::new("git")
        .args(["status", "--porcelain=2", "--branch"])
        .current_dir(cwd)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut dirty = false;
    let mut ahead = None;
    let mut behind = None;
    for line in text.lines() {
        if !line.starts_with('#') {
            dirty = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split_whitespace();
            if let Some(ahead_part) = parts.next() {
                ahead = ahead_part
                    .strip_prefix('+')
                    .and_then(|s| s.parse::<i64>().ok());
            }
            if let Some(behind_part) = parts.next() {
                behind = behind_part
                    .strip_prefix('-')
                    .and_then(|s| s.parse::<i64>().ok());
            }
        }
    }
    Some((dirty, ahead, behind))
}

async fn detect_kube_context_async() -> Option<String> {
    spawn_blocking(detect_kube_context_sync)
        .await
        .ok()
        .flatten()
}

fn detect_kube_context_sync() -> Option<String> {
    for path in kube_config_paths() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') {
                    continue;
                }
                if let Some(value) = trimmed.strip_prefix("current-context:") {
                    let context = value.trim();
                    if !context.is_empty() {
                        return Some(trim_kube_context(context));
                    }
                }
            }
        }
    }
    None
}

fn kube_config_paths() -> Vec<PathBuf> {
    if let Some(paths) = env::var_os("KUBECONFIG") {
        env::split_paths(&paths).collect()
    } else if let Some(home) = env::var_os("HOME") {
        vec![PathBuf::from(home).join(".kube/config")]
    } else {
        Vec::new()
    }
}

fn trim_kube_context(context: &str) -> String {
    context.rsplit('/').next().unwrap_or(context).to_string()
}

#[cfg(test)]
lazy_static! {
    static ref DEVSPACE_OVERRIDE: Mutex<Option<Option<String>>> = Mutex::new(None);
}

#[cfg(test)]
pub(crate) fn set_devspace_override_for_tests(value: Option<String>) {
    *DEVSPACE_OVERRIDE.lock().unwrap() = Some(value);
}

#[cfg(test)]
pub(crate) fn clear_devspace_override_for_tests() {
    *DEVSPACE_OVERRIDE.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use crate::statusline::CustomStatusLineRenderer;
    use crate::tui::FrameRequester;
    use codex_core::config::ConfigOverrides;
    use codex_core::config::ConfigToml;
    use ratatui::buffer::Buffer;
    use tokio::sync::mpsc::unbounded_channel;

    fn overlay_for_tests() -> StatusLineOverlay {
        let mut cfg = Config::load_from_base_config_with_overrides(
            ConfigToml::default(),
            ConfigOverrides::default(),
            std::env::temp_dir(),
        )
        .expect("config");
        cfg.tui_custom_statusline = true;
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let app_event_tx = AppEventSender::new(tx);
        StatusLineOverlay::new(
            &cfg,
            FrameRequester::test_dummy(),
            app_event_tx,
            Some(Box::new(CustomStatusLineRenderer) as Box<dyn StatusLineRenderer>),
        )
        .expect("overlay")
    }

    #[test]
    fn layout_includes_margin_above_run_pill() {
        let overlay = overlay_for_tests();
        let area = Rect::new(0, 0, 80, 10);
        let layout = overlay.layout(area, false).expect("layout available");
        assert_eq!(
            layout.run_pill_area.y,
            area.y + StatusLineOverlay::MARGIN_ABOVE_PILL,
            "run pill should sit one row below the top margin"
        );
        assert_eq!(
            layout.pane_area.y,
            layout.run_pill_area.y
                + layout.run_pill_area.height
                + StatusLineOverlay::MARGIN_ABOVE_PANE,
            "pane area should start after the pill-to-pane margin"
        );
        assert_eq!(
            layout.status_line_area.y,
            area.y + area.height - 1,
            "status line stays anchored to bottom row"
        );
    }

    #[test]
    fn render_leaves_blank_margin_row() {
        let overlay = overlay_for_tests();
        let area = Rect::new(0, 0, 40, 10);
        let layout = overlay.layout(area, false).expect("layout available");
        let mut buf = Buffer::empty(area);
        overlay.render_run_pill(layout.run_pill_area, &mut buf);
        let margin_y = area.y;
        for x in area.x..area.x + area.width {
            let cell = &buf[(x, margin_y)];
            assert_eq!(
                cell.symbol(),
                " ",
                "expected transparent margin symbol at ({x},{margin_y})"
            );
            let style = cell.style();
            assert!(
                style.bg.is_none() || style.bg == Some(ratatui::style::Color::Reset),
                "expected default background at margin cell ({x},{margin_y}) but saw {:?}",
                style.bg
            );
            assert!(
                style.fg.is_none() || style.fg == Some(ratatui::style::Color::Reset),
                "expected default foreground at margin cell ({x},{margin_y}) but saw {:?}",
                style.fg
            );
            assert!(
                style.underline_color.is_none()
                    || style.underline_color == Some(ratatui::style::Color::Reset),
                "expected default underline color at margin cell ({x},{margin_y}) but saw {:?}",
                style.underline_color
            );
        }
    }
}
