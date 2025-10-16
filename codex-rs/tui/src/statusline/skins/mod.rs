use std::time::Instant;

use ratatui::text::Line;

use super::StatusLineRenderer;
use super::StatusLineSnapshot;
use super::render_status_line;
use super::render_status_run_pill;

#[derive(Debug, Default)]
pub(crate) struct CustomStatusLineRenderer;

impl StatusLineRenderer for CustomStatusLineRenderer {
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
