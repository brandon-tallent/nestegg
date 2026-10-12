//! Hit regions are recorded during rendering, including clipped and scrolled rows.
use crate::app::{App, EquityView, Screen};
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Screen(Screen),
    Key(KeyCode),
    Row {
        index: usize,
        view: Option<EquityView>,
    },
    Scroll {
        view: Option<EquityView>,
        bounds: Option<(usize, usize)>,
    },
    Ignore,
    EditSuggestion(usize),
    ToggleRow(usize),
}

#[derive(Default)]
pub struct HitMap(Vec<(Rect, Action)>);
impl HitMap {
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn at(&self, x: u16, y: u16) -> Option<Action> {
        self.0
            .iter()
            .rev()
            .find(|(r, _)| r.contains(Position::new(x, y)))
            .map(|(_, a)| *a)
    }
}

pub fn region(app: &App, area: Rect, action: Action) {
    if !area.is_empty() {
        app.mouse.borrow_mut().0.push((area, action));
    }
}
pub fn key(app: &App, area: Rect, code: KeyCode) {
    region(app, area, Action::Key(code));
}
pub fn scroll(app: &App, area: Rect, view: Option<EquityView>, bounds: Option<(usize, usize)>) {
    region(app, area, Action::Scroll { view, bounds });
}
pub fn row(app: &App, area: Rect, index: usize, view: Option<EquityView>) {
    region(app, area, Action::Row { index, view });
}
pub fn rows(app: &App, area: Rect, start: usize, end: usize, view: Option<EquityView>) {
    scroll(app, area, view, None);
    for index in start..end.min(start.saturating_add(area.height as usize)) {
        row(
            app,
            Rect::new(area.x, area.y + (index - start) as u16, area.width, 1),
            index,
            view,
        );
    }
}
pub fn modal(app: &App, screen: Rect, popup: Rect) {
    app.mouse.borrow_mut().clear();
    key(app, screen, KeyCode::Esc);
    region(app, popup, Action::Ignore);
    if !popup.is_empty() {
        key(
            app,
            Rect::new(
                popup.right().saturating_sub(12).max(popup.x),
                popup.y,
                popup.width.min(12),
                1,
            ),
            KeyCode::Esc,
        );
    }
}
pub fn footer(app: &App, area: Rect, label: &str) {
    let pair = match label {
        "[ ]" => Some((KeyCode::Char('['), KeyCode::Char(']'))),
        "←→" => Some((KeyCode::Left, KeyCode::Right)),
        "↑↓" => Some((KeyCode::Up, KeyCode::Down)),
        _ => None,
    };
    if let Some((left, right)) = pair {
        key(
            app,
            Rect::new(area.x, area.y, area.width / 2, area.height),
            left,
        );
        key(
            app,
            Rect::new(
                area.x + area.width / 2,
                area.y,
                area.width - area.width / 2,
                area.height,
            ),
            right,
        );
    } else if label == "⏎" {
        key(app, area, KeyCode::Enter);
    } else if label.chars().count() == 1 {
        key(app, area, KeyCode::Char(label.chars().next().unwrap()));
    }
}

impl App {
    fn mouse_view(&mut self, view: Option<EquityView>) {
        if let Some(view) = view {
            if self.equity_view != view {
                self.equity_view = view;
                self.equity_selected = 0;
                self.equity_detail = false;
            }
        }
    }
    pub fn handle_mouse(&mut self, event: MouseEvent) {
        let action = self.mouse.borrow().at(event.column, event.row);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let previous = self.last_click.take();
                let now = Instant::now();
                match action {
                    Some(Action::ToggleRow(index)) => {
                        self.tx.selected = index;
                        self.mark_transaction();
                    }
                    Some(Action::EditSuggestion(index)) => {
                        let name = self.edit_suggestions().get(index).cloned();
                        if let (Some(dialog), Some(name)) = (self.editor.as_mut(), name) {
                            dialog.input = name;
                            dialog.suggestion = 0;
                        }
                    }
                    Some(Action::Screen(screen)) => self.switch_screen(screen),
                    Some(Action::Key(code)) => {
                        // Clicking a control leaves text entry, without clearing its query.
                        self.searching = false;
                        self.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
                    }
                    Some(target @ Action::Row { index, view }) => {
                        self.searching = false;
                        self.mouse_view(view);
                        let last = self.list_len().saturating_sub(1);
                        if let Some(cursor) = self.cursor() {
                            *cursor = index.min(last);
                        }
                        let double = previous.is_some_and(|(old, at)| {
                            old == target && now.duration_since(at) <= Duration::from_millis(400)
                        });
                        if double {
                            self.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                        } else {
                            self.last_click = Some((target, now));
                        }
                    }
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                self.last_click = None;
                let delta = if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                };
                match action {
                    Some(Action::Row { view, .. })
                    | Some(Action::Scroll { view, bounds: None }) => {
                        self.mouse_view(view);
                        self.move_cursor(delta);
                    }
                    Some(Action::ToggleRow(_)) => self.move_cursor(delta),
                    Some(Action::Scroll {
                        bounds: Some((offset, max)),
                        ..
                    }) => {
                        if let Some(cursor) = self.cursor() {
                            *cursor = offset.saturating_add_signed(delta).min(max);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use ratatui::{backend::TestBackend, Terminal};
    fn click(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }
    #[test]
    fn rendered_navigation_works_in_wide_and_narrow_layouts() {
        for width in [80, 140] {
            let mut app = App::new(Data::default());
            let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &app)).unwrap();
            let rect = app
                .mouse
                .borrow()
                .0
                .iter()
                .find(|(_, a)| *a == Action::Screen(Screen::Accounts))
                .unwrap()
                .0;
            app.handle_mouse(click(rect.x, rect.y));
            assert_eq!(app.screen, Screen::Accounts);
        }
    }
    #[test]
    fn windowed_rows_select_visible_index_and_double_click_opens() {
        let mut app = App::new(Data::default());
        app.screen = Screen::Transactions;
        app.tx.visible = (0..100).collect();
        rows(&app, Rect::new(20, 4, 50, 8), 30, 38, None);
        app.handle_mouse(click(25, 7));
        assert_eq!(app.tx.selected, 33);
        assert!(!app.tx.detail);
        app.handle_mouse(click(25, 7));
        assert!(app.tx.detail);
        assert_eq!(app.mouse.borrow().at(25, 12), None);
    }
    #[test]
    fn wheel_uses_actual_scroll_offset_and_stops_at_bottom() {
        let mut app = App::new(Data::default());
        app.screen = Screen::Reports;
        app.reports_scroll = 999;
        scroll(&app, Rect::new(20, 4, 50, 8), None, Some((8, 8)));
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            ..click(25, 7)
        });
        assert_eq!(app.reports_scroll, 5);
    }
    #[test]
    fn modal_blocks_rows_and_wheel_but_outside_click_closes() {
        let mut app = App::new(Data::default());
        app.screen = Screen::Transactions;
        app.tx.detail = true;
        app.tx.visible = (0..20).collect();
        rows(&app, Rect::new(20, 4, 50, 8), 0, 8, None);
        modal(&app, Rect::new(0, 0, 100, 40), Rect::new(30, 5, 40, 20));
        app.handle_mouse(click(35, 7));
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            ..click(35, 7)
        });
        assert!(app.tx.detail);
        assert_eq!(app.tx.selected, 0);
        app.handle_mouse(click(1, 1));
        assert!(!app.tx.detail);
    }
    #[test]
    fn clicks_match_rendered_account_rows_without_selecting_group_headers() {
        use crate::data::Account;
        let account = |name: &str, kind: &str, group: &str| Account {
            id: name.into(), name: name.into(), mask: None, kind: kind.into(), kind_display: group.into(),
            subtype: String::new(), institution: String::new(), balance: 5.0, signed_balance: 5.0,
            is_asset: true, is_hidden: false, include_in_net_worth: true, needs_attention: false,
            last_synced: None, credit_limit: None, interest_rate: None, minimum_payment: None,
        };
        for width in [80, 140] {
            let mut data = Data::default();
            data.accounts = vec![account("Cash A", "depository", "Cash"), account("Credit B", "credit", "Credit Cards")];
            let mut app = App::new(data); app.screen = Screen::Accounts;
            let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
            terminal.draw(|f| crate::ui::draw(f, &app)).unwrap();
            let find = |needle: &str| (0..40).find_map(|y| {
                let line: String = (0..width).map(|x| terminal.backend().buffer()[(x,y)].symbol()).collect();
                line.find(needle).map(|x| (x as u16,y))
            }).unwrap();
            let (x,y)=find("Credit B"); app.handle_mouse(click(x,y)); assert_eq!(app.account_selected,1);
            let (x,y)=find("Credit Cards"); app.handle_mouse(click(x,y)); assert_eq!(app.account_selected,1);
        }
    }

}
