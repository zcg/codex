use std::env;
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::statusline::StatusLineGitSnapshot;
use crate::statusline::StatusLineRenderer;
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
}

impl StatusLineOverlay {
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

    pub(crate) fn update_git(&mut self, git: Option<StatusLineGitSnapshot>) {
        self.state.set_git_info(git);
    }

    pub(crate) fn update_kube_context(&mut self, context: Option<String>) {
        self.state.set_kubernetes_context(context);
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

    pub(crate) fn layout(
        &self,
        bottom_pane_area: Rect,
        has_active_view: bool,
    ) -> Option<StatusLineLayout> {
        if has_active_view || bottom_pane_area.height < 3 {
            return None;
        }
        let run_pill_area = Rect {
            x: bottom_pane_area.x,
            y: bottom_pane_area.y,
            width: bottom_pane_area.width,
            height: 1,
        };
        let status_line_area = Rect {
            x: bottom_pane_area.x,
            y: bottom_pane_area.y + bottom_pane_area.height - 1,
            width: bottom_pane_area.width,
            height: 1,
        };
        let pane_area = Rect {
            x: bottom_pane_area.x,
            y: bottom_pane_area.y + 1,
            width: bottom_pane_area.width,
            height: bottom_pane_area.height.saturating_sub(2),
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
