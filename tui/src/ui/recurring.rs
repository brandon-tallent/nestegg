//! Recurring: bills, subscriptions and income inferred from transaction history

use super::widgets::{self, card_with_note};
use crate::app::{App, Screen};
use crate::fmt;
use crate::forecast::{self, Stream};
use crate::theme;
use chrono::NaiveDate;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const DATE_WIDTH: usize = 9;
const CADENCE_WIDTH: usize = 15;
const ACCOUNT_WIDTH: usize = 22;
const LAST_WIDTH: usize = 9;
const AMOUNT_WIDTH: usize = 14;

/// Narrower than this, the account and last-paid columns are dropped
const FULL_TABLE_MIN_WIDTH: usize = 100;

/// Days within which an upcoming payment is called out as due soon
const DUE_SOON_DAYS: i64 = 7;

/// A stream with the next payment expected from it
pub struct Row<'a> {
    pub stream: &'a Stream,
    pub next: NaiveDate,
    pub late: bool,
}

/// Detected streams that pass the search, soonest next payment first
pub fn rows(app: &App) -> Vec<Row<'_>> {
    let today = app.data.today();
    let query = app.query(Screen::Recurring);
    let mut rows: Vec<Row> = app
        .data
        .streams
        .iter()
        .filter(|s| {
            let account = app.data.account_name(&s.account_id);
            query.matches(&[&s.merchant, account, s.cadence.label(), &s.group_type], Some(s.amount))
        })
        .map(|stream| {
            let (next, late) = stream.next_expected(today);
            Row { stream, next, late }
        })
        .collect();
    rows.sort_by(|a, b| a.next.cmp(&b.next).then(a.stream.merchant.cmp(&b.stream.merchant)));
    rows
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.data.today();
    let month_end = forecast::month_end(today);
    let rows = rows(app);

    // Transfers between your own accounts are neither a bill nor income
    let (mut still_due, mut still_expected) = (0.0, 0.0);
    for payment in forecast::upcoming_all(&app.data.streams, today, month_end) {
        if payment.group_type == "transfer" {
            continue;
        }
        if payment.amount < 0.0 {
            still_due -= payment.amount;
        } else {
            still_expected += payment.amount;
        }
    }
    let (mut monthly_bills, mut monthly_income) = (0.0, 0.0);
    for stream in app.data.streams.iter().filter(|s| !s.is_transfer()) {
        if stream.is_income() {
            monthly_income += stream.monthly_amount();
        } else if stream.amount < 0.0 {
            monthly_bills -= stream.monthly_amount();
        }
    }

    let [summary, table] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!(" Still due in {} ", today.format("%B")), theme::muted()),
                Span::styled(fmt::money0(still_due), theme::bold()),
                Span::styled("    Income still expected ", theme::muted()),
                Span::styled(fmt::money0(still_expected), theme::gain(1.0)),
            ]),
            Line::from(vec![
                Span::styled(" Recurring bills average ", theme::muted()),
                Span::styled(format!("{}/mo", fmt::money0(monthly_bills)), theme::text()),
                Span::styled("    Recurring income ", theme::muted()),
                Span::styled(format!("{}/mo", fmt::money0(monthly_income)), theme::text()),
            ]),
        ]),
        summary,
    );

    let block = card_with_note(
        "Recurring",
        Line::styled(format!("{} streams found in your history ", rows.len()), theme::muted()),
    );
    let inner = block.inner(table);
    frame.render_widget(block, table);
    let width = inner.width as usize;
    let full = width >= FULL_TABLE_MIN_WIDTH;
    let fixed = DATE_WIDTH + CADENCE_WIDTH + AMOUNT_WIDTH + if full { ACCOUNT_WIDTH + LAST_WIDTH } else { 0 };
    let name_width = width.saturating_sub(fixed);

    let mut header = vec![
        Span::styled(format!("{:<w$}", "Next", w = DATE_WIDTH), theme::header()),
        Span::styled(format!("{:<w$}", "Name", w = name_width), theme::header()),
        Span::styled(format!("{:<w$}", "Frequency", w = CADENCE_WIDTH), theme::header()),
    ];
    if full {
        header.push(Span::styled(format!("{:<w$}", "Account", w = ACCOUNT_WIDTH), theme::header()));
        header.push(Span::styled(format!("{:<w$}", "Last", w = LAST_WIDTH), theme::header()));
    }
    header.push(Span::styled(format!("{:>w$}", "Amount", w = AMOUNT_WIDTH), theme::header()));
    let [header_area, rows_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Line::from(header), header_area);

    if rows.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No recurring payments found yet"), theme::muted()), rows_area);
        return;
    }
    let (start, end) = widgets::window(app.recurring_selected, rows.len(), rows_area.height as usize);
    let lines: Vec<Line> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let stream = row.stream;
            let date_style = if row.late {
                Style::new().fg(theme::YELLOW)
            } else if (row.next - today).num_days() <= DUE_SOON_DAYS {
                theme::accent()
            } else {
                theme::text()
            };
            let mut name = stream.merchant.clone();
            if row.late {
                name.push_str(" · not seen yet");
            } else if stream.is_transfer() {
                name.push_str(" · transfer");
            }
            let mut spans = vec![
                Span::styled(format!("{:<w$}", fmt::short_date(row.next), w = DATE_WIDTH), date_style),
                Span::styled(format!("{:<w$}", fmt::trunc(&name, name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(format!("{:<w$}", stream.cadence.label(), w = CADENCE_WIDTH), theme::muted()),
            ];
            if full {
                spans.push(Span::styled(
                    format!("{:<w$}", fmt::trunc(app.data.account_name(&stream.account_id), ACCOUNT_WIDTH - 1), w = ACCOUNT_WIDTH),
                    theme::muted(),
                ));
                spans.push(Span::styled(format!("{:<w$}", fmt::short_date(stream.last_date), w = LAST_WIDTH), theme::muted()));
            }
            // A tilde marks an estimate for payments whose amount varies
            let amount = format!("{}{}", if stream.variable { "~" } else { "" }, fmt::signed_money(stream.amount));
            spans.push(Span::styled(format!("{:>w$}", amount, w = AMOUNT_WIDTH), theme::amount(stream.amount)));
            let line = Line::from(spans);
            if start + offset == app.recurring_selected {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}
