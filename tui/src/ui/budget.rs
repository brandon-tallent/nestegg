//! Budget: this month's plan against actuals, by category group

use super::widgets::{self, card_with_note};
use crate::app::{App, Screen};
use crate::fmt;
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use serde_json::Value;
use std::collections::HashMap;

const AMOUNT_WIDTH: usize = 11;
const BAR_WIDTH: usize = 18;

/// Fraction of budget used at which a bar turns from green to yellow
const NEARLY_SPENT: f64 = 0.85;

struct Row {
    name: String,
    planned: f64,
    actual: f64,
    remaining: f64,
}

struct Section {
    name: String,
    income: bool,
    rows: Vec<Row>,
}

fn amounts(entry: &Value) -> Option<(f64, f64, f64)> {
    let month = entry["monthlyAmounts"].as_array()?.first()?;
    Some((
        month["plannedCashFlowAmount"].as_f64().unwrap_or(0.0),
        month["actualAmount"].as_f64().unwrap_or(0.0),
        month["remainingAmount"].as_f64().unwrap_or(0.0),
    ))
}

/// Budget sections in Monarch's order, skipping transfers and empty rows
fn sections(document: &Value) -> Vec<Section> {
    let by_category: HashMap<&str, (f64, f64, f64)> = document["budgetData"]["monthlyAmountsByCategory"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| Some((e["category"]["id"].as_str()?, amounts(e)?)))
                .collect()
        })
        .unwrap_or_default();

    let mut groups: Vec<&Value> = document["categoryGroups"].as_array().map(|g| g.iter().collect()).unwrap_or_default();
    groups.sort_by_key(|g| (g["type"].as_str() != Some("income"), g["order"].as_i64().unwrap_or(0)));

    groups
        .into_iter()
        .filter(|g| g["type"].as_str() != Some("transfer"))
        .filter_map(|group| {
            let mut categories: Vec<&Value> = group["categories"].as_array()?.iter().collect();
            categories.sort_by_key(|c| c["order"].as_i64().unwrap_or(0));
            let rows: Vec<Row> = categories
                .into_iter()
                .filter(|c| c["excludeFromBudget"].as_bool() != Some(true))
                .filter_map(|c| {
                    let (planned, actual, remaining) = *by_category.get(c["id"].as_str()?)?;
                    (planned != 0.0 || actual != 0.0).then(|| Row {
                        name: c["name"].as_str().unwrap_or("").to_string(),
                        planned,
                        actual,
                        remaining,
                    })
                })
                .collect();
            (!rows.is_empty()).then(|| Section {
                name: group["name"].as_str().unwrap_or("").to_string(),
                income: group["type"].as_str() == Some("income"),
                rows,
            })
        })
        .collect()
}

fn progress_bar(actual: f64, planned: f64, income: bool) -> Span<'static> {
    // Spending with nothing budgeted counts as over budget
    let fraction = if planned > 0.0 {
        actual / planned
    } else if actual > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };
    let color = if income {
        theme::POSITIVE
    } else if fraction > 1.0 {
        theme::NEGATIVE
    } else if fraction >= NEARLY_SPENT {
        theme::YELLOW
    } else {
        theme::POSITIVE
    };
    Span::styled(fmt::hbar(fraction, BAR_WIDTH), Style::new().fg(color).bg(theme::SELECTED_BG))
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(document) = &app.data.budget else {
        frame.render_widget(Line::styled(" No budget has been synced yet", theme::muted()), area);
        return;
    };
    let month = document["budgetData"]["totalsByMonth"][0]["month"]
        .as_str()
        .and_then(|m| chrono::NaiveDate::parse_from_str(m, "%Y-%m-%d").ok())
        .map(|m| m.format("%B %Y").to_string())
        .unwrap_or_default();

    let [summary, body] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    let totals = &document["budgetData"]["totalsByMonth"][0];
    let total = |key: &str, field: &str| totals[key][field].as_f64().unwrap_or(0.0);
    let summary_lines = vec![
        Line::from(vec![
            Span::styled(" Income ", theme::muted()),
            Span::styled(fmt::money0(total("totalIncome", "actualAmount")), theme::gain(1.0)),
            Span::styled(format!(" of {}", fmt::money0(total("totalIncome", "plannedAmount"))), theme::muted()),
            Span::styled("     Expenses ", theme::muted()),
            Span::styled(fmt::money0(total("totalExpenses", "actualAmount")), theme::text()),
            Span::styled(format!(" of {}", fmt::money0(total("totalExpenses", "plannedAmount"))), theme::muted()),
        ]),
        Line::from(vec![
            Span::styled(" Fixed ", theme::muted()),
            Span::styled(fmt::money0(total("totalFixedExpenses", "actualAmount")), theme::text()),
            Span::styled(format!(" of {}", fmt::money0(total("totalFixedExpenses", "plannedAmount"))), theme::muted()),
            Span::styled("     Flexible ", theme::muted()),
            Span::styled(fmt::money0(total("totalFlexibleExpenses", "actualAmount")), theme::text()),
            Span::styled(format!(" of {}", fmt::money0(total("totalFlexibleExpenses", "plannedAmount"))), theme::muted()),
            Span::styled("     Left to spend ", theme::muted()),
            Span::styled(
                fmt::money0(total("totalExpenses", "remainingAmount")),
                theme::gain(total("totalExpenses", "remainingAmount")),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(summary_lines), summary);

    let block = card_with_note("Budget", Line::styled(format!("{} ", month), theme::muted()));
    let inner = block.inner(body);
    frame.render_widget(block, body);
    let width = inner.width as usize;
    let name_width = width.saturating_sub(BAR_WIDTH + AMOUNT_WIDTH * 3 + 2);

    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{:<w$}", "Category", w = name_width), theme::header()),
        Span::raw(" ".repeat(BAR_WIDTH + 2)),
        Span::styled(format!("{:>w$}", "Actual", w = AMOUNT_WIDTH), theme::header()),
        Span::styled(format!("{:>w$}", "Budget", w = AMOUNT_WIDTH), theme::header()),
        Span::styled(format!("{:>w$}", "Remaining", w = AMOUNT_WIDTH), theme::header()),
    ])];
    let query = app.query(Screen::Budget);
    for mut section in sections(document) {
        // A section named in the search is shown whole; otherwise only its matching rows
        if !query.matches(&[&section.name], None) {
            section.rows.retain(|row| query.matches(&[&row.name, &section.name], Some(row.actual)));
        }
        if section.rows.is_empty() {
            continue;
        }
        let (planned, actual): (f64, f64) = section
            .rows
            .iter()
            .fold((0.0, 0.0), |acc, row| (acc.0 + row.planned, acc.1 + row.actual));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", fmt::trunc(&section.name, name_width - 1), w = name_width + BAR_WIDTH + 2), theme::accent()),
            Span::styled(format!("{:>w$}", fmt::money0(actual), w = AMOUNT_WIDTH), theme::bold()),
            Span::styled(format!("{:>w$}", fmt::money0(planned), w = AMOUNT_WIDTH), theme::bold()),
        ]));
        for row in &section.rows {
            // For income, exceeding the plan is the good direction
            let remaining_style = if section.income { theme::gain(-row.remaining) } else { theme::gain(row.remaining) };
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<w$}", fmt::trunc(&row.name, name_width.saturating_sub(3)), w = name_width - 2), theme::text()),
                progress_bar(row.actual, row.planned, section.income),
                Span::raw("  "),
                Span::styled(format!("{:>w$}", fmt::money0(row.actual), w = AMOUNT_WIDTH), theme::text()),
                Span::styled(format!("{:>w$}", fmt::money0(row.planned), w = AMOUNT_WIDTH), theme::muted()),
                Span::styled(format!("{:>w$}", fmt::money0(row.remaining), w = AMOUNT_WIDTH), remaining_style),
            ]));
        }
    }

    if lines.len() == 1 {
        lines.push(Line::styled(widgets::no_matches(app, "Nothing budgeted"), theme::muted()));
    }
    // The column header stays put while the rest scrolls
    let body_height = (inner.height as usize).saturating_sub(1);
    let scroll = app.budget_scroll.min(lines.len().saturating_sub(1 + body_height));
    let mut shown = vec![lines[0].clone()];
    shown.extend(lines.into_iter().skip(1 + scroll).take(body_height));
    frame.render_widget(Paragraph::new(shown), inner);
}
