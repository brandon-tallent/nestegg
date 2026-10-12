//! Application state and key handling

use crate::data::{month_of, Account, Data, Holding, MonthFlow, Position, Txn};
use crate::search::Query;
use chrono::{Datelike, Months, NaiveDate};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Rows moved by PageUp / PageDown
const PAGE_ROWS: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Accounts,
    Transactions,
    CashFlow,
    Reports,
    Budget,
    Recurring,
    Goals,
    Investments,
    Equity,
}

impl Screen {
    pub const ALL: [Screen; 10] = [
        Screen::Dashboard,
        Screen::Accounts,
        Screen::Transactions,
        Screen::CashFlow,
        Screen::Reports,
        Screen::Budget,
        Screen::Recurring,
        Screen::Goals,
        Screen::Investments,
        Screen::Equity,
    ];

    /// The digit that opens the screen: 1 to 9, then 0 for the tenth
    pub fn key(self) -> char {
        char::from_digit(((self.index() + 1) % 10) as u32, 10).unwrap_or('?')
    }

    pub fn title(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Accounts => "Accounts",
            Screen::Transactions => "Transactions",
            Screen::CashFlow => "Cash Flow",
            Screen::Reports => "Reports",
            Screen::Budget => "Budget",
            Screen::Recurring => "Recurring",
            Screen::Goals => "Goals",
            Screen::Investments => "Investments",
            Screen::Equity => "Equity",
        }
    }

    /// Name accepted by `--dump` and `--screen`
    pub fn slug(self) -> &'static str {
        match self {
            Screen::Dashboard => "dashboard",
            Screen::Accounts => "accounts",
            Screen::Transactions => "transactions",
            Screen::CashFlow => "cashflow",
            Screen::Reports => "reports",
            Screen::Budget => "budget",
            Screen::Recurring => "recurring",
            Screen::Goals => "goals",
            Screen::Investments => "investments",
            Screen::Equity => "equity",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Screen> {
        Screen::ALL.into_iter().find(|s| s.slug() == slug)
    }

    pub fn index(self) -> usize {
        Screen::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

/// Time window for history charts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    OneMonth,
    ThreeMonths,
    SixMonths,
    YearToDate,
    OneYear,
    All,
}

impl Range {
    const ALL: [Range; 6] = [
        Range::OneMonth,
        Range::ThreeMonths,
        Range::SixMonths,
        Range::YearToDate,
        Range::OneYear,
        Range::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Range::OneMonth => "1M",
            Range::ThreeMonths => "3M",
            Range::SixMonths => "6M",
            Range::YearToDate => "YTD",
            Range::OneYear => "1Y",
            Range::All => "ALL",
        }
    }

    /// First date included in the window ending at `today`
    pub fn start(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Range::OneMonth => today.checked_sub_months(Months::new(1)),
            Range::ThreeMonths => today.checked_sub_months(Months::new(3)),
            Range::SixMonths => today.checked_sub_months(Months::new(6)),
            Range::YearToDate => NaiveDate::from_ymd_opt(today.year(), 1, 1),
            Range::OneYear => today.checked_sub_months(Months::new(12)),
            Range::All => None,
        }
    }

    fn shift(self, forward: bool) -> Range {
        let i = Range::ALL.iter().position(|r| *r == self).unwrap_or(0);
        let next = if forward {
            (i + 1).min(Range::ALL.len() - 1)
        } else {
            i.saturating_sub(1)
        };
        Range::ALL[next]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxFilter {
    All,
    NeedsReview,
    Pending,
    Income,
    Expenses,
    Transfers,
    Recurring,
}

impl TxFilter {
    const ALL: [TxFilter; 7] = [
        TxFilter::All,
        TxFilter::NeedsReview,
        TxFilter::Pending,
        TxFilter::Income,
        TxFilter::Expenses,
        TxFilter::Transfers,
        TxFilter::Recurring,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TxFilter::All => "All",
            TxFilter::NeedsReview => "Needs review",
            TxFilter::Pending => "Pending",
            TxFilter::Income => "Income",
            TxFilter::Expenses => "Expenses",
            TxFilter::Transfers => "Transfers",
            TxFilter::Recurring => "Recurring",
        }
    }

    fn matches(self, txn: &Txn) -> bool {
        match self {
            TxFilter::All => true,
            TxFilter::NeedsReview => txn.needs_review,
            TxFilter::Pending => txn.pending,
            TxFilter::Income => txn.group_type == "income",
            TxFilter::Expenses => txn.group_type == "expense",
            TxFilter::Transfers => txn.group_type == "transfer",
            TxFilter::Recurring => txn.recurring,
        }
    }
}

/// What the Cash Flow breakdown is grouped by
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakdown {
    Category,
    Group,
    Merchant,
}

impl Breakdown {
    pub fn label(self) -> &'static str {
        match self {
            Breakdown::Category => "Category",
            Breakdown::Group => "Group",
            Breakdown::Merchant => "Merchant",
        }
    }

    fn next(self) -> Breakdown {
        match self {
            Breakdown::Category => Breakdown::Group,
            Breakdown::Group => Breakdown::Merchant,
            Breakdown::Merchant => Breakdown::Category,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvestView {
    Holdings,
    ByAccount,
}

/// Which list on the Equity screen the cursor is in. Exercises and sales
/// take the place of the shares held; the grants are always showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquityView {
    Grants,
    Shares,
    History,
}

#[derive(Debug, Default, Clone)]
pub struct TxState {
    pub selected: usize,
    pub search: String,
    pub filter_index: usize,
    /// Restrict to one account id
    pub account: Option<String>,
    /// Restrict to one calendar month and category (drill-down from Cash Flow)
    pub month: Option<NaiveDate>,
    pub category: Option<String>,
    pub detail: bool,
    /// Indices into `Data::transactions` that pass the filters
    pub visible: Vec<usize>,
}

impl TxState {
    pub fn filter(&self) -> TxFilter {
        TxFilter::ALL[self.filter_index]
    }

    pub fn has_filters(&self) -> bool {
        !self.search.is_empty()
            || self.filter_index != 0
            || self.account.is_some()
            || self.month.is_some()
            || self.category.is_some()
    }
}

pub struct App {
    /// What the screens show: the real data, or its disguise
    pub data: Data,
    /// The real data, kept aside while a disguise is showing
    real: Option<Data>,
    /// Fixed for the session so repeated screenshots agree with each other
    disguise_seed: u64,
    pub screen: Screen,
    pub mouse: std::cell::RefCell<crate::mouse::HitMap>,
    pub last_click: Option<(crate::mouse::Action, std::time::Instant)>,
    pub range: Range,
    pub show_help: bool,
    /// Keystrokes are going into the current screen's search
    pub searching: bool,
    /// What each screen is searched for, in `Screen::ALL` order. Transactions
    /// keeps its own in `TxState`, where drill-downs also set it.
    searches: Vec<String>,
    pub show_hidden_accounts: bool,
    /// Whether income and spending figures include one-time items
    pub include_one_time: bool,
    pub quit: bool,
    /// Set when the user asks for the data to be re-read
    pub reload_requested: bool,
    pub status: String,

    pub account_selected: usize,
    pub tx: TxState,
    /// Screen that opened the current transaction drill-down.
    pub tx_return: Option<(Screen, TxState)>,
    pub marked: std::collections::BTreeSet<String>,
    pub editor: Option<crate::edits::Dialog>,
    pub edit_job: Option<crate::edits::Job>,
    pub edit_busy: bool,
    /// Index into `Data::months`
    pub cash_month: usize,
    pub breakdown: Breakdown,
    pub cash_selected: usize,
    pub reports_scroll: usize,
    pub budget_scroll: usize,
    pub recurring_selected: usize,
    pub invest_view: InvestView,
    pub invest_selected: usize,
    pub equity_view: EquityView,
    pub equity_selected: usize,
    /// Whether the selected grant, certificate or event is opened up
    pub equity_detail: bool,
}

impl App {
    pub fn new(data: Data) -> Self {
        let disguise_seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or_default();
        let mut app = Self {
            data,
            real: None,
            disguise_seed,
            screen: Screen::Dashboard,
            mouse: Default::default(),
            last_click: None,
            range: Range::ThreeMonths,
            show_help: false,
            searching: false,
            searches: vec![String::new(); Screen::ALL.len()],
            show_hidden_accounts: false,
            include_one_time: false,
            quit: false,
            reload_requested: false,
            status: String::new(),
            account_selected: 0,
            tx: TxState::default(),
            tx_return: None,
            marked: Default::default(),
            editor: None,
            edit_job: None,
            edit_busy: false,
            cash_month: 0,
            breakdown: Breakdown::Category,
            cash_selected: 0,
            reports_scroll: 0,
            budget_scroll: 0,
            recurring_selected: 0,
            invest_view: InvestView::Holdings,
            invest_selected: 0,
            equity_view: EquityView::Grants,
            equity_selected: 0,
            equity_detail: false,
        };
        app.after_load();
        app
    }

    /// Swap in freshly loaded data, keeping the user's place where possible
    pub fn replace_data(&mut self, data: Data) {
        let previous_month = self.cash_flow_month();
        if self.real.is_some() {
            self.data = crate::anonymize::anonymize(&data, self.disguise_seed);
            self.real = Some(data);
        } else {
            self.data = data;
        }
        self.after_load();
        if let Some(index) = previous_month.and_then(|month| self.data.months.iter().position(|m| m.month == month)) { self.cash_month = index; }
    }

    /// Switch between the real data and a disguised copy safe to share
    pub fn toggle_anonymized(&mut self) {
        match self.real.take() {
            Some(real) => self.data = real,
            None => {
                let disguised = crate::anonymize::anonymize(&self.data, self.disguise_seed);
                self.real = Some(std::mem::replace(&mut self.data, disguised));
            }
        }
        // A search typed against real names would otherwise linger on screen
        self.tx.search.clear();
        self.searches.iter_mut().for_each(String::clear);
        self.searching = false;
        self.tx.detail = false;
        self.after_load();
    }

    fn after_load(&mut self) {
        let current = month_of(self.data.today());
        self.cash_month = self
            .data
            .months
            .iter()
            .position(|m| m.month == current)
            .unwrap_or(self.data.months.len().saturating_sub(1));
        self.refilter();
        self.account_selected = self.account_selected.min(self.listed_accounts().len().saturating_sub(1));
    }

    /// The text a screen is being searched for
    pub fn search_text(&self, screen: Screen) -> &str {
        match screen {
            Screen::Transactions => &self.tx.search,
            other => &self.searches[other.index()],
        }
    }

    fn search_text_mut(&mut self) -> &mut String {
        match self.screen {
            Screen::Transactions => &mut self.tx.search,
            other => &mut self.searches[other.index()],
        }
    }

    /// A screen's search, ready to test its rows
    pub fn query(&self, screen: Screen) -> Query {
        Query::parse(self.search_text(screen))
    }

    /// Accounts in display order: grouped by type, Monarch's order within a
    /// type, narrowed to the Accounts search
    pub fn listed_accounts(&self) -> Vec<&Account> {
        let query = self.query(Screen::Accounts);
        let mut accounts: Vec<&Account> = self
            .data
            .accounts
            .iter()
            .filter(|a| self.show_hidden_accounts || !a.is_hidden)
            .filter(|a| {
                let mask = a.mask.as_deref().unwrap_or("");
                query.matches(&[&a.name, &a.institution, &a.kind_display, &a.subtype, mask], Some(a.balance))
            })
            .collect();
        accounts.sort_by_key(|a| account_kind_rank(&a.kind));
        accounts
    }

    pub fn selected_account(&self) -> Option<&Account> {
        self.listed_accounts().get(self.account_selected).copied()
    }

    /// Holdings narrowed to the Investments search
    pub fn listed_holdings(&self) -> Vec<&Holding> {
        let query = self.query(Screen::Investments);
        self.data
            .holdings
            .iter()
            .filter(|h| query.matches(&[&h.ticker, &h.name, &h.asset_class], Some(h.value)))
            .collect()
    }

    /// Positions narrowed to the Investments search
    pub fn listed_positions(&self) -> Vec<&Position> {
        let query = self.query(Screen::Investments);
        self.data
            .positions
            .iter()
            .filter(|p| query.matches(&[&p.ticker, &p.name, &p.kind, &p.account_name], Some(p.value)))
            .collect()
    }

    /// Whether a transaction counts toward income and spending right now
    pub fn counts(&self, txn: &Txn) -> bool {
        txn.in_reports() && (self.include_one_time || !txn.one_time)
    }

    /// Monthly income and spending, with one-time items in or out as chosen
    pub fn months(&self) -> Vec<MonthFlow> {
        self.data
            .months
            .iter()
            .map(|m| if self.include_one_time { m.with_one_time() } else { *m })
            .collect()
    }

    /// One-time income and spending in a month, whether or not it is being counted
    pub fn one_time_in(&self, month: NaiveDate) -> (f64, f64) {
        self.data
            .months
            .iter()
            .find(|m| m.month == month)
            .map(|m| (m.one_time_income, m.one_time_expense))
            .unwrap_or_default()
    }

    /// Month shown on the Cash Flow screen
    pub fn cash_flow_month(&self) -> Option<NaiveDate> {
        self.data.months.get(self.cash_month).map(|m| m.month)
    }

    /// Recompute which transactions pass the current filters
    pub fn refilter(&mut self) {
        let query = Query::parse(&self.tx.search);
        let filter = self.tx.filter();
        let tx = &self.tx;
        let data = &self.data;
        let visible: Vec<usize> = data
            .transactions
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                filter.matches(t)
                    && tx.account.as_ref().is_none_or(|id| &t.account_id == id)
                    && tx.month.is_none_or(|m| month_of(t.date) == m)
                    && tx.category.as_ref().is_none_or(|c| &t.category == c)
                    && (query.is_empty()
                        || query.matches(
                            &[
                                &t.merchant,
                                &t.category,
                                &t.notes,
                                &t.original_name,
                                &t.tags,
                                data.account_name(&t.account_id),
                                &t.date.format("%Y-%m-%d %b %B").to_string(),
                            ],
                            Some(t.amount),
                        ))
            })
            .map(|(i, _)| i)
            .collect();
        self.tx.visible = visible;
        self.tx.selected = self.tx.selected.min(self.tx.visible.len().saturating_sub(1));
    }

    pub fn selected_transaction(&self) -> Option<&Txn> {
        self.tx
            .visible
            .get(self.tx.selected)
            .and_then(|i| self.data.transactions.get(*i))
    }

    fn clear_tx_filters(&mut self) {
        self.tx.search.clear();
        self.tx.filter_index = 0;
        self.tx.account = None;
        self.tx.month = None;
        self.tx.category = None;
        self.tx.selected = 0;
        self.refilter();
    }

    /// Open Transactions restricted to an account
    fn begin_transaction_drilldown(&mut self) {
        self.tx_return = Some((self.screen, std::mem::take(&mut self.tx)));
    }

    fn open_account_transactions(&mut self, account_id: String) {
        self.begin_transaction_drilldown();
        self.clear_tx_filters();
        self.tx.account = Some(account_id);
        self.refilter();
        self.screen = Screen::Transactions;
    }

    /// Number of rows in the list the current screen scrolls
    pub(crate) fn list_len(&self) -> usize {
        match self.screen {
            Screen::Accounts => self.listed_accounts().len(),
            Screen::Transactions => self.tx.visible.len(),
            Screen::CashFlow => crate::ui::cashflow::breakdown_rows(self, false).len(),
            Screen::Recurring => crate::ui::recurring::rows(self).len(),
            Screen::Investments => match self.invest_view {
                InvestView::Holdings => self.listed_holdings().len(),
                InvestView::ByAccount => self.listed_positions().len(),
            },
            Screen::Equity => crate::ui::equity::list_len(self),
            // Scroll offsets: bounded generously, clamped again when drawn
            Screen::Reports | Screen::Budget => usize::MAX / 2,
            Screen::Dashboard | Screen::Goals => 0,
        }
    }

    pub(crate) fn cursor(&mut self) -> Option<&mut usize> {
        Some(match self.screen {
            Screen::Accounts => &mut self.account_selected,
            Screen::Transactions => &mut self.tx.selected,
            Screen::CashFlow => &mut self.cash_selected,
            Screen::Reports => &mut self.reports_scroll,
            Screen::Budget => &mut self.budget_scroll,
            Screen::Recurring => &mut self.recurring_selected,
            Screen::Investments => &mut self.invest_selected,
            Screen::Equity => &mut self.equity_selected,
            Screen::Dashboard | Screen::Goals => return None,
        })
    }

    pub(crate) fn move_cursor(&mut self, delta: isize) {
        let len = self.list_len();
        if let Some(cursor) = self.cursor() {
            let last = len.saturating_sub(1) as isize;
            *cursor = (*cursor as isize + delta).clamp(0, last.max(0)) as usize;
        }
    }

    fn jump_cursor(&mut self, to_end: bool) {
        let len = self.list_len();
        // Scroll offsets have no real end to jump to
        let scrolls = matches!(self.screen, Screen::Reports | Screen::Budget);
        if let Some(cursor) = self.cursor() {
            *cursor = if to_end && !scrolls { len.saturating_sub(1) } else { 0 };
        }
    }

    pub(crate) fn switch_screen(&mut self, screen: Screen) {
        self.tx_return = None;
        self.last_click = None;
        self.screen = screen;
        self.tx.detail = false;
        self.equity_detail = false;
        self.searching = false;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if self.edit_busy && (matches!(key.code, KeyCode::Char('q') | KeyCode::Char('a')) || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))) {
            self.status = "Wait for the Monarch update to finish".into(); return;
        }
        if self.editor.is_some() { self.handle_edit_key(key); return; }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        if self.searching {
            self.handle_search_key(key);
            return;
        }

        if self.show_help {
            self.show_help = false;
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('r') => self.reload_requested = true,
            KeyCode::Char('a') => self.toggle_anonymized(),
            KeyCode::Char('o') => self.include_one_time = !self.include_one_time,
            KeyCode::Char(c @ '0'..='9') => {
                if let Some(screen) = Screen::ALL.into_iter().find(|s| s.key() == c) {
                    self.switch_screen(screen);
                }
            }
            KeyCode::Tab => {
                let next = (self.screen.index() + 1) % Screen::ALL.len();
                self.switch_screen(Screen::ALL[next]);
            }
            KeyCode::BackTab => {
                let previous = (self.screen.index() + Screen::ALL.len() - 1) % Screen::ALL.len();
                self.switch_screen(Screen::ALL[previous]);
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.move_cursor(PAGE_ROWS as isize),
            KeyCode::PageUp => self.move_cursor(-(PAGE_ROWS as isize)),
            KeyCode::Char('g') | KeyCode::Home => self.jump_cursor(false),
            KeyCode::Char('G') | KeyCode::End => self.jump_cursor(true),
            KeyCode::Char(']') => self.range = self.range.shift(true),
            KeyCode::Char('[') => self.range = self.range.shift(false),
            KeyCode::Char('/') => {
                // The dashboard has no list of its own: search everything there is
                if self.screen == Screen::Dashboard {
                    self.clear_tx_filters();
                    self.switch_screen(Screen::Transactions);
                }
                self.searching = true;
                self.tx.detail = false;
                self.equity_detail = false;
            }
            KeyCode::Esc if self.screen == Screen::Equity && self.equity_detail => self.equity_detail = false,
            // Transactions clears its search along with its other filters
            KeyCode::Esc if self.screen != Screen::Transactions && !self.search_text(self.screen).is_empty() => {
                self.search_text_mut().clear();
                self.reset_cursor();
            }
            _ => self.handle_screen_key(key),
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.searching = false,
            KeyCode::Esc => {
                self.searching = false;
                self.search_text_mut().clear();
            }
            KeyCode::Backspace => {
                self.search_text_mut().pop();
            }
            KeyCode::Char(c) => self.search_text_mut().push(c),
            _ => return,
        }
        self.reset_cursor();
    }

    /// Back to the top of the current screen's list, whose rows just changed
    fn reset_cursor(&mut self) {
        if let Some(cursor) = self.cursor() {
            *cursor = 0;
        }
        if self.screen == Screen::Transactions {
            self.refilter();
        }
    }

    fn handle_screen_key(&mut self, key: KeyEvent) {
        match self.screen {
            Screen::Accounts => match key.code {
                KeyCode::Enter => {
                    if let Some(id) = self.selected_account().map(|a| a.id.clone()) {
                        self.open_account_transactions(id);
                    }
                }
                KeyCode::Char('h') => {
                    self.show_hidden_accounts = !self.show_hidden_accounts;
                    self.account_selected = 0;
                }
                _ => {}
            },
            Screen::Transactions => match key.code {
                KeyCode::Char('t') => self.mark_transaction(),
                KeyCode::Char('T') => { for index in &self.tx.visible { self.marked.insert(self.data.transactions[*index].id.clone()); } }
                KeyCode::Char('u') => self.marked.clear(),
                KeyCode::Char('m') => self.begin_edit(crate::edits::Field::Merchant),
                KeyCode::Char('C') => self.begin_edit(crate::edits::Field::Category),
                KeyCode::Char('f') => {
                    self.tx.filter_index = (self.tx.filter_index + 1) % TxFilter::ALL.len();
                    self.tx.selected = 0;
                    self.refilter();
                }
                KeyCode::Char('c') => self.clear_tx_filters(),
                KeyCode::Enter => self.tx.detail = !self.tx.detail,
                KeyCode::Esc => {
                    if self.tx.detail {
                        self.tx.detail = false;
                    } else if let Some((screen, previous)) = self.tx_return.take() {
                        self.tx = previous;
                        self.refilter();
                        self.screen = screen;
                        self.searching = false;
                    } else if self.tx.has_filters() {
                        self.clear_tx_filters();
                    }
                }
                _ => {}
            },
            Screen::CashFlow => match key.code {
                KeyCode::Char('h') | KeyCode::Left => {
                    self.cash_month = self.cash_month.saturating_sub(1);
                    self.cash_selected = 0;
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.cash_month = (self.cash_month + 1).min(self.data.months.len().saturating_sub(1));
                    self.cash_selected = 0;
                }
                KeyCode::Char('b') => {
                    self.breakdown = self.breakdown.next();
                    self.cash_selected = 0;
                }
                KeyCode::Enter => self.drill_into_cash_flow(),
                _ => {}
            },
            Screen::Investments => {
                if key.code == KeyCode::Char('v') {
                    self.invest_view = match self.invest_view {
                        InvestView::Holdings => InvestView::ByAccount,
                        InvestView::ByAccount => InvestView::Holdings,
                    };
                    self.invest_selected = 0;
                }
            }
            Screen::Equity => match key.code {
                KeyCode::Char('v') => {
                    self.equity_view = match self.equity_view {
                        EquityView::Grants => EquityView::Shares,
                        EquityView::Shares => EquityView::History,
                        EquityView::History => EquityView::Grants,
                    };
                    self.equity_selected = 0;
                    self.equity_detail = false;
                }
                KeyCode::Enter => self.equity_detail = !self.equity_detail && self.list_len() > 0,
                _ => {}
            },
            Screen::Recurring => {
                if key.code == KeyCode::Enter {
                    let selected = crate::ui::recurring::rows(self)
                        .get(self.recurring_selected)
                        .map(|row| (row.stream.merchant.clone(), row.stream.account_id.clone()));
                    if let Some((merchant, account_id)) = selected {
                        self.begin_transaction_drilldown();
                        self.clear_tx_filters();
                        self.tx.search = merchant;
                        self.tx.account = Some(account_id);
                        self.refilter();
                        self.screen = Screen::Transactions;
                    }
                }
            }
            Screen::Dashboard | Screen::Reports | Screen::Budget | Screen::Goals => {}
        }
    }

    /// Open the transactions behind the selected Cash Flow expense row
    fn drill_into_cash_flow(&mut self) {
        let rows = crate::ui::cashflow::breakdown_rows(self, false);
        let Some(name) = rows.get(self.cash_selected).map(|row| row.0.clone()) else {
            return;
        };
        self.begin_transaction_drilldown();
        let month = self.cash_flow_month();
        let breakdown = self.breakdown;
        self.clear_tx_filters();
        self.tx.month = month;
        match breakdown {
            Breakdown::Category => self.tx.category = Some(name),
            // Groups and merchants have no dedicated filter; search covers merchants
            Breakdown::Merchant => self.tx.search = name,
            Breakdown::Group => {}
        }
        self.tx.filter_index = TxFilter::ALL.iter().position(|f| *f == TxFilter::Expenses).unwrap_or(0);
        self.refilter();
        self.screen = Screen::Transactions;
    }

    /// The last `days` days of history from a dated series, honoring `range`
    pub fn in_range<'a, T>(&self, series: &'a [T], date: impl Fn(&T) -> NaiveDate) -> &'a [T] {
        match self.range.start(self.data.today()) {
            Some(start) => {
                let first = series.partition_point(|item| date(item) < start);
                &series[first..]
            }
            None => series,
        }
    }
}

/// Display order of account types, matching Monarch's accounts page
fn account_kind_rank(kind: &str) -> usize {
    const ORDER: [&str; 8] = [
        "depository",
        "credit",
        "brokerage",
        "loan",
        "real_estate",
        "vehicle",
        "equity",
        "valuables",
    ];
    ORDER.iter().position(|k| *k == kind).unwrap_or(ORDER.len())
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    fn key(app: &mut App, code: KeyCode) { app.handle_key(KeyEvent::new(code, KeyModifiers::NONE)); }
    #[test]
    fn escape_closes_detail_then_returns_to_the_origin() {
        for origin in [Screen::CashFlow, Screen::Accounts, Screen::Recurring] {
            let mut app = App::new(Data::default());
            app.screen = origin;
            app.tx.search = "previous transaction query".into();
            app.cash_selected = 3;
            app.account_selected = 2;
            app.begin_transaction_drilldown();
            app.screen = Screen::Transactions;
            app.tx.category = Some("Coffee Shops".into());
            app.tx.detail = true;
            key(&mut app, KeyCode::Esc);
            assert_eq!(app.screen, Screen::Transactions);
            assert!(!app.tx.detail);
            key(&mut app, KeyCode::Esc);
            assert_eq!(app.screen, origin);
            assert_eq!(app.cash_selected, 3);
            assert_eq!(app.account_selected, 2);
            assert_eq!(app.tx.search, "previous transaction query");
        }
    }
    #[test]
    fn explicit_navigation_does_not_keep_a_stale_back_target() {
        let mut app = App::new(Data::default());
        app.screen = Screen::CashFlow;
        app.begin_transaction_drilldown();
        app.switch_screen(Screen::Transactions);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.screen, Screen::Transactions);
        assert!(app.tx_return.is_none());
    }
}
