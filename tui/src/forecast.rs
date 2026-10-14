//! Recurring-payment detection from transaction history.
//!
//! Monarch's own recurring detection misses a lot, so upcoming bills are
//! inferred here instead: any merchant an account pays on a steady rhythm is a
//! stream, and a stream's next dates follow from its last payment.
//!
//! Detection runs per account in two passes. The first looks for payments of
//! an identical amount (subscriptions, loan payments), which separates several
//! subscriptions billed by one merchant. The second takes what is left of each
//! merchant as a whole, which catches bills whose amount varies (utilities,
//! card payments, paychecks).

use crate::data::Txn;
use chrono::{Datelike, Duration, Months, NaiveDate};
use std::collections::{BTreeMap, HashMap};

/// Only this many of a stream's latest payments decide its rhythm, so a
/// schedule that changed long ago does not disqualify it
const RECENT_PAYMENTS: usize = 8;

/// Share of the gaps between payments that must fit the cadence
const MIN_REGULAR_SHARE: f64 = 0.5;
/// How many pauses a stream may have among its recent payments
const MAX_PAUSES: usize = 1;
/// The longest pause, in billing cycles, a stream is taken to resume from
const MAX_PAUSE_CYCLES: i64 = 4;

/// The latest gaps must all fit the cadence: a rhythm that held a year ago
/// but not lately is not a forecast
const RECENT_GAPS_REQUIRED: usize = 2;

/// Amounts within this fraction of the median count as "the same amount"
const FIXED_AMOUNT_TOLERANCE: f64 = 0.02;

/// Payments averaged for the expected amount of a variable bill
const AMOUNT_SAMPLE: usize = 3;

/// A different charge from the same merchant landing this close to a fixed
/// payment's expected date, for a similar amount, means the price changed
const SUPERSEDE_WINDOW_DAYS: i64 = 5;
const SUPERSEDE_AMOUNT_TOLERANCE: f64 = 0.35;

/// Two payments a year apart are only believed to be a yearly bill when
/// their amounts are this close
const YEARLY_AMOUNT_TOLERANCE: f64 = 0.15;

/// Streams smaller than this are noise (interest of a few cents, and the like)
const MIN_STREAM_AMOUNT: f64 = 1.0;

/// A merchant whose recent payments all fall within this fraction of their
/// median is one bill of varying size, not several different charges
const ONE_BILL_AMOUNT_SPREAD: f64 = 0.4;

/// A credit counts as reimbursing a charge when it lands in the same account
/// within this many days either side...
const REIMBURSEMENT_DAYS_BEFORE: i64 = 3;
const REIMBURSEMENT_DAYS_AFTER: i64 = 30;
/// ...covers at least this share of it...
const REIMBURSEMENT_MIN_SHARE: f64 = 0.85;
/// ...and comes from a merchant whose name shares at least this many leading
/// characters with the charge's, e.g. "Clear" and "Amex Clear Plus Credit"
const REIMBURSEMENT_NAME_MIN: usize = 4;
/// A stream is treated as reimbursed when this share of its recent payments were
const REIMBURSED_STREAM_SHARE: f64 = 0.5;

/// History older than this is ignored (long enough to see two yearly payments)
const LOOKBACK_DAYS: i64 = 800;

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Weekly,
    Biweekly,
    Monthly,
    Quarterly,
    Yearly,
}

struct CadenceRule {
    cadence: Cadence,
    /// Gap between payments, in days, that counts as on schedule
    min_gap: i64,
    max_gap: i64,
    /// Payments needed before the rhythm is believed
    min_payments: usize,
    /// Days past the expected date before the stream is presumed ended
    grace_days: i64,
    /// Whether a bill of varying amount may use this cadence. Weekly is
    /// excluded: habitual shopping looks weekly but is not a bill.
    allows_variable_amount: bool,
}

const RULES: [CadenceRule; 5] = [
    CadenceRule { cadence: Cadence::Weekly, min_gap: 6, max_gap: 8, min_payments: 4, grace_days: 4, allows_variable_amount: false },
    CadenceRule { cadence: Cadence::Biweekly, min_gap: 12, max_gap: 17, min_payments: 4, grace_days: 6, allows_variable_amount: true },
    CadenceRule { cadence: Cadence::Monthly, min_gap: 25, max_gap: 37, min_payments: 3, grace_days: 12, allows_variable_amount: true },
    CadenceRule { cadence: Cadence::Quarterly, min_gap: 84, max_gap: 98, min_payments: 3, grace_days: 20, allows_variable_amount: true },
    CadenceRule { cadence: Cadence::Yearly, min_gap: 350, max_gap: 380, min_payments: 2, grace_days: 30, allows_variable_amount: true },
];

impl Cadence {
    pub fn label(self) -> &'static str {
        match self {
            Cadence::Weekly => "Weekly",
            Cadence::Biweekly => "Every 2 weeks",
            Cadence::Monthly => "Monthly",
            Cadence::Quarterly => "Quarterly",
            Cadence::Yearly => "Yearly",
        }
    }

    fn rule(self) -> &'static CadenceRule {
        RULES.iter().find(|r| r.cadence == self).expect("every cadence has a rule")
    }

    fn payments_per_month(self) -> f64 {
        const MONTHS_PER_YEAR: f64 = 12.0;
        match self {
            Cadence::Weekly => 52.0 / MONTHS_PER_YEAR,
            Cadence::Biweekly => 26.0 / MONTHS_PER_YEAR,
            Cadence::Monthly => 1.0,
            Cadence::Quarterly => 4.0 / MONTHS_PER_YEAR,
            Cadence::Yearly => 1.0 / MONTHS_PER_YEAR,
        }
    }

    /// Calendar months between payments, for cadences tied to a day of the month
    fn months(self) -> Option<u32> {
        match self {
            Cadence::Monthly => Some(1),
            Cadence::Quarterly => Some(3),
            Cadence::Yearly => Some(12),
            Cadence::Weekly | Cadence::Biweekly => None,
        }
    }
}

/// One day's payments to a merchant: (date, amount, matched statement credits)
type Payment = (NaiveDate, f64, f64);

/// A payment an account makes or receives on a steady rhythm
#[derive(serde::Serialize, Debug, Clone)]
pub struct Stream {
    pub account_id: String,
    pub merchant: String,
    /// income, expense or transfer, from the latest payment's category
    pub group_type: String,
    pub cadence: Cadence,
    /// Signed like transactions: negative is money out
    pub amount: f64,
    /// The amount changes from payment to payment, so `amount` is an estimate
    pub variable: bool,
    pub last_date: NaiveDate,
    pub payments: usize,
    /// The latest payments the rhythm was judged on, oldest first, as
    /// (date, amount charged, statement credits matched to it)
    #[serde(skip)]
    recent: Vec<Payment>,
    /// Typical gap in days, for cadences not tied to a day of the month
    #[serde(skip)]
    gap_days: i64,
    /// Typical day of the month, for cadences that are
    #[serde(skip)]
    day_of_month: u32,
}

/// One expected future payment
#[derive(serde::Serialize, Debug, Clone)]
pub struct Upcoming {
    pub date: NaiveDate,
    pub merchant: String,
    pub group_type: String,
    pub cadence: Cadence,
    pub amount: f64,
    pub variable: bool,
    /// Expected before today and not yet seen
    pub late: bool,
}

fn median_i64(values: &mut [i64]) -> i64 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

/// `date` moved to `day` of its month, clamped to the month's length
fn with_day_clamped(date: NaiveDate, day: u32) -> NaiveDate {
    (1..=day.max(1))
        .rev()
        .find_map(|d| date.with_day(d))
        .unwrap_or(date)
}

impl Stream {
    /// The payment expected after `after`
    fn next_after(&self, after: NaiveDate) -> NaiveDate {
        match self.cadence.months() {
            Some(months) => {
                let first_of_month = after.with_day(1).unwrap_or(after);
                let target = first_of_month.checked_add_months(Months::new(months)).unwrap_or(after);
                with_day_clamped(target, self.day_of_month)
            }
            None => after + Duration::days(self.gap_days),
        }
    }

    /// Whether the merchant has since charged a different, similar amount
    /// about when this stream's next payment was due: a price change. The
    /// merchant's payments are then better treated as one variable stream.
    fn superseded_by(&self, merchant_transactions: &[&Txn]) -> bool {
        let expected = self.next_after(self.last_date);
        merchant_transactions.iter().any(|txn| {
            txn.date > self.last_date
                && (txn.date - expected).num_days().abs() <= SUPERSEDE_WINDOW_DAYS
                && (txn.amount - self.amount).abs() > self.amount.abs() * FIXED_AMOUNT_TOLERANCE
                && (txn.amount - self.amount).abs() <= self.amount.abs() * SUPERSEDE_AMOUNT_TOLERANCE
        })
    }

    /// The next payment to expect and whether it is overdue. An overdue
    /// payment stays "next" only through its grace period.
    pub fn next_expected(&self, today: NaiveDate) -> (NaiveDate, bool) {
        let grace = Duration::days(self.cadence.rule().grace_days);
        let mut date = self.next_after(self.last_date);
        while date + grace < today {
            date = self.next_after(date);
        }
        (date, date < today)
    }

    /// What the stream comes to per month, on average
    pub fn monthly_amount(&self) -> f64 {
        self.amount * self.cadence.payments_per_month()
    }

    pub fn is_income(&self) -> bool {
        self.group_type == "income"
    }

    pub fn is_transfer(&self) -> bool {
        self.group_type == "transfer"
    }

    /// Expected payments after the last one seen, up to and including `through`
    pub fn upcoming(&self, today: NaiveDate, through: NaiveDate) -> Vec<Upcoming> {
        let mut expected = Vec::new();
        let mut date = self.next_after(self.last_date);
        let grace = Duration::days(self.cadence.rule().grace_days);
        while date <= through {
            // Long overdue means it was skipped or missed, not that it is coming
            if date + grace < today {
                date = self.next_after(date);
                continue;
            }
            expected.push(Upcoming {
                date,
                merchant: self.merchant.clone(),
                group_type: self.group_type.clone(),
                cadence: self.cadence,
                amount: self.amount,
                variable: self.variable,
                late: date < today,
            });
            date = self.next_after(date);
        }
        expected
    }
}

/// Decide whether dated payments form a stream. `payments` are (date, amount)
/// sorted oldest first with one entry per day.
fn classify(
    account_id: &str,
    merchant: &str,
    payments: &[Payment],
    today: NaiveDate,
    fixed_amount: bool,
) -> Option<Stream> {
    let recent = &payments[payments.len().saturating_sub(RECENT_PAYMENTS)..];
    if recent.len() < 2 {
        return None;
    }
    let gaps: Vec<i64> = recent.windows(2).map(|w| (w[1].0 - w[0].0).num_days()).collect();
    let typical_gap = median_i64(&mut gaps.clone());
    let rule = RULES.iter().find(|r| (r.min_gap..=r.max_gap).contains(&typical_gap))?;
    if recent.len() < rule.min_payments || (!fixed_amount && !rule.allows_variable_amount) {
        return None;
    }
    // A subscription paused for a while (a sports package over the summer)
    // comes back on the same rhythm, so one gap of a few cycles does not count
    // against it
    let paused = |gap: &i64| (rule.min_gap * 2..=rule.max_gap * MAX_PAUSE_CYCLES).contains(gap);
    let pauses = gaps.iter().filter(|g| paused(g)).count();
    let on_schedule =
        |gap: &i64| (rule.min_gap..=rule.max_gap).contains(gap) || (pauses <= MAX_PAUSES && paused(gap));
    let regular = gaps.iter().filter(|g| on_schedule(g)).count();
    if (regular as f64) < gaps.len() as f64 * MIN_REGULAR_SHARE {
        return None;
    }
    if !gaps.iter().rev().take(RECENT_GAPS_REQUIRED).all(on_schedule) {
        return None;
    }

    let (last_date, _, _) = *recent.last()?;
    // A stream that has gone quiet well past its next date has ended
    if (today - last_date).num_days() > rule.max_gap + rule.grace_days {
        return None;
    }

    let amounts: Vec<f64> = recent.iter().map(|p| p.1).collect();
    let median_amount = median_f64(&mut amounts.clone());
    let variable = amounts
        .iter()
        .any(|a| (a - median_amount).abs() > median_amount.abs() * FIXED_AMOUNT_TOLERANCE);
    let sample = &recent[recent.len().saturating_sub(AMOUNT_SAMPLE)..];
    let amount = if variable {
        sample.iter().map(|p| p.1).sum::<f64>() / sample.len() as f64
    } else {
        median_amount
    };
    // What the bill really costs: less whatever a card has been crediting back
    let credited = sample.iter().map(|p| p.2).sum::<f64>() / sample.len() as f64;
    let variable = variable || sample.iter().any(|p| (p.2 - credited).abs() > FIXED_AMOUNT_TOLERANCE);
    let amount = amount + credited;
    // With so few payments to go on, a yearly bill must at least cost the same
    if rule.cadence == Cadence::Yearly
        && amounts.iter().any(|a| (a - median_amount).abs() > median_amount.abs() * YEARLY_AMOUNT_TOLERANCE)
    {
        return None;
    }
    if amount.abs() < MIN_STREAM_AMOUNT {
        return None;
    }
    // Money in and money out from one merchant do not make a bill
    if amounts.iter().any(|a| a.signum() != amounts[0].signum()) {
        return None;
    }

    // The latest payments say where the billing day is now, should it have moved
    let mut days: Vec<i64> = sample.iter().map(|p| p.0.day() as i64).collect();
    Some(Stream {
        account_id: account_id.to_string(),
        merchant: merchant.to_string(),
        group_type: String::new(),
        cadence: rule.cadence,
        amount,
        variable,
        last_date,
        payments: payments.len(),
        recent: recent.to_vec(),
        gap_days: typical_gap,
        day_of_month: median_i64(&mut days) as u32,
    })
}

/// Whether the recent payments all lie within `tolerance` of their median
fn is_uniform(payments: &[Payment], tolerance: f64) -> bool {
    let recent = &payments[payments.len().saturating_sub(RECENT_PAYMENTS)..];
    let mut amounts: Vec<f64> = recent.iter().map(|p| p.1).collect();
    if amounts.is_empty() {
        return false;
    }
    let median = median_f64(&mut amounts);
    amounts.iter().all(|a| (a - median).abs() <= median.abs() * tolerance)
}

/// Collapse payments to one per day, oldest first, summing same-day amounts
fn by_day(transactions: &[&Txn]) -> Vec<Payment> {
    let mut days: BTreeMap<NaiveDate, (f64, f64)> = BTreeMap::new();
    for txn in transactions {
        let day = days.entry(txn.date).or_default();
        day.0 += txn.amount;
        day.1 += txn.credited;
    }
    days.into_iter().map(|(date, (amount, credited))| (date, amount, credited)).collect()
}

/// Find every recurring stream in the transaction history
pub fn detect(transactions: &[Txn], today: NaiveDate) -> Vec<Stream> {
    let earliest = today - Duration::days(LOOKBACK_DAYS);
    let mut by_merchant: HashMap<(&str, &str), Vec<&Txn>> = HashMap::new();
    // Matched statement credits are left out: they are accounted for through
    // the purchase they refund, not as a stream of income of their own. Nor
    // are a hidden account's: a 401k contribution is not a bill or a paycheck
    for txn in transactions.iter().filter(|t| !t.is_matched_credit && !t.hidden_by_account) {
        if txn.date >= earliest && txn.amount != 0.0 && !txn.merchant.is_empty() {
            by_merchant.entry((txn.account_id.as_str(), txn.merchant.as_str())).or_default().push(txn);
        }
    }

    let mut streams = Vec::new();
    for ((account_id, merchant), txns) in by_merchant {
        let group_type = txns.iter().max_by_key(|t| t.date).map(|t| t.group_type.as_str()).unwrap_or("");
        let found_before = streams.len();

        // A merchant that is steady as a whole and charges roughly the same
        // each time is a single bill. Splitting it by amount would let two
        // payments that happen to match a year apart pose as a yearly bill and
        // punch holes in the real monthly one.
        let all = by_day(&txns);
        if let Some(stream) = classify(account_id, merchant, &all, today, is_uniform(&all, FIXED_AMOUNT_TOLERANCE)) {
            if is_uniform(&all, ONE_BILL_AMOUNT_SPREAD) {
                streams.push(stream);
                streams[found_before].group_type = group_type.to_string();
                continue;
            }
        }

        // Pass 1: identical amounts, keyed in whole cents
        let mut by_amount: HashMap<i64, Vec<&Txn>> = HashMap::new();
        for txn in &txns {
            by_amount.entry((txn.amount * 100.0).round() as i64).or_default().push(txn);
        }
        let mut leftover: Vec<&Txn> = Vec::new();
        for same_amount in by_amount.into_values() {
            match classify(account_id, merchant, &by_day(&same_amount), today, true) {
                Some(stream) if !stream.superseded_by(&txns) => streams.push(stream),
                _ => leftover.extend(same_amount),
            }
        }

        // Pass 2: whatever is left of the merchant, amount free to vary
        if let Some(stream) = classify(account_id, merchant, &by_day(&leftover), today, false) {
            streams.push(stream);
        }
        for stream in &mut streams[found_before..] {
            stream.group_type = group_type.to_string();
        }
    }
    drop_reimbursed(&mut streams, transactions);
    streams.sort_by(|a, b| a.merchant.cmp(&b.merchant).then(a.last_date.cmp(&b.last_date)));
    streams
}

/// Whether two merchant names refer to the same thing: identical, or one
/// contained in the other ("Clear" in "Amex Clear Plus Credit")
fn names_related(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || (short.len() >= REIMBURSEMENT_NAME_MIN && long.contains(&short))
}

/// Whether a payment was offset: by statement credits matched to it, or by an
/// opposite entry from a related merchant in the same account
fn is_offset(stream: &Stream, payment: &Payment, account_transactions: &[&Txn]) -> bool {
    let (date, amount, credited) = *payment;
    if credited >= amount.abs() * REIMBURSEMENT_MIN_SHARE {
        return true;
    }
    account_transactions.iter().any(|txn| {
        let days = (txn.date - date).num_days();
        txn.amount.signum() == -amount.signum()
            && (-REIMBURSEMENT_DAYS_BEFORE..=REIMBURSEMENT_DAYS_AFTER).contains(&days)
            && txn.amount.abs() >= amount.abs() * REIMBURSEMENT_MIN_SHARE
            && txn.amount.abs() <= amount.abs() * (1.0 + FIXED_AMOUNT_TOLERANCE)
            // Card credits are often filed under the card issuer, with only the
            // statement text naming what they are for
            && (names_related(&stream.merchant, &txn.merchant) || names_related(&stream.merchant, &txn.original_name))
    })
}

/// Remove charges that are routinely refunded, such as a membership a card
/// credits back each time, along with the matching stream of credits. Charge
/// and credit cancel out, so neither is a bill to plan for. The credit may
/// arrive under a related merchant name and need not form a stream itself.
fn drop_reimbursed(streams: &mut Vec<Stream>, transactions: &[Txn]) {
    let mut by_account: HashMap<&str, Vec<&Txn>> = HashMap::new();
    for txn in transactions {
        by_account.entry(txn.account_id.as_str()).or_default().push(txn);
    }
    streams.retain(|stream| {
        let Some(account_transactions) = by_account.get(stream.account_id.as_str()) else {
            return true;
        };
        let offset = stream
            .recent
            .iter()
            .filter(|payment| is_offset(stream, payment, account_transactions))
            .count();
        (offset as f64) < stream.recent.len() as f64 * REIMBURSED_STREAM_SHARE
    });
}

/// Last day of the month containing `date`
pub fn month_end(date: NaiveDate) -> NaiveDate {
    let first = date.with_day(1).unwrap_or(date);
    first
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .unwrap_or(date)
}

/// Expected payments for one account through `through`, soonest first
pub fn upcoming_for_account(streams: &[Stream], account_id: &str, today: NaiveDate, through: NaiveDate) -> Vec<Upcoming> {
    let mut upcoming: Vec<Upcoming> = streams
        .iter()
        .filter(|s| s.account_id == account_id)
        .flat_map(|s| s.upcoming(today, through))
        .collect();
    upcoming.sort_by(|a, b| a.date.cmp(&b.date).then(a.merchant.cmp(&b.merchant)));
    upcoming
}

/// Expected payments across every account through `through`, soonest first
pub fn upcoming_all(streams: &[Stream], today: NaiveDate, through: NaiveDate) -> Vec<Upcoming> {
    let mut upcoming: Vec<Upcoming> = streams.iter().flat_map(|s| s.upcoming(today, through)).collect();
    upcoming.sort_by(|a, b| a.date.cmp(&b.date).then(a.merchant.cmp(&b.merchant)));
    upcoming
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn txn(date: NaiveDate, merchant: &str, amount: f64) -> Txn {
        Txn {
            id: format!("{merchant}-{amount}"),
            date,
            amount,
            credited: 0.0,
            is_matched_credit: false,
            merchant: merchant.to_string(),
            category: String::new(),
            group_type: "expense".to_string(),
            account_id: "checking".to_string(),
            notes: String::new(),
            original_name: String::new(),
            tags: String::new(),
            pending: false,
            recurring: false,
            needs_review: false,
            hide_from_reports: false,
            hidden_by_account: false,
            one_time: false,
        }
    }

    /// One payment on `day` of each month from `first` for `count` months
    fn monthly(merchant: &str, first: NaiveDate, count: u32, amount: impl Fn(u32) -> f64) -> Vec<Txn> {
        (0..count)
            .map(|i| txn(first.checked_add_months(Months::new(i)).unwrap(), merchant, amount(i)))
            .collect()
    }

    #[test]
    fn fixed_monthly_bill_is_due_on_its_usual_day() {
        let today = date(2026, 10, 11);
        let history = monthly("Internet", date(2026, 4, 28), 6, |_| -70.0);
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].cadence, Cadence::Monthly);
        assert!(!streams[0].variable);

        let upcoming = upcoming_for_account(&streams, "checking", today, month_end(today));
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].date, date(2026, 10, 28));
        assert_eq!(upcoming[0].amount, -70.0);
        assert!(!upcoming[0].late);
    }

    #[test]
    fn hidden_account_activity_is_not_a_bill() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Contribution", date(2026, 4, 28), 6, |_| 1050.0);
        for txn in &mut history {
            txn.hide_from_reports = true;
            txn.hidden_by_account = true;
        }
        assert!(detect(&history, today).is_empty());
    }

    #[test]
    fn paid_bill_is_not_listed_again_this_month() {
        let today = date(2026, 10, 11);
        let history = monthly("Mortgage", date(2026, 5, 9), 6, |_| -2400.0);
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert!(upcoming_for_account(&streams, "checking", today, month_end(today)).is_empty());
    }

    #[test]
    fn variable_utility_bill_is_estimated_from_recent_payments() {
        let today = date(2026, 10, 11);
        let history = monthly("Power", date(2026, 3, 20), 7, |i| -(100.0 + 10.0 * i as f64));
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert!(streams[0].variable);
        // Mean of the last three payments: 140, 150, 160
        assert!((streams[0].amount + 150.0).abs() < 1e-9);
    }

    #[test]
    fn price_change_yields_one_stream_not_a_late_one_and_a_new_one() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Streaming", date(2026, 3, 8), 7, |_| -15.0);
        history.push(txn(date(2026, 10, 8), "Streaming", -18.0));
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].last_date, date(2026, 10, 8));
        assert!(upcoming_for_account(&streams, "checking", today, month_end(today)).is_empty());
    }

    #[test]
    fn two_subscriptions_from_one_merchant_are_separate_streams() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Apple", date(2026, 4, 3), 7, |_| -9.99);
        history.extend(monthly("Apple", date(2026, 4, 19), 6, |_| -2.99));
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 2);
        let upcoming = upcoming_for_account(&streams, "checking", today, month_end(today));
        assert_eq!(upcoming.len(), 1);
        assert_eq!(upcoming[0].date, date(2026, 10, 19));
    }

    #[test]
    fn matching_amounts_a_year_apart_do_not_split_a_monthly_bill() {
        let today = date(2026, 10, 11);
        // A water bill that varies month to month but happened to be the same
        // in August of both years
        let history = monthly("Water", date(2025, 6, 5), 17, |i| if i == 2 || i == 14 { -66.8 } else { -(60.0 + (i % 5) as f64 * 3.0) });
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].cadence, Cadence::Monthly);
        assert_eq!(streams[0].last_date, date(2026, 10, 5));
    }

    #[test]
    fn charge_refunded_every_month_is_hidden_but_a_plain_charge_is_kept() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Rides", date(2026, 3, 22), 7, |_| -9.99);
        history.extend(monthly("Rides", date(2026, 3, 22), 7, |_| 9.99));
        history.extend(monthly("Music", date(2026, 3, 14), 7, |_| -10.99));
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].merchant, "Music");
    }

    #[test]
    fn yearly_charge_credited_back_under_a_related_name_is_hidden() {
        let today = date(2026, 10, 11);
        let history = vec![
            txn(date(2025, 5, 4), "Clear", -199.0),
            txn(date(2025, 5, 5), "Amex Clear Plus Credit", 199.0),
            txn(date(2026, 5, 3), "Clear", -209.0),
        ];
        assert!(detect(&history, today).is_empty());

        // The credit can be filed under the card issuer, named only in its statement text
        let mut by_statement = vec![txn(date(2025, 5, 4), "Clear", -199.0), txn(date(2026, 5, 3), "Clear", -209.0)];
        for (day, amount) in [(date(2025, 5, 5), 199.0), (date(2026, 5, 3), 209.0)] {
            let mut credit = txn(day, "Card Issuer", amount);
            credit.original_name = "AMEX CLEAR PLUS CREDIT".to_string();
            by_statement.push(credit);
        }
        assert!(detect(&by_statement, today).iter().all(|stream| stream.merchant != "Clear"));

        // An unrelated refund of the same size does not count
        let unrelated = vec![
            txn(date(2025, 5, 4), "Clear", -199.0),
            txn(date(2025, 5, 5), "Shoe Store", 199.0),
            txn(date(2026, 5, 3), "Clear", -209.0),
        ];
        assert_eq!(detect(&unrelated, today).len(), 1);
    }

    #[test]
    fn one_refunded_month_does_not_hide_a_subscription() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Music", date(2026, 3, 14), 7, |_| -10.99);
        history.push(txn(date(2026, 6, 16), "Music", 10.99));
        assert_eq!(detect(&history, today).len(), 1);
    }

    #[test]
    fn matched_credits_shrink_a_bill_and_never_form_their_own_stream() {
        let today = date(2026, 10, 11);
        let mut history = monthly("TV Service", date(2026, 3, 18), 7, |_| -65.0);
        for charge in &mut history {
            charge.credited = 25.0;
        }
        let mut credits = monthly("Card Entertainment Credit", date(2026, 3, 18), 7, |_| 25.0);
        for credit in &mut credits {
            credit.is_matched_credit = true;
        }
        history.extend(credits);

        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].merchant, "TV Service");
        assert!((streams[0].amount + 40.0).abs() < 1e-9);
    }

    #[test]
    fn fully_credited_charge_is_not_a_bill() {
        let today = date(2026, 10, 11);
        let mut history = monthly("Membership", date(2026, 3, 22), 7, |_| -9.99);
        for charge in &mut history {
            charge.credited = 9.99;
        }
        assert!(detect(&history, today).is_empty());
    }

    #[test]
    fn subscription_resumed_after_a_pause_is_still_a_stream() {
        let today = date(2026, 10, 11);
        let mut history = monthly("TV Service", date(2025, 12, 20), 7, |_| -65.0);
        history.push(txn(date(2026, 9, 20), "TV Service", -65.0));
        let streams = detect(&history, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].next_expected(today).0, date(2026, 10, 20));
    }

    #[test]
    fn cancelled_subscription_is_dropped() {
        let today = date(2026, 10, 11);
        let history = monthly("Gym", date(2026, 1, 5), 6, |_| -40.0);
        assert!(detect(&history, today).is_empty());
    }

    #[test]
    fn irregular_shopping_is_not_a_stream() {
        let today = date(2026, 10, 11);
        let days = [1, 4, 5, 19, 23, 40, 41, 66, 70, 71, 95];
        let history: Vec<Txn> = days
            .iter()
            .map(|d| txn(date(2026, 6, 1) + Duration::days(*d), "Grocer", -(20.0 + *d as f64)))
            .collect();
        assert!(detect(&history, today).is_empty());
    }

    #[test]
    fn weekly_habit_of_varying_amounts_is_not_a_bill_but_a_fixed_transfer_is() {
        let today = date(2026, 10, 11);
        let weeks = |merchant: &str, amount: fn(i64) -> f64| -> Vec<Txn> {
            (0..10).map(|i| txn(date(2026, 8, 3) + Duration::days(7 * i), merchant, amount(i))).collect()
        };
        assert!(detect(&weeks("Coffee", |i| -(4.0 + i as f64)), today).is_empty());

        let streams = detect(&weeks("Brokerage", |_| -500.0), today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].cadence, Cadence::Weekly);
        let upcoming = upcoming_for_account(&streams, "checking", today, month_end(today));
        let dates: Vec<NaiveDate> = upcoming.iter().map(|u| u.date).collect();
        assert_eq!(dates, vec![date(2026, 10, 12), date(2026, 10, 19), date(2026, 10, 26)]);
    }

    #[test]
    fn overdue_payment_is_flagged_late_until_its_grace_runs_out() {
        let history = monthly("Water", date(2026, 4, 6), 6, |_| -30.0);
        let streams = detect(&history, date(2026, 10, 11));
        let upcoming = upcoming_for_account(&streams, "checking", date(2026, 10, 11), date(2026, 10, 31));
        assert_eq!(upcoming.len(), 1);
        assert!(upcoming[0].late);
    }

    #[test]
    fn yearly_bill_needs_matching_amounts() {
        let today = date(2026, 10, 11);
        let same = vec![txn(date(2024, 12, 8), "Vault", -36.0), txn(date(2025, 12, 8), "Vault", -36.0)];
        let streams = detect(&same, today);
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].cadence, Cadence::Yearly);

        let different = vec![txn(date(2024, 10, 21), "Costumes", -150.0), txn(date(2025, 10, 21), "Costumes", -60.0)];
        assert!(detect(&different, today).is_empty());
    }
}
