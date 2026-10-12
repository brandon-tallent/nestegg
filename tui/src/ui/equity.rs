//! Equity: option grants, shares held, the vesting ahead, and what it is all worth

use super::widgets::{self, card, card_with_note};
use super::WIDE;
use crate::app::{App, EquityView, Screen};
use crate::equity::{held_for, long_term_from, Certificate, Event, Exercise, Grant, Source, Split, Tender, VestEvent};
use crate::fmt;
use crate::theme;
use chrono::{Datelike, NaiveDate};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

const LABEL_WIDTH: usize = 9;
const PRICE_WIDTH: usize = 8;
const QUANTITY_WIDTH: usize = 11;
const VALUE_WIDTH: usize = 12;
const DATE_WIDTH: usize = 13;
const HELD_WIDTH: usize = 8;
const SOURCE_WIDTH: usize = 14;

/// Narrower than this, the grants table drops its granted and exercised columns
const FULL_GRANTS_MIN_WIDTH: usize = 84;

/// Rows of headline figures above the panels
const SUMMARY_HEIGHT: u16 = 5;
/// How many of the latest share prices the summary lists, at most
const MAX_PRICES_SHOWN: usize = 6;
const PRICES_LABEL: &str = " Share price ";
const PRICE_ARROW: &str = "  →  ";
const CHART_HEIGHT: u16 = 12;

/// Share prices the scenario table values the holdings at, as multiples of today's
const SCENARIOS: [f64; 5] = [0.5, 1.0, 2.0, 5.0, 10.0];

/// The card that opens over the screen for the selected row
const DETAIL_WIDTH: u16 = 84;
/// What fits inside it, between its borders and padding
const DETAIL_INNER_WIDTH: usize = DETAIL_WIDTH as usize - 4;
const DETAIL_LABEL_WIDTH: usize = 16;
/// One column of figures in a detail card's table
const DETAIL_COLUMN_WIDTH: usize = 13;

/// Columns of the table of certificates sold in a tender
const SOLD_FROM_WIDTH: usize = 11;
const SOLD_HELD_WIDTH: usize = 7;
const SOLD_TAX_WIDTH: usize = 13;
const SOLD_QUANTITY_WIDTH: usize = 9;
const SOLD_MONEY_WIDTH: usize = 10;
const SOLD_GAIN_WIDTH: usize = 11;

/// Carta's 83(b) status for a grant with nothing to file
const NO_83B: &str = "NONE_ELIGIBLE";

fn shares(quantity: f64) -> String {
    fmt::count(quantity.round().max(0.0) as usize)
}

fn price(value: f64) -> String {
    format!("${:.3}", value)
}

fn month_year(date: Option<NaiveDate>) -> String {
    date.map(|d| d.format("%b %Y").to_string()).unwrap_or_else(|| "—".to_string())
}

/// Live grants that pass the search
pub fn grants(app: &App) -> Vec<&Grant> {
    let query = app.query(Screen::Equity);
    app.data
        .equity
        .live_grants()
        .filter(|g| query.matches(&[&g.label, &g.option_type, &g.schedule, "option grant"], None))
        .collect()
}

/// Share certificates still held that pass the search
pub fn certificates(app: &App) -> Vec<&Certificate> {
    let query = app.query(Screen::Equity);
    let equity = &app.data.equity;
    equity
        .held_certificates()
        .filter(|c| {
            let origin = equity.origin(c);
            let words: [&str; 6] =
                [&c.label, &origin.exercised_from, &origin.exercise_type, &c.status, &c.share_class, "shares"];
            query.matches(&words, Some(c.quantity))
        })
        .collect()
}

/// Exercises and sales that pass the search
pub fn history(app: &App) -> Vec<Event> {
    let query = app.query(Screen::Equity);
    app.data
        .equity
        .history()
        .into_iter()
        .filter(|e| query.matches(&[&e.what, &e.detail], Some(e.cash)))
        .collect()
}

/// Rows in the list the cursor moves through
pub fn list_len(app: &App) -> usize {
    match app.equity_view {
        EquityView::Grants => grants(app).len(),
        EquityView::Shares => certificates(app).len(),
        EquityView::History => history(app).len(),
    }
}

/// The cursor's row in one of the screen's lists, when the cursor is in it
fn selected_in(app: &App, view: EquityView) -> Option<usize> {
    (app.equity_view == view).then_some(app.equity_selected)
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    if app.data.equity.is_empty() {
        let block = card("Equity");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Line::styled("No Carta data", theme::muted()), inner);
        return;
    }
    let [summary, body] = Layout::vertical([Constraint::Length(SUMMARY_HEIGHT), Constraint::Min(0)]).areas(area);
    draw_summary(frame, summary, app);

    let grants_height = grants(app).len().max(1) as u16 + 4;
    if area.width >= WIDE {
        let [left, right] = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(body);
        let [grants_area, list] = Layout::vertical([Constraint::Length(grants_height), Constraint::Min(0)]).areas(left);
        draw_grants(frame, grants_area, app);
        draw_list(frame, list, app);
        let scenarios_height = SCENARIOS.len() as u16 + 3;
        let [chart, upcoming, scenarios] = Layout::vertical([
            Constraint::Length(CHART_HEIGHT),
            Constraint::Min(0),
            Constraint::Length(scenarios_height),
        ])
        .areas(right);
        draw_chart(frame, chart, app);
        draw_upcoming(frame, upcoming, app);
        draw_scenarios(frame, scenarios, app);
    } else {
        let [grants_area, upcoming, list] = Layout::vertical([
            Constraint::Length(grants_height),
            Constraint::Percentage(35),
            Constraint::Min(0),
        ])
        .areas(body);
        draw_grants(frame, grants_area, app);
        draw_upcoming(frame, upcoming, app);
        draw_list(frame, list, app);
    }
    if app.equity_detail {
        draw_detail(frame, area, app);
    }
}

fn draw_summary(frame: &mut Frame, area: Rect, app: &App) {
    let equity = &app.data.equity;
    let summary = equity.summary(1.0);
    let mut company = Vec::new();
    if let Some(issuer) = equity.primary() {
        company.push(Span::styled(format!(" {}", issuer.name), theme::bold()));
        if let Some(fmv) = issuer.fmv {
            company.push(Span::styled("    FMV ", theme::muted()));
            company.push(Span::styled(format!("{}/share", price(fmv)), theme::text()));
        }
        if let Some(as_of) = issuer.as_of {
            company.push(Span::styled(format!("    as of {}", fmt::long_date(as_of)), theme::muted()));
        }
    }
    let figure = |label: &str, value: f64| {
        vec![Span::styled(format!("    {} ", label), theme::muted()), Span::styled(fmt::money0(value), theme::text())]
    };
    let mut values = vec![
        Span::styled(" Vested ", theme::muted()),
        Span::styled(fmt::money0(summary.vested_value()), theme::bold()),
    ];
    values.extend(figure("Shares", summary.shares_value));
    values.extend(figure("Vested options", summary.exercisable_value));
    values.extend(figure("Unvested", summary.unvested_value));
    values.extend(figure("All", summary.total_value()));

    let mut exercise = vec![
        Span::styled(" Exercise cost ", theme::muted()),
        Span::styled(fmt::money0(summary.cost_to_exercise), theme::text()),
        Span::styled(format!("    {} options", shares(summary.exercisable)), theme::muted()),
    ];
    if let Some(iso) = summary.exercisable_iso {
        exercise.push(Span::styled(
            format!(" ({} ISO, {} NSO)", shares(iso), shares(summary.exercisable - iso)),
            theme::muted(),
        ));
    }
    exercise.push(Span::styled("    pre-tax", theme::muted()));

    // The share price as the records show it changing: the latest, and as
    // many of those before it as the line has room for
    let mut prices = vec![Span::styled(PRICES_LABEL, theme::muted())];
    let history: Vec<(String, String)> = equity
        .price_history()
        .iter()
        .map(|(date, value)| (price(*value), format!(" {}", date.format("%b %Y"))))
        .collect();
    let mut room = (area.width as usize).saturating_sub(PRICES_LABEL.chars().count());
    let mut first = history.len();
    while first > 0 && history.len() - first < MAX_PRICES_SHOWN {
        let (value, when) = &history[first - 1];
        let arrow = if first < history.len() { PRICE_ARROW.chars().count() } else { 0 };
        let needed = value.chars().count() + when.chars().count() + arrow;
        if needed > room {
            break;
        }
        room -= needed;
        first -= 1;
    }
    let shown = &history[first..];
    for (i, (value, when)) in shown.iter().enumerate() {
        if i > 0 {
            prices.push(Span::styled(PRICE_ARROW, theme::muted()));
        }
        let last = i + 1 == shown.len();
        prices.push(Span::styled(value.clone(), if last { theme::text() } else { theme::muted() }));
        prices.push(Span::styled(when.clone(), theme::muted()));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(company), Line::from(values), Line::from(exercise), Line::from(prices)]),
        area,
    );
}

fn draw_grants(frame: &mut Frame, area: Rect, app: &App) {
    let equity = &app.data.equity;
    let grants = grants(app);
    let selected = selected_in(app, EquityView::Grants);
    let block = card_with_note("Option Grants", Line::styled("FMV − strike ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let full = width >= FULL_GRANTS_MIN_WIDTH;

    let cell = |text: String, w: usize| format!("{:>w$}", text, w = w);
    let mut header = vec![
        Span::styled(format!("{:<w$}", "Grant", w = LABEL_WIDTH), theme::header()),
        Span::styled(cell("Strike".into(), PRICE_WIDTH), theme::header()),
    ];
    if full {
        header.push(Span::styled(cell("Granted".into(), QUANTITY_WIDTH), theme::header()));
        header.push(Span::styled(cell("Exercised".into(), QUANTITY_WIDTH), theme::header()));
    }
    header.push(Span::styled(cell("Exercisable".into(), QUANTITY_WIDTH + 1), theme::header()));
    header.push(Span::styled(cell("Unvested".into(), QUANTITY_WIDTH), theme::header()));
    header.push(Span::styled(cell("Value".into(), VALUE_WIDTH), theme::header()));
    header.push(Span::styled(cell("Vests by".into(), DATE_WIDTH - 2), theme::header()));
    let mut lines = vec![Line::from(header)];

    if grants.is_empty() {
        lines.push(Line::styled(widgets::no_matches(app, "No option grants"), theme::muted()));
    }
    let fmv = |grant: &Grant| equity.issuer(grant.issuer_id).and_then(|i| i.fmv).unwrap_or(0.0);
    let (mut exercisable, mut unvested, mut value) = (0.0, 0.0, 0.0);
    for (index, grant) in grants.iter().enumerate() {
        let spread = (fmv(grant) - grant.exercise_price).max(0.0);
        let worth = grant.exercisable * spread;
        exercisable += grant.exercisable;
        unvested += grant.unvested();
        value += worth;
        let mut spans = vec![
            Span::styled(format!("{:<w$}", fmt::trunc(&grant.label, LABEL_WIDTH - 1), w = LABEL_WIDTH), theme::accent()),
            Span::styled(cell(price(grant.exercise_price), PRICE_WIDTH), theme::muted()),
        ];
        if full {
            spans.push(Span::styled(cell(shares(grant.quantity), QUANTITY_WIDTH), theme::muted()));
            spans.push(Span::styled(cell(shares(grant.exercised), QUANTITY_WIDTH), theme::muted()));
        }
        spans.push(Span::styled(cell(shares(grant.exercisable), QUANTITY_WIDTH + 1), theme::text()));
        spans.push(Span::styled(cell(shares(grant.unvested()), QUANTITY_WIDTH), theme::muted()));
        spans.push(Span::styled(cell(fmt::money0(worth), VALUE_WIDTH), theme::text()));
        let vests = if grant.unvested() > 0.0 { month_year(grant.vesting_end) } else { "vested".to_string() };
        spans.push(Span::styled(cell(vests, DATE_WIDTH - 2), theme::muted()));
        let line = Line::from(spans);
        lines.push(if selected == Some(index) { line.style(theme::selected()) } else { line });
    }
    if grants.len() > 1 {
        let skipped = LABEL_WIDTH + PRICE_WIDTH + if full { QUANTITY_WIDTH * 2 } else { 0 };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", "Total", w = skipped), theme::bold()),
            Span::styled(cell(shares(exercisable), QUANTITY_WIDTH + 1), theme::bold()),
            Span::styled(cell(shares(unvested), QUANTITY_WIDTH), theme::bold()),
            Span::styled(cell(fmt::money0(value), VALUE_WIDTH), theme::bold()),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_list(frame: &mut Frame, area: Rect, app: &App) {
    match app.equity_view {
        EquityView::Grants | EquityView::Shares => draw_shares(frame, area, app),
        EquityView::History => draw_history(frame, area, app),
    }
}

fn draw_shares(frame: &mut Frame, area: Rect, app: &App) {
    let equity = &app.data.equity;
    let today = app.data.today();
    let certificates = certificates(app);
    let selected = selected_in(app, EquityView::Shares);
    let block = card_with_note("Shares Held", Line::styled("v: exercises and sales ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [header_area, rows_area, total_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(inner);

    let cell = |text: String, w: usize| format!("{:>w$}", text, w = w);
    let width = inner.width as usize;
    let show_source = width >= LABEL_WIDTH + SOURCE_WIDTH + DATE_WIDTH + HELD_WIDTH + QUANTITY_WIDTH + VALUE_WIDTH * 2;
    let mut header = vec![Span::styled(format!("{:<w$}", "Shares", w = LABEL_WIDTH), theme::header())];
    if show_source {
        header.push(Span::styled(format!("{:<w$}", "From", w = SOURCE_WIDTH), theme::header()));
    }
    header.push(Span::styled(format!("{:<w$}", "Acquired", w = DATE_WIDTH), theme::header()));
    header.push(Span::styled(format!("{:<w$}", "Held", w = HELD_WIDTH), theme::header()));
    header.push(Span::styled(cell("Quantity".into(), QUANTITY_WIDTH), theme::header()));
    header.push(Span::styled(cell("Cost".into(), VALUE_WIDTH), theme::header()));
    header.push(Span::styled(cell("Value".into(), VALUE_WIDTH), theme::header()));
    frame.render_widget(Line::from(header), header_area);

    if certificates.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No shares held"), theme::muted()), rows_area);
        return;
    }
    let (start, end) = widgets::window(selected.unwrap_or(0), certificates.len(), rows_area.height as usize);
    let lines: Vec<Line> = certificates[start..end]
        .iter()
        .enumerate()
        .map(|(offset, c)| {
            let fmv = equity.issuer(c.issuer_id).and_then(|i| i.fmv).unwrap_or(0.0);
            let source = source(equity.origin(c));
            let long_term = c.acquired.and_then(long_term_from).is_some_and(|from| today >= from);
            let held = c.acquired.map(|d| held_for(d, today)).unwrap_or_default();
            let mut spans = vec![Span::styled(
                format!("{:<w$}", fmt::trunc(&c.label, LABEL_WIDTH - 1), w = LABEL_WIDTH),
                theme::accent(),
            )];
            if show_source {
                spans.push(Span::styled(
                    format!("{:<w$}", fmt::trunc(&source, SOURCE_WIDTH - 1), w = SOURCE_WIDTH),
                    theme::muted(),
                ));
            }
            spans.push(Span::styled(
                format!("{:<w$}", c.acquired.map(fmt::long_date).unwrap_or_default(), w = DATE_WIDTH),
                theme::muted(),
            ));
            // Under a year, a sale's gain is taxed as short-term
            spans.push(Span::styled(
                format!("{:<w$}", held, w = HELD_WIDTH),
                if long_term { theme::muted() } else { theme::accent() },
            ));
            spans.push(Span::styled(cell(shares(c.quantity), QUANTITY_WIDTH), theme::text()));
            spans.push(Span::styled(
                cell(equity.cost_of(c).map(fmt::money0).unwrap_or_else(|| "—".to_string()), VALUE_WIDTH),
                theme::muted(),
            ));
            spans.push(Span::styled(cell(fmt::money0(c.quantity * fmv), VALUE_WIDTH), theme::text()));
            let line = Line::from(spans);
            if selected == Some(start + offset) {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);

    let quantity: f64 = certificates.iter().map(|c| c.quantity).sum();
    let cost: f64 = certificates.iter().filter_map(|c| equity.cost_of(c)).sum();
    let value: f64 = certificates
        .iter()
        .map(|c| c.quantity * equity.issuer(c.issuer_id).and_then(|i| i.fmv).unwrap_or(0.0))
        .sum();
    let skipped = LABEL_WIDTH + DATE_WIDTH + HELD_WIDTH + if show_source { SOURCE_WIDTH } else { 0 };
    frame.render_widget(
        Line::from(vec![
            Span::styled(format!("{:<w$}", "Total", w = skipped), theme::bold()),
            Span::styled(cell(shares(quantity), QUANTITY_WIDTH), theme::bold()),
            Span::styled(cell(fmt::money0(cost), VALUE_WIDTH), theme::bold()),
            Span::styled(cell(fmt::money0(value), VALUE_WIDTH), theme::bold()),
        ]),
        total_area,
    );
}

/// Where shares came from: the grant exercised and whether as ISOs or NSOs
fn source(origin: &Certificate) -> String {
    if origin.exercised_from.is_empty() {
        "Earlier shares".to_string()
    } else {
        format!("{} {}", origin.exercised_from, origin.exercise_type)
    }
}

fn draw_history(frame: &mut Frame, area: Rect, app: &App) {
    let events = history(app);
    let block = card_with_note("Exercises and Sales", Line::styled("v: shares held ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [header_area, rows_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    let width = inner.width as usize;
    let what_width = width.saturating_sub(DATE_WIDTH + QUANTITY_WIDTH + VALUE_WIDTH);

    frame.render_widget(
        Line::from(vec![
            Span::styled(format!("{:<w$}", "Date", w = DATE_WIDTH), theme::header()),
            Span::styled(format!("{:<w$}", "What", w = what_width), theme::header()),
            Span::styled(format!("{:>w$}", "Shares", w = QUANTITY_WIDTH), theme::header()),
            Span::styled(format!("{:>w$}", "Cash", w = VALUE_WIDTH), theme::header()),
        ]),
        header_area,
    );
    if events.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No exercises or sales"), theme::muted()), rows_area);
        return;
    }
    let (start, end) = widgets::window(app.equity_selected, events.len(), rows_area.height as usize);
    let lines: Vec<Line> = events[start..end]
        .iter()
        .enumerate()
        .map(|(offset, event)| {
            let what = format!("{}  {}", event.what, event.detail);
            let line = Line::from(vec![
                Span::styled(
                    format!("{:<w$}", event.date.map(fmt::long_date).unwrap_or_default(), w = DATE_WIDTH),
                    theme::muted(),
                ),
                Span::styled(format!("{:<w$}", fmt::trunc(&what, what_width.saturating_sub(1)), w = what_width), theme::text()),
                Span::styled(format!("{:>w$}", shares(event.shares), w = QUANTITY_WIDTH), theme::muted()),
                Span::styled(format!("{:>w$}", fmt::money0(event.cash), w = VALUE_WIDTH), theme::amount(event.cash)),
            ]);
            if start + offset == app.equity_selected {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let block = card_with_note("Exercisable Options", Line::styled("at FMV ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let curve = app.data.equity.vesting_curve();
    let (Some(first), Some(last)) = (curve.first(), curve.last()) else {
        frame.render_widget(Line::styled("No vesting schedule", theme::muted()), inner);
        return;
    };
    let today = app.data.today();
    let days = |date: NaiveDate| (date - first.0).num_days() as f64;
    // Vesting happens in steps: hold each level until the next tranche lands
    let mut vested: Vec<(f64, f64)> = Vec::new();
    let mut ahead: Vec<(f64, f64)> = Vec::new();
    let mut level = 0.0;
    for (date, value) in &curve {
        let series = if *date <= today { &mut vested } else { &mut ahead };
        if series.is_empty() && *date > today {
            series.push((days(today), level));
        }
        series.push((days(*date), level));
        series.push((days(*date), *value));
        level = *value;
    }
    if let Some(end) = vested.last().copied() {
        vested.push((days(today.min(last.0)), end.1));
    }
    widgets::line_chart(
        frame,
        inner,
        &[
            widgets::Series { points: &ahead, color: theme::ACCENT },
            widgets::Series { points: &vested, color: theme::BLUE },
        ],
        vec![first.0.format("%b %Y").to_string(), last.0.format("all vested %b %Y").to_string()],
    );
}

fn draw_upcoming(frame: &mut Frame, area: Rect, app: &App) {
    let upcoming = app.data.equity.upcoming(app.data.today());
    let remaining: f64 = upcoming.iter().map(|v| v.quantity).sum();
    let note = if upcoming.is_empty() { String::new() } else { format!("{} unvested ", shares(remaining)) };
    let block = card_with_note("Vesting Ahead", Line::styled(note, theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if upcoming.is_empty() {
        frame.render_widget(Line::styled("Fully vested", theme::muted()), inner);
        return;
    }
    let width = inner.width as usize;
    let grants_width = width.saturating_sub(DATE_WIDTH + QUANTITY_WIDTH + VALUE_WIDTH);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{:<w$}", "Date", w = DATE_WIDTH), theme::header()),
        Span::styled(format!("{:<w$}", "Grant", w = grants_width), theme::header()),
        Span::styled(format!("{:>w$}", "Options", w = QUANTITY_WIDTH), theme::header()),
        Span::styled(format!("{:>w$}", "Value", w = VALUE_WIDTH), theme::header()),
    ])];
    // The last row sums up whatever does not fit
    let room = (inner.height as usize).saturating_sub(1);
    let listed = if upcoming.len() > room { room.saturating_sub(1) } else { upcoming.len() };
    for vesting in &upcoming[..listed] {
        let kind = match (vesting.iso > 0.0, vesting.nso > 0.0) {
            (true, false) => " ISO",
            (false, true) => " NSO",
            (true, true) => " ISO+NSO",
            (false, false) => "",
        };
        let grants = format!("{}{}", vesting.grants.join(", "), kind);
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", fmt::long_date(vesting.date), w = DATE_WIDTH), theme::muted()),
            Span::styled(format!("{:<w$}", fmt::trunc(&grants, grants_width.saturating_sub(1)), w = grants_width), theme::text()),
            Span::styled(format!("{:>w$}", shares(vesting.quantity), w = QUANTITY_WIDTH), theme::text()),
            Span::styled(format!("{:>w$}", fmt::money0(vesting.value), w = VALUE_WIDTH), theme::text()),
        ]));
    }
    if listed < upcoming.len() {
        let rest = &upcoming[listed..];
        let through = rest.last().map(|v| v.date.format("%b %Y").to_string()).unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<w$}", format!("{} more dates through {}", rest.len(), through), w = DATE_WIDTH + grants_width),
                theme::muted(),
            ),
            Span::styled(
                format!("{:>w$}", shares(rest.iter().map(|v| v.quantity).sum()), w = QUANTITY_WIDTH),
                theme::muted(),
            ),
            Span::styled(
                format!("{:>w$}", fmt::money0(rest.iter().map(|v| v.value).sum()), w = VALUE_WIDTH),
                theme::muted(),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_scenarios(frame: &mut Frame, area: Rect, app: &App) {
    let equity = &app.data.equity;
    let block = card_with_note("Price Scenarios", Line::styled("pre-tax ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(fmv) = equity.primary().and_then(|i| i.fmv) else {
        frame.render_widget(Line::styled("No share price on record", theme::muted()), inner);
        return;
    };
    let width = inner.width as usize;
    let price_width = width.saturating_sub(VALUE_WIDTH * 3);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("{:<w$}", "Price", w = price_width), theme::header()),
        Span::styled(format!("{:>w$}", "Vested", w = VALUE_WIDTH), theme::header()),
        Span::styled(format!("{:>w$}", "Unvested", w = VALUE_WIDTH), theme::header()),
        Span::styled(format!("{:>w$}", "All", w = VALUE_WIDTH), theme::header()),
    ])];
    for factor in SCENARIOS {
        let summary = equity.summary(factor);
        let label = if factor == 1.0 {
            format!("{}  today", price(fmv))
        } else {
            format!("{}  {}×", price(fmv * factor), factor)
        };
        let style = if factor == 1.0 { theme::bold() } else { theme::text() };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<w$}", fmt::trunc(&label, price_width.saturating_sub(1)), w = price_width), style),
            Span::styled(format!("{:>w$}", fmt::compact(summary.vested_value()), w = VALUE_WIDTH), style),
            Span::styled(format!("{:>w$}", fmt::compact(summary.unvested_value), w = VALUE_WIDTH), theme::muted()),
            Span::styled(format!("{:>w$}", fmt::compact(summary.total_value()), w = VALUE_WIDTH), style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `PARTIALLY_EXERCISED` and `secondary_sale` as `Partially exercised` and `Secondary sale`
fn plain(raw: &str) -> String {
    let words = raw.replace('_', " ");
    let words = if raw.chars().any(char::is_lowercase) { words } else { words.to_lowercase() };
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn a_share(value: f64) -> String {
    format!("{}/share", price(value))
}

fn date_or_dash(date: Option<NaiveDate>) -> String {
    date.map(fmt::long_date).unwrap_or_else(|| "—".to_string())
}

/// `text` broken into lines of at most `width` characters at its spaces
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The line a detail card opens with: what it is about, and of what kind
fn headline(name: &str, rest: String) -> Line<'static> {
    Line::from(vec![Span::styled(name.to_string(), theme::bold()), Span::styled(format!("  {}", rest), theme::muted())])
}

/// `label  value  note` in a detail card, the note in passing
fn field(label: &str, value: String, note: String) -> Line<'static> {
    let mut spans = vec![
        Span::styled(format!("{:<w$}", label, w = DETAIL_LABEL_WIDTH), theme::muted()),
        Span::styled(value, theme::text()),
    ];
    if !note.is_empty() {
        spans.push(Span::styled(format!("  {}", note), theme::muted()));
    }
    Line::from(spans)
}

/// A row of a detail card's table: a label and right-aligned figures
fn figures(label: &str, cells: &[String], style: Style) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{:<w$}", label, w = DETAIL_LABEL_WIDTH), theme::muted())];
    for cell in cells {
        spans.push(Span::styled(format!("{:>w$}", cell, w = DETAIL_COLUMN_WIDTH), style));
    }
    Line::from(spans)
}

/// How the spread on an exercise was taxed, by the kind of option
fn spread_tax(option_type: &str, exercised: Option<NaiveDate>) -> String {
    let year = exercised.map(|d| d.year().to_string()).unwrap_or_else(|| "that year".to_string());
    match option_type {
        "ISO" => format!(", AMT income {}", year),
        "NSO" => format!(", wage income {}", year),
        _ => String::new(),
    }
}

/// What shares were worth over their cost when acquired, as a note
fn over_cost(worth: f64, cost: f64, tax: String) -> String {
    if worth > cost {
        format!("{} spread{}", fmt::money0(worth - cost), tax)
    } else {
        "no spread".to_string()
    }
}

/// What has become of a certificate
fn fate(app: &App, certificate: &Certificate) -> String {
    let equity = &app.data.equity;
    if !certificate.canceled {
        return "still held".to_string();
    }
    if let Some(tender) = equity.tender_of(certificate) {
        return format!("sold {}", date_or_dash(tender.settled));
    }
    let pieces = equity.pieces(certificate);
    if pieces.is_empty() {
        return plain(&certificate.status);
    }
    let labels: Vec<&str> = pieces.iter().map(|c| c.label.as_str()).collect();
    format!("split into {}", labels.join(" and "))
}

/// Whether a sale of a certificate on `sold` kept its tax advantage
fn sale_tax(app: &App, certificate: &Certificate, sold: NaiveDate) -> &'static str {
    let equity = &app.data.equity;
    if let Some(from) = equity.iso_qualifying_from(certificate) {
        return if sold >= from { "qualifying" } else { "disqualifying" };
    }
    match equity.origin(certificate).acquired.and_then(long_term_from) {
        Some(from) if sold >= from => "long-term",
        Some(_) => "short-term",
        None => "",
    }
}

fn draw_detail(frame: &mut Frame, area: Rect, app: &App) {
    let equity = &app.data.equity;
    let (title, lines) = match app.equity_view {
        EquityView::Grants => match grants(app).get(app.equity_selected) {
            Some(grant) => (format!("Grant {}", grant.label), grant_detail(app, grant)),
            None => return,
        },
        EquityView::Shares => match certificates(app).get(app.equity_selected) {
            Some(certificate) => (format!("Shares {}", certificate.label), certificate_detail(app, certificate)),
            None => return,
        },
        EquityView::History => match history(app).get(app.equity_selected).map(|event| event.source) {
            Some(Source::Exercise(index)) => match equity.exercises.get(index) {
                Some(exercise) => ("Exercise".to_string(), exercise_detail(app, exercise)),
                None => return,
            },
            Some(Source::Tender(index)) => match equity.tenders.get(index) {
                Some(tender) => ("Sale".to_string(), tender_detail(app, tender)),
                None => return,
            },
            None => return,
        },
    };
    let popup = widgets::centered(area, DETAIL_WIDTH, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    let block = card_with_note(&title, Line::styled("esc: close ", theme::muted()));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn grant_detail(app: &App, grant: &Grant) -> Vec<Line<'static>> {
    let equity = &app.data.equity;
    let today = app.data.today();
    let company = equity.issuer(grant.issuer_id).map(|i| i.name.as_str()).unwrap_or("");
    let fmv = equity.issuer(grant.issuer_id).and_then(|i| i.fmv);
    let spread = (fmv.unwrap_or(0.0) - grant.exercise_price).max(0.0);

    let mut lines = vec![headline(company, format!("{} stock options", grant.stock_type)), Line::default()];
    lines.push(field("Type", grant.option_type.clone(), String::new()));
    if grant.iso_disqualified {
        lines.push(Line::styled("ISO status lost", Style::new().fg(theme::YELLOW)));
    }
    lines.push(field("Status", plain(&grant.status), String::new()));
    let expires = grant.expires.map(|d| format!("expires {}", fmt::long_date(d))).unwrap_or_default();
    lines.push(field("Granted", date_or_dash(grant.issued), expires));
    let now = match fmv {
        Some(fmv) if fmv > grant.exercise_price => format!("FMV {}, spread {}", price(fmv), price(spread)),
        Some(fmv) => format!("FMV {}, no spread", price(fmv)),
        None => String::new(),
    };
    lines.push(field("Strike", price(grant.exercise_price), now));
    let span = match (grant.vesting_start, grant.vesting_end) {
        (Some(start), Some(end)) => format!("{} to {}", fmt::long_date(start), fmt::long_date(end)),
        _ => String::new(),
    };
    lines.push(field("Vesting", grant.schedule.clone(), span));
    if !grant.cliff.is_empty() {
        lines.push(field("Cliff", grant.cliff.clone(), String::new()));
    }
    if !grant.acceleration.is_empty() {
        lines.push(field("Acceleration", grant.acceleration.clone(), String::new()));
    }
    if let Some(allowed) = grant.early_exercise {
        let allowed = if allowed { "Allowed" } else { "Not allowed" };
        lines.push(field("Early exercise", allowed.to_string(), String::new()));
    }
    if !grant.form_83b.is_empty() && grant.form_83b != NO_83B {
        lines.push(field("83(b) election", plain(&grant.form_83b), String::new()));
    }

    // Each stage of the grant, by how its options are taxed
    let split = equity.split(grant);
    let dash = || "—".to_string();
    let counts = |total: f64, part: Option<Split>, value: Option<f64>| {
        vec![
            shares(total),
            part.map(|p| shares(p.iso)).unwrap_or_else(dash),
            part.map(|p| shares(p.nso)).unwrap_or_else(dash),
            value.map(fmt::money0).unwrap_or_default(),
        ]
    };
    lines.push(Line::default());
    let header = ["Options", "ISO", "NSO", "Value"].map(String::from);
    lines.push(figures("", &header, theme::header()));
    lines.push(figures("Granted", &counts(grant.quantity, split.map(|s| s.granted), None), theme::text()));
    lines.push(figures("Vested", &counts(grant.vested, split.map(|s| s.vested), None), theme::text()));
    lines.push(figures("Exercised", &counts(grant.exercised, split.map(|s| s.exercised), None), theme::text()));
    let exercisable = counts(grant.exercisable, split.map(|s| s.exercisable), Some(grant.exercisable * spread));
    lines.push(figures("Exercisable", &exercisable, theme::bold()));
    let unvested = counts(grant.unvested(), split.map(|s| s.unvested), Some(grant.unvested() * spread));
    lines.push(figures("Unvested", &unvested, theme::text()));
    let cost = |options: f64| fmt::money0(options * grant.exercise_price);
    let to_exercise = vec![
        cost(grant.exercisable),
        split.map(|s| cost(s.exercisable.iso)).unwrap_or_else(dash),
        split.map(|s| cost(s.exercisable.nso)).unwrap_or_else(dash),
    ];
    lines.push(figures("Cost to exercise", &to_exercise, theme::muted()));

    let ahead: Vec<&VestEvent> = equity.vesting_ahead(grant, today).collect();
    if let (Some(next), Some(last)) = (ahead.first(), ahead.last()) {
        let kinds = match (next.iso, next.nso) {
            (Some(iso), Some(nso)) => format!("{} ISO, {} NSO", shares(iso), shares(nso)),
            _ => String::new(),
        };
        lines.push(Line::default());
        lines.push(field("Next vesting", format!("{}  {}", fmt::long_date(next.date), shares(next.quantity)), kinds));
        if ahead.len() > 1 {
            let rest = format!("+{} more through {}", ahead.len() - 1, last.date.format("%b %Y"));
            lines.push(field("", rest, String::new()));
        }
    }

    let exercises: Vec<&Exercise> = equity.exercises_of(grant).collect();
    if !exercises.is_empty() {
        lines.push(Line::default());
        let header = ["Options", "Paid", "Shares", "FMV then"].map(String::from);
        lines.push(figures("Exercised", &header, theme::header()));
        for exercise in exercises {
            let cells = vec![
                format!("{} {}", shares(exercise.quantity), exercise.option_type),
                fmt::money0(exercise.cost),
                exercise.certificate_label.clone(),
                exercise.fmv.map(price).unwrap_or_else(dash),
            ];
            lines.push(figures(&date_or_dash(exercise.date), &cells, theme::text()));
        }
    }

    if !grant.terms.is_empty() {
        lines.push(Line::default());
        lines.extend(wrap(&grant.terms, DETAIL_INNER_WIDTH).into_iter().map(|line| Line::styled(line, theme::muted())));
    }
    lines
}

fn certificate_detail(app: &App, certificate: &Certificate) -> Vec<Line<'static>> {
    let equity = &app.data.equity;
    let today = app.data.today();
    let company = equity.issuer(certificate.issuer_id).map(|i| i.name.as_str()).unwrap_or("");
    let fmv = equity.issuer(certificate.issuer_id).and_then(|i| i.fmv);
    let origin = equity.origin(certificate);
    let exercise = equity.exercise_behind(certificate);
    let cost = equity.cost_of(certificate);

    let mut lines = vec![headline(company, certificate.share_class.clone()), Line::default()];
    let form = match certificate.certificated {
        Some(true) => "certificated",
        Some(false) => "book entry",
        None => "",
    };
    lines.push(field("Status", plain(&certificate.status), form.to_string()));
    lines.push(field("Shares", shares(certificate.quantity), String::new()));
    let cut = equity
        .parent(certificate)
        .map(|parent| format!("split from {} ({})", parent.label, shares(parent.quantity)))
        .unwrap_or_default();
    lines.push(field("Issued", date_or_dash(certificate.issued), cut));
    let held = certificate.acquired.map(|d| format!("held {}", held_for(d, today))).unwrap_or_default();
    lines.push(field("Acquired", date_or_dash(certificate.acquired), held));
    if origin.exercised_from.is_empty() {
        lines.push(field("From", "Not from options".to_string(), String::new()));
    } else {
        let paid = match exercise {
            Some(exercise) if !exercise.payment.is_empty() => format!("paid by {}", exercise.payment),
            _ => String::new(),
        };
        lines.push(field("From", format!("{} {} options", origin.exercised_from, origin.exercise_type), paid));
    }
    if let Some(cost) = cost {
        let each = if certificate.quantity > 0.0 { a_share(cost / certificate.quantity) } else { String::new() };
        lines.push(field("Cost", fmt::money(cost), each));
    }
    // A piece cut later carries the price of the day it was cut, not acquired
    if let Some(then) = exercise.and_then(|x| x.fmv).or(origin.fmv_at_issue) {
        let note = match cost {
            Some(cost) => {
                over_cost(then * certificate.quantity, cost, spread_tax(&origin.exercise_type, origin.acquired))
            }
            None => String::new(),
        };
        lines.push(field("FMV then", a_share(then), note));
    }
    if let Some(fmv) = fmv {
        let value = certificate.quantity * fmv;
        lines.push(field("Value", fmt::money0(value), a_share(fmv)));
        if let Some(cost) = cost {
            lines.push(field("Gain", fmt::money0(value - cost), "pre-tax, at FMV".to_string()));
        }
    }
    if let Some(from) = certificate.acquired.and_then(long_term_from) {
        let (when, note) = if today >= from {
            (format!("Since {}", fmt::long_date(from)), String::new())
        } else {
            (format!("From {}", fmt::long_date(from)), "short-term until then".to_string())
        };
        lines.push(field("Long-term", when, note));
    }
    if let Some(from) = equity.iso_qualifying_from(certificate) {
        let (when, note) = if today >= from {
            (format!("Qualifying since {}", fmt::long_date(from)), String::new())
        } else {
            (format!("Qualifying from {}", fmt::long_date(from)), "disqualifying until then".to_string())
        };
        lines.push(field("ISO sale", when, note));
    }
    lines
}

fn exercise_detail(app: &App, exercise: &Exercise) -> Vec<Line<'static>> {
    let equity = &app.data.equity;
    let company = equity.issuer(exercise.issuer_id).map(|i| i.name.as_str()).unwrap_or("");
    let grant = equity.grants.iter().find(|g| g.label == exercise.grant_label && g.issuer_id == exercise.issuer_id);

    let mut lines = vec![headline(company, date_or_dash(exercise.date)), Line::default()];
    let terms = grant.map(|g| format!("{}, strike {}", g.option_type, price(g.exercise_price))).unwrap_or_default();
    lines.push(field("Grant", exercise.grant_label.clone(), terms));
    let options = format!("{} {} options", shares(exercise.quantity), exercise.option_type);
    lines.push(field("Exercised", options, String::new()));
    let mut how = Vec::new();
    if !exercise.payment.is_empty() {
        how.push(format!("by {}", exercise.payment));
    }
    if exercise.taxes > 0.0 {
        how.push(format!("+{} tax withheld", fmt::money(exercise.taxes)));
    }
    lines.push(field("Paid", fmt::money(exercise.cost), how.join(", ")));
    if let Some(then) = exercise.fmv {
        let tax = spread_tax(&exercise.option_type, exercise.date);
        lines.push(field("FMV then", a_share(then), over_cost(then * exercise.quantity, exercise.cost, tax)));
    }
    let became = equity.certificate(exercise.issuer_id, &exercise.certificate_label).map(|c| fate(app, c));
    lines.push(field("Shares", exercise.certificate_label.clone(), became.unwrap_or_default()));
    lines
}

fn tender_detail(app: &App, tender: &Tender) -> Vec<Line<'static>> {
    let equity = &app.data.equity;
    let each = if tender.shares > 0.0 { tender.proceeds / tender.shares } else { 0.0 };

    let mut lines = vec![headline(&tender.name, String::new()), Line::default()];
    lines.push(field("Status", tender.status.clone(), plain(&tender.kind).to_lowercase()));
    if let (Some(opened), Some(closed)) = (tender.opened, tender.closed) {
        let open = format!("{} to {}", fmt::long_date(opened), fmt::long_date(closed));
        lines.push(field("Offer open", open, String::new()));
    }
    lines.push(field("Settled", date_or_dash(tender.settled), String::new()));
    lines.push(field("Sold", format!("{} shares", shares(tender.shares)), a_share(each)));
    lines.push(field("Proceeds", fmt::money(tender.proceeds), String::new()));

    let sold = equity.sold_in(tender);
    let Some(settled) = tender.settled.filter(|_| !sold.is_empty()) else {
        return lines;
    };
    let row = |cells: [String; 8], style: Style| {
        let [label, from, held, tax, quantity, cost, proceeds, gain] = cells;
        Line::from(vec![
            Span::styled(format!("{:<w$}", label, w = LABEL_WIDTH), style),
            Span::styled(format!("{:<w$}", from, w = SOLD_FROM_WIDTH), style),
            Span::styled(format!("{:<w$}", held, w = SOLD_HELD_WIDTH), style),
            Span::styled(format!("{:<w$}", tax, w = SOLD_TAX_WIDTH), style),
            Span::styled(format!("{:>w$}", quantity, w = SOLD_QUANTITY_WIDTH), style),
            Span::styled(format!("{:>w$}", cost, w = SOLD_MONEY_WIDTH), style),
            Span::styled(format!("{:>w$}", proceeds, w = SOLD_MONEY_WIDTH), style),
            Span::styled(format!("{:>w$}", gain, w = SOLD_GAIN_WIDTH), style),
        ])
    };
    lines.push(Line::default());
    let header = ["Shares", "From", "Held", "Sale", "Quantity", "Cost", "Proceeds", "Gain"].map(String::from);
    lines.push(row(header, theme::header()));
    let (mut quantity, mut cost, mut proceeds) = (0.0, 0.0, 0.0);
    for certificate in &sold {
        let origin = equity.origin(certificate);
        let paid = equity.cost_of(certificate).unwrap_or(0.0);
        let received = certificate.quantity * each;
        quantity += certificate.quantity;
        cost += paid;
        proceeds += received;
        lines.push(row(
            [
                certificate.label.clone(),
                fmt::trunc(&source(origin), SOLD_FROM_WIDTH - 1),
                origin.acquired.map(|d| held_for(d, settled)).unwrap_or_default(),
                sale_tax(app, certificate, settled).to_string(),
                shares(certificate.quantity),
                fmt::money0(paid),
                fmt::money0(received),
                fmt::money0(received - paid),
            ],
            theme::text(),
        ));
    }
    if sold.len() > 1 {
        let blank = String::new;
        lines.push(row(
            [
                "Total".to_string(),
                blank(),
                blank(),
                blank(),
                shares(quantity),
                fmt::money0(cost),
                fmt::money0(proceeds),
                fmt::money0(proceeds - cost),
            ],
            theme::bold(),
        ));
    }
    lines
}
