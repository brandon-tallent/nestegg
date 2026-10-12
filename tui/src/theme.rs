//! Colors and shared styles, loosely following Monarch's palette

use ratatui::style::{Color, Modifier, Style};

/// Monarch's brand orange
pub const ACCENT: Color = Color::Rgb(255, 105, 45);
pub const POSITIVE: Color = Color::Rgb(46, 196, 134);
pub const NEGATIVE: Color = Color::Rgb(240, 100, 105);
pub const TEXT: Color = Color::Rgb(226, 232, 240);
pub const MUTED: Color = Color::Rgb(125, 138, 156);
pub const BORDER: Color = Color::Rgb(58, 70, 88);
pub const SELECTED_BG: Color = Color::Rgb(38, 50, 68);
pub const BLUE: Color = Color::Rgb(90, 160, 250);
pub const PURPLE: Color = Color::Rgb(165, 130, 245);
pub const YELLOW: Color = Color::Rgb(240, 190, 80);
pub const TEAL: Color = Color::Rgb(60, 190, 200);

/// Colors cycled through for categorical breakdowns
pub const SERIES: [Color; 6] = [BLUE, ACCENT, POSITIVE, PURPLE, YELLOW, TEAL];

pub fn text() -> Style {
    Style::new().fg(TEXT)
}

pub fn muted() -> Style {
    Style::new().fg(MUTED)
}

pub fn bold() -> Style {
    Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn header() -> Style {
    Style::new().fg(MUTED).add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    Style::new().bg(SELECTED_BG).add_modifier(Modifier::BOLD)
}

/// Green for gains, red for losses, muted for zero
pub fn gain(value: f64) -> Style {
    if value > 0.0 {
        Style::new().fg(POSITIVE)
    } else if value < 0.0 {
        Style::new().fg(NEGATIVE)
    } else {
        muted()
    }
}

/// Transaction amounts: income green, spending in the normal text color
pub fn amount(value: f64) -> Style {
    if value > 0.0 {
        Style::new().fg(POSITIVE)
    } else {
        text()
    }
}
