//! Transactions: the full history, searchable and filterable

use super::widgets::{self, card};
use crate::app::App;
use crate::data::Txn;
use crate::fmt;
use crate::theme;
use chrono::Datelike;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

const FLAG_WIDTH: usize = 2;
const DATE_WIDTH: usize = 11;
const CATEGORY_WIDTH: usize = 22;
const ACCOUNT_WIDTH: usize = 22;
const AMOUNT_WIDTH: usize = 13;

/// Narrower than this, the account column is dropped
const ACCOUNT_COLUMN_MIN_WIDTH: usize = 96;

const DETAIL_WIDTH: u16 = 64;
const DETAIL_LABEL_WIDTH: usize = 14;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let [filters, table] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    draw_filters(frame, filters, app);
    draw_table(frame, table, app);
    if app.tx.detail {
        if let Some(txn) = app.selected_transaction() {
            draw_detail(frame, area, app, txn);
        }
    }
}

fn draw_filters(frame: &mut Frame, area: Rect, app: &App) {
    let tx = &app.tx;
    let search = if app.searching {
        vec![
            Span::styled(" / ", theme::accent()),
            Span::styled(tx.search.clone(), theme::text()),
            Span::styled("▏", theme::accent()),
        ]
    } else if tx.search.is_empty() {
        vec![Span::styled(" / to search", theme::muted())]
    } else {
        vec![Span::styled(" / ", theme::muted()), Span::styled(tx.search.clone(), theme::accent())]
    };
    let mut first = search;
    first.push(Span::styled("    Filter ", theme::muted()));
    let filter_style = if tx.filter_index == 0 { theme::text() } else { theme::accent() };
    first.push(Span::styled(tx.filter().label(), filter_style));
    if let Some(id) = &tx.account {
        first.push(Span::styled("    Account ", theme::muted()));
        first.push(Span::styled(app.data.account_name(id).to_string(), theme::accent()));
    }
    if let Some(category) = &tx.category {
        first.push(Span::styled("    Category ", theme::muted()));
        first.push(Span::styled(category.clone(), theme::accent()));
    }
    if let Some(month) = tx.month {
        first.push(Span::styled("    Month ", theme::muted()));
        first.push(Span::styled(month.format("%b %Y").to_string(), theme::accent()));
    }

    let (mut money_in, mut money_out) = (0.0, 0.0);
    for txn in tx.visible.iter().filter_map(|i| app.data.transactions.get(*i)) {
        if txn.amount > 0.0 {
            money_in += txn.amount;
        } else {
            money_out -= txn.amount;
        }
    }
    let second = Line::from(vec![
        Span::styled(format!(" {} transactions", fmt::count(tx.visible.len())), theme::muted()),
        Span::styled("    In ", theme::muted()),
        Span::styled(fmt::money0(money_in), theme::gain(1.0)),
        Span::styled("    Out ", theme::muted()),
        Span::styled(fmt::money0(money_out), theme::text()),
    ]);
    frame.render_widget(Paragraph::new(vec![Line::from(first), second]), area);
}

fn draw_table(frame: &mut Frame, area: Rect, app: &App) {
    let block = card("Transactions");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let show_account = width >= ACCOUNT_COLUMN_MIN_WIDTH;
    let fixed = FLAG_WIDTH + DATE_WIDTH + CATEGORY_WIDTH + AMOUNT_WIDTH + if show_account { ACCOUNT_WIDTH } else { 0 };
    let merchant_width = width.saturating_sub(fixed);

    let mut header = vec![
        Span::raw(" ".repeat(FLAG_WIDTH)),
        Span::styled(format!("{:<w$}", "Date", w = DATE_WIDTH), theme::header()),
        Span::styled(format!("{:<w$}", "Merchant", w = merchant_width), theme::header()),
        Span::styled(format!("{:<w$}", "Category", w = CATEGORY_WIDTH), theme::header()),
    ];
    if show_account {
        header.push(Span::styled(format!("{:<w$}", "Account", w = ACCOUNT_WIDTH), theme::header()));
    }
    header.push(Span::styled(format!("{:>w$}", "Amount", w = AMOUNT_WIDTH), theme::header()));

    let [header_area, rows_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Line::from(header), header_area);

    if app.tx.visible.is_empty() {
        frame.render_widget(Line::styled("No transactions match", theme::muted()), rows_area);
        return;
    }

    let this_year = app.data.today().year();
    let (start, end) = widgets::window(app.tx.selected, app.tx.visible.len(), rows_area.height as usize);
    let lines: Vec<Line> = (start..end)
        .filter_map(|row| app.tx.visible.get(row).map(|i| (row, &app.data.transactions[*i])))
        .map(|(row, txn)| {
            let flag = if txn.needs_review {
                Span::styled("● ", theme::accent())
            } else if txn.pending {
                Span::styled("◌ ", theme::muted())
            } else if txn.one_time {
                Span::styled("◆ ", Style::new().fg(theme::YELLOW))
            } else if txn.recurring {
                Span::styled("↻ ", theme::muted())
            } else {
                Span::raw("  ")
            };
            let date = if txn.date.year() == this_year {
                fmt::short_date(txn.date)
            } else {
                txn.date.format("%b %-d '%y").to_string()
            };
            let dim = txn.hide_from_reports || txn.pending;
            let name_style = if dim { theme::muted() } else { theme::text() };
            let mut spans = vec![
                flag,
                Span::styled(format!("{:<w$}", date, w = DATE_WIDTH), theme::muted()),
                Span::styled(
                    format!("{:<w$}", fmt::trunc(&txn.merchant, merchant_width.saturating_sub(1)), w = merchant_width),
                    name_style,
                ),
                Span::styled(
                    format!("{:<w$}", fmt::trunc(&txn.category, CATEGORY_WIDTH - 1), w = CATEGORY_WIDTH),
                    theme::muted(),
                ),
            ];
            if show_account {
                spans.push(Span::styled(
                    format!("{:<w$}", fmt::trunc(app.data.account_name(&txn.account_id), ACCOUNT_WIDTH - 1), w = ACCOUNT_WIDTH),
                    theme::muted(),
                ));
            }
            spans.push(Span::styled(
                format!("{:>w$}", fmt::signed_money(txn.amount), w = AMOUNT_WIDTH),
                theme::amount(txn.amount),
            ));
            let line = Line::from(spans);
            if row == app.tx.selected {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App, txn: &Txn) {
    let field = |label: &str, value: String, style: Style| {
        Line::from(vec![
            Span::styled(format!("{:<w$}", label, w = DETAIL_LABEL_WIDTH), theme::muted()),
            Span::styled(value, style),
        ])
    };
    let mut flags = Vec::new();
    for (set, name) in [
        (txn.pending, "pending"),
        (txn.needs_review, "needs review"),
        (txn.recurring, "recurring"),
        (txn.hide_from_reports, "hidden from reports"),
        (txn.one_time, "one-time"),
    ] {
        if set {
            flags.push(name);
        }
    }

    let mut lines = vec![
        Line::styled(fmt::signed_money(txn.amount), theme::amount(txn.amount).add_modifier(Modifier::BOLD)),
        Line::from(""),
        field("Merchant", txn.merchant.clone(), theme::text()),
        field("Date", fmt::long_date(txn.date), theme::text()),
        field("Category", format!("{} ({})", txn.category, txn.group_type), theme::text()),
        field("Account", app.data.account_name(&txn.account_id).to_string(), theme::text()),
        field("Statement", txn.original_name.clone(), theme::muted()),
    ];
    if !txn.tags.is_empty() {
        lines.push(field("Tags", txn.tags.clone(), theme::text()));
    }
    if !flags.is_empty() {
        lines.push(field("Status", flags.join(", "), Style::new().fg(theme::YELLOW)));
    }
    if !txn.notes.is_empty() {
        lines.push(field("Notes", txn.notes.clone(), theme::text()));
    }

    let popup = widgets::centered(area, DETAIL_WIDTH, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(card("Transaction")),
        popup,
    );
}
