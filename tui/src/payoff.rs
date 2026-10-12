//! Loan payoff projection: when a loan reaches zero at its current payment

use chrono::{Months, NaiveDate};

/// Projections running longer than this are not worth making
const MAX_MONTHS: u32 = 50 * 12;

/// Months of history used to infer the payment when Monarch does not give one
const HISTORY_MONTHS: u32 = 6;

const MONTHS_PER_YEAR: f64 = 12.0;
const PERCENT: f64 = 100.0;

/// Balances below this are treated as paid off
const PAID_OFF: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub payoff: NaiveDate,
    pub monthly_payment: f64,
    /// Whether the payment came from the lender or was inferred from history
    pub payment_inferred: bool,
    /// Balance on today's date and then monthly until it reaches zero
    pub points: Vec<(NaiveDate, f64)>,
}

/// Average monthly drop in the balance over recent history, plus the interest
/// that accrued meanwhile: an estimate of what is being paid each month
fn inferred_payment(balance: f64, monthly_rate: f64, history: &[(NaiveDate, f64)], today: NaiveDate) -> Option<f64> {
    let since = today.checked_sub_months(Months::new(HISTORY_MONTHS))?;
    let (start_date, start_balance) = history.iter().find(|(date, _)| *date >= since)?;
    let months = (today - *start_date).num_days() as f64 / (365.25 / MONTHS_PER_YEAR);
    if months < 1.0 {
        return None;
    }
    let principal_per_month = (start_balance - balance) / months;
    (principal_per_month > 0.0).then(|| principal_per_month + (start_balance + balance) / 2.0 * monthly_rate)
}

/// Project a loan to zero. `balance` and the `history` balances are amounts
/// owed (positive). `annual_rate_percent` and `payment` come from the lender
/// when known; a missing rate is taken as zero and a missing payment is
/// inferred from how fast the balance has been falling.
pub fn project(
    balance: f64,
    annual_rate_percent: Option<f64>,
    payment: Option<f64>,
    history: &[(NaiveDate, f64)],
    today: NaiveDate,
) -> Option<Projection> {
    if balance <= PAID_OFF {
        return None;
    }
    let monthly_rate = annual_rate_percent.unwrap_or(0.0) / PERCENT / MONTHS_PER_YEAR;
    let stated = payment.filter(|p| *p > 0.0);
    let monthly_payment = match stated {
        Some(p) => p,
        None => inferred_payment(balance, monthly_rate, history, today)?,
    };
    // A payment that does not cover the interest never pays the loan off
    if monthly_payment <= balance * monthly_rate {
        return None;
    }

    let mut points = vec![(today, balance)];
    let mut remaining = balance;
    for month in 1..=MAX_MONTHS {
        remaining = (remaining * (1.0 + monthly_rate) - monthly_payment).max(0.0);
        let date = today.checked_add_months(Months::new(month))?;
        points.push((date, remaining));
        if remaining <= PAID_OFF {
            return Some(Projection {
                payoff: date,
                monthly_payment,
                payment_inferred: stated.is_none(),
                points,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn interest_free_loan_pays_off_in_balance_over_payment_months() {
        let projection = project(1200.0, None, Some(100.0), &[], date(2026, 1, 15)).unwrap();
        assert_eq!(projection.payoff, date(2027, 1, 15));
        assert_eq!(projection.points.len(), 13);
        assert!(!projection.payment_inferred);
    }

    #[test]
    fn interest_lengthens_the_payoff() {
        let today = date(2026, 10, 11);
        let without = project(13_348.0, None, Some(904.0), &[], today).unwrap();
        let with = project(13_348.0, Some(24.0), Some(904.0), &[], today).unwrap();
        assert_eq!(without.payoff, date(2028, 1, 11));
        assert!(with.payoff > without.payoff);
    }

    #[test]
    fn payment_is_inferred_from_a_falling_balance() {
        let today = date(2026, 10, 1);
        // Falling 500 a month for six months
        let history: Vec<(NaiveDate, f64)> =
            (0..=6).map(|i| (date(2026, 4 + i, 1), 6000.0 - 500.0 * i as f64)).collect();
        let projection = project(3000.0, None, None, &history, today).unwrap();
        assert!(projection.payment_inferred);
        assert!((projection.monthly_payment - 500.0).abs() < 5.0);
        // Six months at about 500, give or take the last partial payment
        assert!((date(2027, 4, 1)..=date(2027, 5, 1)).contains(&projection.payoff));
    }

    #[test]
    fn no_projection_without_progress_or_when_interest_outruns_the_payment() {
        let today = date(2026, 10, 1);
        let flat = vec![(date(2026, 5, 1), 5000.0), (date(2026, 9, 1), 5000.0)];
        assert_eq!(project(5000.0, None, None, &flat, today), None);
        assert_eq!(project(100_000.0, Some(12.0), Some(500.0), &[], today), None);
        assert_eq!(project(0.0, None, Some(100.0), &[], today), None);
    }
}
