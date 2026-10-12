//! Duplicate-transaction detection for accounts whose history gets re-imported.
//!
//! Some connections (Apple Card is one) re-sync the account's entire history
//! whenever they are set up again, and Monarch stores the second copy
//! alongside the first. This finds those second copies so they can be removed.
//!
//! A transaction is a re-imported duplicate when an identical one (same date,
//! amount and statement text) was created in Monarch on an earlier day, and it
//! was itself created long after its own date, which is what a bulk import of
//! old history looks like. Two genuine same-day purchases of the same amount
//! are both created within days of happening, so they are never flagged.

use chrono::NaiveDate;
use serde_json::Value;
use std::collections::BTreeMap;

/// A copy created more than this many days after the transaction's own date
/// came from a re-import of history rather than from normal syncing
const REIMPORT_LAG_DAYS: i64 = 14;

/// A transaction to delete because an earlier copy of it exists
#[derive(Debug, Clone, PartialEq)]
pub struct Duplicate {
    pub transaction_id: String,
    pub date: String,
    pub amount: f64,
    pub statement: String,
    /// Day Monarch created this copy
    pub created: NaiveDate,
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

/// Day Monarch created a transaction, from its `createdAt` timestamp
fn created_day(transaction: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text(transaction, "/createdAt").get(..10)?, "%Y-%m-%d").ok()
}

/// Re-imported copies in the named account. The earliest-created copies of
/// each transaction are kept; `transactions` are raw API objects and nothing
/// is changed here.
pub fn plan(transactions: &[Value], account_name: &str) -> Vec<Duplicate> {
    // Identical transactions, split by the day Monarch created each copy
    let mut groups: BTreeMap<(String, i64, String), BTreeMap<NaiveDate, Vec<&Value>>> = BTreeMap::new();
    for transaction in transactions {
        if !text(transaction, "/account/displayName").eq_ignore_ascii_case(account_name) {
            continue;
        }
        let (Some(created), Some(amount)) = (created_day(transaction), transaction["amount"].as_f64()) else {
            continue;
        };
        let key = (
            text(transaction, "/date").to_string(),
            (amount * 100.0).round() as i64,
            text(transaction, "/plaidName").to_string(),
        );
        groups.entry(key).or_default().entry(created).or_default().push(transaction);
    }

    let mut duplicates = Vec::new();
    for ((date, _, statement), batches) in &groups {
        let mut batches = batches.iter();
        let Some((_, originals)) = batches.next() else {
            continue;
        };
        let Ok(transaction_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
            continue;
        };
        for (created, copies) in batches {
            if (*created - transaction_date).num_days() <= REIMPORT_LAG_DAYS {
                continue;
            }
            // A re-import brings back each original once; anything beyond
            // that count is a different transaction that merely looks alike
            for copy in copies.iter().take(originals.len()) {
                duplicates.push(Duplicate {
                    transaction_id: text(copy, "/id").to_string(),
                    date: date.clone(),
                    amount: copy["amount"].as_f64().unwrap_or_default(),
                    statement: statement.clone(),
                    created: *created,
                });
            }
        }
    }
    duplicates
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn txn(id: &str, date: &str, amount: f64, created: &str, account: &str) -> Value {
        json!({
            "id": id, "date": date, "amount": amount, "plaidName": "COFFEE SHOP",
            "createdAt": format!("{created}T12:00:00.000000+00:00"),
            "account": { "id": "a", "displayName": account },
        })
    }

    #[test]
    fn second_import_of_old_history_is_flagged_and_the_first_kept() {
        let transactions = vec![
            txn("first", "2022-06-04", -1117.0, "2025-04-07", "Apple Card"),
            txn("second", "2022-06-04", -1117.0, "2025-11-14", "Apple Card"),
        ];
        let duplicates = plan(&transactions, "Apple Card");
        assert_eq!(duplicates.len(), 1);
        assert_eq!(duplicates[0].transaction_id, "second");
    }

    #[test]
    fn four_identical_purchases_imported_twice_lose_exactly_four() {
        let mut transactions = Vec::new();
        for i in 0..4 {
            transactions.push(txn(&format!("a{i}"), "2022-06-04", -1117.0, "2025-04-07", "Apple Card"));
            transactions.push(txn(&format!("b{i}"), "2022-06-04", -1117.0, "2025-11-14", "Apple Card"));
        }
        let duplicates = plan(&transactions, "Apple Card");
        assert_eq!(duplicates.len(), 4);
        assert!(duplicates.iter().all(|d| d.transaction_id.starts_with('b')));
    }

    #[test]
    fn two_real_purchases_on_one_day_are_both_kept() {
        // Created a day apart, both right after they happened: normal syncing
        let transactions = vec![
            txn("morning", "2026-03-10", -5.0, "2026-03-10", "Apple Card"),
            txn("evening", "2026-03-10", -5.0, "2026-03-11", "Apple Card"),
        ];
        assert!(plan(&transactions, "Apple Card").is_empty());
    }

    #[test]
    fn a_new_lookalike_in_the_reimport_is_not_deleted() {
        // One original, but the re-import has two: only one of them is its copy
        let transactions = vec![
            txn("first", "2024-01-05", -9.99, "2025-04-07", "Apple Card"),
            txn("copy", "2024-01-05", -9.99, "2025-11-14", "Apple Card"),
            txn("extra", "2024-01-05", -9.99, "2025-11-14", "Apple Card"),
        ];
        assert_eq!(plan(&transactions, "Apple Card").len(), 1);
    }

    #[test]
    fn other_accounts_and_differing_amounts_are_untouched() {
        let transactions = vec![
            txn("x1", "2022-06-04", -20.0, "2025-04-07", "Other Card"),
            txn("x2", "2022-06-04", -20.0, "2025-11-14", "Other Card"),
            txn("y1", "2022-06-04", -20.0, "2025-04-07", "Apple Card"),
            txn("y2", "2022-06-04", -21.0, "2025-11-14", "Apple Card"),
        ];
        assert!(plan(&transactions, "Apple Card").is_empty());
    }
}
