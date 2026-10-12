//! Invents private company holdings to stand in for the real ones.
//!
//! Nothing of the real holdings is kept but their rough size. How many grants
//! there are, every date, strike and share count, the exercises and the tender
//! sale are all made up, along the lines such holdings usually follow: a
//! new-hire grant with a cliff, refreshers every year or so, and a share price
//! the company revalues yearly. What is made up is consistent with itself
//! (tranches add up to their grant, exercises to the shares held), so the
//! Equity screen adds up the way it does for real records.

use super::{hash, unit, CENTS_PER_DOLLAR, SCALE_MAX, SCALE_MIN};
use crate::equity::{Certificate, Equity, Exercise, Grant, Issuer, Snapshot, Tender, VestEvent};
use chrono::{Datelike, Duration, Months, NaiveDate};

/// How many grants are invented
const GRANTS_MIN: i64 = 3;
const GRANTS_MAX: i64 = 6;

/// Months from one grant's vesting start to the next one's, and from the
/// latest one's to today
const MONTHS_BETWEEN_GRANTS: (i64, i64) = (9, 16);
const MONTHS_SINCE_LATEST_GRANT: (i64, i64) = (2, 10);
/// Vesting starts on a day every month has
const VESTING_DAY: (i64, i64) = (1, 28);
/// Days from the start of vesting to the board approving the grant
const APPROVAL_DAYS: (i64, i64) = (10, 45);

/// A grant vests monthly over four years, the first after a year's cliff
const VESTING_MONTHS: u32 = 48;
const CLIFF_MONTHS: u32 = 12;
/// Options lapse ten years after they are granted
const EXPIRY_MONTHS: u32 = 120;

/// Size of a refresher grant next to the new-hire one
const REFRESHER_SIZE: (f64, f64) = (0.15, 0.5);
/// Chance the new-hire grant is over the yearly ISO limit, and the share of
/// each of its tranches that is then still ISO
const SPLIT_CHANCE: f64 = 0.5;
const SPLIT_ISO_SHARE: (f64, f64) = (0.3, 0.7);
/// Chance a refresher is granted as ISOs rather than NSOs
const REFRESHER_ISO_CHANCE: f64 = 0.55;

/// Share price when the first grant was made, and how much each yearly
/// revaluation multiplies it by
const FIRST_PRICE: (f64, f64) = (0.25, 4.0);
const REVALUATION: (f64, f64) = (0.9, 2.2);
/// Days before the first grant that its price was set
const FIRST_PRICE_DAYS_BEFORE: (i64, i64) = (30, 300);
const MONTHS_BETWEEN_REVALUATIONS: u32 = 12;
/// Days a revaluation falls either side of the anniversary
const REVALUATION_DRIFT_DAYS: i64 = 25;
const LEAST_PRICE: f64 = 0.01;

/// Exercises of the new-hire grant, and the chance of one from the next grant
const FIRST_GRANT_EXERCISES: (i64, i64) = (1, 2);
const SECOND_GRANT_EXERCISE_CHANCE: f64 = 0.4;
/// Share of what could be exercised on the day that is
const EXERCISED_SHARE: (f64, f64) = (0.2, 0.6);
/// An exercise comes at least this long after the first tranche vests, and
/// this long before today
const EXERCISE_DAYS_AFTER_VESTING: i64 = 30;
const EXERCISE_DAYS_AGO: i64 = 20;
/// Tax withheld on exercising NSOs, as a share of the spread
const WITHHOLDING_RATE: f64 = 0.3;

/// Chance some of the first shares were sold in a tender offer, and how many
const TENDER_CHANCE: f64 = 0.65;
const TENDER_SOLD_SHARE: (f64, f64) = (0.2, 0.5);
/// What a tender pays over the share price of the day
const TENDER_PREMIUM: (f64, f64) = (1.0, 1.4);
/// A tender settles at least this long after the shares were bought, and
/// this long before today
const TENDER_DAYS_AFTER_EXERCISE: i64 = 100;
const TENDER_DAYS_AGO: i64 = 14;
/// Days before settlement that the offer opened and closed
const TENDER_OPEN_DAYS: (i64, i64) = (35, 50);
const TENDER_CLOSE_DAYS: (i64, i64) = (10, 20);

/// Where the company's grant and certificate numbers stand at the first of
/// each, and how far they advance between one and the next
const FIRST_GRANT_NUMBER: (i64, i64) = (20, 400);
const GRANT_NUMBER_STEP: (i64, i64) = (30, 400);
const FIRST_CERTIFICATE_NUMBER: (i64, i64) = (3, 60);
const CERTIFICATE_NUMBER_STEP: (i64, i64) = (1, 40);

/// The first grant's size while the holdings are being valued, large enough
/// that rounding to whole options does not show
const PROBE_GRANT: f64 = 1_000_000.0;
/// The first grant's size when the real holdings give nothing to go by
const DEFAULT_GRANT: f64 = 40_000.0;
const LARGEST_GRANT: f64 = 5_000_000.0;

/// Grant sizes and share counts are round numbers, to this many digits
const SIZE_DIGITS: i32 = 2;

const ISSUER_ID: i64 = 1;
const COMPANY_NAME: &str = "Private Company 1";
const STOCK_TYPE: &str = "Common";
const ISO: &str = "ISO";
const NSO: &str = "NSO";
const SPLIT: &str = "ISO/NSO Split";
/// Carta's words for the states a grant, a certificate and a tender can be in
const GRANT_OUTSTANDING: &str = "OUTSTANDING";
const GRANT_PARTIALLY_EXERCISED: &str = "PARTIALLY_EXERCISED";
const GRANT_FULLY_EXERCISED: &str = "FULLY_EXERCISED";
const NO_83B: &str = "NONE_ELIGIBLE";
const HELD: &str = "Outstanding";
const CUT: &str = "Transferred";
const SOLD: &str = "Secondary";
const TENDER_SETTLED: &str = "Closed & Settled";
const TENDER_KIND: &str = "secondary_sale";
const PAYMENT: &str = "ACH";

const CLIFF_SCHEDULE: &str = "1/48 monthly, 1 year cliff";
const CLIFF_SUMMARY: &str = "Yes, 25% vests at 1 year";
const CLIFF_TERMS: &str = "1/4th of the shares vest on the 12-month anniversary of the vesting start date, and 1/48th \
                           on the same day of each month after.";
const MONTHLY_SCHEDULE: &str = "1/48 monthly, no cliff";
const MONTHLY_TERMS: &str = "1/48th of the shares vest on the same day of each month after the vesting start date.";

/// A repeatable run of random draws
struct Dice {
    seed: u64,
    drawn: u64,
}

impl Dice {
    fn unit(&mut self) -> f64 {
        self.drawn += 1;
        unit(hash(self.seed, &self.drawn.to_string()))
    }

    fn between(&mut self, (low, high): (f64, f64)) -> f64 {
        low + self.unit() * (high - low)
    }

    /// A whole number from `low` to `high`, both included
    fn whole(&mut self, (low, high): (i64, i64)) -> i64 {
        low + (self.unit() * (high - low + 1) as f64) as i64
    }

    fn chance(&mut self, probability: f64) -> bool {
        self.unit() < probability
    }
}

/// One grant as planned: when it was made, how it vests, and how large it is
/// next to the first
struct GrantPlan {
    label: String,
    vesting_start: NaiveDate,
    issued: NaiveDate,
    cliff: bool,
    size: f64,
    /// Share of each tranche that is ISO: all, none, or part for a split grant
    iso_share: f64,
    strike: f64,
}

impl GrantPlan {
    fn option_type(&self) -> &'static str {
        if self.iso_share >= 1.0 {
            ISO
        } else if self.iso_share <= 0.0 {
            NSO
        } else {
            SPLIT
        }
    }

    /// Months from the start of vesting to the first tranche
    fn first_month(&self) -> u32 {
        if self.cliff {
            CLIFF_MONTHS
        } else {
            1
        }
    }

    fn vesting_date(&self, month: u32) -> NaiveDate {
        self.vesting_start.checked_add_months(Months::new(month)).unwrap_or(self.vesting_start)
    }

    fn first_vesting(&self) -> NaiveDate {
        self.vesting_date(self.first_month())
    }

    /// The tranches of a grant of `quantity` options, oldest first, as (date,
    /// options, of which ISO). Each is a whole number and they add up exactly.
    fn tranches(&self, quantity: f64) -> Vec<(NaiveDate, f64, f64)> {
        let (mut vested, mut vested_iso) = (0.0, 0.0);
        (self.first_month()..=VESTING_MONTHS)
            .map(|month| {
                let date = self.vesting_date(month);
                let by_now = (quantity * month as f64 / VESTING_MONTHS as f64).round();
                let iso_by_now = (by_now * self.iso_share).round();
                let tranche = (date, by_now - vested, iso_by_now - vested_iso);
                (vested, vested_iso) = (by_now, iso_by_now);
                tranche
            })
            .collect()
    }
}

struct ExercisePlan {
    /// Index of the grant exercised
    grant: usize,
    date: NaiveDate,
    /// Share of what could be exercised that day
    share: f64,
}

struct TenderPlan {
    /// Where the settlement falls between the earliest it could be and the latest
    timing: f64,
    sold_share: f64,
    premium: f64,
    open_days: i64,
    close_days: i64,
}

/// Everything about the invented holdings but their size
struct Plan {
    grants: Vec<GrantPlan>,
    /// The share price from each revaluation on, oldest first
    prices: Vec<(NaiveDate, f64)>,
    /// Oldest first
    exercises: Vec<ExercisePlan>,
    tender: Option<TenderPlan>,
    /// Numbers for the share certificates, in the order they are issued
    certificate_numbers: Vec<i64>,
}

fn cents(amount: f64) -> f64 {
    (amount * CENTS_PER_DOLLAR).round() / CENTS_PER_DOLLAR
}

/// `quantity` as the round number a grant would be made for
fn round_size(quantity: f64) -> f64 {
    if quantity < 1.0 {
        return 0.0;
    }
    let unit = 10f64.powi(quantity.log10().floor() as i32 + 1 - SIZE_DIGITS).max(1.0);
    (quantity / unit).round() * unit
}

/// The share price on `date`, given each revaluation oldest first
fn price_on(prices: &[(NaiveDate, f64)], date: NaiveDate) -> f64 {
    let first = prices.first().map(|price| price.1).unwrap_or_default();
    prices.iter().rev().find(|price| price.0 <= date).map(|price| price.1).unwrap_or(first)
}

/// The day a share of the way from `from` to `to`
fn between_days(from: NaiveDate, to: NaiveDate, share: f64) -> NaiveDate {
    from + Duration::days((share * ((to - from).num_days() + 1) as f64) as i64)
}

impl Plan {
    /// Draw a plan with `grants` grants, as of `today`
    fn draw(dice: &mut Dice, grants: i64, today: NaiveDate) -> Plan {
        // Work back from the latest grant to the first
        let mut months_ago = vec![dice.whole(MONTHS_SINCE_LATEST_GRANT)];
        for _ in 1..grants {
            let later = months_ago.last().copied().unwrap_or_default();
            months_ago.push(later + dice.whole(MONTHS_BETWEEN_GRANTS));
        }
        months_ago.reverse();
        let vesting_day = dice.whole(VESTING_DAY) as u32;
        let starts: Vec<NaiveDate> = months_ago
            .iter()
            .map(|months| {
                let month = today.checked_sub_months(Months::new(*months as u32)).unwrap_or(today);
                month.with_day(vesting_day).unwrap_or(month)
            })
            .collect();

        let mut prices = vec![(
            starts[0] - Duration::days(dice.whole(FIRST_PRICE_DAYS_BEFORE)),
            cents(dice.between(FIRST_PRICE)),
        )];
        loop {
            let (last_date, last_price) = prices[prices.len() - 1];
            let drift = Duration::days(dice.whole((-REVALUATION_DRIFT_DAYS, REVALUATION_DRIFT_DAYS)));
            let change = dice.between(REVALUATION);
            let Some(date) = last_date.checked_add_months(Months::new(MONTHS_BETWEEN_REVALUATIONS)) else {
                break;
            };
            if date + drift > today {
                break;
            }
            let mut price = cents(last_price * change).max(LEAST_PRICE);
            // A revaluation that changed nothing would not show as one
            if price == last_price {
                price += LEAST_PRICE;
            }
            prices.push((date + drift, price));
        }
        let mut number = dice.whole(FIRST_GRANT_NUMBER);
        let mut plans = Vec::new();
        for (index, vesting_start) in starts.iter().enumerate() {
            let issued = (*vesting_start + Duration::days(dice.whole(APPROVAL_DAYS))).min(today);
            let (split, split_share) = (dice.chance(SPLIT_CHANCE), dice.between(SPLIT_ISO_SHARE));
            let (size, as_iso) = (dice.between(REFRESHER_SIZE), dice.chance(REFRESHER_ISO_CHANCE));
            let first = index == 0;
            plans.push(GrantPlan {
                label: format!("ES-{}", number),
                vesting_start: *vesting_start,
                issued,
                cliff: first,
                size: if first { 1.0 } else { size },
                iso_share: match (first, split, as_iso) {
                    (true, true, _) => split_share,
                    (true, false, _) | (false, _, true) => 1.0,
                    (false, _, false) => 0.0,
                },
                strike: price_on(&prices, issued),
            });
            number += dice.whole(GRANT_NUMBER_STEP);
        }

        let mut exercises = Vec::new();
        let counts = [dice.whole(FIRST_GRANT_EXERCISES), i64::from(dice.chance(SECOND_GRANT_EXERCISE_CHANCE))];
        for (grant, count) in counts.into_iter().enumerate() {
            for _ in 0..count {
                let (timing, share) = (dice.unit(), dice.between(EXERCISED_SHARE));
                let earliest = plans[grant].first_vesting() + Duration::days(EXERCISE_DAYS_AFTER_VESTING);
                let latest = today - Duration::days(EXERCISE_DAYS_AGO);
                if earliest <= latest {
                    exercises.push(ExercisePlan { grant, date: between_days(earliest, latest, timing), share });
                }
            }
        }
        exercises.sort_by_key(|exercise| exercise.date);

        let tender = TenderPlan {
            timing: dice.unit(),
            sold_share: dice.between(TENDER_SOLD_SHARE),
            premium: dice.between(TENDER_PREMIUM),
            open_days: dice.whole(TENDER_OPEN_DAYS),
            close_days: dice.whole(TENDER_CLOSE_DAYS),
        };
        let tender = dice.chance(TENDER_CHANCE).then_some(tender);

        // One certificate for each exercise, and two for the one a tender cuts
        let mut number = dice.whole(FIRST_CERTIFICATE_NUMBER);
        let certificate_numbers = (0..exercises.len() + 2)
            .map(|_| {
                number += dice.whole(CERTIFICATE_NUMBER_STEP);
                number
            })
            .collect();

        Plan { grants: plans, prices, exercises, tender, certificate_numbers }
    }

    /// The holdings this plan comes to when its first grant is for `first_grant` options
    fn holdings(&self, first_grant: f64, today: NaiveDate) -> Equity {
        let mut equity = Equity::default();
        let quantities: Vec<f64> =
            self.grants.iter().map(|plan| round_size(first_grant * plan.size).max(VESTING_MONTHS as f64)).collect();
        let tranches: Vec<Vec<(NaiveDate, f64, f64)>> =
            self.grants.iter().zip(&quantities).map(|(plan, quantity)| plan.tranches(*quantity)).collect();

        // Exercises, oldest first, each drawing on what its grant had vested by then
        let mut exercised = vec![0.0; self.grants.len()];
        // The certificate each exercise bought, by its place among the certificates
        let mut bought: Vec<usize> = Vec::new();
        for planned in &self.exercises {
            let plan = &self.grants[planned.grant];
            let as_iso = plan.iso_share > 0.0;
            // A split grant's ISOs are exercised first: they are the ones worth holding
            let vested: f64 = tranches[planned.grant]
                .iter()
                .filter(|tranche| tranche.0 <= planned.date)
                .map(|tranche| if as_iso { tranche.2 } else { tranche.1 })
                .sum();
            let available = vested - exercised[planned.grant];
            let quantity = round_size(available * planned.share).min(available);
            if quantity < 1.0 {
                continue;
            }
            exercised[planned.grant] += quantity;
            let (cost, fmv) = (cents(quantity * plan.strike), price_on(&self.prices, planned.date));
            let taxes = if as_iso { 0.0 } else { cents((fmv - plan.strike).max(0.0) * quantity * WITHHOLDING_RATE) };
            let option_type = if as_iso { ISO } else { NSO };
            bought.push(equity.certificates.len());
            equity.certificates.push(Certificate {
                issuer_id: ISSUER_ID,
                share_class: STOCK_TYPE.to_string(),
                certificated: Some(false),
                issued: Some(planned.date),
                acquired: Some(planned.date),
                fmv_at_issue: Some(fmv),
                quantity,
                cost: Some(cost),
                status: HELD.to_string(),
                exercise_type: option_type.to_string(),
                exercised_from: plan.label.clone(),
                ..Certificate::default()
            });
            equity.exercises.push(Exercise {
                issuer_id: ISSUER_ID,
                grant_label: plan.label.clone(),
                option_type: option_type.to_string(),
                date: Some(planned.date),
                quantity,
                cost,
                taxes,
                payment: PAYMENT.to_string(),
                fmv: Some(fmv),
                ..Exercise::default()
            });
        }

        // A tender buys part of the first shares: Carta cancels their
        // certificate and issues one for what was sold and one for the rest
        if let (Some(tender), Some(first)) = (&self.tender, equity.certificates.first().cloned()) {
            let acquired = first.acquired.unwrap_or(today);
            let earliest = acquired + Duration::days(TENDER_DAYS_AFTER_EXERCISE);
            let latest = today - Duration::days(TENDER_DAYS_AGO);
            let sold = round_size(first.quantity * tender.sold_share).min(first.quantity - 1.0);
            if earliest <= latest && sold >= 1.0 {
                let settled = between_days(earliest, latest, tender.timing);
                let price = cents(price_on(&self.prices, settled) * tender.premium);
                equity.certificates[0].status = CUT.to_string();
                equity.certificates[0].canceled = true;
                let piece = |quantity: f64, status: &str| Certificate {
                    issued: Some(settled),
                    fmv_at_issue: Some(price_on(&self.prices, settled)),
                    quantity,
                    // The cost stays with the certificate the piece was cut from
                    cost: Some(0.0),
                    status: status.to_string(),
                    canceled: status == SOLD,
                    exercise_type: String::new(),
                    exercised_from: String::new(),
                    ..first.clone()
                };
                equity.certificates.push(piece(sold, SOLD));
                equity.certificates.push(piece(first.quantity - sold, HELD));
                equity.tenders.push(Tender {
                    name: format!("{} Tender Offer", settled.year()),
                    status: TENDER_SETTLED.to_string(),
                    kind: TENDER_KIND.to_string(),
                    opened: Some(settled - Duration::days(tender.open_days)),
                    closed: Some(settled - Duration::days(tender.close_days)),
                    settled: Some(settled),
                    shares: sold,
                    proceeds: cents(sold * price),
                });
            }
        }

        // Certificates are numbered as they are issued
        let mut order: Vec<usize> = (0..equity.certificates.len()).collect();
        order.sort_by_key(|index| equity.certificates[*index].issued);
        for (number, index) in self.certificate_numbers.iter().zip(order) {
            equity.certificates[index].label = format!("CS-{}", number);
        }
        for (exercise, certificate) in equity.exercises.iter_mut().zip(bought) {
            exercise.certificate_label = equity.certificates[certificate].label.clone();
        }
        equity.certificates.sort_by_key(|certificate| certificate.acquired);

        for (index, plan) in self.grants.iter().enumerate() {
            let id = index as i64 + 1;
            let quantity = quantities[index];
            let vested: f64 = tranches[index].iter().filter(|tranche| tranche.0 <= today).map(|tranche| tranche.1).sum();
            let status = match exercised[index] {
                none if none <= 0.0 => GRANT_OUTSTANDING,
                all if all >= quantity => GRANT_FULLY_EXERCISED,
                _ => GRANT_PARTIALLY_EXERCISED,
            };
            equity.grants.push(Grant {
                id,
                issuer_id: ISSUER_ID,
                label: plan.label.clone(),
                option_type: plan.option_type().to_string(),
                stock_type: STOCK_TYPE.to_string(),
                status: status.to_string(),
                expires: plan.issued.checked_add_months(Months::new(EXPIRY_MONTHS)),
                issued: Some(plan.issued),
                quantity,
                vested,
                exercised: exercised[index],
                exercisable: vested - exercised[index],
                exercise_price: plan.strike,
                canceled: false,
                vesting_start: Some(plan.vesting_start),
                vesting_end: tranches[index].last().map(|tranche| tranche.0),
                schedule: if plan.cliff { CLIFF_SCHEDULE } else { MONTHLY_SCHEDULE }.to_string(),
                cliff: if plan.cliff { CLIFF_SUMMARY } else { "" }.to_string(),
                terms: if plan.cliff { CLIFF_TERMS } else { MONTHLY_TERMS }.to_string(),
                acceleration: String::new(),
                early_exercise: Some(false),
                iso_disqualified: false,
                form_83b: NO_83B.to_string(),
            });
            equity.vesting.extend(tranches[index].iter().map(|(date, options, iso)| VestEvent {
                grant_id: id,
                date: *date,
                quantity: *options,
                iso: Some(*iso),
                nso: Some(options - iso),
            }));
        }
        equity.vesting.sort_by_key(|event| (event.date, event.grant_id));

        let (valued, fmv) = self.prices[self.prices.len() - 1];
        equity.snapshots.push(Snapshot { issuer_id: ISSUER_ID, date: valued, fmv: Some(fmv) });
        equity.issuers.push(Issuer { id: ISSUER_ID, name: COMPANY_NAME.to_string(), fmv: Some(fmv), as_of: Some(today) });
        equity
    }
}

/// Invented holdings in place of `real`, worth about `scale` times as much
/// (and a factor of their own on top, so the stake cannot be read back from
/// the rest of the disguise). The same `seed` always invents the same ones.
pub fn invent(real: &Equity, seed: u64, today: NaiveDate, scale: f64) -> Equity {
    if real.is_empty() {
        return Equity::default();
    }
    let mut dice = Dice { seed: hash(seed, "equity"), drawn: 0 };
    let worth = real.summary(1.0).vested_value() * scale * dice.between((SCALE_MIN, SCALE_MAX));
    // Never the number of grants there really are
    let mut grants = dice.whole((GRANTS_MIN, GRANTS_MAX));
    if grants as usize == real.live_grants().count() {
        grants = if grants == GRANTS_MAX { GRANTS_MIN } else { grants + 1 };
    }
    let plan = Plan::draw(&mut dice, grants, today);

    // Size the grants so that what is vested comes to what it should
    let probed = plan.holdings(PROBE_GRANT, today).summary(1.0).vested_value();
    let first_grant = if worth > 0.0 && probed > 0.0 { PROBE_GRANT * worth / probed } else { DEFAULT_GRANT };
    let mut equity = plan.holdings(first_grant.min(LARGEST_GRANT), today);
    equity.captured_at = real.captured_at;
    if let Some(captured) = real.captured_at {
        equity.issuers.iter_mut().for_each(|issuer| issuer.as_of = Some(captured.date_naive().min(today)));
    }
    equity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::equity::Split;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn today() -> NaiveDate {
        date(2026, 10, 12)
    }

    /// Real holdings worth $400,000 vested: four grants of 100,000 options at a $1 spread
    fn real() -> Equity {
        let grant = |id: i64| Grant {
            id,
            issuer_id: 7,
            label: format!("ES-{}", 9000 + id),
            issued: Some(date(2012, 3, 9)),
            quantity: 100_000.0,
            vested: 100_000.0,
            exercisable: 100_000.0,
            exercise_price: 1.0,
            vesting_start: Some(date(2012, 2, 1)),
            vesting_end: Some(date(2016, 2, 1)),
            ..Grant::default()
        };
        Equity {
            issuers: vec![Issuer { id: 7, name: "Acme Rockets".into(), fmv: Some(2.0), as_of: Some(today()) }],
            grants: (1..=4).map(grant).collect(),
            ..Equity::default()
        }
    }

    const SEEDS: std::ops::Range<u64> = 0..200;

    #[test]
    fn nothing_is_invented_for_someone_with_no_equity() {
        assert!(invent(&Equity::default(), 7, today(), 0.5).is_empty());
    }

    #[test]
    fn the_same_seed_invents_the_same_holdings_and_another_seed_others() {
        let label = |seed: u64| invent(&real(), seed, today(), 0.5).grants[0].label.clone();
        let quantity = |seed: u64| invent(&real(), seed, today(), 0.5).grants[0].quantity;
        assert_eq!((label(7), quantity(7)), (label(7), quantity(7)));
        assert!(SEEDS.map(label).collect::<std::collections::HashSet<_>>().len() > 100);
    }

    #[test]
    fn nothing_of_the_real_grants_is_kept() {
        let real = real();
        for seed in SEEDS {
            let equity = invent(&real, seed, today(), 0.5);
            assert_ne!(equity.live_grants().count(), 4, "seed {seed}");
            assert!((GRANTS_MIN..=GRANTS_MAX).contains(&(equity.grants.len() as i64)));
            assert_eq!(equity.issuers.len(), 1);
            assert_eq!(equity.issuers[0].name, COMPANY_NAME);
            for grant in &equity.grants {
                assert!(real.grants.iter().all(|r| r.label != grant.label && r.issued != grant.issued));
                assert!(grant.issued.unwrap() <= today() && grant.vesting_start.unwrap() <= today());
            }
        }
    }

    #[test]
    fn a_grant_adds_up_through_its_tranches_and_exercises() {
        for seed in SEEDS {
            let equity = invent(&real(), seed, today(), 0.5);
            for grant in &equity.grants {
                let tranches: Vec<&VestEvent> = equity.vesting.iter().filter(|e| e.grant_id == grant.id).collect();
                assert_eq!(tranches.iter().map(|e| e.quantity).sum::<f64>(), grant.quantity, "seed {seed}");
                let vested: f64 = tranches.iter().filter(|e| e.date <= today()).map(|e| e.quantity).sum();
                assert_eq!(grant.vested, vested);
                for tranche in &tranches {
                    let (iso, nso) = (tranche.iso.unwrap(), tranche.nso.unwrap());
                    assert!(iso >= 0.0 && nso >= 0.0 && tranche.quantity.fract() == 0.0);
                    assert_eq!(iso + nso, tranche.quantity);
                }
                let exercised: f64 = equity.exercises_of(grant).map(|x| x.quantity).sum();
                assert_eq!(grant.exercised, exercised);
                assert_eq!(grant.exercisable, grant.vested - grant.exercised);
                assert!(grant.exercisable >= 0.0);
                // Never more ISOs exercised than had vested as ISOs
                let split = equity.split(grant).unwrap();
                assert!(split.exercised.iso <= split.vested.iso && split.exercised.nso <= split.vested.nso);
                assert_eq!(split.exercisable.iso + split.exercisable.nso, grant.exercisable);
                assert_ne!(split.granted, Split::default());
            }
        }
    }

    #[test]
    fn shares_trace_back_to_the_exercise_that_bought_them() {
        let (mut with_shares, mut with_tender) = (0, 0);
        for seed in SEEDS {
            let equity = invent(&real(), seed, today(), 0.5);
            with_shares += usize::from(equity.held_certificates().count() > 0);
            let mut labels: Vec<&str> = equity.certificates.iter().map(|c| c.label.as_str()).collect();
            labels.sort();
            labels.dedup();
            assert_eq!(labels.len(), equity.certificates.len(), "seed {seed}");
            for exercise in &equity.exercises {
                let certificate = equity.certificate(ISSUER_ID, &exercise.certificate_label).unwrap();
                assert_eq!(certificate.quantity, exercise.quantity);
                assert_eq!(certificate.acquired, exercise.date);
                assert!(exercise.date.unwrap() < today());
            }
            for certificate in equity.held_certificates() {
                let exercise = equity.exercise_behind(certificate).unwrap();
                assert_eq!(equity.grant_behind(certificate).unwrap().label, exercise.grant_label);
                assert!(equity.cost_of(certificate).unwrap() > 0.0);
            }
            for tender in &equity.tenders {
                with_tender += 1;
                let sold = equity.sold_in(tender);
                assert_eq!(sold.len(), 1);
                assert_eq!(sold[0].quantity, tender.shares);
                let whole = equity.parent(sold[0]).unwrap();
                let pieces: f64 = equity.pieces(whole).iter().map(|c| c.quantity).sum();
                assert_eq!(pieces, whole.quantity);
                assert!(tender.opened < tender.closed && tender.closed < tender.settled);
                assert!(tender.settled.unwrap() < today() && tender.settled > whole.acquired);
            }
            // Held shares are what was exercised, less what a tender bought
            let held: f64 = equity.held_certificates().map(|c| c.quantity).sum();
            let exercised: f64 = equity.exercises.iter().map(|x| x.quantity).sum();
            let sold: f64 = equity.tenders.iter().map(|t| t.shares).sum();
            assert_eq!(held, exercised - sold);
        }
        assert!(with_shares > 150 && with_tender > 50, "{with_shares} {with_tender}");
    }

    #[test]
    fn the_holdings_are_worth_about_what_was_asked_for() {
        let mut close = 0;
        for seed in SEEDS {
            let mut dice = Dice { seed: hash(seed, "equity"), drawn: 0 };
            let worth = 400_000.0 * 0.5 * dice.between((SCALE_MIN, SCALE_MAX));
            let vested = invent(&real(), seed, today(), 0.5).summary(1.0).vested_value();
            close += usize::from((vested / worth - 1.0).abs() < 0.1);
        }
        // All but the few whose options are under water and cannot be sized up to it
        assert!(close > 180, "{close}");
    }

    #[test]
    fn the_share_price_is_on_record_from_the_first_grant_to_today() {
        for seed in SEEDS {
            let equity = invent(&real(), seed, today(), 0.5);
            let history = equity.price_history();
            assert!(history.first().unwrap().0 <= equity.grants[0].issued.unwrap(), "seed {seed}");
            assert_eq!(history.last().unwrap().1, equity.issuers[0].fmv.unwrap());
            assert!(history.iter().all(|price| price.1 >= LEAST_PRICE && price.0 <= today()));
            let values = equity.value_history(today());
            assert_eq!(values.last().unwrap().1, equity.summary(1.0).vested_value());
        }
    }

    #[test]
    fn sizes_are_round_numbers() {
        assert_eq!(round_size(86_432.0), 86_000.0);
        assert_eq!(round_size(4_351.0), 4_400.0);
        assert_eq!(round_size(37.0), 37.0);
        assert_eq!(round_size(0.4), 0.0);
    }
}
