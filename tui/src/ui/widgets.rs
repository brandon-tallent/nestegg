//! Building blocks shared by the screens

use crate::app::App;
use crate::fmt;
use crate::theme;
use chrono::NaiveDate;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, Padding};
use ratatui::Frame;

/// Smallest area a line chart is worth drawing in
const MIN_CHART_WIDTH: u16 = 24;
const MIN_CHART_HEIGHT: u16 = 5;

/// Share of the value range left empty above and below a charted line
const CHART_Y_PADDING: f64 = 0.08;

/// What an empty list says: that the search found nothing, or `otherwise`
pub fn no_matches(app: &App, otherwise: &str) -> String {
    match app.search_text(app.screen) {
        "" => otherwise.to_string(),
        search => format!("Nothing matches \"{}\"", search),
    }
}

/// A bordered panel with a title, the basic unit of every screen
pub fn card(title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme::BORDER))
        .title(Span::styled(format!(" {} ", title), theme::bold()))
        .padding(Padding::horizontal(1))
}

/// A card whose title line also carries right-aligned detail
pub fn card_with_note<'a>(title: &str, note: Line<'a>) -> Block<'a> {
    card(title).title(note.right_aligned())
}

/// Rows `[start, end)` to show so that `selected` stays roughly centered
pub fn window(selected: usize, len: usize, height: usize) -> (usize, usize) {
    if len <= height {
        return (0, len);
    }
    let start = selected.saturating_sub(height / 2).min(len - height);
    (start, start + height)
}

/// A rectangle of at most the given size centered in `area`
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

/// One series on a line chart
pub struct Series<'a> {
    pub points: &'a [(f64, f64)],
    pub color: Color,
}

/// Draw one or more lines sharing axes. `x_labels` run left to right.
pub fn line_chart(frame: &mut Frame, area: Rect, series: &[Series], x_labels: Vec<String>) {
    if area.width < MIN_CHART_WIDTH || area.height < MIN_CHART_HEIGHT {
        return;
    }
    let all = series.iter().flat_map(|s| s.points.iter());
    let (mut x_min, mut x_max, mut y_min, mut y_max) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for (x, y) in all {
        x_min = x_min.min(*x);
        x_max = x_max.max(*x);
        y_min = y_min.min(*y);
        y_max = y_max.max(*y);
    }
    if x_min > x_max {
        frame.render_widget(Line::styled("No history in this range", theme::muted()), area);
        return;
    }
    let pad = ((y_max - y_min) * CHART_Y_PADDING).max(1.0);
    // A series that never goes negative should not get a negative axis from padding
    let low = if y_min >= 0.0 { (y_min - pad).max(0.0) } else { y_min - pad };
    let high = y_max + pad;
    if x_min == x_max {
        x_max = x_min + 1.0;
    }

    let datasets = series
        .iter()
        .map(|s| {
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(s.color))
                .data(s.points)
        })
        .collect();
    let y_labels = vec![
        fmt::compact(low),
        fmt::compact((low + high) / 2.0),
        fmt::compact(high),
    ];
    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .style(Style::new().fg(theme::BORDER))
                .bounds([x_min, x_max])
                .labels(x_labels.into_iter().map(|l| Span::styled(l, theme::muted())).collect::<Vec<_>>()),
        )
        .y_axis(
            Axis::default()
                .style(Style::new().fg(theme::BORDER))
                .bounds([low, high])
                .labels(y_labels.into_iter().map(|l| Span::styled(l, theme::muted())).collect::<Vec<_>>()),
        );
    frame.render_widget(chart, area);
}

/// Chart a dated series as a single line, x axis in days
pub fn date_chart(frame: &mut Frame, area: Rect, points: &[(NaiveDate, f64)], color: Color) {
    let Some(first) = points.first().map(|p| p.0) else {
        frame.render_widget(Line::styled("No history in this range", theme::muted()), area);
        return;
    };
    let xy: Vec<(f64, f64)> = points
        .iter()
        .map(|(date, value)| ((*date - first).num_days() as f64, *value))
        .collect();
    let last = points[points.len() - 1].0;
    let middle = points[points.len() / 2].0;
    let labels = if points.len() > 2 {
        vec![fmt::short_date(first), fmt::short_date(middle), fmt::short_date(last)]
    } else {
        vec![fmt::short_date(first), fmt::short_date(last)]
    };
    line_chart(frame, area, &[Series { points: &xy, color }], labels);
}

/// `label  value` with the value pushed to the right edge of `width`
pub fn spread<'a>(left: Vec<Span<'a>>, right: Vec<Span<'a>>, width: usize) -> Line<'a> {
    let used: usize = left.iter().chain(right.iter()).map(|s| s.content.chars().count()).sum();
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
    spans.extend(right);
    Line::from(spans)
}

/// Change between the first and last value of a series, as (delta, percent)
pub fn change(first: f64, last: f64) -> (f64, f64) {
    let delta = last - first;
    let pct = if first.abs() > f64::EPSILON { delta / first.abs() * 100.0 } else { 0.0 };
    (delta, pct)
}

/// `▲ $1,234 (+1.2%)` colored by direction
pub fn change_spans(delta: f64, pct: f64) -> Vec<Span<'static>> {
    let arrow = if delta >= 0.0 { "▲" } else { "▼" };
    vec![Span::styled(
        format!("{} {} ({})", arrow, fmt::money0(delta.abs()), fmt::signed_pct(pct)),
        theme::gain(delta),
    )]
}
