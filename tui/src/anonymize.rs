//! Swaps real data for plausible stand-ins so a screen can be shared.
//!
//! Merchants and accounts get invented names, and every amount is rescaled.
//! The disguise is consistent: one merchant always maps to the same invented
//! name and the same scaling, so charts, totals and recurring payments still
//! hang together. Dates, categories and fund tickers are left as they are.

use crate::data::{Account, Data};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Range the overall scaling of amounts is drawn from. Kept away from 1 so
/// figures are clearly not the real ones.
const SCALE_MIN: f64 = 0.35;
const SCALE_MAX: f64 = 0.85;

/// How far a single merchant's amounts drift from the overall scaling, so one
/// known real amount does not reveal all the others
const MERCHANT_JITTER: f64 = 0.12;

/// Most a credit score is shifted, in points
const CREDIT_SCORE_SHIFT: i64 = 25;
const CREDIT_SCORE_MIN: i64 = 300;
const CREDIT_SCORE_MAX: i64 = 850;

const FIRST_WORDS: [&str; 40] = [
    "Amber", "Birch", "Cedar", "Copper", "Coral", "Crescent", "Golden", "Harbor", "Hazel", "Indigo", "Ivory", "Juniper",
    "Lantern", "Linden", "Maple", "Marigold", "Meadow", "Mesa", "Mistral", "Nimbus", "Northern", "Orchard", "Pebble",
    "Pine", "Prairie", "Quarry", "Redwood", "River", "Saffron", "Sage", "Silver", "Slate", "Summit", "Sunny", "Tidal",
    "Timber", "Valley", "Velvet", "Willow", "Winter",
];

const SECOND_WORDS: [&str; 40] = [
    "Anchor", "Arrow", "Badger", "Bell", "Bridge", "Brook", "Canyon", "Compass", "Crane", "Crown", "Falcon", "Fern",
    "Finch", "Forge", "Fox", "Gate", "Grove", "Harvest", "Heron", "Hill", "Kettle", "Lark", "Lighthouse", "Mill",
    "Oak", "Otter", "Owl", "Path", "Peak", "Pond", "Quill", "Raven", "Ridge", "Sparrow", "Spring", "Stone", "Thistle",
    "Trail", "Wharf", "Wren",
];

const BANKS: [&str; 8] = [
    "First Harbor Bank", "Northfield Credit Union", "Summit Trust", "Lakeshore Bank", "Granite Federal",
    "Beacon Savings", "Cedar Point Financial", "Meridian Bank",
];

/// What kind of business an invented merchant looks like, by spending category
fn business_kinds(category: &str) -> &'static [&'static str] {
    let category = category.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| category.contains(w));
    if has(&["coffee"]) {
        &["Coffee", "Roasters", "Espresso Bar"]
    } else if has(&["restaurant", "bar", "food"]) {
        &["Grill", "Kitchen", "Bistro", "Diner", "Tavern", "Cantina"]
    } else if has(&["grocer"]) {
        &["Market", "Grocers", "Foods"]
    } else if has(&["gas", "auto", "parking", "transit", "taxi"]) {
        &["Fuel", "Auto", "Garage", "Transit"]
    } else if has(&["travel", "vacation", "hotel"]) {
        &["Hotel", "Airways", "Lodge", "Travel"]
    } else if has(&["paycheck", "income"]) {
        &["Industries", "Partners", "Labs", "Group"]
    } else if has(&["subscription", "phone", "internet", "cable"]) {
        &["Media", "Online", "Wireless", "Digital"]
    } else if has(&["electric", "water", "garbage", "utilit"]) {
        &["Utilities", "Water District", "Power"]
    } else if has(&["insurance", "mortgage", "loan", "financial"]) {
        &["Mutual", "Lending", "Insurance"]
    } else if has(&["entertainment", "fitness"]) {
        &["Theater", "Arena", "Studio", "Club"]
    } else if has(&["medical", "dentist", "pharmacy"]) {
        &["Clinic", "Dental", "Pharmacy"]
    } else {
        &["Goods", "Outfitters", "Supply Co", "Mercantile", "Trading Co"]
    }
}

/// FNV-1a, salted so each session invents different names
fn hash(seed: u64, text: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET ^ seed;
    for byte in text.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(PRIME);
    }
    // Final mix so nearby inputs do not share their high bits
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

/// A value in `[0, 1)` derived from `hash`
fn unit(hash: u64) -> f64 {
    (hash >> 11) as f64 / (1u64 << 53) as f64
}

struct Disguise {
    seed: u64,
    scale: f64,
    merchants: HashMap<String, String>,
    taken: HashSet<String>,
}

impl Disguise {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            scale: SCALE_MIN + unit(hash(seed, "scale")) * (SCALE_MAX - SCALE_MIN),
            merchants: HashMap::new(),
            taken: HashSet::new(),
        }
    }

    /// The invented name standing in for a real merchant
    fn merchant(&mut self, real: &str, category: &str, transfer: bool) -> String {
        if real.is_empty() {
            return String::new();
        }
        // Moves between your own accounts all look alike once the names are gone
        if transfer {
            return "Transfer".to_string();
        }
        if let Some(name) = self.merchants.get(real) {
            return name.clone();
        }
        let kinds = business_kinds(category);
        let mut h = hash(self.seed, real);
        let name = loop {
            let name = format!(
                "{} {} {}",
                FIRST_WORDS[(h % FIRST_WORDS.len() as u64) as usize],
                SECOND_WORDS[((h >> 16) % SECOND_WORDS.len() as u64) as usize],
                kinds[((h >> 32) % kinds.len() as u64) as usize],
            );
            if self.taken.insert(name.clone()) {
                break name;
            }
            h = hash(h, real);
        };
        self.merchants.insert(real.to_string(), name.clone());
        name
    }

    /// Scaling for one merchant's amounts: the overall scale, nudged per merchant
    fn merchant_scale(&self, real: &str) -> f64 {
        let nudge = (unit(hash(self.seed, &format!("amount:{}", real))) * 2.0 - 1.0) * MERCHANT_JITTER;
        self.scale * (1.0 + nudge)
    }
}

/// Invented name for an account: its kind and a running number
fn account_label(account: &Account, counts: &mut HashMap<String, usize>) -> String {
    let base = if account.subtype.is_empty() { account.kind_display.clone() } else { account.subtype.clone() };
    let count = counts.entry(base.clone()).or_default();
    *count += 1;
    format!("{} {}", base, count)
}

/// Scale the money fields of a stored document, leaving ratios alone
fn scale_amounts(value: &mut Value, scale: f64) {
    match value {
        Value::Object(fields) => {
            for (key, field) in fields.iter_mut() {
                let lower = key.to_ascii_lowercase();
                let is_money = ["amount", "balance", "contribution", "sum", "total"]
                    .iter()
                    .any(|needle| lower.contains(needle));
                match field {
                    Value::Number(n) if is_money => {
                        if let Some(scaled) = n.as_f64().and_then(|v| serde_json::Number::from_f64(v * scale)) {
                            *field = Value::Number(scaled);
                        }
                    }
                    other => scale_amounts(other, scale),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| scale_amounts(item, scale)),
        _ => {}
    }
}

/// A copy of `data` with names invented and amounts rescaled. The same `seed`
/// always produces the same disguise.
pub fn anonymize(data: &Data, seed: u64) -> Data {
    let mut disguise = Disguise::new(seed);
    let scale = disguise.scale;
    let mut out = data.clone();

    // Accounts: numbered by kind, with invented institutions and masks
    let mut counts = HashMap::new();
    let mut account_names: HashMap<String, String> = HashMap::new();
    // Visible accounts are numbered first so the list reads 1, 2, 3 without gaps
    let mut order: Vec<usize> = (0..out.accounts.len()).collect();
    order.sort_by_key(|&i| out.accounts[i].is_hidden);
    for i in order {
        let account = &mut out.accounts[i];
        let label = account_label(account, &mut counts);
        account_names.insert(account.id.clone(), label.clone());
        let h = hash(seed, &account.id);
        account.name = label;
        account.institution = BANKS[(h % BANKS.len() as u64) as usize].to_string();
        account.mask = account.mask.as_ref().map(|_| format!("{:04}", (h >> 20) % 10_000));
        account.balance *= scale;
        account.signed_balance *= scale;
        account.credit_limit = account.credit_limit.map(|v| v * scale);
        account.minimum_payment = account.minimum_payment.map(|v| v * scale);
    }

    for txn in &mut out.transactions {
        let factor = disguise.merchant_scale(&txn.merchant);
        txn.merchant = disguise.merchant(&txn.merchant, &txn.category, txn.group_type == "transfer");
        txn.amount *= factor;
        txn.credited *= factor;
        txn.original_name = txn.merchant.to_uppercase();
        txn.notes.clear();
        txn.tags.clear();
    }

    // Balances, net worth and holdings share the one scale so charts and
    // totals stay consistent with each other
    for point in &mut out.net_worth {
        point.net_worth *= scale;
        point.assets *= scale;
        point.liabilities *= scale;
    }
    for series in out.balances.values_mut() {
        for point in series {
            point.1 *= scale;
        }
    }
    // Share counts are scaled rather than prices, so tickers keep real quotes
    for holding in &mut out.holdings {
        holding.quantity *= scale;
        holding.value *= scale;
        holding.cost_basis = holding.cost_basis.map(|v| v * scale);
    }
    for position in &mut out.positions {
        if let Some(name) = account_names.get(&position.account_id) {
            position.account_name = name.clone();
        }
        position.quantity *= scale;
        position.value *= scale;
        position.cost_basis = position.cost_basis.map(|v| v * scale);
    }
    out.portfolio.total_value *= scale;

    if let Some(budget) = &mut out.budget {
        scale_amounts(budget, scale);
    }
    if let Some(goals) = &mut out.goals {
        scale_amounts(goals, scale);
        if let Some(list) = goals["savingsGoals"].as_array_mut() {
            for (i, goal) in list.iter_mut().enumerate() {
                goal["name"] = Value::String(format!("Savings Goal {}", i + 1));
                if let Some(allocations) = goal["allocationAmountsByAccount"].as_array_mut() {
                    for allocation in allocations {
                        let id = allocation["account"]["id"].as_str().unwrap_or_default().to_string();
                        let name = account_names.get(&id).cloned().unwrap_or_else(|| "Account".to_string());
                        allocation["account"]["displayName"] = Value::String(name);
                    }
                }
            }
        }
    }

    let shift = (unit(hash(seed, "credit")) * (2 * CREDIT_SCORE_SHIFT + 1) as f64) as i64 - CREDIT_SCORE_SHIFT;
    for score in &mut out.credit_scores {
        score.1 = (score.1 + shift).clamp(CREDIT_SCORE_MIN, CREDIT_SCORE_MAX);
    }

    // Equity: share counts take the common scale and prices a factor of their
    // own, so neither the stake nor the company's valuation can be read back
    let price_scale = SCALE_MIN + unit(hash(seed, "equity-price")) * (SCALE_MAX - SCALE_MIN);
    let equity = &mut out.equity;
    for (i, issuer) in equity.issuers.iter_mut().enumerate() {
        issuer.name = format!("Private Company {}", i + 1);
        issuer.fmv = issuer.fmv.map(|v| v * price_scale);
    }
    for grant in &mut equity.grants {
        grant.quantity = (grant.quantity * scale).round();
        grant.vested = (grant.vested * scale).round();
        grant.exercised = (grant.exercised * scale).round();
        grant.exercisable = (grant.exercisable * scale).round();
        grant.exercise_price *= price_scale;
    }
    for event in &mut equity.vesting {
        event.quantity = (event.quantity * scale).round();
        event.iso = event.iso.map(|v| (v * scale).round());
        event.nso = event.nso.map(|v| (v * scale).round());
    }
    for certificate in &mut equity.certificates {
        certificate.quantity = (certificate.quantity * scale).round();
        certificate.cost = certificate.cost.map(|v| v * scale * price_scale);
        certificate.fmv_at_issue = certificate.fmv_at_issue.map(|v| v * price_scale);
    }
    for exercise in &mut equity.exercises {
        exercise.quantity = (exercise.quantity * scale).round();
        exercise.cost *= scale * price_scale;
        exercise.taxes *= scale * price_scale;
        exercise.fmv = exercise.fmv.map(|v| v * price_scale);
    }
    for (i, tender) in equity.tenders.iter_mut().enumerate() {
        tender.name = format!("Tender Offer {}", i + 1);
        tender.shares = (tender.shares * scale).round();
        tender.proceeds *= scale * price_scale;
    }
    for snapshot in &mut equity.snapshots {
        snapshot.fmv = snapshot.fmv.map(|v| v * price_scale);
    }

    out.anonymized = true;
    out.derive();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Txn;
    use chrono::NaiveDate;

    fn txn(merchant: &str, amount: f64, group_type: &str) -> Txn {
        Txn {
            date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            amount,
            credited: 0.0,
            is_matched_credit: false,
            merchant: merchant.to_string(),
            category: "Coffee Shops".to_string(),
            group_type: group_type.to_string(),
            account_id: "a1".to_string(),
            notes: "private note".to_string(),
            original_name: merchant.to_uppercase(),
            tags: "family".to_string(),
            pending: false,
            recurring: false,
            needs_review: false,
            hide_from_reports: false,
            one_time: false,
        }
    }

    fn sample() -> Data {
        Data {
            transactions: vec![
                txn("Real Cafe", -5.0, "expense"),
                txn("Real Cafe", -7.0, "expense"),
                txn("Other Place", -5.0, "expense"),
                txn("Savings Move", -100.0, "transfer"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn no_real_merchant_name_or_note_survives() {
        let out = anonymize(&sample(), 7);
        for t in &out.transactions {
            assert!(!t.merchant.contains("Real") && !t.merchant.contains("Other"), "{}", t.merchant);
            assert!(!t.original_name.contains("REAL"));
            assert!(t.notes.is_empty() && t.tags.is_empty());
        }
        assert!(out.anonymized);
    }

    #[test]
    fn one_merchant_keeps_one_name_and_one_scaling() {
        let out = anonymize(&sample(), 7);
        let (a, b, other) = (&out.transactions[0], &out.transactions[1], &out.transactions[2]);
        assert_eq!(a.merchant, b.merchant);
        assert_ne!(a.merchant, other.merchant);
        // -5 and -7 stay in proportion, so a fixed bill stays a fixed bill
        assert!((a.amount / b.amount - 5.0 / 7.0).abs() < 1e-9);
        assert!(a.amount != -5.0);
    }

    #[test]
    fn same_seed_gives_the_same_disguise_and_another_seed_a_different_one() {
        let (first, again, other) = (anonymize(&sample(), 7), anonymize(&sample(), 7), anonymize(&sample(), 8));
        assert_eq!(first.transactions[0].merchant, again.transactions[0].merchant);
        assert_eq!(first.transactions[0].amount, again.transactions[0].amount);
        assert_ne!(first.transactions[0].amount, other.transactions[0].amount);
    }

    #[test]
    fn transfers_lose_their_names() {
        let out = anonymize(&sample(), 7);
        assert_eq!(out.transactions[3].merchant, "Transfer");
    }
}
