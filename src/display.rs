//! Terminal display formatting for Monarch Money data

use crate::models::*;
use colored::Colorize;
use rust_decimal::Decimal;
use tabled::{Table, Tabled};

#[derive(Tabled)]
pub struct AccountRow {
    #[tabled(rename = "Account")]
    pub name: String,
    #[tabled(rename = "Type")]
    pub account_type: String,
    #[tabled(rename = "Institution")]
    pub institution: String,
    #[tabled(rename = "Balance")]
    pub balance: String,
}

impl From<&Account> for AccountRow {
    fn from(account: &Account) -> Self {
        let name = account
            .display_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let account_type = account
            .account_type
            .as_ref()
            .and_then(|t| t.display.clone().or(t.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());

        let institution = account
            .credential
            .as_ref()
            .and_then(|c| c.institution.as_ref())
            .and_then(|i| i.name.clone())
            .or_else(|| account.institution.as_ref().and_then(|i| i.name.clone()))
            .unwrap_or_else(|| "Manual".to_string());

        let balance = account
            .display_balance
            .or(account.current_balance)
            .map(|b| format_currency(b))
            .unwrap_or_else(|| "N/A".to_string());

        Self {
            name,
            account_type,
            institution,
            balance,
        }
    }
}

#[derive(Tabled)]
pub struct TransactionRow {
    #[tabled(rename = "Date")]
    pub date: String,
    #[tabled(rename = "Merchant")]
    pub merchant: String,
    #[tabled(rename = "Category")]
    pub category: String,
    #[tabled(rename = "Account")]
    pub account: String,
    #[tabled(rename = "Amount")]
    pub amount: String,
}

impl From<&Transaction> for TransactionRow {
    fn from(txn: &Transaction) -> Self {
        let merchant = txn
            .merchant
            .as_ref()
            .and_then(|m| m.name.clone())
            .or_else(|| txn.plaid_name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let category = txn
            .category
            .as_ref()
            .and_then(|c| c.name.clone())
            .unwrap_or_else(|| "Uncategorized".to_string());

        let account = txn
            .account
            .display_name
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let amount = format_currency_colored(txn.amount);

        Self {
            date: txn.date.clone(),
            merchant,
            category,
            account,
            amount,
        }
    }
}

#[derive(Tabled)]
pub struct HoldingRow {
    #[tabled(rename = "Security")]
    pub security: String,
    #[tabled(rename = "Ticker")]
    pub ticker: String,
    #[tabled(rename = "Account")]
    pub account: String,
    #[tabled(rename = "Quantity")]
    pub quantity: String,
    #[tabled(rename = "Value")]
    pub value: String,
    #[tabled(rename = "Gain/Loss")]
    pub gain_loss: String,
}

impl From<&Holding> for HoldingRow {
    fn from(holding: &Holding) -> Self {
        let security = holding
            .security
            .as_ref()
            .and_then(|s| s.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let ticker = holding
            .security
            .as_ref()
            .and_then(|s| s.ticker.clone())
            .unwrap_or_else(|| "-".to_string());

        // Aggregate holdings don't have per-account breakdown
        let account = "-".to_string();

        let quantity = holding
            .quantity
            .map(|q| format!("{:.4}", q))
            .unwrap_or_else(|| "-".to_string());

        let value = holding
            .total_value
            .map(|v| format_currency(v))
            .unwrap_or_else(|| "N/A".to_string());

        let gain_loss = holding
            .security_price_change_dollars
            .map(|gl| format_currency_colored(gl))
            .unwrap_or_else(|| "-".to_string());

        Self {
            security,
            ticker,
            account,
            quantity,
            value,
            gain_loss,
        }
    }
}

#[derive(Tabled)]
pub struct RecurringRow {
    #[tabled(rename = "Name")]
    pub name: String,
    #[tabled(rename = "Type")]
    pub stream_type: String,
    #[tabled(rename = "Frequency")]
    pub frequency: String,
    #[tabled(rename = "Amount")]
    pub amount: String,
    #[tabled(rename = "Next Date")]
    pub next_date: String,
}

impl From<&RecurringStream> for RecurringRow {
    fn from(stream: &RecurringStream) -> Self {
        let name = stream
            .display_name
            .clone()
            .or_else(|| stream.merchant.as_ref().and_then(|m| m.name.clone()))
            .unwrap_or_else(|| "Unknown".to_string());

        let stream_type = stream
            .stream_type
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let frequency = stream
            .frequency
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());

        let amount = stream
            .amount
            .map(|a| format_currency_colored(a))
            .unwrap_or_else(|| "N/A".to_string());

        let next_date = stream
            .next_forecasted_date
            .clone()
            .unwrap_or_else(|| "-".to_string());

        Self {
            name,
            stream_type,
            frequency,
            amount,
            next_date,
        }
    }
}

fn format_currency(amount: Decimal) -> String {
    if amount.is_sign_negative() {
        format!("-${:.2}", amount.abs())
    } else {
        format!("${:.2}", amount)
    }
}

fn format_currency_colored(amount: Decimal) -> String {
    let formatted = format_currency(amount);
    if amount.is_sign_negative() {
        formatted.red().to_string()
    } else if amount.is_sign_positive() && amount != Decimal::ZERO {
        formatted.green().to_string()
    } else {
        formatted
    }
}

pub fn print_accounts_table(accounts: &[Account]) {
    let rows: Vec<AccountRow> = accounts.iter().map(AccountRow::from).collect();
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_transactions_table(transactions: &[Transaction]) {
    let rows: Vec<TransactionRow> = transactions.iter().map(TransactionRow::from).collect();
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_holdings_table(holdings: &[Holding]) {
    let rows: Vec<HoldingRow> = holdings.iter().map(HoldingRow::from).collect();
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_recurring_table(streams: &[RecurringStream]) {
    let rows: Vec<RecurringRow> = streams.iter().map(RecurringRow::from).collect();
    let table = Table::new(rows);
    println!("{}", table);
}

pub fn print_summary(
    accounts: &[Account],
    transactions_count: usize,
    holdings_count: usize,
    net_worth: Option<Decimal>,
) {
    println!();
    println!("{}", "=== Summary ===".bold());
    println!("Accounts: {}", accounts.len());
    println!("Transactions: {}", transactions_count);
    println!("Holdings: {}", holdings_count);

    if let Some(nw) = net_worth {
        println!("Net Worth: {}", format_currency_colored(nw));
    }

    // Calculate totals by account type
    let mut assets_total = Decimal::ZERO;
    let mut liabilities_total = Decimal::ZERO;

    for account in accounts {
        if account.include_in_net_worth.unwrap_or(true) && !account.is_hidden.unwrap_or(false) {
            let balance = account.display_balance.or(account.current_balance).unwrap_or(Decimal::ZERO);
            if account.is_asset.unwrap_or(true) {
                assets_total += balance;
            } else {
                liabilities_total += balance.abs();
            }
        }
    }

    println!();
    println!("Assets: {}", format_currency(assets_total).green());
    println!("Liabilities: {}", format_currency(liabilities_total).red());
    println!(
        "Calculated Net Worth: {}",
        format_currency_colored(assets_total - liabilities_total)
    );
}
