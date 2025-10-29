use std::borrow::Cow;
use std::time::Duration;
use std::time::Instant;

use crate::exec_cell::spinner;
use crate::key_hint;
use crate::status::line_display_width;
use crate::status::truncate_line_to_width;
use crossterm::event::KeyCode;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

mod palette;
pub(crate) mod skins;
pub(crate) mod state;

pub(crate) use skins::CustomStatusLineRenderer;
pub(crate) use state::StatusLineState;

use palette::BASE;
use palette::GREEN;
use palette::GREEN_LIGHT;
use palette::LAVENDER;
use palette::MAUVE;
use palette::PEACH;
use palette::PEACH_LIGHT;
use palette::RED;
use palette::RED_LIGHT;
use palette::ROSEWATER;
use palette::SKY;
use palette::SUBTEXT0;
use palette::TEAL;
use palette::YELLOW;
use palette::YELLOW_LIGHT;
use palette::queue_preview_style;

const LEFT_CURVE: &str = "";
const RIGHT_CURVE: &str = "";
const LEFT_CHEVRON: &str = "";
const RIGHT_CHEVRON: &str = "";
const GIT_ICON: &str = " ";
const AWS_ICON: &str = " ";
const K8S_ICON: &str = "☸ ";
const HOSTNAME_ICON: &str = " ";
const CONTEXT_ICON: &str = " ";
const PROGRESS_LEFT_EMPTY: &str = "";
const PROGRESS_MID_EMPTY: &str = "";
const PROGRESS_RIGHT_EMPTY: &str = "";
const PROGRESS_LEFT_FULL: &str = "";
const PROGRESS_MID_FULL: &str = "";
const PROGRESS_RIGHT_FULL: &str = "";
const MODEL_ICONS: &[char] = &['󰚩', '󱚝', '󱚟', '󱚡', '󱚣', '󱚥'];
const DEVSPACE_ICONS: &[&str] = &["󰠖 ", "󰠶 ", "󰋩 ", "󰚌 "];
const CONTEXT_PADDING: usize = 4;
const DEFAULT_STATUS_MESSAGE: &str = "Ready when you are";

pub(crate) trait StatusLineRenderer: std::fmt::Debug + Send + Sync {
    fn render(&self, snapshot: &StatusLineSnapshot, width: u16, now: Instant) -> Line<'static>;

    fn render_run_pill(
        &self,
        snapshot: &StatusLineSnapshot,
        width: u16,
        now: Instant,
    ) -> Line<'static>;
}

fn span<S>(text: S, style: Style) -> Span<'static>
where
    S: Into<Cow<'static, str>>,
{
    Span::styled(text.into(), style)
}

fn accent_fg(color: Color) -> Style {
    Style::default().fg(color)
}

fn segment_fill(color: Color) -> Style {
    Style::default().fg(BASE).bg(color)
}

fn status_spinner(start_time: Option<Instant>) -> Span<'static> {
    let mut span = spinner(start_time);
    if span.content.as_ref() == "•" {
        return "◦".dim();
    }
    span.style = span.style.add_modifier(Modifier::DIM);
    span
}

fn bridge_left(prev: Color, next: Color) -> Style {
    Style::default().fg(prev).bg(next)
}

fn bridge_right(prev: Color, next: Color) -> Style {
    Style::default().fg(next).bg(prev)
}

fn dim_text() -> Style {
    Style::default().fg(SUBTEXT0).add_modifier(Modifier::DIM)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineSnapshot {
    pub cwd_display: Option<String>,
    pub cwd_basename: Option<String>,
    pub cwd_fallback: Option<String>,
    pub model: Option<StatusLineModelSnapshot>,
    pub tokens: Option<StatusLineTokenSnapshot>,
    pub context: Option<StatusLineContextSnapshot>,
    pub run_state: Option<StatusLineRunState>,
    pub git: Option<StatusLineGitSnapshot>,
    pub environment: StatusLineEnvironmentSnapshot,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineEnvironmentSnapshot {
    pub devspace: Option<StatusLineDevspaceSnapshot>,
    pub hostname: Option<String>,
    pub aws_profile: Option<String>,
    pub kubernetes_context: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineModelSnapshot {
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineTokenSnapshot {
    pub total: TokenCountSnapshot,
    #[allow(dead_code)]
    pub last: Option<TokenCountSnapshot>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub(crate) struct TokenCountSnapshot {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

impl TokenCountSnapshot {
    fn blended_total(&self) -> i64 {
        self.input_without_cache() + self.output_tokens
    }

    fn input_without_cache(&self) -> i64 {
        self.input_tokens.saturating_sub(self.cached_input_tokens)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineContextSnapshot {
    pub percent_remaining: u8,
    pub tokens_in_context: i64,
    pub window: i64,
}

impl StatusLineContextSnapshot {
    #[allow(dead_code)]
    fn percent_used(&self) -> u8 {
        100u8.saturating_sub(self.percent_remaining)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineGitSnapshot {
    pub branch: Option<String>,
    pub dirty: bool,
    pub ahead: Option<i64>,
    pub behind: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StatusLineDevspaceSnapshot {
    pub name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusLineRunState {
    pub label: String,
    pub spinner_started_at: Option<Instant>,
    pub timer: Option<RunTimerSnapshot>,
    pub queued_messages: Vec<String>,
    pub show_interrupt_hint: bool,
    pub status_changed_at: Instant,
}

impl Default for StatusLineRunState {
    fn default() -> Self {
        Self {
            label: String::new(),
            spinner_started_at: None,
            timer: None,
            queued_messages: Vec::new(),
            show_interrupt_hint: false,
            status_changed_at: Instant::now(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RunTimerSnapshot {
    pub elapsed_running: Duration,
    pub last_resume_at: Option<Instant>,
    pub is_paused: bool,
}

impl RunTimerSnapshot {
    fn elapsed_at(&self, now: Instant) -> Duration {
        if self.is_paused {
            return self.elapsed_running;
        }
        let Some(last_resume) = self.last_resume_at else {
            return self.elapsed_running;
        };
        self.elapsed_running
            .saturating_add(now.saturating_duration_since(last_resume))
    }
}

pub(crate) fn format_elapsed_compact(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        let minutes = elapsed_secs / 60;
        let seconds = elapsed_secs % 60;
        return format!("{minutes}m {seconds:02}s");
    }
    let hours = elapsed_secs / 3600;
    let minutes = (elapsed_secs % 3600) / 60;
    let seconds = elapsed_secs % 60;
    format!("{hours}h {minutes:02}m {seconds:02}s")
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PathVariant {
    Full,
    Basename,
    Hidden,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TokenVariant {
    Full,
    Compact,
    Minimal,
    Hidden,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ContextVariant {
    Bar,
    Compact,
    Hidden,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GitVariant {
    BranchWithStatus,
    BranchOnly,
    Hidden,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum RunLabelVariant {
    Full,
    Short,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DegradeOp {
    DropDevspace,
    DropKubernetes,
    DropAwsProfile,
    DropHostname,
    DropQueuePreview,
    HideInterruptHint,
    HideRunTimer,
    ShortenRunLabel,
    HideRunLabel,
    SimplifyGit,
    SimplifyTokens,
    MinimalTokens,
    HideTokens,
    SimplifyContext,
    HideContext,
    BasenamePath,
    HidePath,
    HideGit,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct EnvironmentInclusion {
    hostname: bool,
    aws_profile: bool,
    kubernetes: bool,
    devspace: bool,
}

impl EnvironmentInclusion {
    fn new(snapshot: &StatusLineEnvironmentSnapshot) -> Self {
        Self {
            hostname: snapshot.hostname.is_some(),
            aws_profile: snapshot.aws_profile.is_some(),
            kubernetes: snapshot.kubernetes_context.is_some(),
            devspace: snapshot.devspace.is_some(),
        }
    }

    fn empty() -> Self {
        Self {
            hostname: false,
            aws_profile: false,
            kubernetes: false,
            devspace: false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DefaultStatusLineRenderer;

impl StatusLineRenderer for DefaultStatusLineRenderer {
    fn render(&self, snapshot: &StatusLineSnapshot, width: u16, now: Instant) -> Line<'static> {
        render_status_line(snapshot, width, now)
    }

    fn render_run_pill(
        &self,
        snapshot: &StatusLineSnapshot,
        width: u16,
        now: Instant,
    ) -> Line<'static> {
        render_status_run_pill(snapshot, width, now)
    }
}

pub(crate) fn render_status_line(
    snapshot: &StatusLineSnapshot,
    width: u16,
    now: Instant,
) -> Line<'static> {
    let mut model = RenderModel::new(snapshot, now);
    let target_width = width as usize;

    loop {
        if let Some(line) = model.try_render_line(target_width) {
            return line;
        }
        if !model.apply_next_degrade() {
            let fallback = model.fallback_line();
            return truncate_line_to_width(fallback, target_width);
        }
    }
}

pub(crate) fn render_status_run_pill(
    snapshot: &StatusLineSnapshot,
    width: u16,
    now: Instant,
) -> Line<'static> {
    let target_width = width as usize;
    if target_width == 0 {
        return Line::from(Vec::<Span<'static>>::new());
    }

    let mut model = RenderModel::new(snapshot, now);
    model.path_variant = PathVariant::Hidden;
    model.token_variant = TokenVariant::Hidden;
    model.context_variant = ContextVariant::Hidden;
    model.git_variant = GitVariant::Hidden;
    model.env = EnvironmentInclusion::empty();
    model.include_queue_preview = true;
    model.show_interrupt_hint = false;

    let mut attempts = 0usize;
    loop {
        let segments = model.run_state_segments(snapshot.run_state.as_ref());
        let spans = capsule_spans(segments);
        let mut line = Line::from(spans.clone());
        let display_width = line_display_width(&line);
        if display_width <= target_width {
            if display_width < target_width {
                let padding = " ".repeat(target_width - display_width);
                line.spans.push(Span::raw(padding));
            }
            return line;
        }
        if !degrade_run_capsule(&mut model) {
            return truncate_line_to_width(Line::from(spans), target_width);
        }
        attempts += 1;
        if attempts > 8 {
            return truncate_line_to_width(Line::from(spans), target_width);
        }
    }
}

struct RenderModel<'a> {
    snapshot: &'a StatusLineSnapshot,
    now: Instant,
    path_variant: PathVariant,
    token_variant: TokenVariant,
    context_variant: ContextVariant,
    git_variant: GitVariant,
    include_queue_preview: bool,
    show_interrupt_hint: bool,
    show_run_timer: bool,
    show_run_label: bool,
    run_label_variant: RunLabelVariant,
    env: EnvironmentInclusion,
    degrade_cursor: usize,
}

impl<'a> RenderModel<'a> {
    fn new(snapshot: &'a StatusLineSnapshot, now: Instant) -> Self {
        let run_state = snapshot.run_state.as_ref();
        let has_timer = run_state.and_then(|state| state.timer.as_ref()).is_some();
        let show_hint = run_state
            .map(|state| state.show_interrupt_hint)
            .unwrap_or(false);
        Self {
            snapshot,
            now,
            path_variant: PathVariant::Full,
            token_variant: TokenVariant::Hidden,
            context_variant: ContextVariant::Bar,
            git_variant: GitVariant::BranchWithStatus,
            include_queue_preview: true,
            show_interrupt_hint: show_hint,
            show_run_timer: has_timer,
            show_run_label: run_state.is_some(),
            run_label_variant: RunLabelVariant::Full,
            env: EnvironmentInclusion::new(&snapshot.environment),
            degrade_cursor: 0,
        }
    }

    fn fallback_line(&self) -> Line<'static> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(path) = self
            .snapshot
            .cwd_fallback
            .as_ref()
            .or(self.snapshot.cwd_display.as_ref())
        {
            parts.push(path.clone());
        }
        if let Some(model) = self.snapshot.model.as_ref() {
            parts.push(model.label.clone());
        }
        if let Some(git) = self.snapshot.git.as_ref()
            && let Some(branch) = git.branch.as_ref()
        {
            let mut branch_text = branch.clone();
            if git.dirty {
                branch_text.push('*');
            }
            parts.push(branch_text);
        }
        if parts.is_empty() {
            return Line::from("codex");
        }
        Line::from(parts.join(" | "))
    }

    fn apply_next_degrade(&mut self) -> bool {
        const DEGRADE_ORDER: &[DegradeOp] = &[
            DegradeOp::DropQueuePreview,
            DegradeOp::HideInterruptHint,
            DegradeOp::HideRunTimer,
            DegradeOp::ShortenRunLabel,
            DegradeOp::HideRunLabel,
            DegradeOp::BasenamePath,
            DegradeOp::SimplifyTokens,
            DegradeOp::MinimalTokens,
            DegradeOp::HideTokens,
            DegradeOp::SimplifyContext,
            DegradeOp::HideContext,
            DegradeOp::SimplifyGit,
            DegradeOp::HideGit,
            DegradeOp::DropDevspace,
            DegradeOp::DropKubernetes,
            DegradeOp::DropAwsProfile,
            DegradeOp::DropHostname,
            DegradeOp::HidePath,
        ];

        while self.degrade_cursor < DEGRADE_ORDER.len() {
            let op = DEGRADE_ORDER[self.degrade_cursor];
            self.degrade_cursor += 1;
            if self.apply_degrade(op) {
                return true;
            }
        }
        false
    }

    fn apply_degrade(&mut self, op: DegradeOp) -> bool {
        match op {
            DegradeOp::DropDevspace if self.env.devspace => {
                self.env.devspace = false;
                true
            }
            DegradeOp::DropKubernetes if self.env.kubernetes => {
                self.env.kubernetes = false;
                true
            }
            DegradeOp::DropAwsProfile if self.env.aws_profile => {
                self.env.aws_profile = false;
                true
            }
            DegradeOp::DropHostname if self.env.hostname => {
                self.env.hostname = false;
                true
            }
            DegradeOp::DropQueuePreview if self.include_queue_preview => {
                self.include_queue_preview = false;
                true
            }
            DegradeOp::HideInterruptHint if self.show_interrupt_hint => {
                self.show_interrupt_hint = false;
                true
            }
            DegradeOp::HideRunTimer if self.show_run_timer => {
                self.show_run_timer = false;
                true
            }
            DegradeOp::ShortenRunLabel
                if self.show_run_label && self.run_label_variant == RunLabelVariant::Full =>
            {
                self.run_label_variant = RunLabelVariant::Short;
                true
            }
            DegradeOp::HideRunLabel if self.show_run_label => {
                self.show_run_label = false;
                true
            }
            DegradeOp::SimplifyGit if self.git_variant == GitVariant::BranchWithStatus => {
                self.git_variant = GitVariant::BranchOnly;
                true
            }
            DegradeOp::SimplifyTokens if self.token_variant == TokenVariant::Full => {
                self.token_variant = TokenVariant::Compact;
                true
            }
            DegradeOp::MinimalTokens if self.token_variant == TokenVariant::Compact => {
                self.token_variant = TokenVariant::Minimal;
                true
            }
            DegradeOp::HideTokens if self.token_variant != TokenVariant::Hidden => {
                self.token_variant = TokenVariant::Hidden;
                true
            }
            DegradeOp::SimplifyContext if self.context_variant == ContextVariant::Bar => {
                self.context_variant = ContextVariant::Compact;
                true
            }
            DegradeOp::HideContext if self.context_variant != ContextVariant::Hidden => {
                self.context_variant = ContextVariant::Hidden;
                true
            }
            DegradeOp::BasenamePath if self.path_variant == PathVariant::Full => {
                self.path_variant = PathVariant::Basename;
                true
            }
            DegradeOp::HidePath if self.path_variant != PathVariant::Hidden => {
                self.path_variant = PathVariant::Hidden;
                true
            }
            DegradeOp::HideGit if self.git_variant != GitVariant::Hidden => {
                self.git_variant = GitVariant::Hidden;
                true
            }
            _ => false,
        }
    }

    fn try_render_line(&self, target_width: usize) -> Option<Line<'static>> {
        let left_spans = self.render_left_segments()?;
        let right_spans = self.render_right_segments()?;

        let left_line = Line::from(left_spans.clone());
        let right_line = Line::from(right_spans.clone());
        let left_width = line_display_width(&left_line);
        let right_width = line_display_width(&right_line);
        let available_for_middle = target_width.checked_sub(left_width + right_width)?;
        let (middle_spans, _middle_width) = self.render_middle(available_for_middle)?;

        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.extend(left_spans);
        spans.extend(middle_spans);
        spans.extend(right_spans);

        let line = Line::from(spans);
        debug_assert!(line_display_width(&line) <= target_width);
        if line_display_width(&line) == target_width {
            Some(line)
        } else {
            None
        }
    }

    fn render_left_segments(&self) -> Option<Vec<Span<'static>>> {
        let segments = self.collect_left_segments();
        if segments.is_empty() {
            return Some(Vec::new());
        }

        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut previous: Option<Color> = None;
        for segment in segments {
            let accent = segment.accent;
            if let Some(prev) = previous {
                spans.push(span(LEFT_CHEVRON, bridge_left(prev, accent)));
            } else {
                spans.push(span(LEFT_CURVE, accent_fg(accent)));
            }
            spans.extend(segment.into_padded_spans());
            previous = Some(accent);
        }
        if let Some(last) = previous {
            spans.push(span(LEFT_CHEVRON, accent_fg(last)));
        }
        Some(spans)
    }

    fn collect_left_segments(&self) -> Vec<PowerlineSegment> {
        let mut segments: Vec<PowerlineSegment> = Vec::new();
        segments.extend(self.run_state_segments(self.snapshot.run_state.as_ref()));
        if let Some(segment) = self.path_segment() {
            segments.push(segment);
        }
        if let Some(segment) = self.model_segment() {
            segments.push(segment);
        }
        segments
    }

    fn path_segment(&self) -> Option<PowerlineSegment> {
        let text = self.path_text()?;
        Some(PowerlineSegment::text(LAVENDER, text))
    }

    fn path_text(&self) -> Option<String> {
        match self.path_variant {
            PathVariant::Hidden => None,
            PathVariant::Full => self
                .snapshot
                .cwd_display
                .as_ref()
                .map(|path| truncate_graphemes(path, 40)),
            PathVariant::Basename => self
                .snapshot
                .cwd_basename
                .clone()
                .or_else(|| self.snapshot.cwd_fallback.clone())
                .map(|path| truncate_graphemes(&path, 28)),
        }
    }

    fn model_segment(&self) -> Option<PowerlineSegment> {
        let model = self.snapshot.model.as_ref()?;
        let mut spans: Vec<Span<'static>> = Vec::new();
        let icon = select_model_icon(&model.label).to_string();
        spans.push(icon.into());
        if !model.label.is_empty() {
            spans.push(" ".into());
            spans.push(Span::styled(
                model.label.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(detail) = model.detail.as_ref() {
            spans.push(" ".into());
            spans.push(Span::styled(
                detail.clone(),
                Style::default().fg(BASE).add_modifier(Modifier::ITALIC),
            ));
        }
        if let Some(tokens) = self.format_token_summary() {
            spans.push(" ".into());
            spans.push(Span::styled(tokens, dim_text()));
        }
        Some(PowerlineSegment::from_spans(SKY, spans))
    }

    fn format_token_summary(&self) -> Option<String> {
        let tokens = self.snapshot.tokens.as_ref()?;
        match self.token_variant {
            TokenVariant::Hidden => None,
            TokenVariant::Minimal => Some(format!(
                "Σ{}",
                format_token_count(tokens.total.blended_total())
            )),
            TokenVariant::Compact | TokenVariant::Full => {
                let mut parts = Vec::new();
                parts.push(format!(
                    "Σ{}",
                    format_token_count(tokens.total.blended_total())
                ));
                parts.push(format!(
                    "↑{}",
                    format_token_count(tokens.total.input_without_cache())
                ));
                if tokens.total.cached_input_tokens > 0 {
                    parts.push(format!(
                        "↺{}",
                        format_token_count(tokens.total.cached_input_tokens)
                    ));
                }
                parts.push(format!(
                    "↓{}",
                    format_token_count(tokens.total.output_tokens)
                ));
                Some(parts.join(" "))
            }
        }
    }

    fn run_state_segments(&self, state: Option<&StatusLineRunState>) -> Vec<PowerlineSegment> {
        let Some(state) = state else {
            return Vec::new();
        };

        let mut segments: Vec<PowerlineSegment> = Vec::new();

        let mut capsule_spans: Vec<Span<'static>> = Vec::new();
        if self.show_run_timer {
            let elapsed_secs = state
                .timer
                .as_ref()
                .map(|timer| timer.elapsed_at(self.now).as_secs())
                .unwrap_or(0);
            capsule_spans.push(Span::raw(format!(
                "󰔟 {}",
                format_elapsed_compact(elapsed_secs)
            )));
        }

        if self.show_run_label {
            if !capsule_spans.is_empty() {
                capsule_spans.push(" ".into());
            }
            capsule_spans.push(status_spinner(state.spinner_started_at));
            let label = self.run_label_text(state);
            if !label.trim().is_empty() {
                capsule_spans.push(" ".into());
                capsule_spans.push(Span::raw(label.trim().to_string()));
            }
        }

        if capsule_spans.is_empty() {
            let accent = self.status_capsule_accent(state);
            segments.push(PowerlineSegment::from_spans(
                accent,
                vec![status_spinner(state.spinner_started_at)],
            ));
        } else {
            let accent = self.status_capsule_accent(state);
            segments.push(PowerlineSegment::from_spans(accent, capsule_spans));
        }

        if self.include_queue_preview && !state.queued_messages.is_empty() {
            let (preview, extra) = queue_preview(&state.queued_messages);
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push("next:".dim());
            spans.push(" ".into());
            spans.push(Span::styled(preview, queue_preview_style()));
            if extra > 0 {
                spans.push(" ".into());
                spans.push(Span::styled(format!("(+{extra})"), queue_preview_style()));
            }
            spans.push(" ".into());
            spans.push(key_hint::alt(KeyCode::Up).into());
            spans.push(" edit".dim());
            segments.push(PowerlineSegment::from_spans(MAUVE, spans));
        }

        segments
    }
    fn run_label_text(&self, state: &StatusLineRunState) -> String {
        let mut label = match self.run_label_variant {
            RunLabelVariant::Full => state.label.clone(),
            RunLabelVariant::Short => state
                .label
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string(),
        };
        if label.trim().is_empty() {
            DEFAULT_STATUS_MESSAGE.to_string()
        } else {
            if label.starts_with(' ') || label.ends_with(' ') {
                label = label.trim().to_string();
            }
            label
        }
    }

    fn status_capsule_accent(&self, state: &StatusLineRunState) -> Color {
        if state
            .timer
            .as_ref()
            .map(|timer| !timer.is_paused)
            .unwrap_or(false)
        {
            GREEN
        } else {
            MAUVE
        }
    }

    fn render_right_segments(&self) -> Option<Vec<Span<'static>>> {
        let segments = self.collect_right_segments();
        if segments.is_empty() {
            return Some(Vec::new());
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut previous_accent: Option<Color> = None;
        for segment in segments {
            let accent = segment.accent;
            if let Some(prev) = previous_accent {
                spans.push(span(RIGHT_CHEVRON, bridge_right(prev, accent)));
            } else {
                spans.push(span(RIGHT_CHEVRON, accent_fg(accent)));
            }
            spans.extend(segment.into_padded_spans());
            previous_accent = Some(accent);
        }
        if let Some(last) = previous_accent {
            spans.push(span(RIGHT_CURVE, accent_fg(last)));
        }
        Some(spans)
    }

    fn collect_right_segments(&self) -> Vec<PowerlineSegment> {
        let mut segments: Vec<PowerlineSegment> = Vec::new();
        if self.env.devspace
            && let Some(devspace) = self.snapshot.environment.devspace.as_ref()
        {
            let icon = devspace_icon(&devspace.name);
            let text = format!("{icon}{}", truncate_graphemes(&devspace.name, 16));
            if !text.trim().is_empty() {
                segments.push(PowerlineSegment::text(MAUVE, text));
            }
        }
        if self.env.hostname
            && let Some(host) = self.snapshot.environment.hostname.as_ref()
        {
            let text = format!("{HOSTNAME_ICON}{}", truncate_graphemes(host, 20));
            segments.push(PowerlineSegment::text(ROSEWATER, text));
        }
        if let Some(git) = self.build_git_segment() {
            segments.push(git);
        }
        if self.env.aws_profile
            && let Some(profile) = self.snapshot.environment.aws_profile.as_ref()
        {
            let trimmed = profile.trim_start_matches("export AWS_PROFILE=");
            let text = format!("{AWS_ICON}{}", truncate_graphemes(trimmed, 16));
            segments.push(PowerlineSegment::text(PEACH, text));
        }
        if self.env.kubernetes
            && let Some(ctx) = self.snapshot.environment.kubernetes_context.as_ref()
        {
            let trimmed = ctx
                .trim_start_matches("arn:aws:eks:")
                .trim_start_matches("gke_");
            let text = format!("{K8S_ICON}{}", truncate_graphemes(trimmed, 18));
            segments.push(PowerlineSegment::text(TEAL, text));
        }
        segments
    }

    fn build_git_segment(&self) -> Option<PowerlineSegment> {
        let git = self.snapshot.git.as_ref()?;
        let branch = git.branch.as_ref()?;
        let mut text = format!("{GIT_ICON}{branch}");
        if git.dirty {
            text.push('*');
        }
        if let Some(ahead) = git.ahead.filter(|value| *value > 0) {
            text.push_str(&format!(" ↑{ahead}"));
        }
        if let Some(behind) = git.behind.filter(|value| *value > 0) {
            text.push_str(&format!(" ↓{behind}"));
        }
        Some(PowerlineSegment::text(SKY, truncate_graphemes(&text, 24)))
    }

    fn render_middle(&self, width: usize) -> Option<(Vec<Span<'static>>, usize)> {
        if width == 0 {
            return Some((Vec::new(), 0));
        }
        match self.context_variant {
            ContextVariant::Hidden => {
                Some((vec![span(" ".repeat(width), Style::default())], width))
            }
            ContextVariant::Compact => self
                .render_context_compact(width)
                .map(|spans| (spans, width)),
            ContextVariant::Bar => self.render_context_bar(width).map(|spans| (spans, width)),
        }
    }

    fn render_context_compact(&self, width: usize) -> Option<Vec<Span<'static>>> {
        let context = self.snapshot.context.as_ref()?;
        let percentage = if context.window > 0 {
            (context.tokens_in_context as f64 / context.window as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let text = format!("{CONTEXT_ICON} {percentage:.1}%");
        let display_width = UnicodeWidthStr::width(text.as_str());
        if display_width > width {
            return None;
        }
        let mut spans = vec![span(text, dim_text())];
        if width > display_width {
            spans.push(span(" ".repeat(width - display_width), Style::default()));
        }
        Some(spans)
    }

    fn render_context_bar(&self, width: usize) -> Option<Vec<Span<'static>>> {
        let context = self.snapshot.context.as_ref()?;
        if width <= CONTEXT_PADDING * 2 + 2 {
            return Some(vec![span(" ".repeat(width), Style::default())]);
        }

        let available = width.saturating_sub(CONTEXT_PADDING * 2);
        let percent_remaining = f64::from(context.percent_remaining);
        let percent_used = (100.0 - percent_remaining).clamp(0.0, 100.0);

        let label = format!("{CONTEXT_ICON}Context ");
        let percent_text = format!(" {percent_remaining:.1}% left");
        let label_width = UnicodeWidthStr::width(label.as_str());
        let percent_width = UnicodeWidthStr::width(percent_text.as_str());
        let curves_width = 2usize;
        let text_width = label_width + percent_width + curves_width;
        if available <= text_width {
            return Some(vec![span(" ".repeat(width), Style::default())]);
        }

        let fill_width = available - text_width;
        if fill_width < 4 {
            return Some(vec![span(" ".repeat(width), Style::default())]);
        }

        let filled = ((fill_width as f64) * (percent_used / 100.0)).round() as usize;
        let (accent, light_bg) = context_bar_colors(percent_used);

        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(span(" ".repeat(CONTEXT_PADDING), Style::default()));
        spans.push(span(LEFT_CURVE, accent_fg(accent)));
        spans.push(span(label, segment_fill(accent)));
        spans.extend(build_progress_bar(fill_width, filled, accent, light_bg));
        spans.push(span(percent_text, segment_fill(accent)));
        spans.push(span(RIGHT_CURVE, accent_fg(accent)));
        spans.push(span(" ".repeat(CONTEXT_PADDING), Style::default()));
        Some(spans)
    }
}

fn degrade_run_capsule(model: &mut RenderModel<'_>) -> bool {
    const OPS: &[DegradeOp] = &[DegradeOp::DropQueuePreview, DegradeOp::HideRunTimer];
    for op in OPS {
        if model.apply_degrade(*op) {
            return true;
        }
    }
    false
}

struct PowerlineSegment {
    accent: Color,
    spans: Vec<Span<'static>>,
}

impl PowerlineSegment {
    fn text(accent: Color, text: String) -> Self {
        Self {
            accent,
            spans: vec![Span::from(text)],
        }
    }

    fn from_spans(accent: Color, spans: Vec<Span<'static>>) -> Self {
        Self { accent, spans }
    }

    fn into_padded_spans(self) -> Vec<Span<'static>> {
        let mut output = Vec::with_capacity(self.spans.len() + 2);
        output.push(pad_segment_span(self.accent));
        for mut span in self.spans {
            apply_segment_fill(&mut span, self.accent);
            output.push(span);
        }
        output.push(pad_segment_span(self.accent));
        output
    }
}

fn capsule_spans(segments: Vec<PowerlineSegment>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut iter = segments.into_iter();
    if let Some(first) = iter.next() {
        let mut previous_accent = first.accent;
        spans.push(span(LEFT_CURVE, accent_fg(previous_accent)));
        spans.extend(first.into_padded_spans());
        for segment in iter {
            let accent = segment.accent;
            spans.push(span(LEFT_CHEVRON, bridge_left(previous_accent, accent)));
            spans.extend(segment.into_padded_spans());
            previous_accent = accent;
        }
        spans.push(span(RIGHT_CURVE, accent_fg(previous_accent)));
    }
    spans
}

fn pad_segment_span(accent: Color) -> Span<'static> {
    let mut span: Span<'static> = " ".into();
    apply_segment_fill(&mut span, accent);
    span
}

fn apply_segment_fill(span: &mut Span<'static>, accent: Color) {
    span.style = span.style.bg(accent);
    if span.style.fg.is_none() {
        span.style = span.style.fg(BASE);
    }
}

fn truncate_graphemes(text: &str, max_graphemes: usize) -> String {
    if max_graphemes == 0 {
        return String::new();
    }
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    if graphemes.len() <= max_graphemes {
        return text.to_string();
    }
    if max_graphemes == 1 {
        return "…".to_string();
    }
    let mut truncated = graphemes[..max_graphemes - 1].concat();
    truncated.push('…');
    truncated
}

fn queue_preview(commands: &[String]) -> (String, usize) {
    if commands.is_empty() {
        return (String::new(), 0);
    }
    let raw = commands
        .first()
        .map(|value| value.lines().next().unwrap_or(""))
        .unwrap_or("");
    let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut preview = if normalized.is_empty() {
        String::new()
    } else {
        normalized
    };

    const MAX_WIDTH: usize = 32;
    let width = UnicodeWidthStr::width(preview.as_str());
    if width > MAX_WIDTH {
        let mut truncated = String::new();
        let mut used = 0usize;
        for grapheme in preview.graphemes(true) {
            let g_width = UnicodeWidthStr::width(grapheme);
            if used + g_width > MAX_WIDTH.saturating_sub(1) {
                break;
            }
            truncated.push_str(grapheme);
            used += g_width;
        }
        truncated.push('…');
        preview = truncated;
    }

    (preview, commands.len().saturating_sub(1))
}

fn build_progress_bar(
    fill_width: usize,
    filled_width: usize,
    accent: Color,
    light_bg: Color,
) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(fill_width);
    for position in 0..fill_width {
        let glyph = select_progress_char(position, fill_width, filled_width);
        spans.push(span(glyph, Style::default().fg(accent).bg(light_bg)));
    }
    spans
}

fn select_progress_char(position: usize, fill_width: usize, filled_width: usize) -> &'static str {
    if position == 0 {
        if filled_width > 0 {
            PROGRESS_LEFT_FULL
        } else {
            PROGRESS_LEFT_EMPTY
        }
    } else if position == fill_width.saturating_sub(1) {
        if position < filled_width {
            PROGRESS_RIGHT_FULL
        } else {
            PROGRESS_RIGHT_EMPTY
        }
    } else if position < filled_width {
        PROGRESS_MID_FULL
    } else {
        PROGRESS_MID_EMPTY
    }
}

fn format_token_count(value: i64) -> String {
    const MILLION: f64 = 1_000_000.0;
    const THOUSAND: f64 = 1_000.0;
    let clamped = value.max(0);
    let value_f64 = clamped as f64;
    if value_f64 >= MILLION {
        let mut formatted = format!("{:.1}M", value_f64 / MILLION);
        if formatted.ends_with(".0M") {
            formatted.truncate(formatted.len() - 3);
            formatted.push('M');
        }
        formatted
    } else if value_f64 >= THOUSAND {
        let mut formatted = format!("{:.1}k", value_f64 / THOUSAND);
        if formatted.ends_with(".0k") {
            formatted.truncate(formatted.len() - 3);
            formatted.push('k');
        }
        formatted
    } else {
        clamped.to_string()
    }
}

fn select_model_icon(model: &str) -> char {
    match MODEL_ICONS {
        [] => '󰚩',
        icons => {
            if model.is_empty() {
                return icons[0];
            }
            let mut hash: u64 = 0;
            for byte in model.as_bytes() {
                hash = hash.wrapping_mul(131).wrapping_add(*byte as u64);
            }
            icons[(hash as usize) % icons.len()]
        }
    }
}

fn devspace_icon(name: &str) -> &'static str {
    match DEVSPACE_ICONS {
        [] => "󰠖 ",
        icons => {
            let mut hash: u64 = 0;
            for byte in name.as_bytes() {
                hash = hash.wrapping_mul(167).wrapping_add(*byte as u64);
            }
            icons[(hash as usize) % icons.len()]
        }
    }
}

fn context_bar_colors(percent_used: f64) -> (Color, Color) {
    match percent_used {
        value if value <= 60.0 => (GREEN, GREEN_LIGHT),
        value if value <= 80.0 => (YELLOW, YELLOW_LIGHT),
        value if value <= 92.0 => (PEACH, PEACH_LIGHT),
        _ => (RED, RED_LIGHT),
    }
}

#[cfg(test)]
mod tests {
    use super::skins::CustomStatusLineRenderer;
    use super::*;
    use insta::assert_snapshot;
    use ratatui::style::Modifier;
    use ratatui::style::Style;
    use std::time::Duration;
    use std::time::Instant;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn elapsed_formatting_matches_indicator() {
        assert_eq!(format_elapsed_compact(0), "0s");
        assert_eq!(format_elapsed_compact(59), "59s");
        assert_eq!(format_elapsed_compact(60), "1m 00s");
        assert_eq!(format_elapsed_compact(3_661), "1h 01m 01s");
    }

    #[test]
    fn queue_preview_handles_extra_count() {
        let long = "x".repeat(80);
        let (preview, extra) = queue_preview(&[long, "second".to_string(), "third".to_string()]);
        assert!(preview.ends_with('…'));
        assert_eq!(extra, 2);
        assert!(UnicodeWidthStr::width(preview.as_str()) <= 32);
    }

    #[test]
    fn context_bar_colors_follow_thresholds() {
        let (green, _) = context_bar_colors(10.0);
        assert_eq!(green, GREEN);
        let (yellow, _) = context_bar_colors(70.0);
        assert_eq!(yellow, YELLOW);
        let (peach, _) = context_bar_colors(85.0);
        assert_eq!(peach, PEACH);
        let (red, _) = context_bar_colors(98.0);
        assert_eq!(red, RED);
    }

    #[test]
    fn renderer_renders_core_segments() {
        let snapshot = StatusLineSnapshot {
            cwd_display: Some("codex".to_string()),
            model: Some(StatusLineModelSnapshot {
                label: "codex-model".to_string(),
                detail: Some("high".to_string()),
            }),
            tokens: Some(StatusLineTokenSnapshot {
                total: TokenCountSnapshot {
                    input_tokens: 600,
                    cached_input_tokens: 0,
                    output_tokens: 424,
                    ..TokenCountSnapshot::default()
                },
                last: None,
            }),
            context: Some(StatusLineContextSnapshot {
                percent_remaining: 80,
                ..StatusLineContextSnapshot::default()
            }),
            git: Some(StatusLineGitSnapshot {
                branch: Some("main".to_string()),
                dirty: true,
                ahead: Some(1),
                behind: None,
            }),
            environment: StatusLineEnvironmentSnapshot {
                hostname: Some("vermissian".to_string()),
                aws_profile: Some("prod".to_string()),
                ..StatusLineEnvironmentSnapshot::default()
            },
            ..StatusLineSnapshot::default()
        };
        let renderer = DefaultStatusLineRenderer;
        let line = renderer.render(&snapshot, 80, Instant::now());
        let rendered: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(rendered.contains("codex-model"));
        assert!(rendered.contains("high"));
        assert!(!rendered.contains('Σ'));
        assert!(rendered.contains("main*"));
        assert!(rendered.contains(" codex") || rendered.contains(" tui"));
        assert!(rendered.contains("vermissian"));
    }

    #[test]
    fn renderer_snapshot_wide_width() {
        let snapshot = sample_snapshot();
        let now = Instant::now();
        let renderer = DefaultStatusLineRenderer;
        let line = renderer.render(&snapshot, 80, now);
        assert_snapshot!("statusline_wide_80", snapshot_line_repr(&line));
    }

    #[test]
    fn renderer_snapshot_narrow_width_degrades() {
        let snapshot = sample_snapshot();
        let now = Instant::now();
        let renderer = DefaultStatusLineRenderer;
        let line = renderer.render(&snapshot, 40, now);
        assert_snapshot!("statusline_narrow_40", snapshot_line_repr(&line));
    }

    #[test]
    fn renderer_run_pill_includes_timer_queue_and_hint() {
        let snapshot = sample_snapshot();
        let now = Instant::now();
        let renderer = DefaultStatusLineRenderer;
        let repr = snapshot_line_repr(&renderer.render_run_pill(&snapshot, 80, now));
        assert!(repr.contains("2m 05s"), "timer text missing: {repr}");
        assert!(
            repr.contains("Applying patch"),
            "run label missing from pill: {repr}"
        );
        assert!(repr.contains("next:"), "queue prefix missing: {repr}");
        assert!(repr.contains("git status"), "queue preview missing: {repr}");
        assert!(repr.contains("(+1)"), "queue extra count missing: {repr}");
        assert!(repr.contains("alt + ↑"), "hint missing: {repr}");
    }

    #[test]
    fn renderer_run_pill_idle_is_blank_capsule() {
        let mut snapshot = sample_snapshot();
        snapshot.run_state = None;
        let now = Instant::now();
        let renderer = DefaultStatusLineRenderer;
        let repr = snapshot_line_repr(&renderer.render_run_pill(&snapshot, 60, now));
        assert!(
            repr.lines().all(|line| line.contains("plain \"")),
            "idle pill should collapse to plain padding: {repr}"
        );
    }

    #[test]
    fn custom_renderer_matches_default_statusline() {
        let snapshot = sample_snapshot();
        let now = Instant::now();
        let default_line = DefaultStatusLineRenderer.render(&snapshot, 80, now);
        let custom_line = CustomStatusLineRenderer.render(&snapshot, 80, now);
        assert_eq!(
            snapshot_line_repr(&custom_line),
            snapshot_line_repr(&default_line)
        );
    }

    #[test]
    fn custom_renderer_matches_default_run_pill() {
        let snapshot = sample_snapshot();
        let now = Instant::now();
        let default_line = DefaultStatusLineRenderer.render_run_pill(&snapshot, 60, now);
        let custom_line = CustomStatusLineRenderer.render_run_pill(&snapshot, 60, now);
        assert_eq!(
            snapshot_line_repr(&custom_line),
            snapshot_line_repr(&default_line)
        );
    }

    #[test]
    fn run_label_defaults_to_waiting_message() {
        let now = Instant::now();
        let snapshot = StatusLineSnapshot {
            context: Some(StatusLineContextSnapshot {
                percent_remaining: 100,
                tokens_in_context: 0,
                window: 1,
            }),
            run_state: Some(StatusLineRunState {
                status_changed_at: now,
                ..StatusLineRunState::default()
            }),
            ..StatusLineSnapshot::default()
        };
        let renderer = DefaultStatusLineRenderer;
        let line = renderer.render(&snapshot, 120, now);
        let has_default = line
            .spans
            .iter()
            .any(|span| span.content.contains(DEFAULT_STATUS_MESSAGE));
        assert!(
            has_default,
            "status capsule should show default message when label empty"
        );
    }

    fn sample_snapshot() -> StatusLineSnapshot {
        StatusLineSnapshot {
            cwd_display: Some("~/workspace/codex".to_string()),
            cwd_basename: Some("codex".to_string()),
            cwd_fallback: Some("codex".to_string()),
            model: Some(StatusLineModelSnapshot {
                label: "gpt-5-codex".to_string(),
                detail: Some("high".to_string()),
            }),
            tokens: Some(StatusLineTokenSnapshot {
                total: TokenCountSnapshot {
                    total_tokens: 48_234,
                    input_tokens: 30_000,
                    cached_input_tokens: 8_000,
                    output_tokens: 18_234,
                    reasoning_output_tokens: 234,
                },
                last: Some(TokenCountSnapshot {
                    total_tokens: 2_345,
                    input_tokens: 1_200,
                    cached_input_tokens: 200,
                    output_tokens: 900,
                    reasoning_output_tokens: 45,
                }),
            }),
            context: Some(StatusLineContextSnapshot {
                percent_remaining: 68,
                tokens_in_context: 52_000,
                window: 160_000,
            }),
            run_state: Some(StatusLineRunState {
                label: "Applying patch".to_string(),
                spinner_started_at: None,
                timer: Some(RunTimerSnapshot {
                    elapsed_running: Duration::from_secs(125),
                    last_resume_at: None,
                    is_paused: true,
                }),
                queued_messages: vec!["git status".to_string(), "cargo test --all".to_string()],
                show_interrupt_hint: true,
                status_changed_at: Instant::now(),
            }),
            git: Some(StatusLineGitSnapshot {
                branch: Some("feature/fix-tests".to_string()),
                dirty: true,
                ahead: Some(1),
                behind: Some(0),
            }),
            environment: StatusLineEnvironmentSnapshot {
                devspace: Some(StatusLineDevspaceSnapshot {
                    name: "earth".to_string(),
                }),
                hostname: Some("vermissian".to_string()),
                aws_profile: Some("prod".to_string()),
                kubernetes_context: Some("codex-dev".to_string()),
            },
        }
    }

    fn snapshot_line_repr(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .enumerate()
            .map(|(idx, span)| {
                format!(
                    "{idx:02}: {} {:?}",
                    describe_style(span.style),
                    span.content.as_ref()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn describe_style(style: Style) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(fg) = style.fg {
            parts.push(format!("fg={fg:?}"));
        }
        if let Some(bg) = style.bg {
            parts.push(format!("bg={bg:?}"));
        }
        if style.add_modifier != Modifier::empty() {
            parts.push(format!("mod={:?}", style.add_modifier));
        }
        if parts.is_empty() {
            "plain".to_string()
        } else {
            parts.join("|")
        }
    }
}
