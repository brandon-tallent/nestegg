//! Investments: portfolio value over time, allocation, holdings and positions

use super::widgets::{self, card, card_with_note};
use super::WIDE;
use crate::app::{App, InvestView};
use crate::fmt;
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use std::collections::HashMap;

const TICKER_WIDTH: usize = 8;
const QUANTITY_WIDTH: usize = 12;
const PRICE_WIDTH: usize = 11;
const VALUE_WIDTH: usize = 13;
const CHANGE_WIDTH: usize = 9;
const GAIN_WIDTH: usize = 12;
const WEIGHT_WIDTH: usize = 7;

/// Narrower than this, quantity and price columns are dropped
const FULL_TABLE_MIN_WIDTH: usize = 100;

const ALLOCATION_BAR_WIDTH: usize = 10;
const ALLOCATION_VALUE_WIDTH: usize = 10;

/// Rows given to the chart and allocation panels
const TOP_PANEL_HEIGHT: u16 = 12;

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let [top, table] = Layout::vertical([Constraint::Length(TOP_PANEL_HEIGHT), Constraint::Min(0)]).areas(area);
    if area.width >= WIDE {
        let [chart, allocation] =
            Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).areas(top);
        draw_value(frame, chart, app);
        draw_allocation(frame, allocation, app);
    } else {
        draw_value(frame, top, app);
    }
    match app.invest_view {
        InvestView::Holdings => draw_holdings(frame, table, app),
        InvestView::ByAccount => draw_positions(frame, table, app),
    }
}

fn draw_value(frame: &mut Frame, area: Rect, app: &App) {
    let portfolio = app.data.portfolio;
    let block = card_with_note(
        "Portfolio",
        Line::styled(format!("{} today ", fmt::signed_pct(portfolio.one_day_change_pct)), theme::gain(portfolio.one_day_change_pct)),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let history = app.data.investment_history();
    let history = app.in_range(&history, |p| p.0);
    let [summary, chart] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    let mut headline = vec![Span::styled(fmt::money0(portfolio.total_value), theme::bold()), Span::raw("  ")];
    if let (Some(first), Some(last)) = (history.first(), history.last()) {
        let (delta, pct) = widgets::change(first.1, last.1);
        headline.extend(widgets::change_spans(delta, pct));
        headline.push(Span::styled(format!("  {} incl. contributions", app.range.label()), theme::muted()));
    }
    frame.render_widget(Line::from(headline), summary);
    widgets::date_chart(frame, chart, history, theme::BLUE);
}

fn asset_class_label(raw: &str) -> String {
    match raw {
        "stocks" => "Stocks".to_string(),
        "bonds" => "Bonds".to_string(),
        "cash" => "Cash".to_string(),
        "mixed" => "Mixed funds".to_string(),
        "unclassified" => "Unclassified".to_string(),
        other => fmt::frequency(other),
    }
}

fn draw_allocation(frame: &mut Frame, area: Rect, app: &App) {
    let block = card("Allocation");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut by_class: HashMap<&str, f64> = HashMap::new();
    for holding in &app.data.holdings {
        *by_class.entry(holding.asset_class.as_str()).or_default() += holding.value;
    }
    let total: f64 = by_class.values().sum();
    let mut classes: Vec<(&str, f64)> = by_class.into_iter().collect();
    classes.sort_by(|a, b| b.1.total_cmp(&a.1));

    let width = inner.width as usize;
    let name_width = width.saturating_sub(ALLOCATION_BAR_WIDTH + ALLOCATION_VALUE_WIDTH + WEIGHT_WIDTH);
    let lines: Vec<Line> = classes
        .iter()
        .enumerate()
        .map(|(i, (class, value))| {
            let share = if total > 0.0 { value / total } else { 0.0 };
            Line::from(vec![
                Span::styled(format!("{:<w$}", fmt::trunc(&asset_class_label(class), name_width.saturating_sub(1)), w = name_width), theme::text()),
                Span::styled(fmt::hbar(share, ALLOCATION_BAR_WIDTH), Style::new().fg(theme::SERIES[i % theme::SERIES.len()])),
                Span::styled(format!("{:>w$}", fmt::money0(*value), w = ALLOCATION_VALUE_WIDTH), theme::text()),
                Span::styled(format!("{:>w$.1}%", share * 100.0, w = WEIGHT_WIDTH - 1), theme::muted()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `+$1,234` gain against cost basis, or a dash when Monarch has no basis
fn gain_span(value: f64, cost_basis: Option<f64>) -> Span<'static> {
    match cost_basis.filter(|basis| *basis > 0.0) {
        Some(basis) => {
            let gain = value - basis;
            let text = format!("{}{}", if gain >= 0.0 { "+" } else { "" }, fmt::money0(gain));
            Span::styled(format!("{:>w$}", text, w = GAIN_WIDTH), theme::gain(gain))
        }
        None => Span::styled(format!("{:>w$}", "—", w = GAIN_WIDTH), theme::muted()),
    }
}

/// Quoted price, or the price implied by value and quantity for funds
/// Monarch has no quote for
fn price_text(price: Option<f64>, value: f64, quantity: f64) -> String {
    match price {
        Some(price) => fmt::money(price),
        None if quantity > 0.0 => fmt::money(value / quantity),
        None => "—".to_string(),
    }
}

fn quantity_text(quantity: f64) -> String {
    if quantity.fract().abs() < 0.0005 {
        format!("{:.0}", quantity)
    } else {
        format!("{:.3}", quantity)
    }
}

fn draw_holdings(frame: &mut Frame, area: Rect, app: &App) {
    let block = card_with_note("Holdings", Line::styled("v: by account ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let full = width >= FULL_TABLE_MIN_WIDTH;
    let fixed = TICKER_WIDTH + VALUE_WIDTH + CHANGE_WIDTH + GAIN_WIDTH + WEIGHT_WIDTH
        + if full { QUANTITY_WIDTH + PRICE_WIDTH } else { 0 };
    let name_width = width.saturating_sub(fixed);
    let total: f64 = app.data.holdings.iter().map(|h| h.value).sum();

    let mut header = vec![
        Span::styled(format!("{:<w$}", "Symbol", w = TICKER_WIDTH), theme::header()),
        Span::styled(format!("{:<w$}", "Name", w = name_width), theme::header()),
    ];
    if full {
        header.push(Span::styled(format!("{:>w$}", "Shares", w = QUANTITY_WIDTH), theme::header()));
        header.push(Span::styled(format!("{:>w$}", "Price", w = PRICE_WIDTH), theme::header()));
    }
    header.push(Span::styled(format!("{:>w$}", "Today", w = CHANGE_WIDTH), theme::header()));
    header.push(Span::styled(format!("{:>w$}", "Value", w = VALUE_WIDTH), theme::header()));
    header.push(Span::styled(format!("{:>w$}", "Gain", w = GAIN_WIDTH), theme::header()));
    header.push(Span::styled(format!("{:>w$}", "Weight", w = WEIGHT_WIDTH), theme::header()));

    let [header_area, rows_area] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Line::from(header), header_area);

    let holdings = app.listed_holdings();
    if holdings.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No holdings"), theme::muted()), rows_area);
        return;
    }
    let (start, end) = widgets::window(app.invest_selected, holdings.len(), rows_area.height as usize);
    let lines: Vec<Line> = holdings[start..end]
        .iter()
        .enumerate()
        .map(|(offset, h)| {
            let ticker = if h.ticker.is_empty() { "—" } else { &h.ticker };
            let mut spans = vec![
                Span::styled(format!("{:<w$}", fmt::trunc(ticker, TICKER_WIDTH - 1), w = TICKER_WIDTH), theme::accent()),
                Span::styled(format!("{:<w$}", fmt::trunc(&h.name, name_width.saturating_sub(1)), w = name_width), theme::text()),
            ];
            if full {
                spans.push(Span::styled(format!("{:>w$}", quantity_text(h.quantity), w = QUANTITY_WIDTH), theme::muted()));
                spans.push(Span::styled(
                    format!("{:>w$}", price_text(h.price, h.value, h.quantity), w = PRICE_WIDTH),
                    theme::muted(),
                ));
            }
            spans.push(match h.day_change_pct {
                Some(pct) => Span::styled(format!("{:>w$}", fmt::signed_pct(pct), w = CHANGE_WIDTH), theme::gain(pct)),
                None => Span::styled(format!("{:>w$}", "—", w = CHANGE_WIDTH), theme::muted()),
            });
            spans.push(Span::styled(format!("{:>w$}", fmt::money0(h.value), w = VALUE_WIDTH), theme::text()));
            spans.push(gain_span(h.value, h.cost_basis));
            let weight = if total > 0.0 { h.value / total * 100.0 } else { 0.0 };
            spans.push(Span::styled(format!("{:>w$.1}%", weight, w = WEIGHT_WIDTH - 1), theme::muted()));
            let line = Line::from(spans);
            if start + offset == app.invest_selected {
                line.style(theme::selected())
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

fn draw_positions(frame: &mut Frame, area: Rect, app: &App) {
    let block = card_with_note("Positions by Account", Line::styled("v: by holding ", theme::muted()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let full = width >= FULL_TABLE_MIN_WIDTH;
    let fixed = TICKER_WIDTH + VALUE_WIDTH + GAIN_WIDTH + 2 + if full { QUANTITY_WIDTH + PRICE_WIDTH } else { 0 };
    let name_width = width.saturating_sub(fixed);

    // Account headers are lines but not selectable, so track the selected line
    let positions = app.listed_positions();
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_line = 0;
    let mut previous_account: Option<&str> = None;
    for (i, p) in positions.iter().enumerate() {
        if previous_account != Some(p.account_id.as_str()) {
            if previous_account.is_some() {
                lines.push(Line::from(""));
            }
            // The account's whole value, whatever the search has narrowed it to
            let total: f64 =
                app.data.positions.iter().filter(|x| x.account_id == p.account_id).map(|x| x.value).sum();
            let subtype = app.data.account(&p.account_id).map(|a| a.subtype.as_str()).unwrap_or("");
            lines.push(widgets::spread(
                vec![
                    Span::styled(p.account_name.clone(), theme::accent()),
                    Span::styled(format!("  {}", subtype), theme::muted()),
                ],
                vec![Span::styled(fmt::money0(total), theme::bold())],
                width,
            ));
            previous_account = Some(p.account_id.as_str());
        }
        if i == app.invest_selected {
            selected_line = lines.len();
        }
        let ticker = if p.ticker.is_empty() { "—" } else { &p.ticker };
        let mut spans = vec![
            Span::raw("  "),
            Span::styled(format!("{:<w$}", fmt::trunc(ticker, TICKER_WIDTH - 1), w = TICKER_WIDTH), theme::text()),
            Span::styled(
                format!("{:<w$}", fmt::trunc(&format!("{} · {}", p.name, p.kind), name_width.saturating_sub(1)), w = name_width),
                theme::muted(),
            ),
        ];
        if full {
            spans.push(Span::styled(format!("{:>w$}", quantity_text(p.quantity), w = QUANTITY_WIDTH), theme::muted()));
            spans.push(Span::styled(
                format!("{:>w$}", price_text(p.price, p.value, p.quantity), w = PRICE_WIDTH),
                theme::muted(),
            ));
        }
        spans.push(gain_span(p.value, p.cost_basis));
        spans.push(Span::styled(format!("{:>w$}", fmt::money0(p.value), w = VALUE_WIDTH), theme::text()));
        let line = Line::from(spans);
        lines.push(if i == app.invest_selected { line.style(theme::selected()) } else { line });
    }

    if lines.is_empty() {
        frame.render_widget(Line::styled(widgets::no_matches(app, "No positions"), theme::muted()), inner);
        return;
    }
    let (start, end) = widgets::window(selected_line, lines.len(), inner.height as usize);
    frame.render_widget(Paragraph::new(lines[start..end].to_vec()), inner);
}
