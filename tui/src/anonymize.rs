//! Swaps real data for plausible stand-ins so a screen can be shared.
//!
//! Merchants, accounts and funds get invented names, and every amount is
//! rescaled. Transactions are thinned out and moved a few days later, each
//! investment account's balance is dealt afresh among its holdings, and the
//! private company holdings are invented outright (see `equity`). The
//! disguise is consistent: one merchant always maps to the same invented
//! name, scaling and move, so charts, totals and recurring payments still
//! hang together. Categories are left as they are.

pub mod equity;

use crate::data::{Account, Data, Txn};
use crate::dice::{hash, unit};
use chrono::{Duration, NaiveDate};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Range the overall scaling of amounts is drawn from. Kept away from 1 so
/// figures are clearly not the real ones.
const SCALE_MIN: f64 = 0.35;
const SCALE_MAX: f64 = 0.85;

/// How far a single merchant's amounts drift from the overall scaling, so one
/// known real amount does not reveal all the others
const MERCHANT_JITTER: f64 = 0.12;

/// Days a merchant's transactions are moved later, so that none keeps its
/// real date. One merchant's all move together, which keeps a bill's rhythm.
const DATE_SHIFT_MIN: i64 = 1;
const DATE_SHIFT_MAX: i64 = 10;

/// Days either side of a month's end that a moved payment should land clear of
const MONTH_END_MARGIN: i64 = 3;

/// Range the share of one-off transactions left out is drawn from, so the
/// history has neither the real number of rows nor the real mix on any day
const THINNING_MIN: f64 = 0.25;
const THINNING_MAX: f64 = 0.45;

const CENTS_PER_DOLLAR: f64 = 100.0;

/// Range an invented security's share price differs from the real one's by.
/// Drawn evenly between halving and doubling, not between the two numbers.
const PRICE_FACTOR_MIN: f64 = 0.4;
const PRICE_FACTOR_MAX: f64 = 2.5;

/// A money market fund's share price, which an invented one keeps
const STABLE_SHARE_PRICE: f64 = 1.0;
const STABLE_PRICE_TOLERANCE: f64 = 0.005;

/// Least weight a holding is dealt when an account's balance is split afresh
const WEIGHT_FLOOR: f64 = 0.02;
/// How a cash holding's weight compares: mostly what is left over from buying funds
const CASH_WEIGHT: f64 = 0.1;

/// Monarch's asset class for money market funds and the like
const CASH_CLASS: &str = "cash";
/// Asset class assumed for a security Monarch lists no holding for
const UNKNOWN_CLASS: &str = "unclassified";

/// Mutual fund tickers are five letters ending in X; money market funds end in XX
const FUND_TICKER_LENGTH: usize = 5;
const FUND_TICKER_SUFFIX: &str = "X";
const MONEY_FUND_TICKER_SUFFIX: &str = "XX";
const ALPHABET_LENGTH: u64 = 26;

/// Invented names tried for a security before one is told apart by share class
const MAX_NAME_TRIES: u64 = 64;
/// Tickers tried at one length before a letter is added
const MAX_TICKER_TRIES: u64 = 32;

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

/// Fund companies an invented fund is sold by
const FUND_FAMILIES: [&str; 10] = [
    "Northfield", "Lakeshore", "Granite", "Beacon", "Meridian", "Harborview", "Pinecrest", "Bluestone", "Fairmount",
    "Kestrel",
];

/// What a listed company's invented name ends in
const COMPANY_SUFFIXES: [&str; 5] = ["Inc", "Corp", "Group", "Holdings", "Technologies"];

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

/// What an invented fund invests in, by the asset class Monarch files the real one under
fn fund_kinds(asset_class: &str) -> &'static [&'static str] {
    match asset_class {
        "stocks" => &[
            "Total Market Index Fund", "500 Index Fund", "Large Cap Growth Fund", "Small Cap Value Fund",
            "International Index Fund", "Dividend Growth Fund", "Mid Cap Index Fund", "Emerging Markets Fund",
        ],
        "bonds" => &[
            "Total Bond Index Fund", "Intermediate Treasury Fund", "Municipal Bond Fund", "Inflation Protected Bond Fund",
        ],
        CASH_CLASS => &["Money Market Fund", "Treasury Money Market Fund", "Government Cash Reserves Fund"],
        "mixed" => &[
            "Target Retirement 2040 Fund", "Target Retirement 2050 Fund", "Target Retirement 2060 Fund",
            "Balanced Index Fund", "Moderate Allocation Fund",
        ],
        _ => &[
            "Stable Value Fund", "Growth Portfolio", "Diversified Equity Trust", "Core Blend Portfolio",
            "Conservative Portfolio",
        ],
    }
}

/// What tells one security from another: its ticker, or its name when it has none
fn security_key<'a>(ticker: &'a str, name: &'a str) -> &'a str {
    if ticker.is_empty() {
        name
    } else {
        ticker
    }
}

/// A security's price and share count once its value has been changed to
/// `value` and its price multiplied by `price_factor`
fn reprice(price: Option<f64>, quantity: f64, real_value: f64, value: f64, price_factor: f64) -> (Option<f64>, f64) {
    // Monarch has no quote for some funds; their price is implied by the value
    let implied = if quantity > 0.0 { Some(real_value / quantity) } else { None };
    let quantity = match price.or(implied).map(|each| each * price_factor) {
        Some(each) if each > 0.0 => value / each,
        _ => 0.0,
    };
    (price.map(|each| each * price_factor), quantity)
}

struct Disguise {
    seed: u64,
    scale: f64,
    /// Share of one-off transactions left out
    thinning: f64,
    merchants: HashMap<String, String>,
    /// Invented (ticker, name) by the real security's key
    securities: HashMap<String, (String, String)>,
    /// Invented names and tickers already given out
    taken: HashSet<String>,
    tickers: HashSet<String>,
}

impl Disguise {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            scale: SCALE_MIN + unit(hash(seed, "scale")) * (SCALE_MAX - SCALE_MIN),
            thinning: THINNING_MIN + unit(hash(seed, "thinning")) * (THINNING_MAX - THINNING_MIN),
            merchants: HashMap::new(),
            securities: HashMap::new(),
            taken: HashSet::new(),
            tickers: HashSet::new(),
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

    /// Days each merchant's transactions are moved later. The number is drawn
    /// per merchant, but one that would carry some of its payments past a
    /// month's end and leave others like them behind gives way to the next
    /// that would not: a paycheck that comes on the 30th one month and the
    /// 29th the next should not count twice in one month and never in another.
    fn date_shifts<'a>(&self, transactions: &'a [Txn]) -> HashMap<&'a str, i64> {
        let mut days_left: HashMap<&str, Vec<i64>> = HashMap::new();
        for txn in transactions {
            let left = (crate::forecast::month_end(txn.date) - txn.date).num_days();
            days_left.entry(txn.merchant.as_str()).or_default().push(left);
        }
        let choices = DATE_SHIFT_MAX - DATE_SHIFT_MIN + 1;
        days_left
            .into_iter()
            .map(|(merchant, days_left)| {
                let drawn = (unit(hash(self.seed, &format!("date:{}", merchant))) * choices as f64) as i64;
                // With `left` days of its month to go, a payment lands `left - shift` days before the end
                let near_month_end = |shift: &i64| {
                    days_left.iter().filter(|left| (-MONTH_END_MARGIN..MONTH_END_MARGIN).contains(&(*left - shift))).count()
                };
                let shift = (0..choices)
                    .map(|step| DATE_SHIFT_MIN + (drawn + step) % choices)
                    .min_by_key(near_month_end)
                    .unwrap_or(DATE_SHIFT_MIN);
                (merchant, shift)
            })
            .collect()
    }

    /// Whether a one-off transaction is left out. The two sides of a transfer,
    /// and a charge and its refund, share a merchant, a day and an amount, and
    /// so stay or go together.
    fn leaves_out(&self, txn: &Txn) -> bool {
        let cents = (txn.amount.abs() * CENTS_PER_DOLLAR).round() as i64;
        unit(hash(self.seed, &format!("thin:{}:{}:{}", txn.merchant, txn.date, cents))) < self.thinning
    }

    /// The invented ticker and name standing in for a real security. `kind`
    /// is Monarch's security type where a position gives one.
    fn security(&mut self, ticker: &str, name: &str, asset_class: &str, kind: &str) -> (String, String) {
        let key = security_key(ticker, name);
        if let Some(invented) = self.securities.get(key) {
            return invented.clone();
        }
        let kind = kind.to_lowercase();
        let fund_ticker = ticker.len() == FUND_TICKER_LENGTH && ticker.ends_with(FUND_TICKER_SUFFIX);
        let exchange_fund = kind.contains("etf");
        // A plain ticker with nothing saying it is a fund is a company's shares
        let company = kind.contains("stock") || (kind.is_empty() && !ticker.is_empty() && !fund_ticker);
        let kinds = fund_kinds(asset_class);
        let mut h = hash(self.seed, &format!("security:{}", key));
        let mut tries = 0;
        let invented_name = loop {
            let mut invented = if company {
                format!(
                    "{} {} {}",
                    FIRST_WORDS[(h % FIRST_WORDS.len() as u64) as usize],
                    SECOND_WORDS[((h >> 16) % SECOND_WORDS.len() as u64) as usize],
                    COMPANY_SUFFIXES[((h >> 32) % COMPANY_SUFFIXES.len() as u64) as usize],
                )
            } else {
                let family = FUND_FAMILIES[(h % FUND_FAMILIES.len() as u64) as usize];
                let holds = kinds[((h >> 16) % kinds.len() as u64) as usize];
                if exchange_fund {
                    format!("{} {} ETF", family, holds.trim_end_matches(" Fund"))
                } else {
                    format!("{} {}", family, holds)
                }
            };
            // There are only so many funds to invent: past that, share classes tell them apart
            if tries >= MAX_NAME_TRIES {
                invented = format!("{} R{}", invented, tries - MAX_NAME_TRIES + 1);
            }
            if self.taken.insert(invented.clone()) {
                break invented;
            }
            tries += 1;
            h = hash(h, key);
        };
        let invented = (self.ticker_like(ticker, &invented_name, h), invented_name);
        self.securities.insert(key.to_string(), invented.clone());
        invented
    }

    /// An unused ticker of the same shape as `real`, spelled from the initials
    /// of the invented name where those are free
    fn ticker_like(&mut self, real: &str, name: &str, mut h: u64) -> String {
        if real.is_empty() {
            return String::new();
        }
        let suffix = match real.len() == FUND_TICKER_LENGTH {
            true if real.ends_with(MONEY_FUND_TICKER_SUFFIX) => MONEY_FUND_TICKER_SUFFIX,
            true if real.ends_with(FUND_TICKER_SUFFIX) => FUND_TICKER_SUFFIX,
            _ => "",
        };
        let mut length = real.len().min(FUND_TICKER_LENGTH) - suffix.len();
        let mut initials: Vec<char> = name
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .filter(char::is_ascii_alphabetic)
            .map(|letter| letter.to_ascii_uppercase())
            .collect();
        let mut tries = 0;
        loop {
            let mut stem: String = initials.iter().take(length).collect();
            while stem.len() < length {
                h = hash(h, name);
                stem.push((b'A' + (h % ALPHABET_LENGTH) as u8) as char);
            }
            let ticker = format!("{}{}", stem, suffix);
            if self.tickers.insert(ticker.clone()) {
                return ticker;
            }
            // Taken: draw the letters instead, and more of them if that keeps failing
            initials.clear();
            tries += 1;
            if tries % MAX_TICKER_TRIES == 0 {
                length += 1;
            }
        }
    }

    /// Factor an invented security's share price differs from the real one's by
    fn price_factor(&self, key: &str, asset_class: &str, price: Option<f64>) -> f64 {
        let stable = price.is_some_and(|each| (each - STABLE_SHARE_PRICE).abs() < STABLE_PRICE_TOLERANCE);
        if asset_class == CASH_CLASS && stable {
            return 1.0;
        }
        let draw = unit(hash(self.seed, &format!("price:{}", key)));
        PRICE_FACTOR_MIN * (PRICE_FACTOR_MAX / PRICE_FACTOR_MIN).powf(draw)
    }

    /// How much of an account's balance one of its holdings is dealt, before
    /// the account's weights are made to add up. Most come out small and a few
    /// large, as in a real portfolio.
    fn weight(&self, account_id: &str, key: &str, asset_class: &str) -> f64 {
        let draw = unit(hash(self.seed, &format!("weight:{}:{}", account_id, key)));
        let weight = WEIGHT_FLOOR - (1.0 - draw).ln();
        if asset_class == CASH_CLASS {
            weight * CASH_WEIGHT
        } else {
            weight
        }
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

/// Rename and rescale the transactions, leave out a share of the one-off
/// ones, and move each merchant's a few days later
fn disguise_transactions(disguise: &mut Disguise, real: &Data, out: &mut Data, today: NaiveDate) {
    // Whoever is paid on a rhythm keeps a whole history, so that recurring
    // payments are still found in the disguise and no month misses its rent
    let steady: HashSet<(&str, &str)> =
        real.streams.iter().map(|stream| (stream.account_id.as_str(), stream.merchant.as_str())).collect();
    let shifts = disguise.date_shifts(&real.transactions);
    out.transactions.retain_mut(|txn| {
        let kept_whole =
            txn.is_income() || txn.recurring || steady.contains(&(txn.account_id.as_str(), txn.merchant.as_str()));
        if !kept_whole && disguise.leaves_out(txn) {
            return false;
        }
        txn.date += Duration::days(shifts.get(txn.merchant.as_str()).copied().unwrap_or(DATE_SHIFT_MIN));
        // Moved past today, it has not happened yet
        if txn.date > today {
            return false;
        }
        let factor = disguise.merchant_scale(&txn.merchant);
        txn.merchant = disguise.merchant(&txn.merchant, &txn.category, txn.group_type == "transfer");
        txn.amount *= factor;
        txn.credited *= factor;
        txn.original_name = txn.merchant.to_uppercase();
        txn.notes.clear();
        txn.tags.clear();
        true
    });
    // Newest first again, now that the dates have moved
    out.transactions.sort_by(|a, b| b.date.cmp(&a.date));
}

/// Give every security an invented name, ticker and price, and deal each
/// account's balance afresh among what it holds, so neither the funds nor
/// their weights are the real ones. An account's total is kept, which keeps
/// its balance, the net worth and the portfolio chart in agreement.
fn disguise_investments(disguise: &mut Disguise, real: &Data, out: &mut Data, account_names: &HashMap<String, String>) {
    let scale = disguise.scale;
    let classes: HashMap<&str, &str> =
        real.holdings.iter().map(|h| (security_key(&h.ticker, &h.name), h.asset_class.as_str())).collect();
    let class_of = |key: &str| classes.get(key).copied().unwrap_or(UNKNOWN_CLASS);

    // What each account holds in all, and the weights its holdings were dealt
    let mut accounts: HashMap<&str, (f64, f64)> = HashMap::new();
    for position in real.positions.iter().filter(|p| p.value > 0.0) {
        let totals = accounts.entry(position.account_id.as_str()).or_default();
        totals.0 += position.value;
        let key = security_key(&position.ticker, &position.name);
        totals.1 += disguise.weight(&position.account_id, key, class_of(key));
    }

    // Each security's value across accounts, before and after
    let mut securities: HashMap<&str, (f64, f64)> = HashMap::new();
    for (position, real_position) in out.positions.iter_mut().zip(&real.positions) {
        let key = security_key(&real_position.ticker, &real_position.name);
        let value = match accounts.get(real_position.account_id.as_str()) {
            Some((total, weights)) if real_position.value > 0.0 => {
                let value = total * scale * disguise.weight(&real_position.account_id, key, class_of(key)) / weights;
                let totals = securities.entry(key).or_default();
                totals.0 += real_position.value;
                totals.1 += value;
                value
            }
            _ => real_position.value * scale,
        };
        let price_factor = disguise.price_factor(key, class_of(key), real_position.price);
        (position.price, position.quantity) =
            reprice(real_position.price, real_position.quantity, real_position.value, value, price_factor);
        position.cost_basis = match real_position.value > 0.0 {
            true => real_position.cost_basis.map(|basis| basis * value / real_position.value),
            false => real_position.cost_basis.map(|basis| basis * scale),
        };
        position.value = value;
        (position.ticker, position.name) =
            disguise.security(&real_position.ticker, &real_position.name, class_of(key), &real_position.kind);
        if let Some(name) = account_names.get(&position.account_id) {
            position.account_name = name.clone();
        }
    }
    out.positions.sort_by(|a, b| a.account_name.cmp(&b.account_name).then(b.value.total_cmp(&a.value)));

    for (holding, real_holding) in out.holdings.iter_mut().zip(&real.holdings) {
        let key = security_key(&real_holding.ticker, &real_holding.name);
        let value = match securities.get(key) {
            Some((before, after)) if *before > 0.0 => real_holding.value * after / before,
            _ => real_holding.value * scale,
        };
        let price_factor = disguise.price_factor(key, &real_holding.asset_class, real_holding.price);
        (holding.price, holding.quantity) =
            reprice(real_holding.price, real_holding.quantity, real_holding.value, value, price_factor);
        holding.cost_basis = match real_holding.value > 0.0 {
            true => real_holding.cost_basis.map(|basis| basis * value / real_holding.value),
            false => real_holding.cost_basis.map(|basis| basis * scale),
        };
        holding.value = value;
        (holding.ticker, holding.name) =
            disguise.security(&real_holding.ticker, &real_holding.name, &real_holding.asset_class, "");
    }
    out.holdings.sort_by(|a, b| b.value.total_cmp(&a.value));
    out.portfolio.total_value *= scale;
}

/// A copy of `data` with names invented and amounts rescaled. The same `seed`
/// always produces the same disguise.
pub fn anonymize(data: &Data, seed: u64) -> Data {
    disguised(data, seed, data.today())
}

fn disguised(data: &Data, seed: u64, today: NaiveDate) -> Data {
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

    disguise_transactions(&mut disguise, data, &mut out, today);

    // Balances and net worth share the one scale so charts and totals stay
    // consistent with each other
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
    disguise_investments(&mut disguise, data, &mut out, &account_names);

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

    out.equity = equity::invent(&data.equity, seed, today, scale);

    out.anonymized = true;
    out.derive();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Holding, Position, Txn};

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    const TODAY: (i32, u32, u32) = (2026, 10, 12);

    fn today() -> NaiveDate {
        date(TODAY.0, TODAY.1, TODAY.2)
    }

    fn txn(merchant: &str, amount: f64, group_type: &str) -> Txn {
        Txn {
            date: date(2026, 9, 1),
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
            // Monarch counts these as recurring, so none is left out
            recurring: true,
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

    /// The transactions of `out` whose amounts came from a real merchant's, by its invented name
    fn of<'a>(out: &'a Data, merchant: &str) -> Vec<&'a Txn> {
        out.transactions.iter().filter(|t| t.merchant == merchant).collect()
    }

    #[test]
    fn no_real_merchant_name_or_note_survives() {
        let out = disguised(&sample(), 7, today());
        assert_eq!(out.transactions.len(), 4);
        for t in &out.transactions {
            assert!(!t.merchant.contains("Real") && !t.merchant.contains("Other"), "{}", t.merchant);
            assert!(!t.original_name.contains("REAL"));
            assert!(t.notes.is_empty() && t.tags.is_empty());
        }
        assert!(out.anonymized);
    }

    #[test]
    fn one_merchant_keeps_one_name_and_one_scaling() {
        let mut disguise = Disguise::new(7);
        let cafe = disguise.merchant("Real Cafe", "Coffee Shops", false);
        let other = disguise.merchant("Other Place", "Coffee Shops", false);
        assert_ne!(cafe, other);
        let out = disguised(&sample(), 7, today());
        let mut amounts: Vec<f64> = of(&out, &cafe).iter().map(|t| t.amount).collect();
        amounts.sort_by(f64::total_cmp);
        // -5 and -7 stay in proportion, so a fixed bill stays a fixed bill
        assert_eq!(amounts.len(), 2);
        assert!((amounts[1] / amounts[0] - 5.0 / 7.0).abs() < 1e-9);
        assert!(amounts[1] != -5.0);
        assert_eq!(of(&out, &other).len(), 1);
    }

    #[test]
    fn same_seed_gives_the_same_disguise_and_another_seed_a_different_one() {
        let (first, again, other) =
            (disguised(&sample(), 7, today()), disguised(&sample(), 7, today()), disguised(&sample(), 8, today()));
        assert_eq!(first.transactions[0].merchant, again.transactions[0].merchant);
        assert_eq!(first.transactions[0].amount, again.transactions[0].amount);
        assert_eq!(first.transactions[0].date, again.transactions[0].date);
        let amounts = |data: &Data| -> Vec<i64> {
            let mut amounts: Vec<i64> = data.transactions.iter().map(|t| (t.amount * 1e6) as i64).collect();
            amounts.sort();
            amounts
        };
        assert_ne!(amounts(&first), amounts(&other));
    }

    #[test]
    fn transfers_lose_their_names() {
        let out = disguised(&sample(), 7, today());
        assert_eq!(out.transactions.iter().filter(|t| t.merchant == "Transfer").count(), 1);
    }

    #[test]
    fn every_transaction_moves_later_and_a_merchant_moves_as_one() {
        let mut data = sample();
        data.transactions.push(Txn { date: date(2026, 8, 1), ..txn("Real Cafe", -6.0, "expense") });
        let out = disguised(&data, 7, today());
        let cafe = Disguise::new(7).merchant("Real Cafe", "Coffee Shops", false);
        let mut days: Vec<i64> = of(&out, &cafe).iter().map(|t| (t.date - date(2026, 8, 1)).num_days()).collect();
        days.sort();
        let shift = days[0];
        assert!((DATE_SHIFT_MIN..=DATE_SHIFT_MAX).contains(&shift));
        // September 1 is 31 days after August 1, before and after
        assert_eq!(days, vec![shift, shift + 31, shift + 31]);
        // Newest first
        assert!(out.transactions.windows(2).all(|pair| pair[0].date >= pair[1].date));
    }

    #[test]
    fn a_paycheck_near_the_end_of_the_month_keeps_to_two_a_month() {
        use chrono::{Datelike, Weekday};
        // Paid on the 15th and the last day of the month, or the Friday before a weekend
        let on_a_weekday = |day: NaiveDate| match day.weekday() {
            Weekday::Sat => day - Duration::days(1),
            Weekday::Sun => day - Duration::days(2),
            _ => day,
        };
        let months: Vec<NaiveDate> = (1..=12).map(|month| date(2025, month, 1)).collect();
        let paydays = months
            .iter()
            .flat_map(|first| [first.with_day(15).unwrap(), crate::forecast::month_end(*first)])
            .map(on_a_weekday);
        let data = Data {
            transactions: paydays.map(|day| Txn { date: day, ..txn("Employer", 3000.0, "income") }).collect(),
            ..Default::default()
        };
        for seed in 0..50 {
            let out = disguised(&data, seed, today());
            for month in &months[1..] {
                let paid = out.transactions.iter().filter(|t| crate::data::month_of(t.date) == *month).count();
                assert_eq!(paid, 2, "seed {seed}, {month}");
            }
        }
    }

    #[test]
    fn a_transaction_moved_past_today_has_not_happened_yet() {
        let mut data = sample();
        data.transactions.iter_mut().for_each(|t| t.date = today());
        assert!(disguised(&data, 7, today()).transactions.is_empty());
    }

    #[test]
    fn one_off_transactions_are_thinned_out_and_steady_ones_kept_whole() {
        let day = |i: usize| date(2025, 1, 1) + Duration::days((i % 300) as i64);
        let one_off = |i: usize| Txn {
            date: day(i),
            recurring: false,
            category: "Shopping".to_string(),
            ..txn(&format!("Shop {}", i % 40), -(i as f64), "expense")
        };
        let mut data = Data { transactions: (1..=1000).map(one_off).collect(), ..Default::default() };
        // Rent that Monarch knows as recurring, and paychecks it does not
        data.transactions.extend((1..=100).map(|i| Txn { date: day(i), ..txn("Landlord", -900.0, "expense") }));
        data.transactions
            .extend((1..=100).map(|i| Txn { date: day(i), recurring: false, ..txn("Employer", 2000.0, "income") }));
        let out = disguised(&data, 7, today());
        let kept = out.transactions.iter().filter(|t| t.category == "Shopping").count() as f64;
        let thinning = Disguise::new(7).thinning;
        assert!((THINNING_MIN..=THINNING_MAX).contains(&thinning));
        assert!((kept / 1000.0 - (1.0 - thinning)).abs() < 0.05, "{kept}");
        assert_eq!(out.transactions.len() as f64 - kept, 200.0);
    }

    #[test]
    fn both_sides_of_a_transfer_stay_or_go_together() {
        let side = |i: usize, amount: f64, account: &str| Txn {
            date: date(2025, 1, 1) + Duration::days(i as i64),
            recurring: false,
            account_id: account.to_string(),
            ..txn("To Savings", amount * i as f64, "transfer")
        };
        let mut data = Data::default();
        for i in 1..=200 {
            data.transactions.push(side(i, -1.0, "checking"));
            data.transactions.push(side(i, 1.0, "savings"));
        }
        let out = disguised(&data, 7, today());
        let (sent, received): (Vec<&Txn>, Vec<&Txn>) = out.transactions.iter().partition(|t| t.amount < 0.0);
        assert!(sent.len() < 200 && !sent.is_empty());
        assert_eq!(sent.len(), received.len());
        for (out, back) in sent.iter().zip(&received) {
            assert_eq!(out.date, back.date);
            assert!((out.amount + back.amount).abs() < 1e-9);
        }
    }

    fn position(account: &str, ticker: &str, name: &str, value: f64, price: Option<f64>) -> Position {
        Position {
            account_id: account.to_string(),
            account_name: format!("Real {}", account),
            name: name.to_string(),
            ticker: ticker.to_string(),
            kind: "Mutual Fund".to_string(),
            quantity: price.map(|each| value / each).unwrap_or(value / 20.0),
            price,
            value,
            cost_basis: Some(value / 2.0),
        }
    }

    fn invested() -> Data {
        let positions = vec![
            position("ira", "VTSAX", "Vanguard Total Stock Market Index Fund", 6000.0, Some(150.0)),
            position("ira", "VMFXX", "Vanguard Federal Money Market Fund", 1000.0, Some(1.0)),
            position("ira", "", "LRG CAP GRTH III R1", 3000.0, None),
            position("taxable", "VTSAX", "Vanguard Total Stock Market Index Fund", 4000.0, Some(150.0)),
            position("taxable", "AAPL", "Apple Inc", 2000.0, Some(250.0)),
        ];
        let holding = |ticker: &str, name: &str, class: &str, price: Option<f64>| {
            let value: f64 = positions.iter().filter(|p| p.ticker == ticker && p.name == name).map(|p| p.value).sum();
            Holding {
                name: name.to_string(),
                ticker: ticker.to_string(),
                asset_class: class.to_string(),
                quantity: price.map(|each| value / each).unwrap_or(value / 20.0),
                price,
                value,
                cost_basis: Some(value / 2.0),
                day_change_pct: None,
            }
        };
        let holdings = vec![
            holding("VTSAX", "Vanguard Total Stock Market Index Fund", "stocks", Some(150.0)),
            holding("", "LRG CAP GRTH III R1", "unclassified", None),
            holding("AAPL", "Apple Inc", "stocks", Some(250.0)),
            holding("VMFXX", "Vanguard Federal Money Market Fund", "cash", Some(1.0)),
        ];
        let mut data = Data { holdings, positions, ..Default::default() };
        data.positions[4].kind = "Stock".to_string();
        data.portfolio.total_value = 16_000.0;
        data
    }

    #[test]
    fn no_real_fund_name_ticker_or_price_survives() {
        let real = invested();
        let out = disguised(&real, 7, today());
        let real_words = ["Vanguard", "VTSAX", "VMFXX", "LRG", "Apple", "AAPL"];
        let shown = out
            .holdings
            .iter()
            .map(|h| (&h.ticker, &h.name, h.price))
            .chain(out.positions.iter().map(|p| (&p.ticker, &p.name, p.price)));
        for (ticker, name, price) in shown {
            assert!(real_words.iter().all(|word| !ticker.contains(word) && !name.contains(word)), "{ticker} {name}");
            assert!(price != Some(150.0) && price != Some(250.0));
        }
        // Tickers keep their shape: funds end in X, and what had none still has none
        let tickers: Vec<&str> = out.holdings.iter().map(|h| h.ticker.as_str()).collect();
        assert_eq!(tickers.iter().filter(|t| t.len() == 5 && t.ends_with("XX")).count(), 1);
        assert_eq!(tickers.iter().filter(|t| t.len() == 5 && t.ends_with('X') && !t.ends_with("XX")).count(), 1);
        assert_eq!(tickers.iter().filter(|t| t.len() == 4).count(), 1);
        assert_eq!(tickers.iter().filter(|t| t.is_empty()).count(), 1);
        // A money market fund is still a dollar a share
        assert_eq!(out.holdings.iter().find(|h| h.ticker.ends_with("XX")).unwrap().price, Some(1.0));
    }

    #[test]
    fn a_security_has_one_stand_in_wherever_it_is_held() {
        let out = disguised(&invested(), 7, today());
        let (ticker, name) = Disguise::new(7).security("VTSAX", "", "stocks", "Mutual Fund");
        assert_eq!(out.holdings.iter().filter(|h| h.ticker == ticker && h.name == name).count(), 1);
        assert_eq!(out.positions.iter().filter(|p| p.ticker == ticker && p.name == name).count(), 2);
        let mut names: Vec<&str> = out.holdings.iter().map(|h| h.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn an_account_keeps_its_total_but_not_its_weights() {
        let real = invested();
        let out = disguised(&real, 7, today());
        let scale = Disguise::new(7).scale;
        let total = |positions: &[Position], account: &str| -> f64 {
            positions.iter().filter(|p| p.account_id == account).map(|p| p.value).sum()
        };
        for account in ["ira", "taxable"] {
            assert!((total(&out.positions, account) - total(&real.positions, account) * scale).abs() < 1e-6);
        }
        let weights = |positions: &[Position]| -> Vec<i64> {
            let mut weights: Vec<i64> =
                positions.iter().map(|p| (p.value / total(positions, &p.account_id) * 1000.0) as i64).collect();
            weights.sort();
            weights
        };
        assert_ne!(weights(&out.positions), weights(&real.positions));
        // Holdings add up to the positions, security by security and in all
        for holding in &out.holdings {
            let held: f64 = out.positions.iter().filter(|p| p.name == holding.name).map(|p| p.value).sum();
            assert!((holding.value - held).abs() < 1e-6);
            assert!((holding.quantity * holding.price.unwrap_or(holding.value / holding.quantity) - holding.value).abs() < 1e-6);
        }
        assert!((out.portfolio.total_value - 16_000.0 * scale).abs() < 1e-6);
        assert!(out.holdings.windows(2).all(|pair| pair[0].value >= pair[1].value));
    }
}
