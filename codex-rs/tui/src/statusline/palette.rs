use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

#[allow(clippy::disallowed_methods)]
pub(crate) const BASE: Color = Color::Rgb(30, 30, 46);
#[allow(clippy::disallowed_methods)]
pub(crate) const LAVENDER: Color = Color::Rgb(180, 190, 254);
#[allow(clippy::disallowed_methods)]
pub(crate) const SKY: Color = Color::Rgb(137, 220, 235);
#[allow(clippy::disallowed_methods)]
pub(crate) const MAUVE: Color = Color::Rgb(203, 166, 247);
#[allow(clippy::disallowed_methods)]
pub(crate) const PEACH: Color = Color::Rgb(250, 179, 135);
#[allow(clippy::disallowed_methods)]
pub(crate) const GREEN: Color = Color::Rgb(166, 227, 161);
#[allow(clippy::disallowed_methods)]
pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175);
#[allow(clippy::disallowed_methods)]
pub(crate) const RED: Color = Color::Rgb(243, 139, 168);
#[allow(clippy::disallowed_methods)]
pub(crate) const ROSEWATER: Color = Color::Rgb(245, 224, 220);
#[allow(clippy::disallowed_methods)]
pub(crate) const TEAL: Color = Color::Rgb(148, 226, 213);
#[allow(clippy::disallowed_methods)]
#[allow(dead_code)]
pub(crate) const SURFACE0: Color = Color::Rgb(49, 50, 68);
#[allow(clippy::disallowed_methods)]
pub(crate) const SUBTEXT0: Color = Color::Rgb(166, 173, 200);
#[allow(clippy::disallowed_methods)]
pub(crate) const GREEN_LIGHT: Color = Color::Rgb(86, 127, 81);
#[allow(clippy::disallowed_methods)]
pub(crate) const YELLOW_LIGHT: Color = Color::Rgb(149, 136, 95);
#[allow(clippy::disallowed_methods)]
pub(crate) const PEACH_LIGHT: Color = Color::Rgb(150, 107, 81);
#[allow(clippy::disallowed_methods)]
pub(crate) const RED_LIGHT: Color = Color::Rgb(146, 83, 100);

pub(crate) fn queue_preview_style() -> Style {
    Style::default()
        .fg(SUBTEXT0)
        .add_modifier(Modifier::ITALIC | Modifier::DIM)
}
