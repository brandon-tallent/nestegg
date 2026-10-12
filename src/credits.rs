//! Statement-credit reconciliation.
//!
//! Card issuers refund some purchases with a statement credit (a membership
//! fee credited back, a monthly dining or streaming allowance). Monarch files
//! those credits under whatever merchant and category it guesses, often the
//! card issuer and "Credit Card Payment", so the purchase counts as spending
//! while its refund does not.
//!
//! This pairs each such credit with the purchase it refunds and plans the
//! changes that make them net out: the credit takes the purchase's category,
//! and either the purchase's merchant or a consistent name for that kind of
//! credit.

use chrono::NaiveDate;
use serde_json::Value;

/// How long before a credit its purchase may have posted, and how long after
/// (a credit occasionally lands a day ahead of the charge it refunds)
const PURCHASE_DAYS_BEFORE: i64 = 21;
const PURCHASE_DAYS_AFTER: i64 = 2;

/// Slack when comparing a credit against a purchase amount, in dollars
const AMOUNT_EPSILON: f64 = 0.005;

/// One kind of statement credit and how to recognise the purchase it refunds
pub struct CreditKind {
    /// Lowercase texts, any of which identifies the credit in its statement
    /// description (issuers have worded the same credit differently over time)
    pub statements: &'static [&'static str],
    /// Lowercase fragments of merchant names the refunded purchase may have;
    /// empty means any merchant
    pub merchants: &'static [&'static str],
    /// Lowercase fragments of category names the refunded purchase may have;
    /// empty means any category
    pub categories: &'static [&'static str],
    /// Merchant the credit is filed under. `None` files it under the
    /// purchase's own merchant, which suits a credit tied to one merchant.
    pub merchant_name: Option<&'static str>,
}

pub const CREDIT_KINDS: &[CreditKind] = &[
    CreditKind { statements: &["clear plus credit", "clear new york"], merchants: &["clear"], categories: &[], merchant_name: None },
    CreditKind { statements: &["walmart+ credit"], merchants: &["walmart"], categories: &[], merchant_name: None },
    CreditKind { statements: &["uber one credit"], merchants: &["uber"], categories: &[], merchant_name: None },
    CreditKind { statements: &["lululemon credit"], merchants: &["lululemon"], categories: &[], merchant_name: None },
    CreditKind {
        statements: &["digital entertainment credit"],
        merchants: &["new york times", "disney", "hulu", "peacock", "espn", "wall street journal", "youtube", "paramount"],
        categories: &[],
        merchant_name: Some("Amex Digital Entertainment Credit"),
    },
    CreditKind {
        statements: &["airline fee reimbursement"],
        merchants: &["united", "southwest", "delta", "jetblue", "hawaiian", "alaska", "american airlines"],
        categories: &[],
        merchant_name: Some("Amex Airline Fee Reimbursement"),
    },
    CreditKind {
        statements: &["resy credit"],
        merchants: &[],
        categories: &["restaurant", "coffee"],
        merchant_name: Some("Amex Resy Credit"),
    },
    CreditKind {
        statements: &["hotel credit"],
        merchants: &[],
        categories: &["travel", "hotel"],
        merchant_name: Some("Amex Hotel Credit"),
    },
];

/// A change to make to one credit so it nets out against its purchase
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
    pub transaction_id: String,
    pub date: String,
    pub amount: f64,
    pub statement: String,
    pub from_merchant: String,
    pub from_category: String,
    /// New merchant name, when it should change
    pub merchant: Option<String>,
    /// New category as (id, name), when it should change
    pub category: Option<(String, String)>,
    /// The purchase the credit was paired with, as "date merchant amount"
    pub paired_with: Option<String>,
}

fn text<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).and_then(Value::as_str).unwrap_or("")
}

fn date_of(transaction: &Value) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text(transaction, "/date"), "%Y-%m-%d").ok()
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.is_empty() || needles.iter().any(|needle| haystack.contains(needle))
}

/// The purchase a credit refunds: same account, within the window, at least
/// the credit's size. An exact amount wins, then the nearest date.
fn find_purchase<'a>(kind: &CreditKind, credit: &Value, transactions: &'a [Value]) -> Option<&'a Value> {
    let credit_date = date_of(credit)?;
    let credit_amount = credit["amount"].as_f64()?;
    let account = text(credit, "/account/id");

    transactions
        .iter()
        .filter(|t| text(t, "/account/id") == account)
        .filter_map(|t| {
            let amount = t["amount"].as_f64()?;
            let days = (credit_date - date_of(t)?).num_days();
            let eligible = amount < 0.0
                && amount.abs() + AMOUNT_EPSILON >= credit_amount
                && (-PURCHASE_DAYS_AFTER..=PURCHASE_DAYS_BEFORE).contains(&days)
                && contains_any(&text(t, "/merchant/name").to_lowercase(), kind.merchants)
                && contains_any(&text(t, "/category/name").to_lowercase(), kind.categories);
            let exact = (amount.abs() - credit_amount).abs() <= AMOUNT_EPSILON;
            eligible.then_some((!exact, days.abs(), t))
        })
        .min_by_key(|(inexact, days, _)| (*inexact, *days))
        .map(|(_, _, purchase)| purchase)
}

/// A statement credit matched to the purchase it refunds
#[derive(Debug, Clone, PartialEq)]
pub struct Offset {
    pub credit_transaction_id: String,
    pub purchase_transaction_id: String,
    pub amount: f64,
}

/// Every statement credit that could be matched to a purchase, whether or not
/// it needed refiling. Lets anything downstream show a purchase net of its
/// credit instead of as a charge and a separate refund.
pub fn offsets(transactions: &[Value]) -> Vec<Offset> {
    transactions
        .iter()
        .filter_map(|credit| {
            let amount = credit["amount"].as_f64().filter(|amount| *amount > 0.0)?;
            let statement = text(credit, "/plaidName").to_lowercase();
            let kind = CREDIT_KINDS.iter().find(|kind| contains_any(&statement, kind.statements))?;
            let purchase = find_purchase(kind, credit, transactions)?;
            Some(Offset {
                credit_transaction_id: text(credit, "/id").to_string(),
                purchase_transaction_id: text(purchase, "/id").to_string(),
                amount,
            })
        })
        .collect()
}

/// Every credit that is not already filed where it nets out, with its fix.
/// `transactions` are raw API objects; nothing is changed here.
pub fn plan(transactions: &[Value]) -> Vec<Fix> {
    let mut fixes = Vec::new();
    for credit in transactions {
        if credit["amount"].as_f64().is_none_or(|amount| amount <= 0.0) {
            continue;
        }
        let statement = text(credit, "/plaidName").to_lowercase();
        let Some(kind) = CREDIT_KINDS.iter().find(|kind| contains_any(&statement, kind.statements)) else {
            continue;
        };
        let purchase = find_purchase(kind, credit, transactions);

        let current_merchant = text(credit, "/merchant/name").trim();
        let wanted_merchant = match (kind.merchant_name, purchase) {
            (Some(name), _) => Some(name.to_string()),
            (None, Some(purchase)) => Some(text(purchase, "/merchant/name").trim().to_string()),
            (None, None) => None,
        };
        let merchant = wanted_merchant.filter(|name| !name.is_empty() && name != current_merchant);

        // Without a purchase to copy from there is no right category to pick
        let category = purchase
            .map(|p| (text(p, "/category/id").to_string(), text(p, "/category/name").to_string()))
            .filter(|(id, _)| !id.is_empty() && id != text(credit, "/category/id"));

        if merchant.is_none() && category.is_none() {
            continue;
        }
        fixes.push(Fix {
            transaction_id: text(credit, "/id").to_string(),
            date: text(credit, "/date").to_string(),
            amount: credit["amount"].as_f64().unwrap_or_default(),
            statement: text(credit, "/plaidName").to_string(),
            from_merchant: current_merchant.to_string(),
            from_category: text(credit, "/category/name").to_string(),
            merchant,
            category,
            paired_with: purchase.map(|p| {
                format!(
                    "{} {} {:.2}",
                    text(p, "/date"),
                    text(p, "/merchant/name").trim(),
                    p["amount"].as_f64().unwrap_or_default()
                )
            }),
        });
    }
    fixes.sort_by(|a, b| a.date.cmp(&b.date).then(a.transaction_id.cmp(&b.transaction_id)));
    fixes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn txn(id: &str, date: &str, amount: f64, merchant: &str, category: (&str, &str), statement: &str) -> Value {
        json!({
            "id": id, "date": date, "amount": amount, "plaidName": statement,
            "account": { "id": "card" },
            "merchant": { "id": format!("m-{merchant}"), "name": merchant },
            "category": { "id": category.0, "name": category.1 },
        })
    }

    #[test]
    fn offsets_link_every_matched_credit_even_when_nothing_needs_refiling() {
        let transactions = vec![
            txn("charge", "2026-04-22", -9.99, "Uber", ("c-subs", "Subscriptions"), "UBER ONE"),
            txn("credit", "2026-04-22", 9.99, "Uber", ("c-subs", "Subscriptions"), "Platinum Uber One Credit"),
            txn("lonely", "2026-06-03", 300.0, "Amex Hotel Credit", ("c-travel", "Travel & Vacation"), "Platinum Hotel Credit"),
        ];
        assert!(plan(&transactions).is_empty());
        assert_eq!(
            offsets(&transactions),
            vec![Offset {
                credit_transaction_id: "credit".to_string(),
                purchase_transaction_id: "charge".to_string(),
                amount: 9.99,
            }]
        );
    }

    #[test]
    fn membership_credit_takes_its_purchases_merchant_and_category() {
        let transactions = vec![
            txn("charge", "2026-05-03", -209.0, "Clear", ("c-travel", "Travel & Vacation"), "CLEAR *CLEARME.COM"),
            txn("credit", "2026-05-03", 209.0, "American Express", ("c-pay", "Credit Card Payment"), "AMEX CLEAR PLUS CREDIT"),
        ];
        let fixes = plan(&transactions);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].transaction_id, "credit");
        assert_eq!(fixes[0].merchant.as_deref(), Some("Clear"));
        assert_eq!(fixes[0].category, Some(("c-travel".to_string(), "Travel & Vacation".to_string())));
    }

    #[test]
    fn older_wording_of_the_same_credit_is_recognised() {
        let transactions = vec![
            txn("charge", "2023-05-04", -149.0, "Clear", ("c-legal", "Financial & Legal Services"), "CLEARME.COM *CLEAR"),
            txn("credit", "2023-05-05", 149.0, "Clear", ("c-subs", "Subscriptions"), "CLEAR NEW YORK"),
        ];
        let fixes = plan(&transactions);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].merchant, None);
        assert_eq!(fixes[0].category.as_ref().map(|c| c.1.as_str()), Some("Financial & Legal Services"));
    }

    #[test]
    fn allowance_credit_gets_one_name_and_the_category_of_what_it_paid_for() {
        let transactions = vec![
            txn("nyt", "2026-05-02", -21.0, "New York Times", ("c-subs", "Subscriptions"), "NEW YORK TIMES"),
            txn("other", "2026-05-02", -21.0, "Hardware Store", ("c-home", "Home Improvement"), "HARDWARE"),
            txn("credit", "2026-05-05", 21.0, "American Express", ("c-fun", "Entertainment & Recreation"), "Platinum Digital Entertainment Credit"),
        ];
        let fixes = plan(&transactions);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].merchant.as_deref(), Some("Amex Digital Entertainment Credit"));
        assert_eq!(fixes[0].category.as_ref().map(|c| c.1.as_str()), Some("Subscriptions"));
    }

    #[test]
    fn credit_already_netting_out_is_left_alone() {
        let transactions = vec![
            txn("charge", "2026-04-22", -9.99, "Uber", ("c-subs", "Subscriptions"), "UBER ONE"),
            txn("credit", "2026-04-22", 9.99, "Uber", ("c-subs", "Subscriptions"), "Platinum Uber One Credit"),
        ];
        assert!(plan(&transactions).is_empty());
    }

    #[test]
    fn exact_amount_beats_a_nearer_larger_purchase() {
        let transactions = vec![
            txn("small", "2026-05-01", -13.95, "Walmart+", ("c-subs", "Subscriptions"), "WALMART.COM W+"),
            txn("big", "2026-05-06", -88.10, "Walmart", ("c-shop", "Shopping"), "WM SUPERCENTER"),
            txn("credit", "2026-05-07", 13.95, "Walmart", ("c-shop", "Shopping"), "Platinum Walmart+ Credit"),
        ];
        let fixes = plan(&transactions);
        assert_eq!(fixes[0].merchant.as_deref(), Some("Walmart+"));
        assert_eq!(fixes[0].category.as_ref().map(|c| c.0.as_str()), Some("c-subs"));
    }

    #[test]
    fn credit_with_no_purchase_in_range_keeps_its_category() {
        let transactions = vec![
            txn("credit", "2026-06-03", 300.0, "American Express", ("c-pay", "Credit Card Payment"), "Platinum Hotel Credit"),
        ];
        let fixes = plan(&transactions);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].merchant.as_deref(), Some("Amex Hotel Credit"));
        assert_eq!(fixes[0].category, None);
        assert_eq!(fixes[0].paired_with, None);
    }

    #[test]
    fn ordinary_refunds_and_purchases_are_ignored() {
        let transactions = vec![
            txn("refund", "2026-05-21", 139.0, "JetBlue", ("c-travel", "Travel & Vacation"), "JETBLUE AIRWAYS"),
            txn("charge", "2026-05-20", -139.0, "JetBlue", ("c-travel", "Travel & Vacation"), "JETBLUE AIRWAYS"),
        ];
        assert!(plan(&transactions).is_empty());
    }
}
