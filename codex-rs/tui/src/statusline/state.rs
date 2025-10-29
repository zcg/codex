use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::status::format_directory_display;
use crate::tui::FrameRequester;
use codex_core::config::Config;
use codex_core::protocol::TokenUsage;
use codex_core::protocol::TokenUsageInfo;
use codex_core::protocol_config_types::ReasoningEffort;
use ratatui::text::Line;

use super::DEFAULT_STATUS_MESSAGE;
use super::DefaultStatusLineRenderer;
use super::RunTimerSnapshot;
use super::StatusLineContextSnapshot;
use super::StatusLineDevspaceSnapshot;
use super::StatusLineGitSnapshot;
use super::StatusLineModelSnapshot;
use super::StatusLineRenderer;
use super::StatusLineRunState;
use super::StatusLineSnapshot;
use super::StatusLineTokenSnapshot;
use super::TokenCountSnapshot;

#[derive(Debug)]
pub(crate) struct StatusLineState {
    cwd: PathBuf,
    frame_requester: FrameRequester,
    renderer: Box<dyn StatusLineRenderer>,
    snapshot: StatusLineSnapshot,
    run_timer: Option<RunTimer>,
    queued_messages: Vec<String>,
    esc_hint: bool,
    context_window_hint: Option<i64>,
}

impl StatusLineState {
    pub(crate) fn new(config: &Config, frame_requester: FrameRequester) -> Self {
        Self::with_renderer(
            config,
            frame_requester,
            Box::<DefaultStatusLineRenderer>::default(),
        )
    }

    pub(crate) fn with_renderer(
        config: &Config,
        frame_requester: FrameRequester,
        renderer: Box<dyn StatusLineRenderer>,
    ) -> Self {
        let cwd = config.cwd.clone();
        let mut state = Self {
            cwd: cwd.clone(),
            frame_requester,
            renderer,
            snapshot: StatusLineSnapshot::default(),
            run_timer: None,
            queued_messages: Vec::new(),
            esc_hint: true,
            context_window_hint: config.model_context_window,
        };
        state.set_working_directory(&cwd);
        state.set_idle_run_state(Instant::now());
        state
    }

    pub(crate) fn set_renderer(&mut self, renderer: Box<dyn StatusLineRenderer>) {
        self.renderer = renderer;
        self.request_redraw();
    }

    pub(crate) fn set_working_directory(&mut self, cwd: &Path) {
        self.cwd = cwd.to_path_buf();
        let display = format_directory_display(cwd, None);
        let basename = cwd
            .file_name()
            .map(|os| os.to_string_lossy().to_string())
            .filter(|s| !s.is_empty());
        self.snapshot.cwd_display = Some(display.clone());
        self.snapshot.cwd_basename = basename.clone();
        self.snapshot.cwd_fallback = basename.or(Some(display));
        self.request_redraw();
    }

    pub(crate) fn update_model(
        &mut self,
        label: impl Into<String>,
        effort: Option<ReasoningEffort>,
    ) {
        let detail = reasoning_detail(effort);
        self.snapshot.model = Some(StatusLineModelSnapshot {
            label: label.into(),
            detail,
        });
        self.request_redraw();
    }

    pub(crate) fn update_tokens(&mut self, info: Option<TokenUsageInfo>) {
        if let Some(info) = info {
            let context_window = info.model_context_window.or(self.context_window_hint);
            let (token_snapshot, context_snapshot) =
                token_snapshot_from_info(&info, context_window);
            self.snapshot.tokens = Some(token_snapshot);
            self.snapshot.context = context_snapshot;
        } else {
            self.snapshot.tokens = None;
            self.snapshot.context = None;
        }
        self.request_redraw();
    }

    pub(crate) fn set_git_info(&mut self, git: Option<StatusLineGitSnapshot>) {
        self.snapshot.git = git;
        self.request_redraw();
    }

    pub(crate) fn set_devspace(&mut self, devspace: Option<String>) {
        self.snapshot.environment.devspace =
            devspace.map(|name| StatusLineDevspaceSnapshot { name });
        self.request_redraw();
    }

    pub(crate) fn set_hostname(&mut self, hostname: Option<String>) {
        self.snapshot.environment.hostname = hostname;
        self.request_redraw();
    }

    pub(crate) fn set_interrupt_hint_visible(&mut self, visible: bool) {
        if self.esc_hint == visible {
            return;
        }
        self.esc_hint = visible;
        if let Some(run_state) = self.snapshot.run_state.as_mut() {
            run_state.show_interrupt_hint = visible;
        }
        self.request_redraw();
    }

    pub(crate) fn set_aws_profile(&mut self, profile: Option<String>) {
        self.snapshot.environment.aws_profile = profile;
        self.request_redraw();
    }

    pub(crate) fn set_kubernetes_context(&mut self, context: Option<String>) {
        self.snapshot.environment.kubernetes_context = context;
        self.request_redraw();
    }

    pub(crate) fn set_session_id(&mut self, session_id: Option<String>) {
        let _ = session_id;
    }

    pub(crate) fn set_queued_messages(&mut self, messages: Vec<String>) {
        self.queued_messages = messages;
        if let Some(run_state) = self.snapshot.run_state.as_mut() {
            run_state.queued_messages = self.queued_messages.clone();
        }
        self.request_redraw();
    }

    pub(crate) fn update_run_header(&mut self, header: &str) {
        if let Some(run_state) = self.snapshot.run_state.as_mut() {
            if run_state.label != header {
                run_state.label = header.to_string();
                run_state.status_changed_at = Instant::now();
                self.request_redraw();
            }
        } else {
            self.snapshot.run_state = Some(StatusLineRunState {
                label: header.to_string(),
                show_interrupt_hint: self.esc_hint,
                queued_messages: self.queued_messages.clone(),
                status_changed_at: Instant::now(),
                ..StatusLineRunState::default()
            });
            self.request_redraw();
        }
    }
    fn set_idle_run_state(&mut self, now: Instant) {
        let run_state = StatusLineRunState {
            label: DEFAULT_STATUS_MESSAGE.to_string(),
            spinner_started_at: None,
            timer: Some(RunTimerSnapshot {
                elapsed_running: Duration::ZERO,
                last_resume_at: None,
                is_paused: true,
            }),
            queued_messages: self.queued_messages.clone(),
            show_interrupt_hint: false,
            status_changed_at: now,
        };
        self.snapshot.run_state = Some(run_state);
        self.request_redraw();
    }

    pub(crate) fn start_task(&mut self, header: impl Into<String>) {
        let header = header.into();
        let now = Instant::now();
        match self.run_timer.as_mut() {
            Some(timer) => timer.resume(now),
            None => self.run_timer = Some(RunTimer::new(now)),
        }
        let mut run_state = self.snapshot.run_state.clone().unwrap_or_default();
        run_state.label = header;
        run_state.show_interrupt_hint = self.esc_hint;
        run_state.queued_messages = self.queued_messages.clone();
        run_state.status_changed_at = now;
        self.snapshot.run_state = Some(run_state);
        self.request_redraw();
    }

    pub(crate) fn complete_task(&mut self) {
        let now = Instant::now();
        if let Some(timer) = self.run_timer.as_mut() {
            timer.pause(now);
        }
        self.run_timer = None;
        self.set_idle_run_state(now);
        self.request_redraw();
    }

    pub(crate) fn resume_timer(&mut self) {
        if let Some(timer) = self.run_timer.as_mut() {
            timer.resume(Instant::now());
            self.request_redraw();
        }
    }

    pub(crate) fn elapsed_seconds(&self) -> Option<u64> {
        let timer = self.run_timer.as_ref()?;
        Some(timer.snapshot(Instant::now()).elapsed_running.as_secs())
    }

    pub(crate) fn snapshot_for_render(&self, now: Instant) -> StatusLineSnapshot {
        let mut snapshot = self.snapshot.clone();
        if let (Some(run_state), Some(timer)) =
            (snapshot.run_state.as_mut(), self.run_timer.as_ref())
        {
            run_state.timer = Some(timer.snapshot(now));
            run_state.spinner_started_at = Some(timer.spinner_started_at);
            run_state.queued_messages = self.queued_messages.clone();
            run_state.show_interrupt_hint = self.esc_hint;
        }
        let timer_active = self
            .run_timer
            .as_ref()
            .map(|timer| !timer.is_paused)
            .unwrap_or(false);
        if timer_active {
            self.frame_requester
                .schedule_frame_in(Duration::from_millis(48));
        }
        snapshot
    }

    pub(crate) fn render_line(&self, width: u16) -> Line<'static> {
        let now = Instant::now();
        let mut snapshot = self.snapshot_for_render(now);
        snapshot.run_state = None;
        self.renderer.render(&snapshot, width, now)
    }

    pub(crate) fn render_run_pill(&self, width: u16) -> Line<'static> {
        let now = Instant::now();
        let mut snapshot = self.snapshot_for_render(now);
        if snapshot.run_state.is_none() {
            snapshot.run_state = Some(StatusLineRunState {
                label: DEFAULT_STATUS_MESSAGE.to_string(),
                spinner_started_at: None,
                timer: Some(RunTimerSnapshot {
                    elapsed_running: Duration::ZERO,
                    last_resume_at: None,
                    is_paused: true,
                }),
                queued_messages: Vec::new(),
                show_interrupt_hint: false,
                status_changed_at: now,
            });
        }
        self.renderer.render_run_pill(&snapshot, width, now)
    }

    fn request_redraw(&self) {
        self.frame_requester.schedule_frame();
    }
}

#[derive(Debug)]
struct RunTimer {
    elapsed_running: Duration,
    last_resume_at: Option<Instant>,
    is_paused: bool,
    spinner_started_at: Instant,
}

impl RunTimer {
    fn new(now: Instant) -> Self {
        Self {
            elapsed_running: Duration::ZERO,
            last_resume_at: Some(now),
            is_paused: false,
            spinner_started_at: now,
        }
    }

    fn resume(&mut self, now: Instant) {
        if self.is_paused {
            self.last_resume_at = Some(now);
            self.is_paused = false;
        }
    }

    fn pause(&mut self, now: Instant) {
        if self.is_paused {
            return;
        }
        if let Some(last) = self.last_resume_at {
            self.elapsed_running += now.saturating_duration_since(last);
        }
        self.is_paused = true;
    }

    fn snapshot(&self, now: Instant) -> RunTimerSnapshot {
        let mut elapsed = self.elapsed_running;
        let mut last_resume = self.last_resume_at;
        if !self.is_paused {
            if let Some(last) = self.last_resume_at {
                elapsed += now.saturating_duration_since(last);
            }
            last_resume = Some(now);
        }
        RunTimerSnapshot {
            elapsed_running: elapsed,
            last_resume_at: last_resume,
            is_paused: self.is_paused,
        }
    }
}

fn reasoning_detail(effort: Option<ReasoningEffort>) -> Option<String> {
    match effort {
        Some(ReasoningEffort::High) => Some("high".to_string()),
        Some(ReasoningEffort::Low) => Some("low".to_string()),
        _ => None,
    }
}

fn token_snapshot_from_info(
    info: &TokenUsageInfo,
    context_window: Option<i64>,
) -> (StatusLineTokenSnapshot, Option<StatusLineContextSnapshot>) {
    let total = info.total_token_usage.clone();
    let last = info.last_token_usage.clone();

    let token_snapshot = StatusLineTokenSnapshot {
        total: TokenCountSnapshot {
            total_tokens: total.total_tokens,
            input_tokens: total.input_tokens,
            cached_input_tokens: total.cached_input_tokens,
            output_tokens: total.output_tokens,
            reasoning_output_tokens: total.reasoning_output_tokens,
        },
        last: Some(TokenCountSnapshot {
            total_tokens: last.total_tokens,
            input_tokens: last.input_tokens,
            cached_input_tokens: last.cached_input_tokens,
            output_tokens: last.output_tokens,
            reasoning_output_tokens: last.reasoning_output_tokens,
        }),
    };

    let context_snapshot = context_window.map(|window| {
        let percent = context_percent_remaining(&last, window);
        StatusLineContextSnapshot {
            percent_remaining: percent,
            tokens_in_context: last.tokens_in_context_window(),
            window,
        }
    });

    (token_snapshot, context_snapshot)
}

fn context_percent_remaining(last: &TokenUsage, context_window: i64) -> u8 {
    const BASELINE_TOKENS: i64 = 12_000;
    if context_window <= BASELINE_TOKENS {
        return 0;
    }
    let effective_window = context_window - BASELINE_TOKENS;
    if effective_window <= 0 {
        return 0;
    }
    let used = (last.tokens_in_context_window() - BASELINE_TOKENS).max(0);
    let remaining = (effective_window - used).max(0);
    let percent = (remaining * 100) / effective_window;
    percent.clamp(0, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_core::protocol::TokenUsage;

    #[test]
    fn context_snapshot_matches_status_values() {
        let window = 272_000;
        let info = TokenUsageInfo {
            total_token_usage: TokenUsage {
                total_tokens: 540_000,
                input_tokens: 420_000,
                cached_input_tokens: 160_000,
                output_tokens: 120_000,
                reasoning_output_tokens: 60_000,
            },
            last_token_usage: TokenUsage {
                total_tokens: 110_300,
                input_tokens: 74_000,
                cached_input_tokens: 18_000,
                output_tokens: 36_300,
                reasoning_output_tokens: 12_000,
            },
            model_context_window: Some(window),
        };

        let (_, context_snapshot) = token_snapshot_from_info(&info, info.model_context_window);
        let context = context_snapshot.expect("context snapshot");

        assert_eq!(context.window, window);
        assert_eq!(context.tokens_in_context, 98_300);
        assert_eq!(context.percent_remaining, 66);
    }

    #[test]
    fn run_timer_snapshot_advances_in_real_seconds() {
        let start = Instant::now();
        let timer = RunTimer::new(start);
        let first_tick = start + Duration::from_millis(1_200);
        let snapshot = timer.snapshot(first_tick);
        assert_eq!(snapshot.elapsed_running.as_millis(), 1_200);
        assert_eq!(snapshot.elapsed_at(first_tick).as_secs(), 1);

        let later = first_tick + Duration::from_millis(1_000);
        assert_eq!(snapshot.elapsed_at(later).as_secs(), 2);
    }
}
