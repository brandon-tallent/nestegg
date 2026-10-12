//! Screen layout: navigation, header, footer, and dispatch to each screen

pub mod accounts;
pub mod budget;
pub mod cashflow;
pub mod dashboard;
pub mod equity;
pub mod goals;
pub mod investments;
pub mod recurring;
pub mod reports;
pub mod transactions;
pub mod widgets;

use crate::app::{App, Screen};
use crate::theme;
use chrono::Utc;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Below this width the sidebar collapses into a tab strip
const SIDEBAR_MIN_WIDTH: u16 = 100;
const SIDEBAR_WIDTH: u16 = 20;

/// Width at which screens switch from stacked to side-by-side panels
pub const WIDE: u16 = 110;

const HELP_WIDTH: u16 = 76;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    app.mouse.borrow_mut().clear();
    let main = if area.width >= SIDEBAR_MIN_WIDTH {
        let [sidebar, main] =
            Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(0)]).areas(area);
        draw_sidebar(frame, sidebar, app);
        main
    } else {
        let [tabs, main] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        draw_tabs(frame, tabs, app);
        main
    };

    let [header, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(main);
    draw_header(frame, header, app);
    match app.screen {
        Screen::Dashboard => dashboard::draw(frame, body, app),
        Screen::Accounts => accounts::draw(frame, body, app),
        Screen::Transactions => transactions::draw(frame, body, app),
        Screen::CashFlow => cashflow::draw(frame, body, app),
        Screen::Reports => reports::draw(frame, body, app),
        Screen::Budget => budget::draw(frame, body, app),
        Screen::Recurring => recurring::draw(frame, body, app),
        Screen::Goals => goals::draw(frame, body, app),
        Screen::Investments => investments::draw(frame, body, app),
        Screen::Equity => equity::draw(frame, body, app),
    }
    draw_footer(frame, footer, app);

    if app.editor.is_some() { transactions::draw_edit(frame, area, app); }

    if app.show_help {
        crate::mouse::modal(app, area, Rect::default());
        draw_help(frame, area);
    }
}

/// `2h ago`, `3d ago`
fn synced_label(app: &App) -> String {
    let Some(at) = app.data.synced_at else {
        return "never synced".to_string();
    };
    let minutes = (Utc::now() - at).num_minutes().max(0);
    let age = match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{}m ago", minutes),
        60..=1439 => format!("{}h ago", minutes / 60),
        _ => format!("{}d ago", minutes / 1440),
    };
    format!("Synced {}", age)
}

fn draw_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(Style::new().fg(theme::BORDER));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ❖ ", theme::accent()),
            Span::styled("nestegg", theme::bold()),
        ]),
        Line::from(""),
    ];
    for screen in Screen::ALL.iter() {
        crate::mouse::region(app, Rect::new(inner.x, inner.y + lines.len() as u16, inner.width, 1).intersection(inner), crate::mouse::Action::Screen(*screen));
        let active = *screen == app.screen;
        let bar = if active { "▌" } else { " " };
        let style = if active { theme::accent() } else { theme::text() };
        let mut line = Line::from(vec![
            Span::styled(bar, theme::accent()),
            Span::styled(format!(" {} ", screen.key()), theme::muted()),
            Span::styled(screen.title(), style),
        ]);
        if active {
            line = line.style(Style::new().bg(theme::SELECTED_BG));
        }
        lines.push(line);
    }
    let nav_height = lines.len() as u16;
    frame.render_widget(Paragraph::new(lines), inner);

    let mut status = vec![Line::styled(format!("  {}", synced_label(app)), theme::muted())];
    if app.data.anonymized {
        status.insert(0, Line::styled("  ANONYMIZED", Style::new().fg(theme::YELLOW)));
    }
    // On a short terminal the navigation needs the rows more than the status does
    let status_height = status.len() as u16 + 1;
    if inner.height >= nav_height + status_height {
        let [_, bottom] = Layout::vertical([Constraint::Min(0), Constraint::Length(status_height)]).areas(inner);
        frame.render_widget(Paragraph::new(status), bottom);
    }
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![Span::styled("❖ ", theme::accent())];
    for screen in Screen::ALL.iter() {
        if *screen == app.screen {
            spans.push(Span::styled(
                format!(" {} {} ", screen.key(), screen.title()),
                theme::accent().bg(theme::SELECTED_BG),
            ));
        } else {
            // Inactive tabs shrink to a number and a few letters to fit 80 columns
            let short: String = screen.title().chars().take(4).collect();
            spans.push(Span::styled(format!(" {}{} ", screen.key(), short), theme::muted()));
        }
    }
    let mut x = area.x;
    for (index, span) in spans.iter().enumerate() {
        if index > 0 {
            crate::mouse::region(app, Rect::new(x, area.y, span.width() as u16, 1).intersection(area), crate::mouse::Action::Screen(Screen::ALL[index - 1]));
        }
        x += span.width() as u16;
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let mut left = vec![Span::styled(format!(" {}", app.screen.title()), theme::bold())];
    if matches!(app.screen, Screen::Dashboard | Screen::CashFlow | Screen::Reports) {
        let label = if app.include_one_time { "  one-time items included" } else { "  one-time items excluded" };
        left.push(Span::styled(label, theme::muted()));
    }
    // Always visible, so a shared screenshot is never mistaken for real figures
    if app.data.anonymized {
        left.push(Span::styled("  ANONYMIZED", Style::new().fg(theme::YELLOW).add_modifier(Modifier::BOLD)));
    }
    // Transactions shows its search beside its other filters
    if app.screen != Screen::Transactions {
        let search = app.search_text(app.screen);
        if app.searching {
            left.push(Span::styled("  / ", theme::accent()));
            left.push(Span::styled(search.to_string(), theme::text()));
            left.push(Span::styled("▏", theme::accent()));
        } else if !search.is_empty() {
            left.push(Span::styled("  / ", theme::muted()));
            left.push(Span::styled(search.to_string(), theme::accent()));
            left.push(Span::styled("  esc clears", theme::muted()));
        }
    }
    if !app.marked.is_empty() { left.push(Span::styled(format!("  {} marked", app.marked.len()), theme::accent())); }
    if app.edit_busy { left.push(Span::styled("  Applying to Monarch…", theme::accent())); }
    if !app.status.is_empty() {
        left.push(Span::styled(format!("  {}", app.status), Style::new().fg(theme::YELLOW)));
    }
    frame.render_widget(Line::from(left), area);

    // The range only matters on screens that chart history
    if matches!(app.screen, Screen::Dashboard | Screen::Accounts | Screen::Investments) {
        let range = Line::from(vec![
            Span::styled("[ ", theme::muted()),
            Span::styled(app.range.label(), theme::accent()),
            Span::styled(" ] ", theme::muted()),
        ])
        .right_aligned();
        let width = range.width() as u16;
        let x = area.right().saturating_sub(width);
        crate::mouse::key(app, Rect::new(x, area.y, width / 2, 1).intersection(area), ratatui::crossterm::event::KeyCode::Char('['));
        crate::mouse::key(app, Rect::new(x + width / 2, area.y, width - width / 2, 1).intersection(area), ratatui::crossterm::event::KeyCode::Char(']'));
        frame.render_widget(range, area);
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let specific: &[(&str, &str)] = match app.screen {
        Screen::Dashboard => &[("/", "search"), ("[ ]", "range"), ("o", "one-time")],
        Screen::Accounts => &[("/", "search"), ("⏎", "transactions"), ("h", "hidden"), ("[ ]", "range")],
        Screen::Transactions => &[("/", "search"), ("t", "mark"), ("m", "merchant"), ("C", "category"), ("⏎", "details"), ("f", "filter"), ("c", "clear")],
        Screen::CashFlow => &[("/", "search"), ("←→", "month"), ("b", "breakdown"), ("o", "one-time"), ("⏎", "transactions")],
        Screen::Reports => &[("/", "search"), ("↑↓", "scroll"), ("o", "one-time")],
        Screen::Budget => &[("/", "search"), ("↑↓", "scroll")],
        Screen::Recurring => &[("/", "search"), ("⏎", "payment history")],
        Screen::Goals => &[("/", "search")],
        Screen::Investments => &[("/", "search"), ("v", "view"), ("[ ]", "range")],
        Screen::Equity => &[("/", "search"), ("v", "grants, shares, history"), ("⏎", "details")],
    };
    let general: &[(&str, &str)] = &[("0-9", "go"), ("a", "anonymize"), ("r", "reload"), ("?", "help"), ("q", "quit")];

    let mut spans = vec![Span::raw(" ")];
    for (key, label) in specific.iter().chain(general.iter()) {
        let x = area.x + spans.iter().map(|s| s.width() as u16).sum::<u16>();
        let width = key.chars().count() as u16 + label.chars().count() as u16 + 1;
        crate::mouse::footer(app, Rect::new(x, area.y, width, 1).intersection(area), key);
        spans.push(Span::styled(*key, Style::new().fg(theme::TEXT).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {}   ", label), theme::muted()));
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let rows: &[(&str, &str)] = &[
        ("1-9, 0, Tab", "Switch screen"),
        ("↑ ↓ / j k", "Move selection or scroll"),
        ("Mouse", "Click to select, double-click to open; wheel to scroll"),
        ("PgUp PgDn, g G", "Page, jump to top or bottom"),
        ("[ ]", "Shorter or longer chart range"),
        ("r", "Reload from the database"),
        ("a", "Swap names and amounts for stand-ins, to share a screen"),
        ("o", "Include or exclude items tagged One-time in Monarch"),
        ("/", "Search the screen: any words, plus >100  <50  $45 for amounts"),
        ("Esc", "Clear the search"),
        ("", ""),
        ("Accounts", ""),
        ("  Enter", "Show the account's transactions"),
        ("  h", "Show or hide hidden accounts"),
        ("Transactions", ""),
        ("  /", "Searches merchant, category, account, notes, tags, date"),
        ("  f", "Cycle filter (review, pending, income, …)"),
        ("  Enter", "Transaction details"),
        ("  c, Esc", "Clear filters; Esc returns from a drill-down"),
        ("  t / T / u", "Mark a row / mark all visible / unmark all"),
        ("  m / C", "Change marked merchants / categories in Monarch"),
        ("Cash Flow", ""),
        ("  ← → / h l", "Previous or next month"),
        ("  b", "Break down by category, group, merchant"),
        ("  Enter", "Transactions behind the selected row"),
        ("Investments", ""),
        ("  v", "Holdings or positions by account"),
        ("Equity", ""),
        ("  v", "Move between grants, shares held, and exercises and sales"),
        ("  Enter", "Everything on record for the selected grant, shares or event"),
        ("", ""),
        ("q", "Quit"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(key, what)| {
            if what.is_empty() {
                Line::styled(*key, theme::header())
            } else {
                Line::from(vec![
                    Span::styled(format!("{:<18}", key), theme::accent()),
                    Span::styled(*what, theme::text()),
                ])
            }
        })
        .collect();
    let popup = widgets::centered(area, HELP_WIDTH, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(widgets::card("Keys")), popup);
}
