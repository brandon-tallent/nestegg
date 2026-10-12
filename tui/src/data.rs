//! Loads the mirror database into memory.
//!
//! The whole dataset is small (tens of thousands of rows), so everything is
//! read up front and screens work from plain vectors.

use anyhow::{Context, Result};
use crate::equity::{Certificate, Equity, Exercise, Grant, Issuer, Snapshot, Tender, VestEvent};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub mask: Option<String>,
    /// depository, credit, brokerage, loan, real_estate, ...
    pub kind: String,
    /// Cash, Credit Cards, Investments, ...
    pub kind_display: String,
    pub subtype: String,
    pub institution: String,
    /// Balance as Monarch displays it (positive for liabilities too)
    pub balance: f64,
    /// Negative for liabilities
    pub signed_balance: f64,
    pub is_asset: bool,
    pub is_hidden: bool,
    pub include_in_net_worth: bool,
    pub needs_attention: bool,
    pub last_synced: Option<DateTime<Utc>>,
    pub credit_limit: Option<f64>,
    /// Annual rate in percent and required monthly payment, for loans
    pub interest_rate: Option<f64>,
    pub minimum_payment: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Txn {
    pub date: NaiveDate,
    pub amount: f64,
    /// Statement credits matched to this purchase (positive), so its true
    /// cost is `amount + credited`
    pub credited: f64,
    /// This transaction is itself a statement credit matched to a purchase
    pub is_matched_credit: bool,
    pub merchant: String,
    pub category: String,
    /// income, expense or transfer
    pub group_type: String,
    pub account_id: String,
    pub notes: String,
    pub original_name: String,
    pub tags: String,
    pub pending: bool,
    pub recurring: bool,
    pub needs_review: bool,
    pub hide_from_reports: bool,
    /// Tagged in Monarch as an exceptional item (a tax settlement, a share
    /// sale) that should not count toward everyday income and spending
    pub one_time: bool,
}

impl Txn {
    /// Counts toward income/spending totals
    pub fn in_reports(&self) -> bool {
        !self.hide_from_reports && self.group_type != "transfer"
    }

    pub fn is_income(&self) -> bool {
        self.group_type == "income"
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NetWorthPoint {
    pub date: NaiveDate,
    pub net_worth: f64,
    pub assets: f64,
    pub liabilities: f64,
}

/// One security across every account that holds it
#[derive(Debug, Clone)]
pub struct Holding {
    pub name: String,
    pub ticker: String,
    pub asset_class: String,
    pub quantity: f64,
    pub price: Option<f64>,
    pub value: f64,
    pub cost_basis: Option<f64>,
    pub day_change_pct: Option<f64>,
}

/// One security in one account
#[derive(Debug, Clone)]
pub struct Position {
    pub account_id: String,
    pub account_name: String,
    pub name: String,
    pub ticker: String,
    pub kind: String,
    pub quantity: f64,
    pub price: Option<f64>,
    pub value: f64,
    pub cost_basis: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Category {
    pub name: String,
    pub group_id: String,
}

#[derive(Debug, Clone)]
pub struct CategoryGroup {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Portfolio {
    pub total_value: f64,
    pub one_day_change_pct: f64,
}

/// Name of the Monarch tag that marks an exceptional transaction
pub const ONE_TIME_TAG: &str = "One-time";

/// Income and spending for one calendar month
#[derive(Debug, Clone, Copy)]
pub struct MonthFlow {
    /// First day of the month
    pub month: NaiveDate,
    /// Everyday income and spending, leaving out one-time items
    pub income: f64,
    pub expense: f64,
    /// One-time items, kept apart so they can be shown or left out
    pub one_time_income: f64,
    pub one_time_expense: f64,
}

impl MonthFlow {
    /// The same month with one-time items folded into the totals
    pub fn with_one_time(self) -> MonthFlow {
        MonthFlow {
            income: self.income + self.one_time_income,
            expense: self.expense + self.one_time_expense,
            ..self
        }
    }

    pub fn savings(&self) -> f64 {
        self.income - self.expense
    }

    pub fn savings_rate(&self) -> f64 {
        if self.income > 0.0 {
            self.savings() / self.income * 100.0
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Data {
    pub synced_at: Option<DateTime<Utc>>,
    pub accounts: Vec<Account>,
    /// Newest first
    pub transactions: Vec<Txn>,
    /// Oldest first
    pub net_worth: Vec<NetWorthPoint>,
    /// Daily signed balance per account id, oldest first
    pub balances: HashMap<String, Vec<(NaiveDate, f64)>>,
    pub holdings: Vec<Holding>,
    pub positions: Vec<Position>,
    pub portfolio: Portfolio,
    pub categories: Vec<Category>,
    pub groups: Vec<CategoryGroup>,
    pub budget: Option<Value>,
    pub goals: Option<Value>,
    /// (reported date, score), oldest first
    pub credit_scores: Vec<(NaiveDate, i64)>,
    /// Oldest first, covering every month that has transactions
    pub months: Vec<MonthFlow>,
    /// Recurring payments inferred from transaction history
    pub streams: Vec<crate::forecast::Stream>,
    /// Private company holdings captured from Carta
    pub equity: Equity,
    /// Names and amounts have been swapped for stand-ins
    pub anonymized: bool,
    /// A made-up household (`--demo`), not anyone's records
    pub demo: bool,
}

/// Monarch's account type for private company equity
const EQUITY_ACCOUNT_KIND: &str = "equity";

pub fn month_of(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

impl Data {
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn account_name(&self, id: &str) -> &str {
        self.account(id).map(|a| a.name.as_str()).unwrap_or("")
    }

    pub fn today(&self) -> NaiveDate {
        Utc::now().date_naive()
    }

    /// Latest net worth figures Monarch reported
    pub fn latest_net_worth(&self) -> Option<NetWorthPoint> {
        self.net_worth.last().copied()
    }

    /// Sum of daily balances across investment accounts, oldest first
    pub fn investment_history(&self) -> Vec<(NaiveDate, f64)> {
        let mut by_date: std::collections::BTreeMap<NaiveDate, f64> = Default::default();
        for account in self.accounts.iter().filter(|a| a.kind == "brokerage") {
            if let Some(series) = self.balances.get(&account.id) {
                for (date, balance) in series {
                    *by_date.entry(*date).or_default() += balance;
                }
            }
        }
        by_date.into_iter().collect()
    }

    fn compute_months(&mut self) {
        let mut by_month: std::collections::BTreeMap<NaiveDate, [f64; 4]> = Default::default();
        for txn in self.transactions.iter().filter(|t| t.in_reports()) {
            let totals = by_month.entry(month_of(txn.date)).or_default();
            // [income, expense, one-time income, one-time expense]
            let slot = usize::from(!txn.is_income()) + if txn.one_time { 2 } else { 0 };
            totals[slot] += if txn.is_income() { txn.amount } else { -txn.amount };
        }
        self.months = by_month
            .into_iter()
            .map(|(month, [income, expense, one_time_income, one_time_expense])| MonthFlow {
                month,
                income,
                expense,
                one_time_income,
                one_time_expense,
            })
            .collect();
    }

    /// Let the Carta holdings stand in for the equity account Monarch tracks
    /// by hand: its balance becomes what is vested at the latest share price,
    /// and its history is rebuilt from the grants, exercises, sales and price
    /// changes on record.
    fn apply_equity(&mut self) {
        if self.equity.is_empty() {
            return;
        }
        let today = self.today();
        let value = self.equity.summary(1.0).vested_value();
        let history = self.equity.value_history(today);
        // Carta is one portfolio; Monarch's largest equity account is its stand-in
        let Some(account) = self
            .accounts
            .iter_mut()
            .filter(|a| a.kind == EQUITY_ACCOUNT_KIND)
            .max_by(|a, b| a.balance.total_cmp(&b.balance))
        else {
            return;
        };
        account.balance = value;
        account.signed_balance = value;
        account.institution = "Carta".to_string();
        account.needs_attention = false;
        account.last_synced = self.equity.captured_at.or(account.last_synced);
        self.balances.insert(account.id.clone(), history);
    }

    /// Recompute everything derived from the loaded rows
    pub fn derive(&mut self) {
        self.apply_equity();
        self.compute_months();
        self.streams = crate::forecast::detect(&self.transactions, self.today());
    }
}

pub async fn connect(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(database_url)
        .await
        .context("Could not connect to the mirror database")
}

async fn document(pool: &PgPool, kind: &str) -> Result<Option<Value>> {
    let row = sqlx::query("SELECT payload::text AS payload FROM latest_documents WHERE kind = $1")
        .bind(kind)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some(row) => Some(serde_json::from_str(&row.get::<String, _>("payload"))?),
        None => None,
    })
}

pub async fn load(pool: &PgPool) -> Result<Data> {
    let mut data = Data {
        synced_at: sqlx::query("SELECT run_at FROM latest_run")
            .fetch_optional(pool)
            .await?
            .map(|row| row.get("run_at")),
        ..Default::default()
    };

    let rows = sqlx::query(
        r#"
        SELECT account_id,
               COALESCE(display_name, '') AS name,
               mask,
               COALESCE(account_type, 'other') AS kind,
               COALESCE(raw->'type'->>'display', 'Other') AS kind_display,
               COALESCE(raw->'subtype'->>'display', '') AS subtype,
               COALESCE(institution_name, '') AS institution,
               COALESCE(display_balance, 0)::float8 AS balance,
               COALESCE(signed_balance, 0)::float8 AS signed_balance,
               COALESCE(is_asset, true) AS is_asset,
               COALESCE(is_hidden, false) AS is_hidden,
               COALESCE(include_in_net_worth, true) AS include_in_net_worth,
               (COALESCE(credential_update_required, false)
                   OR COALESCE(sync_disabled, false)
                   OR deactivated_at IS NOT NULL) AS needs_attention,
               last_synced_at,
               credit_limit::float8 AS credit_limit,
               (raw->>'interestRate')::float8 AS interest_rate,
               (raw->>'minimumPayment')::float8 AS minimum_payment
        FROM latest_accounts
        ORDER BY account_order NULLS LAST, display_name
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.accounts = rows
        .iter()
        .map(|r| Account {
            id: r.get("account_id"),
            name: r.get("name"),
            mask: r.get("mask"),
            kind: r.get("kind"),
            kind_display: r.get("kind_display"),
            subtype: r.get("subtype"),
            institution: r.get("institution"),
            balance: r.get("balance"),
            signed_balance: r.get("signed_balance"),
            is_asset: r.get("is_asset"),
            is_hidden: r.get("is_hidden"),
            include_in_net_worth: r.get("include_in_net_worth"),
            needs_attention: r.get("needs_attention"),
            last_synced: r.get("last_synced_at"),
            credit_limit: r.get("credit_limit"),
            interest_rate: r.get("interest_rate"),
            minimum_payment: r.get("minimum_payment"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT t.date,
               t.amount::float8 AS amount,
               COALESCE((SELECT sum(o.amount) FROM credit_offsets o
                         WHERE o.purchase_transaction_id = t.transaction_id), 0)::float8 AS credited,
               EXISTS (SELECT 1 FROM credit_offsets o
                       WHERE o.credit_transaction_id = t.transaction_id) AS is_matched_credit,
               trim(COALESCE(t.merchant_name, t.plaid_name, '')) AS merchant,
               COALESCE(t.category_name, 'Uncategorized') AS category,
               COALESCE(t.category_group_type, 'expense') AS group_type,
               t.account_id,
               COALESCE(t.notes, '') AS notes,
               COALESCE(t.plaid_name, '') AS original_name,
               COALESCE((
                   SELECT string_agg(g.name, ', ' ORDER BY g.name)
                   FROM transaction_tags tt JOIN tags g ON g.id = tt.tag_id
                   WHERE tt.transaction_id = t.transaction_id
               ), '') AS tags,
               COALESCE(t.pending, false) AS pending,
               COALESCE(t.is_recurring, false) AS recurring,
               COALESCE(t.needs_review, false) AS needs_review,
               COALESCE(t.hide_from_reports, false) AS hide_from_reports
        FROM latest_transactions t
        ORDER BY t.date DESC, t.monarch_created_at DESC NULLS LAST, t.transaction_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.transactions = rows
        .iter()
        .map(|r| Txn {
            date: r.get("date"),
            amount: r.get("amount"),
            credited: r.get("credited"),
            is_matched_credit: r.get("is_matched_credit"),
            merchant: r.get("merchant"),
            category: r.get("category"),
            group_type: r.get("group_type"),
            account_id: r.get("account_id"),
            notes: r.get("notes"),
            original_name: r.get("original_name"),
            tags: r.get("tags"),
            pending: r.get("pending"),
            recurring: r.get("recurring"),
            needs_review: r.get("needs_review"),
            hide_from_reports: r.get("hide_from_reports"),
            one_time: r.get::<String, _>("tags").split(", ").any(|tag| tag.eq_ignore_ascii_case(ONE_TIME_TAG)),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT snapshot_date,
               COALESCE(net_worth, 0)::float8 AS net_worth,
               COALESCE(assets_total, 0)::float8 AS assets,
               COALESCE(liabilities_total, 0)::float8 AS liabilities
        FROM aggregate_snapshots
        ORDER BY snapshot_date
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.net_worth = rows
        .iter()
        .map(|r| NetWorthPoint {
            date: r.get("snapshot_date"),
            net_worth: r.get("net_worth"),
            assets: r.get("assets"),
            liabilities: r.get("liabilities"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT account_id, balance_date, COALESCE(balance, 0)::float8 AS balance
        FROM account_daily_balances
        ORDER BY account_id, balance_date
        "#,
    )
    .fetch_all(pool)
    .await?;
    for r in &rows {
        data.balances
            .entry(r.get("account_id"))
            .or_default()
            .push((r.get("balance_date"), r.get("balance")));
    }

    let rows = sqlx::query(
        r#"
        SELECT COALESCE(h.security_name, (
                   SELECT min(p.name) FROM latest_positions p
                   WHERE p.aggregate_holding_id = h.holding_id
               ), 'Unknown') AS name,
               COALESCE(h.ticker, '') AS ticker,
               COALESCE(s.asset_class, 'unclassified') AS asset_class,
               COALESCE(h.quantity, 0)::float8 AS quantity,
               h.price::float8 AS price,
               COALESCE(h.value, 0)::float8 AS value,
               h.cost_basis::float8 AS cost_basis,
               h.price_change_percent::float8 AS day_change_pct
        FROM latest_holdings h
        LEFT JOIN securities s ON s.id = h.security_id
        ORDER BY h.value DESC NULLS LAST
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.holdings = rows
        .iter()
        .map(|r| Holding {
            name: r.get("name"),
            ticker: r.get("ticker"),
            asset_class: r.get("asset_class"),
            quantity: r.get("quantity"),
            price: r.get("price"),
            value: r.get("value"),
            cost_basis: r.get("cost_basis"),
            day_change_pct: r.get("day_change_pct"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT COALESCE(account_id, '') AS account_id,
               COALESCE(account_name, 'Unknown account') AS account_name,
               COALESCE(name, 'Unknown') AS name,
               COALESCE(ticker, '') AS ticker,
               COALESCE(security_type_display, 'Other') AS kind,
               COALESCE(quantity, 0)::float8 AS quantity,
               closing_price::float8 AS price,
               COALESCE(value, 0)::float8 AS value,
               cost_basis::float8 AS cost_basis
        FROM latest_positions
        ORDER BY account_name, value DESC NULLS LAST
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.positions = rows
        .iter()
        .map(|r| Position {
            account_id: r.get("account_id"),
            account_name: r.get("account_name"),
            name: r.get("name"),
            ticker: r.get("ticker"),
            kind: r.get("kind"),
            quantity: r.get("quantity"),
            price: r.get("price"),
            value: r.get("value"),
            cost_basis: r.get("cost_basis"),
        })
        .collect();

    if let Some(row) = sqlx::query(
        r#"
        SELECT COALESCE(total_value, 0)::float8 AS total_value,
               COALESCE(one_day_change_percent, 0)::float8 AS one_day_change_pct
        FROM portfolio_snapshots
        WHERE run_id = (SELECT id FROM latest_run)
        "#,
    )
    .fetch_optional(pool)
    .await?
    {
        data.portfolio = Portfolio {
            total_value: row.get("total_value"),
            one_day_change_pct: row.get("one_day_change_pct"),
        };
    }

    let rows = sqlx::query(
        r#"
        SELECT name, COALESCE(group_id, '') AS group_id
        FROM categories
        WHERE NOT is_disabled
        ORDER BY category_order
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.categories = rows
        .iter()
        .map(|r| Category {
            name: r.get("name"),
            group_id: r.get("group_id"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT id, COALESCE(name, '') AS name
        FROM category_groups
        ORDER BY group_order
        "#,
    )
    .fetch_all(pool)
    .await?;
    data.groups = rows
        .iter()
        .map(|r| CategoryGroup {
            id: r.get("id"),
            name: r.get("name"),
        })
        .collect();

    data.budget = document(pool, "budget").await?;
    data.goals = document(pool, "goals").await?;
    if let Some(credit) = document(pool, "credit_score").await? {
        let mut scores: Vec<(NaiveDate, i64)> = credit["creditScoreSnapshots"]
            .as_array()
            .map(|snapshots| {
                snapshots
                    .iter()
                    .filter_map(|s| {
                        let date = NaiveDate::parse_from_str(s["reportedDate"].as_str()?, "%Y-%m-%d").ok()?;
                        Some((date, s["score"].as_i64()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        scores.sort();
        data.credit_scores = scores;
    }

    // A mirror that has never had a Carta capture loaded simply has no equity
    data.equity = load_equity(pool).await.unwrap_or_default();

    data.derive();
    Ok(data)
}

async fn load_equity(pool: &PgPool) -> Result<Equity> {
    let mut equity = Equity::default();

    let rows = sqlx::query("SELECT issuer_id, name, fmv::float8 AS fmv, captured_at FROM equity_issuers")
        .fetch_all(pool)
        .await?;
    equity.captured_at = rows.iter().map(|r| r.get::<DateTime<Utc>, _>("captured_at")).max();
    equity.issuers = rows
        .iter()
        .map(|r| Issuer {
            id: r.get("issuer_id"),
            name: r.get("name"),
            fmv: r.get("fmv"),
            as_of: Some(r.get::<DateTime<Utc>, _>("captured_at").date_naive()),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT grant_id, issuer_id, label, issue_date, expiration_date,
               COALESCE(granted_as, option_type, '') AS option_type,
               COALESCE(stock_type, '') AS stock_type, COALESCE(status, '') AS status,
               quantity::float8 AS quantity, vested::float8 AS vested, exercised::float8 AS exercised,
               exercisable::float8 AS exercisable, exercise_price::float8 AS exercise_price,
               is_canceled, vesting_start, vesting_end, COALESCE(vesting_schedule, '') AS vesting_schedule,
               COALESCE(cliff, '') AS cliff, COALESCE(vesting_terms, '') AS vesting_terms,
               COALESCE(acceleration, '') AS acceleration, early_exercise,
               COALESCE(iso_disqualified, FALSE) AS iso_disqualified,
               COALESCE(form_83b_status, '') AS form_83b_status
        FROM equity_grants
        ORDER BY issue_date, label
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.grants = rows
        .iter()
        .map(|r| Grant {
            id: r.get("grant_id"),
            issuer_id: r.get("issuer_id"),
            label: r.get("label"),
            option_type: r.get("option_type"),
            stock_type: r.get("stock_type"),
            status: r.get("status"),
            expires: r.get("expiration_date"),
            issued: r.get("issue_date"),
            quantity: r.get("quantity"),
            vested: r.get("vested"),
            exercised: r.get("exercised"),
            exercisable: r.get("exercisable"),
            exercise_price: r.get("exercise_price"),
            canceled: r.get("is_canceled"),
            vesting_start: r.get("vesting_start"),
            vesting_end: r.get("vesting_end"),
            schedule: r.get("vesting_schedule"),
            cliff: r.get("cliff"),
            terms: r.get("vesting_terms"),
            acceleration: r.get("acceleration"),
            early_exercise: r.get("early_exercise"),
            iso_disqualified: r.get("iso_disqualified"),
            form_83b: r.get("form_83b_status"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT grant_id, vest_date, quantity::float8 AS quantity,
               iso_quantity::float8 AS iso, nso_quantity::float8 AS nso
        FROM equity_vesting_events
        ORDER BY vest_date, grant_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.vesting = rows
        .iter()
        .map(|r| VestEvent {
            grant_id: r.get("grant_id"),
            date: r.get("vest_date"),
            quantity: r.get("quantity"),
            iso: r.get("iso"),
            nso: r.get("nso"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT issuer_id, label, COALESCE(share_class, '') AS share_class, is_certificated,
               issue_date, acquired_date, fmv_at_acquisition::float8 AS fmv_at_issue,
               quantity::float8 AS quantity, cost::float8 AS cost,
               COALESCE(status, '') AS status, is_canceled,
               COALESCE(exercise_type, '') AS exercise_type, COALESCE(exercised_from, '') AS exercised_from
        FROM equity_certificates
        ORDER BY acquired_date, certificate_id
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.certificates = rows
        .iter()
        .map(|r| Certificate {
            issuer_id: r.get("issuer_id"),
            label: r.get("label"),
            share_class: r.get("share_class"),
            certificated: r.get("is_certificated"),
            issued: r.get("issue_date"),
            acquired: r.get("acquired_date"),
            fmv_at_issue: r.get("fmv_at_issue"),
            quantity: r.get("quantity"),
            cost: r.get("cost"),
            status: r.get("status"),
            canceled: r.get("is_canceled"),
            exercise_type: r.get("exercise_type"),
            exercised_from: r.get("exercised_from"),
        })
        .collect();

    // An exercise Carta marks canceled still happened when its shares exist:
    // the certificate was later sold or split. Ones with no certificate on
    // record (a company since restructured) are left out. The certificate
    // also knows whether ISOs or NSOs were exercised (the exercise record
    // only repeats the grant's type). An afternoon exercise is timestamped
    // the next day in UTC; the certificate's date is then the day it was made.
    let rows = sqlx::query(
        r#"
        SELECT x.issuer_id, COALESCE(x.grant_label, '') AS grant_label,
               COALESCE(x.certificate_label, '') AS certificate_label,
               COALESCE(c.exercise_type, x.option_type, '') AS option_type,
               CASE WHEN c.acquired_date = (x.exercised_at AT TIME ZONE 'UTC')::date - 1
                    THEN c.acquired_date
                    ELSE (x.exercised_at AT TIME ZONE 'UTC')::date
               END AS exercised_on,
               x.quantity::float8 AS quantity, COALESCE(x.cost, 0)::float8 AS cost, x.fmv::float8 AS fmv,
               COALESCE(x.taxes, 0)::float8 AS taxes, COALESCE(x.payment_method, '') AS payment
        FROM equity_exercises x
        LEFT JOIN equity_certificates c USING (certificate_id)
        WHERE x.status <> 'CANCELED' OR c.certificate_id IS NOT NULL
        ORDER BY x.exercised_at
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.exercises = rows
        .iter()
        .map(|r| Exercise {
            issuer_id: r.get("issuer_id"),
            grant_label: r.get("grant_label"),
            certificate_label: r.get("certificate_label"),
            option_type: r.get("option_type"),
            date: r.get("exercised_on"),
            quantity: r.get("quantity"),
            cost: r.get("cost"),
            taxes: r.get("taxes"),
            payment: r.get("payment"),
            fmv: r.get("fmv"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT COALESCE(name, 'Tender offer') AS name, COALESCE(status, '') AS status,
               COALESCE(kind, '') AS kind, start_date, end_date, settlement_date,
               COALESCE(shares_sold, 0)::float8 AS shares, COALESCE(proceeds, 0)::float8 AS proceeds
        FROM equity_tenders
        ORDER BY settlement_date
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.tenders = rows
        .iter()
        .map(|r| Tender {
            name: r.get("name"),
            status: r.get("status"),
            kind: r.get("kind"),
            opened: r.get("start_date"),
            closed: r.get("end_date"),
            settled: r.get("settlement_date"),
            shares: r.get("shares"),
            proceeds: r.get("proceeds"),
        })
        .collect();

    let rows = sqlx::query(
        r#"
        SELECT issuer_id, snapshot_date, fmv::float8 AS fmv
        FROM equity_snapshots
        ORDER BY snapshot_date
        "#,
    )
    .fetch_all(pool)
    .await?;
    equity.snapshots = rows
        .iter()
        .map(|r| Snapshot { issuer_id: r.get("issuer_id"), date: r.get("snapshot_date"), fmv: r.get("fmv") })
        .collect();

    Ok(equity)
}
