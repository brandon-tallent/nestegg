//! Cash Flow: income against spending by month, with a breakdown of the selected month

use super::widgets::{self, card, card_with_note};
use crate::app::{App, Breakdown, Screen};
use crate::data::{month_of, MonthFlow};
use crate::fmt;
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashMap;

/// Columns each month takes in the bar chart: two bars and a gap
const MONTH_COLUMN_WIDTH: usize = 7;
const BAR_WIDTH: usize = 2;

/// Rows of the bar chart card, including border and month labels
const BARS_CARD_HEIGHT: u16 = 11;

const BREAKDOWN_AMOUNT_WIDTH: usize = 11;
const BREAKDOWN_PERCENT_WIDTH: usize = 6;

/// Share of a breakdown row given to its bar
const BREAKDOWN_BAR_SHARE: f64 = 0.3;

/// Totals for the selected month, grouped by the current breakdown and
/// sorted largest first. `income` picks income rows; otherwise spending.
pub fn breakdown_rows(app: &App, income: bool) -> Vec<(String, f64)> {
    let Some(month) = app.cash_flow_month() else {
        return Vec::new();
    };
    let group_of_category: HashMap<&str, &str> = app
        .data
        .categories
        .iter()
        .filter_map(|c| {
            let group = app.data.groups.iter().find(|g| g.id == c.group_id)?;
            Some((c.name.as_str(), group.name.as_str()))
        })
        .collect();

    let mut totals: HashMap<String, f64> = HashMap::new();
    for txn in &app.data.transactions {
        if month_of(txn.date) != month || !app.counts(txn) || txn.is_income() != income {
            continue;
        }
        let key = match app.breakdown {
            Breakdown::Category => txn.category.as_str(),
            Breakdown::Group => group_of_category.get(txn.category.as_str()).copied().unwrap_or("Other"),
            Breakdown::Merchant => txn.merchant.as_str(),
        };
        *totals.entry(key.to_string()).or_default() += if income { txn.amount } else { -txn.amount };
    }
    let query = app.query(Screen::CashFlow);
    let mut rows: Vec<(String, f64)> =
        totals.into_iter().filter(|(name, total)| query.matches(&[name], Some(*total))).collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(flow) = app.months().get(app.cash_month).copied() else {
        frame.render_widget(Line::styled(" No transactions to chart", theme::muted()), area);
        return;
    };
    let [bars, summary, breakdown] = Layout::vertical([
        Constraint::Length(BARS_CARD_HEIGHT),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .areas(area);
    draw_bars(frame, bars, app);
    draw_summary(frame, summary, flow, app);

    let [income, spending] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(breakdown);
    draw_breakdown(frame, income, app, true);
    draw_breakdown(frame, spending, app, false);
}

/// A column of `height` cells filled from the bottom to `fraction`
fn bar_cell(fraction: f64, height: usize, row_from_top: usize) -> &'static str {
    const PARTIAL: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let eighths = (fraction.clamp(0.0, 1.0) * height as f64 * 8.0).round() as usize;
    let row_from_bottom = height - 1 - row_from_top;
    let filled = eighths.saturating_sub(row_from_bottom * 8).min(8);
    PARTIAL[filled]
}

fn draw_bars(frame: &mut Frame, area: Rect, app: &App) {
    let legend = Line::from(vec![
        Span::styled("█ ", Style::new().fg(theme::POSITIVE)),
        Span::styled("income  ", theme::muted()),
        Span::styled("█ ", Style::new().fg(theme::NEGATIVE)),
        Span::styled("spending ", theme::muted()),
    ]);
    let block = card_with_note("Income & Spending", legend);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Keep the selected month in view, preferring the most recent months
    let columns = (inner.width as usize / MONTH_COLUMN_WIDTH).max(1);
    let months = &app.months();
    let end = (app.cash_month + columns / 2 + 1).max(columns).min(months.len());
    let start = end.saturating_sub(columns);
    let shown = &months[start..end];

    let peak = shown.iter().map(|m| m.income.max(m.expense)).fold(1.0, f64::max);
    let chart_height = (inner.height as usize).saturating_sub(1);
    let gap = " ".repeat(MONTH_COLUMN_WIDTH - BAR_WIDTH * 2 - 1);
    let mut lines = Vec::with_capacity(inner.height as usize);
    for row in 0..chart_height {
        let mut spans = Vec::with_capacity(shown.len() * 3);
        for month in shown {
            spans.push(Span::styled(
                bar_cell(month.income / peak, chart_height, row).repeat(BAR_WIDTH),
                Style::new().fg(theme::POSITIVE),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                bar_cell(month.expense / peak, chart_height, row).repeat(BAR_WIDTH),
                Style::new().fg(theme::NEGATIVE),
            ));
            spans.push(Span::raw(gap.clone()));
        }
        lines.push(Line::from(spans));
    }
    let labels: Vec<Span> = shown
        .iter()
        .enumerate()
        .map(|(i, month)| {
            let label = format!("{:<w$}", month.month.format("%b"), w = MONTH_COLUMN_WIDTH);
            if start + i == app.cash_month {
                Span::styled(label, theme::accent())
            } else {
                Span::styled(label, theme::muted())
            }
        })
        .collect();
    lines.push(Line::from(labels));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_summary(frame: &mut Frame, area: Rect, flow: MonthFlow, app: &App) {
    let block = card(&format!("‹ {} ›", flow.month.format("%B %Y")));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut spans = vec![
        Span::styled("Income ", theme::muted()),
        Span::styled(fmt::money0(flow.income), theme::gain(1.0)),
        Span::styled("    Spending ", theme::muted()),
        Span::styled(fmt::money0(flow.expense), theme::text()),
        Span::styled("    Saved ", theme::muted()),
        Span::styled(fmt::money0(flow.savings()), theme::gain(flow.savings())),
        Span::styled("    Savings rate ", theme::muted()),
        Span::styled(format!("{:.0}%", flow.savings_rate()), theme::gain(flow.savings())),
    ];
    // Say what the one-time items in this month amount to, in or out
    let (one_time_income, one_time_expense) = app.one_time_in(flow.month);
    if one_time_income != 0.0 || one_time_expense != 0.0 {
        let mut parts = Vec::new();
        if one_time_income != 0.0 {
            parts.push(format!("{} income", fmt::money0(one_time_income)));
        }
        if one_time_expense != 0.0 {
            parts.push(format!("{} spending", fmt::money0(one_time_expense)));
        }
        let verb = if app.include_one_time { "includes" } else { "excludes" };
        spans.push(Span::styled(
            format!("    {} one-time: {}", verb, parts.join(", ")),
            Style::new().fg(theme::YELLOW),
        ));
    }
    frame.render_widget(Line::from(spans), inner);
}

fn draw_breakdown(frame: &mut Frame, area: Rect, app: &App, income: bool) {
    let rows = breakdown_rows(app, income);
    let title = if income { "Income" } else { "Spending" };
    let block = card_with_note(title, Line::styled(format!("by {} ", app.breakdown.label().to_lowercase()), theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if rows.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "Nothing this month"), theme::muted()), inner);
        return;
    }
    let total: f64 = rows.iter().map(|r| r.1).sum();
    let peak = rows.iter().map(|r| r.1).fold(0.0, f64::max);
    let width = inner.width as usize;
    let bar_width = (width as f64 * BREAKDOWN_BAR_SHARE) as usize;
    let name_width = width.saturating_sub(bar_width + BREAKDOWN_AMOUNT_WIDTH + BREAKDOWN_PERCENT_WIDTH + 1);
    let color = if income { theme::POSITIVE } else { theme::ACCENT };

    // Only the spending list is navigable
    let selected = if income { usize::MAX } else { app.cash_selected };
    let (start, end) = widgets::window(app.cash_selected.min(rows.len() - 1), rows.len(), inner.height as usize);
    let (start, end) = if income { (0, rows.len().min(inner.height as usize)) } else { (start, end) };
    if !income { crate::mouse::rows(app, inner, start, end, None); }
    let lines: Vec<Line> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(offset, (name, amount))| {
            let share = if total > 0.0 { amount / total * 100.0 } else { 0.0 };
            let fraction = if peak > 0.0 { amount / peak } else { 0.0 };
            let line = Line::from(vec![
                Span::styled(format!("{:<w$}", fmt::trunc(name, name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(fmt::hbar(fraction, bar_width), Style::new().fg(color)),
                Span::raw(" "),
                Span::styled(format!("{:>w$}", fmt::money0(*amount), w = BREAKDOWN_AMOUNT_WIDTH), theme::text()),
                Span::styled(format!("{:>w$.0}%", share, w = BREAKDOWN_PERCENT_WIDTH - 1), theme::muted()),
            ]);
            if start + offset == selected {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
