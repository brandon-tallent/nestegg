//! Goals: savings goals with balances and contributions

use super::widgets::{self, card};
use crate::app::{App, Screen};
use crate::fmt;
use crate::theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Rows each goal card takes
const GOAL_CARD_HEIGHT: u16 = 7;

/// Width of a goal's progress bar
const PROGRESS_BAR_WIDTH: usize = 30;

pub struct Goal {
    pub name: String,
    pub balance: f64,
    pub target: Option<f64>,
    /// 0.0 to 1.0, when the goal has a target
    pub progress: Option<f64>,
    pub planned_monthly: f64,
    pub contributed_this_month: f64,
    pub net_contribution: f64,
    pub accounts: Vec<(String, f64)>,
    pub archived: bool,
}

/// Goals parsed from the stored goals document
pub fn goals(app: &App) -> Vec<Goal> {
    let Some(document) = &app.data.goals else {
        return Vec::new();
    };
    document["savingsGoals"]
        .as_array()
        .map(|goals| {
            goals
                .iter()
                .map(|g| Goal {
                    name: g["name"].as_str().unwrap_or("Goal").to_string(),
                    balance: g["currentBalance"].as_f64().unwrap_or(0.0),
                    target: g["targetAmount"].as_f64(),
                    progress: g["targetAmount"].as_f64().and(g["progress"].as_f64()),
                    planned_monthly: g["plannedMonthlyContribution"].as_f64().unwrap_or(0.0),
                    contributed_this_month: g["currentMonthActualBudgetAmount"].as_f64().unwrap_or(0.0),
                    net_contribution: g["netContribution"].as_f64().unwrap_or(0.0),
                    accounts: g["allocationAmountsByAccount"]
                        .as_array()
                        .map(|allocations| {
                            allocations
                                .iter()
                                .map(|a| {
                                    (
                                        a["account"]["displayName"].as_str().unwrap_or("Account").to_string(),
                                        a["totalAmount"].as_f64().unwrap_or(0.0),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    archived: !g["archivedAt"].is_null(),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let query = app.query(Screen::Goals);
    let mut goals = goals(app);
    goals.retain(|goal| {
        let mut fields: Vec<&str> = vec![&goal.name];
        fields.extend(goal.accounts.iter().map(|(account, _)| account.as_str()));
        query.matches(&fields, Some(goal.balance))
    });
    if goals.is_empty() {
        let block = card("Goals");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        frame.render_widget(Line::styled(widgets::no_matches(app, "No goals set up in Monarch"), theme::muted()), inner);
        return;
    }

    let total: f64 = goals.iter().map(|g| g.balance).sum();
    let [summary, list] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Line::from(vec![
            Span::styled(" Saved toward goals ", theme::muted()),
            Span::styled(fmt::money0(total), theme::bold()),
        ]),
        summary,
    );

    let slots = Layout::vertical(vec![Constraint::Length(GOAL_CARD_HEIGHT); goals.len()]).split(list);
    for (goal, slot) in goals.iter().zip(slots.iter()) {
        if slot.height < GOAL_CARD_HEIGHT {
            break;
        }
        let title = if goal.archived { format!("{} (archived)", goal.name) } else { goal.name.clone() };
        let block = card(&title);
        let inner = block.inner(*slot);
        frame.render_widget(block, *slot);

        let mut lines = vec![match (goal.target, goal.progress) {
            (Some(target), Some(progress)) => Line::from(vec![
                Span::styled(fmt::money0(goal.balance), theme::bold()),
                Span::styled(format!(" of {}  ", fmt::money0(target)), theme::muted()),
                Span::styled(fmt::hbar(progress, PROGRESS_BAR_WIDTH), theme::gain(1.0)),
                Span::styled(format!(" {:.0}%", progress * 100.0), theme::muted()),
            ]),
            _ => Line::from(vec![
                Span::styled(fmt::money0(goal.balance), theme::bold()),
                Span::styled("  no target set", theme::muted()),
            ]),
        }];
        lines.push(Line::from(vec![
            Span::styled("This month ", theme::muted()),
            Span::styled(fmt::money0(goal.contributed_this_month), theme::text()),
            Span::styled(format!(" of {} planned", fmt::money0(goal.planned_monthly)), theme::muted()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Net contributions ", theme::muted()),
            Span::styled(fmt::money0(goal.net_contribution), theme::text()),
        ]));
        for (account, amount) in &goal.accounts {
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", account), theme::muted()),
                Span::styled(fmt::money0(*amount), theme::text()),
            ]));
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}
