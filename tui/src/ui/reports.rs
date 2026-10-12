//! Reports: spending by category across recent months

use super::widgets::{self, card_with_note};
use crate::app::{App, Screen};
use crate::data::month_of;
use crate::fmt;
use crate::theme;
use chrono::NaiveDate;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashMap;

const NAME_WIDTH: usize = 24;
const CELL_WIDTH: usize = 9;

/// Most months shown, however wide the terminal
const MAX_MONTHS: usize = 12;

/// Steps of background shading used to show a cell's size within its row
const HEAT_LEVELS: [Color; 4] = [
    Color::Reset,
    Color::Rgb(40, 36, 44),
    Color::Rgb(72, 44, 40),
    Color::Rgb(110, 54, 36),
];

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let block = card_with_note("Spending by Category", Line::styled("monthly totals ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;

    // Name column, one cell per month, then an average column
    let month_count = (width.saturating_sub(NAME_WIDTH + CELL_WIDTH) / CELL_WIDTH).clamp(1, MAX_MONTHS);
    let all = app.months();
    let months: Vec<NaiveDate> = all[all.len().saturating_sub(month_count)..].iter().map(|m| m.month).collect();
    if months.is_empty() {
        frame.render_widget(Line::styled("No transactions yet", theme::muted()), inner);
        return;
    }
    let column: HashMap<NaiveDate, usize> = months.iter().enumerate().map(|(i, m)| (*m, i)).collect();

    let mut by_category: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut income = vec![0.0; months.len()];
    for txn in app.data.transactions.iter().filter(|t| app.counts(t)) {
        let Some(&col) = column.get(&month_of(txn.date)) else {
            continue;
        };
        if txn.is_income() {
            income[col] += txn.amount;
        } else {
            by_category.entry(txn.category.as_str()).or_insert_with(|| vec![0.0; months.len()])[col] -= txn.amount;
        }
    }
    let mut rows: Vec<(&str, Vec<f64>)> = by_category.into_iter().collect();
    rows.sort_by(|a, b| {
        let (ta, tb): (f64, f64) = (a.1.iter().sum(), b.1.iter().sum());
        tb.total_cmp(&ta).then_with(|| a.0.cmp(b.0))
    });
    let spending: Vec<f64> = (0..months.len()).map(|col| rows.iter().map(|r| r.1[col]).sum()).collect();
    // The totals above stay whole; the search only narrows the categories listed
    let query = app.query(Screen::Reports);
    rows.retain(|(name, _)| query.matches(&[name], None));

    let mut header = vec![Span::styled(format!("{:<w$}", "Category", w = NAME_WIDTH), theme::header())];
    header.extend(months.iter().map(|m| Span::styled(format!("{:>w$}", m.format("%b '%y"), w = CELL_WIDTH), theme::header())));
    header.push(Span::styled(format!("{:>w$}", "Avg", w = CELL_WIDTH), theme::header()));

    let totals_line = |label: &str, values: &[f64], style: Style| {
        let mut spans = vec![Span::styled(format!("{:<w$}", label, w = NAME_WIDTH), theme::bold())];
        spans.extend(values.iter().map(|v| Span::styled(format!("{:>w$}", fmt::compact(*v), w = CELL_WIDTH), style)));
        let average = values.iter().sum::<f64>() / values.len() as f64;
        spans.push(Span::styled(format!("{:>w$}", fmt::compact(average), w = CELL_WIDTH), style));
        Line::from(spans)
    };
    let saved: Vec<f64> = income.iter().zip(&spending).map(|(i, s)| i - s).collect();

    // Header and three summary rows are pinned; categories scroll beneath them
    const PINNED_ROWS: usize = 5;
    let body_height = (inner.height as usize).saturating_sub(PINNED_ROWS);
    let scroll = app.reports_scroll.min(rows.len().saturating_sub(body_height));

    let mut lines = vec![
        Line::from(header),
        totals_line("Income", &income, theme::gain(1.0)),
        totals_line("Spending", &spending, theme::text()),
        totals_line("Saved", &saved, theme::muted()),
        Line::from(""),
    ];
    if rows.is_empty() {
        lines.push(Line::styled(widgets::no_matches(app, "No spending in these months"), theme::muted()));
    }
    for (name, values) in rows.iter().skip(scroll).take(body_height) {
        let peak = values.iter().copied().fold(0.0, f64::max);
        let mut spans = vec![Span::styled(format!("{:<w$}", fmt::trunc(name, NAME_WIDTH - 1), w = NAME_WIDTH), theme::text())];
        for value in values {
            let level = if peak <= 0.0 || *value <= 0.0 {
                0
            } else {
                ((value / peak) * (HEAT_LEVELS.len() - 1) as f64).ceil() as usize
            };
            let text = if *value == 0.0 { "·".to_string() } else { fmt::compact(*value) };
            let style = if *value == 0.0 { theme::muted() } else { theme::text() };
            spans.push(Span::styled(
                format!("{:>w$}", text, w = CELL_WIDTH),
                style.bg(HEAT_LEVELS[level.min(HEAT_LEVELS.len() - 1)]),
            ));
        }
        let average = values.iter().sum::<f64>() / values.len() as f64;
        spans.push(Span::styled(format!("{:>w$}", fmt::compact(average), w = CELL_WIDTH), theme::muted()));
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
