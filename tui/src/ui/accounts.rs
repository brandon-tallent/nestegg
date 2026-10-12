//! Accounts: every account grouped by type, with balance history for the selection

use super::widgets::{self, card, card_with_note};
use super::WIDE;
use crate::app::App;
use crate::data::Account;
use crate::fmt;
use crate::forecast;
use crate::payoff;
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

const BALANCE_WIDTH: usize = 14;

/// Rows of recent transactions shown under the balance chart
const DETAIL_TRANSACTIONS: usize = 6;

/// Most expected payments listed before the rest are summarized
const DETAIL_UPCOMING: usize = 12;

/// Rows the balance chart keeps even when the lists below want more
const MIN_DETAIL_CHART_HEIGHT: u16 = 7;

/// Rows of the detail pane's headline block
const DETAIL_SUMMARY_HEIGHT: u16 = 3;

/// A payoff further out than this is stated but not drawn: the projection
/// would squeeze the balance history into a sliver of the chart
const MAX_CHARTED_PAYOFF_DAYS: i64 = 6 * 365;

/// Rows the stacked layout needs before the detail panel is shown
const MIN_HEIGHT_FOR_STACKED_DETAIL: u16 = 30;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let [summary, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    draw_summary(frame, summary, app);

    if area.width >= WIDE {
        let [list, detail] =
            Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).areas(body);
        draw_list(frame, list, app);
        draw_detail(frame, detail, app);
    } else if body.height >= MIN_HEIGHT_FOR_STACKED_DETAIL {
        let [list, detail] =
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(body);
        draw_list(frame, list, app);
        draw_detail(frame, detail, app);
    } else {
        draw_list(frame, body, app);
    }
}

fn draw_summary(frame: &mut Frame, area: Rect, app: &App) {
    let Some(latest) = app.data.latest_net_worth() else {
        return;
    };
    let line = Line::from(vec![
        Span::styled(" Net worth ", theme::muted()),
        Span::styled(fmt::money0(latest.net_worth), theme::bold()),
        Span::styled("    Assets ", theme::muted()),
        Span::styled(fmt::money0(latest.assets), theme::gain(1.0)),
        Span::styled("    Liabilities ", theme::muted()),
        Span::styled(fmt::money0(latest.liabilities.abs()), theme::gain(-1.0)),
    ]);
    frame.render_widget(line, area);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    let accounts = app.listed_accounts();
    let hidden = app.data.accounts.iter().filter(|a| a.is_hidden).count();
    let note = if app.show_hidden_accounts {
        "showing hidden ".to_string()
    } else if hidden > 0 {
        format!("{} hidden ", hidden)
    } else {
        String::new()
    };
    let block = card_with_note("Accounts", Line::styled(note, theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;

    // Group headers are lines but not selectable, so track which line each account lands on
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0;
    let mut previous_kind: Option<&str> = None;
    for (i, account) in accounts.iter().enumerate() {
        if previous_kind != Some(account.kind.as_str()) {
            if previous_kind.is_some() {
                lines.push(Line::from(""));
            }
            let total: f64 = accounts
                .iter()
                .filter(|a| a.kind == account.kind && a.include_in_net_worth)
                .map(|a| a.balance)
                .sum();
            lines.push(widgets::spread(
                vec![Span::styled(account.kind_display.clone(), theme::accent())],
                vec![Span::styled(fmt::money0(total), theme::bold())],
                width,
            ));
            previous_kind = Some(account.kind.as_str());
        }
        if i == app.account_selected {
            selected_line = lines.len();
        }
        lines.push(account_line(account, width, i == app.account_selected));
    }

    if lines.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No accounts"), theme::muted()), inner);
        return;
    }
    let (start, end) = widgets::window(selected_line, lines.len(), inner.height as usize);
    frame.render_widget(Paragraph::new(lines[start..end].to_vec()), inner);
}

fn account_line(account: &Account, width: usize, selected: bool) -> Line<'static> {
    let dim = account.is_hidden || !account.include_in_net_worth;
    let name_style = if dim { theme::muted() } else { theme::text() };
    let marker = if account.needs_attention {
        Span::styled("! ", Style::new().fg(theme::YELLOW).add_modifier(Modifier::BOLD))
    } else {
        Span::raw("  ")
    };
    let mask = account.mask.as_deref().map(|m| format!(" ··{}", m)).unwrap_or_default();
    let name_width = width.saturating_sub(BALANCE_WIDTH + 2);
    let label = format!("{}{}", account.name, mask);
    let detail = if account.subtype.is_empty() { String::new() } else { format!("  {}", account.subtype) };
    let shown = fmt::trunc(&label, name_width);
    let remaining = name_width.saturating_sub(shown.chars().count());

    let mut line = Line::from(vec![
        marker,
        Span::styled(shown, name_style),
        Span::styled(format!("{:<w$}", fmt::trunc(&detail, remaining), w = remaining), theme::muted()),
        Span::styled(format!("{:>w$}", fmt::money(account.balance), w = BALANCE_WIDTH), name_style),
    ]);
    if selected {
        line = line.style(theme::selected());
    }
    line
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let Some(account) = app.selected_account() else {
        frame.render_widget(card("Account"), area);
        return;
    };
    let block = card_with_note(&account.name, Line::styled(format!("{} ", account.institution), theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let transactions: Vec<_> = app
        .data
        .transactions
        .iter()
        .filter(|t| t.account_id == account.id)
        .take(DETAIL_TRANSACTIONS)
        .collect();
    let today = app.data.today();
    let month_end = forecast::month_end(today);
    let upcoming = forecast::upcoming_for_account(&app.data.streams, &account.id, today, month_end);

    // The chart gives way to the lists, but only down to a readable height
    let recent_height = transactions.len() as u16 + 1;
    let wanted_upcoming = upcoming.len().min(DETAIL_UPCOMING) as u16 + 1;
    let spare = inner
        .height
        .saturating_sub(DETAIL_SUMMARY_HEIGHT + MIN_DETAIL_CHART_HEIGHT + recent_height);
    let upcoming_height = if upcoming.is_empty() { 0 } else { wanted_upcoming.min(spare) };
    let [summary, chart, upcoming_area, recent] = Layout::vertical([
        Constraint::Length(DETAIL_SUMMARY_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(upcoming_height),
        Constraint::Length(recent_height),
    ])
    .areas(inner);

    let history = app
        .data
        .balances
        .get(&account.id)
        .map(|series| app.in_range(series, |p| p.0))
        .unwrap_or(&[]);
    // Liabilities are stored negative; show them the way the balance column does
    let sign = if account.is_asset { 1.0 } else { -1.0 };
    let points: Vec<_> = history.iter().map(|(date, balance)| (*date, balance * sign)).collect();

    let projection = if account.kind == "loan" {
        let owed: Vec<_> = app
            .data
            .balances
            .get(&account.id)
            .map(|series| series.iter().map(|(date, balance)| (*date, -balance)).collect())
            .unwrap_or_default();
        payoff::project(account.balance, account.interest_rate, account.minimum_payment, &owed, today)
    } else {
        None
    };

    let mut headline = vec![Span::styled(fmt::money(account.balance), theme::bold()), Span::raw("  ")];
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        let (delta, pct) = widgets::change(first.1, last.1);
        // Owing less is the good direction for a liability
        let mut spans = widgets::change_spans(delta, pct);
        if !account.is_asset {
            spans[0].style = theme::gain(-delta);
        }
        headline.extend(spans);
        headline.push(Span::styled(format!("  {}", app.range.label()), theme::muted()));
    }
    let mut facts = vec![Span::styled(
        format!("{} · {}", account.kind_display, if account.subtype.is_empty() { "—" } else { &account.subtype }),
        theme::muted(),
    )];
    if let Some(limit) = account.credit_limit.filter(|l| *l > 0.0) {
        facts.push(Span::styled(
            format!("   {:.0}% of {} limit", account.balance / limit * 100.0, fmt::money0(limit)),
            theme::muted(),
        ));
    }
    let mut status = Vec::new();
    if let Some(synced) = account.last_synced {
        status.push(Span::styled(format!("Updated {}", synced.format("%b %-d %H:%M UTC")), theme::muted()));
    }
    if account.needs_attention {
        status.push(Span::styled("   needs attention in Monarch", Style::new().fg(theme::YELLOW)));
    }
    if !account.include_in_net_worth {
        status.push(Span::styled("   excluded from net worth", theme::muted()));
    }
    if let Some(projection) = &projection {
        status.push(Span::styled(
            format!(
                "   Paid off {} at {}{}/mo",
                projection.payoff.format("%b %Y"),
                if projection.payment_inferred { "~" } else { "" },
                fmt::money0(projection.monthly_payment),
            ),
            theme::accent(),
        ));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(headline), Line::from(facts), Line::from(status)]),
        summary,
    );

    let color = if account.is_asset { theme::POSITIVE } else { theme::BLUE };
    match (&projection, points.first()) {
        (Some(projection), Some((first, _))) if (projection.payoff - today).num_days() <= MAX_CHARTED_PAYOFF_DAYS => {
            // History and the projected run to zero share one time axis
            let days = |date: chrono::NaiveDate| (date - *first).num_days() as f64;
            let history: Vec<(f64, f64)> = points.iter().map(|(date, value)| (days(*date), *value)).collect();
            let projected: Vec<(f64, f64)> =
                projection.points.iter().map(|(date, value)| (days(*date), *value)).collect();
            // A rule at today, where what happened gives way to what is expected
            let now = [(days(today), 0.0), (days(today), account.balance.abs())];
            widgets::line_chart(
                frame,
                chart,
                &[
                    widgets::Series { points: &now, color: theme::BORDER },
                    widgets::Series { points: &projected, color: theme::ACCENT },
                    widgets::Series { points: &history, color },
                ],
                // Labels sit at the ends of the axis, so only the ends are named
                vec![fmt::short_date(*first), projection.payoff.format("paid off %b %Y").to_string()],
            );
        }
        _ => widgets::date_chart(frame, chart, &points, color),
    }

    let width = recent.width as usize;
    const DATE_WIDTH: usize = 7;
    const AMOUNT_WIDTH: usize = 12;
    let name_width = width.saturating_sub(DATE_WIDTH + AMOUNT_WIDTH);

    // Header plus at least one row, or the section is not worth drawing
    if upcoming_height >= 2 {
        let net: f64 = upcoming.iter().map(|u| u.amount).sum();
        let rows = (upcoming_height as usize - 1).min(upcoming.len());
        // The last row becomes a summary when not everything fits
        let listed = if rows < upcoming.len() { rows.saturating_sub(1) } else { rows };
        let mut lines = vec![widgets::spread(
            vec![Span::styled(format!("Upcoming through {}", fmt::short_date(month_end)), theme::header())],
            vec![
                Span::styled("net ", theme::muted()),
                Span::styled(fmt::signed_money(net), theme::amount(net)),
            ],
            width,
        )];
        lines.extend(upcoming.iter().take(listed).map(|u| {
            let date_style = if u.late { Style::new().fg(theme::YELLOW) } else { theme::muted() };
            let mut label = format!("{} · {}", u.merchant, u.cadence.label().to_lowercase());
            if u.late {
                label.push_str(" · not seen yet");
            }
            // A tilde marks an estimate for bills whose amount varies
            let amount = format!("{}{}", if u.variable { "~" } else { "" }, fmt::signed_money(u.amount));
            Line::from(vec![
                Span::styled(format!("{:<w$}", fmt::short_date(u.date), w = DATE_WIDTH), date_style),
                Span::styled(format!("{:<w$}", fmt::trunc(&label, name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(format!("{:>w$}", amount, w = AMOUNT_WIDTH), theme::amount(u.amount)),
            ])
        }));
        if listed < upcoming.len() {
            let rest: f64 = upcoming.iter().skip(listed).map(|u| u.amount).sum();
            lines.push(widgets::spread(
                vec![Span::styled(format!("       + {} more", upcoming.len() - listed), theme::muted())],
                vec![Span::styled(fmt::signed_money(rest), theme::muted())],
                width,
            ));
        }
        frame.render_widget(Paragraph::new(lines), upcoming_area);
    }

    let mut lines = vec![Line::styled("Recent transactions", theme::header())];
    lines.extend(transactions.iter().map(|t| {
        Line::from(vec![
            Span::styled(format!("{:<w$}", fmt::short_date(t.date), w = DATE_WIDTH), theme::muted()),
            Span::styled(format!("{:<w$}", fmt::trunc(&t.merchant, name_width.saturating_sub(1)), w = name_width), theme::text()),
            Span::styled(format!("{:>w$}", fmt::signed_money(t.amount), w = AMOUNT_WIDTH), theme::amount(t.amount)),
        ])
    }));
    frame.render_widget(Paragraph::new(lines), recent);
}
