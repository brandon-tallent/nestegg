//! Dashboard: net worth, this month's spending, and a glance at everything else

use super::widgets::{self, card, card_with_note, Series};
use super::WIDE;
use crate::app::App;
use crate::data::month_of;
use crate::fmt;
use crate::forecast;
use crate::theme;
use chrono::{Datelike, Months};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Days ahead covered by the upcoming-recurring card
const UPCOMING_DAYS: i64 = 30;

/// Rows a card needs before it is worth showing in the narrow layout
const MIN_CARD_HEIGHT: u16 = 6;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if area.width >= WIDE {
        let [top, bottom] =
            Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(52)]).areas(area);
        let [net_worth, spending] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(top);
        let [transactions, recurring, side] = Layout::horizontal([
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ])
        .areas(bottom);
        let [investments, extras] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(side);

        draw_net_worth(frame, net_worth, app);
        draw_spending(frame, spending, app);
        draw_transactions(frame, transactions, app);
        draw_recurring(frame, recurring, app);
        draw_investments(frame, investments, app);
        draw_extras(frame, extras, app);
    } else {
        let [net_worth, spending, transactions] = Layout::vertical([
            Constraint::Percentage(45),
            Constraint::Length(4),
            Constraint::Min(MIN_CARD_HEIGHT),
        ])
        .areas(area);
        draw_net_worth(frame, net_worth, app);
        draw_cash_flow_summary(frame, spending, app);
        draw_transactions(frame, transactions, app);
    }
}

fn draw_net_worth(frame: &mut Frame, area: Rect, app: &App) {
    let history = app.in_range(&app.data.net_worth, |p| p.date);
    let block = card("Net Worth");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(latest) = app.data.latest_net_worth() else {
        frame.render_widget(Line::styled("No net worth history yet", theme::muted()), inner);
        return;
    };
    let (delta, pct) = history
        .first()
        .map(|first| widgets::change(first.net_worth, latest.net_worth))
        .unwrap_or((0.0, 0.0));

    let [summary, chart] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let mut headline = vec![Span::styled(fmt::money0(latest.net_worth), theme::bold()), Span::raw("  ")];
    headline.extend(widgets::change_spans(delta, pct));
    headline.push(Span::styled(format!("  {}", app.range.label()), theme::muted()));
    let detail = Line::from(vec![
        Span::styled("Assets ", theme::muted()),
        Span::styled(fmt::money0(latest.assets), theme::text()),
        Span::styled("   Liabilities ", theme::muted()),
        Span::styled(fmt::money0(latest.liabilities.abs()), theme::text()),
    ]);
    frame.render_widget(Paragraph::new(vec![Line::from(headline), detail]), summary);

    let points: Vec<_> = history.iter().map(|p| (p.date, p.net_worth)).collect();
    widgets::date_chart(frame, chart, &points, theme::POSITIVE);
}

/// Running total of spending by day of month, as (day, total so far)
fn cumulative_spending(app: &App, month: chrono::NaiveDate, through_day: u32) -> Vec<(f64, f64)> {
    let mut by_day = [0.0f64; 32];
    for txn in &app.data.transactions {
        if month_of(txn.date) == month && app.counts(txn) && !txn.is_income() {
            by_day[txn.date.day() as usize] -= txn.amount;
        }
    }
    let mut total = 0.0;
    (1..=through_day)
        .map(|day| {
            total += by_day[day as usize];
            (day as f64, total)
        })
        .collect()
}

fn draw_spending(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.data.today();
    let this_month = month_of(today);
    let last_month = this_month.checked_sub_months(Months::new(1)).unwrap_or(this_month);
    let last_month_days = (this_month - last_month).num_days() as u32;

    let current = cumulative_spending(app, this_month, today.day());
    let previous = cumulative_spending(app, last_month, last_month_days);
    let spent = current.last().map(|p| p.1).unwrap_or(0.0);
    // Compare against the same point in last month, not its final total
    let previous_to_date = previous
        .get(today.day().min(last_month_days) as usize - 1)
        .map(|p| p.1)
        .unwrap_or(0.0);

    let note = Line::from(vec![
        Span::styled("━ ", theme::accent()),
        Span::styled("this month  ", theme::muted()),
        Span::styled("━ ", theme::muted()),
        Span::styled("last month ", theme::muted()),
    ]);
    let block = card_with_note("Spending", note);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [summary, chart] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    let difference = spent - previous_to_date;
    let comparison = if difference <= 0.0 {
        Span::styled(format!("{} less than last month", fmt::money0(-difference)), theme::gain(1.0))
    } else {
        Span::styled(format!("{} more than last month", fmt::money0(difference)), theme::gain(-1.0))
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(fmt::money0(spent), theme::bold()),
                Span::styled(format!("  spent in {}", today.format("%B")), theme::muted()),
            ]),
            Line::from(comparison),
        ]),
        summary,
    );
    widgets::line_chart(
        frame,
        chart,
        &[
            Series { points: &previous, color: theme::MUTED },
            Series { points: &current, color: theme::ACCENT },
        ],
        vec!["1".to_string(), "15".to_string(), last_month_days.to_string()],
    );
}

/// Compact income / spending / savings line for narrow terminals
fn draw_cash_flow_summary(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.data.today();
    let block = card(&format!("{} Cash Flow", today.format("%B")));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(flow) = app.months().into_iter().find(|m| m.month == month_of(today)) else {
        frame.render_widget(Line::styled("No transactions this month", theme::muted()), inner);
        return;
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Income ", theme::muted()),
            Span::styled(fmt::money0(flow.income), theme::gain(1.0)),
            Span::styled("   Spending ", theme::muted()),
            Span::styled(fmt::money0(flow.expense), theme::text()),
            Span::styled("   Saved ", theme::muted()),
            Span::styled(fmt::money0(flow.savings()), theme::gain(flow.savings())),
        ]),
        Line::from(vec![
            Span::styled("Savings rate ", theme::muted()),
            Span::styled(format!("{:.0}%", flow.savings_rate()), theme::text()),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_transactions(frame: &mut Frame, area: Rect, app: &App) {
    let review = app.data.transactions.iter().filter(|t| t.needs_review).count();
    let note = if review > 0 {
        Line::styled(format!("{} to review ", review), theme::accent())
    } else {
        Line::styled("Most recent ", theme::muted())
    };
    let block = card_with_note("Transactions", note);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    const DATE_WIDTH: usize = 7;
    const AMOUNT_WIDTH: usize = 12;
    let name_width = width.saturating_sub(DATE_WIDTH + AMOUNT_WIDTH);
    let lines: Vec<Line> = app
        .data
        .transactions
        .iter()
        .take(inner.height as usize)
        .map(|t| {
            Line::from(vec![
                Span::styled(format!("{:<w$}", fmt::short_date(t.date), w = DATE_WIDTH), theme::muted()),
                Span::styled(format!("{:<w$}", fmt::trunc(&t.merchant, name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(format!("{:>w$}", fmt::signed_money(t.amount), w = AMOUNT_WIDTH), theme::amount(t.amount)),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_recurring(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.data.today();
    let horizon = today + chrono::Duration::days(UPCOMING_DAYS);
    // Transfers between your own accounts are not bills
    let upcoming: Vec<_> = forecast::upcoming_all(&app.data.streams, today, horizon)
        .into_iter()
        .filter(|u| u.group_type != "transfer")
        .collect();
    let due: f64 = upcoming.iter().filter(|u| u.amount < 0.0).map(|u| -u.amount).sum();
    let block = card_with_note(
        "Recurring",
        Line::styled(format!("{} due in {}d ", fmt::money0(due), UPCOMING_DAYS), theme::muted()),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    const DATE_WIDTH: usize = 7;
    const AMOUNT_WIDTH: usize = 12;
    let name_width = width.saturating_sub(DATE_WIDTH + AMOUNT_WIDTH);
    let lines: Vec<Line> = upcoming
        .iter()
        .take(inner.height as usize)
        .map(|u| {
            let date_style = if u.late { Style::new().fg(theme::YELLOW) } else { theme::muted() };
            // A tilde marks an estimate for payments whose amount varies
            let amount = format!("{}{}", if u.variable { "~" } else { "" }, fmt::money0(u.amount.abs()));
            Line::from(vec![
                Span::styled(format!("{:<w$}", fmt::short_date(u.date), w = DATE_WIDTH), date_style),
                Span::styled(format!("{:<w$}", fmt::trunc(&u.merchant, name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(format!("{:>w$}", amount, w = AMOUNT_WIDTH), theme::amount(u.amount)),
            ])
        })
        .collect();
    if lines.is_empty() {
        frame.render_widget(Line::styled("Nothing upcoming", theme::muted()), inner);
    } else {
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn draw_investments(frame: &mut Frame, area: Rect, app: &App) {
    let portfolio = app.data.portfolio;
    let block = card_with_note(
        "Investments",
        Line::styled(format!("{} today ", fmt::signed_pct(portfolio.one_day_change_pct)), theme::gain(portfolio.one_day_change_pct)),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    const VALUE_WIDTH: usize = 11;
    let name_width = width.saturating_sub(VALUE_WIDTH);
    let mut lines = vec![Line::styled(fmt::money0(portfolio.total_value), theme::bold())];
    for holding in app.data.holdings.iter().take((inner.height as usize).saturating_sub(1)) {
        let label = if holding.ticker.is_empty() { &holding.name } else { &holding.ticker };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", fmt::trunc(label, name_width.saturating_sub(1)), w = name_width), theme::text()),
            Span::styled(format!("{:>w$}", fmt::money0(holding.value), w = VALUE_WIDTH), theme::muted()),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Credit score and goals share the last card
fn draw_extras(frame: &mut Frame, area: Rect, app: &App) {
    let block = card("Credit Score & Goals");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    if let Some((date, score)) = app.data.credit_scores.last() {
        let mut spans = vec![
            Span::styled(score.to_string(), theme::bold()),
            Span::styled(format!("  as of {}", fmt::short_date(*date)), theme::muted()),
        ];
        if let Some((_, first)) = app.data.credit_scores.first() {
            let delta = score - first;
            if delta != 0 {
                spans.push(Span::styled(format!("  {:+}", delta), theme::gain(delta as f64)));
            }
        }
        lines.push(Line::from(spans));
    } else {
        lines.push(Line::styled("No credit score", theme::muted()));
    }
    lines.push(Line::from(""));
    for goal in super::goals::goals(app) {
        lines.push(Line::from(vec![
            Span::styled(fmt::trunc(&goal.name, (inner.width as usize).saturating_sub(12)), theme::text()),
            Span::styled(format!("  {}", fmt::money0(goal.balance)), theme::muted()),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
